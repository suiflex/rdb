//! Table browsing and the sidebar tree: the Items/Queries/History tab switch,
//! opening a table into a browse query, the pagination footer, the Mongo filter
//! bar and JSON tree, refresh, expanding schema headers and table columns, and
//! the sidebar filter box.
//!
//! Split out of `main`; the handler bodies are unchanged.

use std::collections::HashSet;
use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::*;

pub(crate) fn wire(window: &MainWindow, state: &AppState, fns: &AppFns) {
    let AppState {
        rt,
        store,
        panes,
        current,
        cur_engine,
        raw_nodes,
        expanded_tables,
        loaded_dbs,
        collapsed_categories,
        sidebar_filter,
        workspace_tabs,
        active_tab_id,
        current_connection_id,
        query_console,
        last_view,
        browse_trigger,
        collapsed_history_groups,
        completion_nodes,
        ..
    } = state.clone();
    let AppFns {
        rebuild_query_tree,
        load_editor_text,
        save_active_tab,
        restore_tab,
        guard_pending,
        run_sql,
        ..
    } = fns.clone();
    let browse = panes[0].browse.clone();
    let edit_buf = panes[0].edit_buf.clone();
    let displayed_grid = panes[0].displayed_grid.clone();
    let results = panes[0].results.clone();

    // ----- sidebar Items / Queries / History tabs -----
    {
        let rebuild_query_tree = rebuild_query_tree.clone();
        window.on_sidebar_mode_changed(move |_mode| {
            rebuild_query_tree("");
        });
    }

    // ----- open table: build an engine-appropriate "browse" query, show it in
    // the editor, and run it (TablePlus-style click-to-view). -----
    // Shared page runner: build the browse text for the current state, mirror
    // it in the editor, and run it. Used by open-table and the footer nav.
    // Refuse to navigate away from uncommitted edits: a fresh page would
    // silently drop them. Returns true (and says so) when edits are pending.

    let run_browse: Rc<dyn Fn(usize)> = {
        let cur_engine = cur_engine.clone();
        let panes = panes.clone();
        let run_sql = run_sql.clone();
        let load_editor_text = load_editor_text.clone();
        Rc::new(move |pane| {
            let Some(engine) = *cur_engine.borrow() else {
                return;
            };
            let pane = pane.min(1);
            let st = panes[pane].browse.lock().unwrap().clone();
            let Some(table) = st.table else {
                return;
            };
            // "No limit" (0) has no meaning for paged browse; fall back to the
            // engine default page size so the footer nav still works.
            let limit = if st.limit == 0 {
                default_browse_limit(engine)
            } else {
                st.limit
            };
            let text = browse_text(
                engine,
                &table,
                st.page,
                limit,
                &st.mongo_filter,
                &st.col_filters,
            );
            load_editor_text(pane, &text);
            run_sql(pane, text);
        })
    };
    // Wire the deferred handle so per-column browse filters can re-query.
    *browse_trigger.borrow_mut() = Some(Rc::new({
        let run_browse = run_browse.clone();
        move || run_browse(0)
    }));

    {
        let weak = window.as_weak();
        let cur_engine = cur_engine.clone();
        let browse = browse.clone();
        let current = current.clone();
        let rt = rt.clone();
        let run_browse = run_browse.clone();
        let edit_buf = edit_buf.clone();
        let guard_pending = guard_pending.clone();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let current_connection_id = current_connection_id.clone();
        let save_active_tab = save_active_tab.clone();
        let restore_tab = restore_tab.clone();
        let query_console = query_console.clone();
        let last_view = last_view.clone();
        let displayed_grid = displayed_grid.clone();
        let results = results.clone();
        let store = store.clone();
        window.on_open_table(move |db, label| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if guard_pending(&w) {
                return;
            }
            let Some(engine) = *cur_engine.borrow() else {
                return;
            };
            let label = label.to_string();
            let db = db.to_string();
            let database_name = if db.is_empty() {
                w.get_bc_db().to_string()
            } else {
                db.clone()
            };
            let schema_name = if matches!(engine, rdb_connstore::Engine::Postgres) {
                w.get_schema_name().to_string()
            } else {
                String::new()
            };
            let connection_id = current_connection_id
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_default();
            let tab_id = table_tab_id(&connection_id, &database_name, &schema_name, &label);
            // Drop the lookup guard before save/restore lock the same tab list.
            let existing_index = {
                let tabs = workspace_tabs.lock().unwrap();
                workspace_tab_index(&tabs, Some(&tab_id))
            };
            if let Some(index) = existing_index {
                save_active_tab(&w);
                restore_tab(&w, index);
                return;
            }
            let table = rdb_core::write::TableRef {
                database: (!db.is_empty()).then(|| db.clone()),
                schema: matches!(engine, rdb_connstore::Engine::Postgres)
                    .then(|| w.get_schema_name().to_string()),
                name: label.clone(),
            };
            save_active_tab(&w);
            // Fresh container: page 0, keep the user's limit, forget totals.
            {
                let mut st = browse.lock().unwrap();
                st.table = Some(table.clone());
                st.page = 0;
                st.total = None;
                st.pk_cols.clear();
                st.mongo_filter.clear();
                st.col_filters.clear();
            }
            {
                let badge = connection_badge_info(&store.borrow(), &connection_id);
                let tab = WorkspaceTab {
                    id: tab_id.clone(),
                    title: label.clone(),
                    kind: "table".into(),
                    query_text: String::new(),
                    table: Some(table.clone()),
                    browse: browse.lock().unwrap().clone(),
                    results: Vec::new(),
                    active_result: 0,
                    view: None,
                    indexes: Vec::new(),
                    loading: true,
                    pinned: false,
                    group: 0,
                    split: false,
                    split_ratio: 0.5,
                    pane1_query: String::new(),
                    connection_id: (!connection_id.is_empty()).then(|| connection_id.clone()),
                    engine: badge.engine,
                    connection_name: badge.name,
                    color: badge.color,
                    has_custom_color: badge.has_custom_color,
                };
                let active_id = active_tab_id.lock().unwrap().clone();
                let mut tabs = workspace_tabs.lock().unwrap();
                if let Some(index) = replaceable_table_tab_index(&tabs, active_id.as_deref()) {
                    tabs[index] = tab;
                } else {
                    tabs.push(tab);
                }
                *active_tab_id.lock().unwrap() = Some(tab_id.clone());
                set_workspace_tabs(&w, &tabs, Some(&tab_id));
            }
            w.set_mongo_filter(SharedString::default());
            w.set_fn_mode(false);
            w.set_active_table(SharedString::from(label));
            w.set_show_structure(false);
            w.set_total_rows(-1);
            w.set_grid_read_only(true);
            w.set_index_rows(ModelRc::from(Rc::new(VecModel::<IndexRow>::default())));
            *last_view.lock().unwrap() = None;
            *displayed_grid.lock().unwrap() = None;
            results.lock().unwrap().clear();
            set_result_tabs(&w, 0, &[], 0);
            clear_grid(&w, 0);
            run_browse(0);

            // Fetch total + primary key off-thread; footer updates when done.
            let weak2 = weak.clone();
            let current = current.clone();
            let browse = browse.clone();
            let edit_buf = edit_buf.clone();
            let workspace_tabs = workspace_tabs.clone();
            let active_tab_id = active_tab_id.clone();
            let query_console = query_console.clone();
            rt.spawn(async move {
                let picked = {
                    let guard = current.lock().await;
                    guard.as_ref().map(|(e, d)| (*e, d.clone()))
                };
                let Some((engine, driver)) = picked.as_ref() else {
                    return;
                };
                let total = driver.count(&table).await.ok();
                let pk = driver.primary_key(&table).await.unwrap_or_default();
                let indexes = fetch_indexes(*engine, driver, &table).await;
                let (page, limit) = {
                    let mut tabs = workspace_tabs.lock().unwrap();
                    let Some(tab) = tabs.iter_mut().find(|tab| tab.id == tab_id) else {
                        return;
                    };
                    tab.browse.total = total;
                    tab.browse.pk_cols = pk.clone();
                    tab.indexes = indexes.clone();
                    (tab.browse.page, tab.browse.limit)
                };
                let is_active = active_tab_id.lock().unwrap().as_deref() == Some(&tab_id);
                if is_active {
                    let mut st = browse.lock().unwrap();
                    st.total = total;
                    st.pk_cols = pk.clone();
                    // The page result usually lands before this reply and
                    // re-anchors the buffer with empty pk_cols — top it up.
                    let mut b = edit_buf.lock().unwrap();
                    b.table = Some(table.clone());
                    b.pk_cols = pk.clone();
                }
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak2.upgrade() {
                        sync_query_console(&w, &query_console);
                        if active_tab_id.lock().unwrap().as_deref() != Some(&tab_id) {
                            return;
                        }
                        let rows: Vec<IndexRow> = indexes
                            .into_iter()
                            .map(|(name, definition)| IndexRow {
                                name: name.into(),
                                definition: definition.into(),
                            })
                            .collect();
                        w.set_index_rows(ModelRc::from(Rc::new(VecModel::from(rows))));
                        w.set_total_rows(total.map(|t| t as i32).unwrap_or(-1));
                        w.set_grid_read_only(pk.is_empty());
                        w.set_sort_text(if pk.is_empty() {
                            SharedString::default()
                        } else {
                            SharedString::from(format!("sort: {} ↑", pk[0]))
                        });
                        let shown = (w.get_page_end() - w.get_page_start()).max(0) as u64
                            + u64::from(w.get_page_end() > 0);
                        let (start, end, prev, next) = page_bounds(page, limit, total, shown);
                        w.set_page_start(start as i32);
                        w.set_page_end(end as i32);
                        w.set_can_prev(prev);
                        w.set_can_next(next);
                    }
                });
            });
        });
    }

    {
        let weak = window.as_weak();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        window.on_pin_table(move |_db, label| {
            let Some(id) = active_tab_id.lock().unwrap().clone() else {
                return;
            };
            let mut tabs = workspace_tabs.lock().unwrap();
            if let Some(tab) = tabs
                .iter_mut()
                .find(|tab| tab.id == id && tab.table.as_ref().is_some_and(|t| label == t.name))
            {
                tab.pinned = true;
                if let Some(w) = weak.upgrade() {
                    set_workspace_tabs(&w, &tabs, Some(&id));
                }
            }
        });
    }

    // ----- pagination footer: prev / next / refresh / limit -----
    {
        let weak = window.as_weak();
        let browse = browse.clone();
        let run_browse = run_browse.clone();
        let guard_pending = guard_pending.clone();
        window.on_prev_page(move || {
            if weak.upgrade().is_some_and(|w| guard_pending(&w)) {
                return;
            }
            {
                let mut st = browse.lock().unwrap();
                if st.page == 0 {
                    return;
                }
                st.page -= 1;
            }
            run_browse(0);
        });
    }
    {
        let weak = window.as_weak();
        let browse = browse.clone();
        let run_browse = run_browse.clone();
        let guard_pending = guard_pending.clone();
        window.on_next_page(move || {
            if weak.upgrade().is_some_and(|w| guard_pending(&w)) {
                return;
            }
            browse.lock().unwrap().page += 1;
            run_browse(0);
        });
    }
    {
        let weak = window.as_weak();
        let browse = browse.clone();
        let current = current.clone();
        let rt = rt.clone();
        let run_browse = run_browse.clone();
        let guard_pending = guard_pending.clone();
        window.on_refresh_page(move || {
            if weak.upgrade().is_some_and(|w| guard_pending(&w)) {
                return;
            }
            run_browse(0);
            // Re-count in the background so the total tracks external writes.
            let table = browse.lock().unwrap().table.clone();
            let Some(table) = table else { return };
            let weak2 = weak.clone();
            let current = current.clone();
            let browse = browse.clone();
            rt.spawn(async move {
                let driver = {
                    let guard = current.lock().await;
                    guard.as_ref().map(|(_, d)| d.clone())
                };
                let Some(driver) = driver else {
                    return;
                };
                let total = driver.count(&table).await.ok();
                browse.lock().unwrap().total = total;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak2.upgrade() {
                        w.set_total_rows(total.map(|t| t as i32).unwrap_or(-1));
                    }
                });
            });
        });
    }
    {
        let weak = window.as_weak();
        let browse = browse.clone();
        let run_browse = run_browse.clone();
        let guard_pending = guard_pending.clone();
        window.on_set_limit(move |text| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            // Echo the validated limit so manual edits and paging stay in sync.
            // 0 renders as "No limit" (the streaming sentinel).
            let echo = |w: &MainWindow, l: u64| {
                w.set_limit_text(SharedString::from(if l == 0 {
                    "No limit".to_string()
                } else {
                    l.to_string()
                }));
            };
            if guard_pending(&w) {
                echo(&w, browse.lock().unwrap().limit);
                return;
            }
            // "No limit" / 0 / all / none -> stream the whole result (limit 0).
            let lower = text.trim().to_ascii_lowercase();
            if lower.is_empty()
                || lower == "0"
                || lower == "all"
                || lower == "none"
                || lower.contains("no limit")
            {
                {
                    let mut st = browse.lock().unwrap();
                    st.limit = 0;
                    st.page = 0;
                }
                echo(&w, 0);
                run_browse(0);
                return;
            }
            // The manual field may contain dot separators — keep digits only.
            let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
            let Some(l) = digits.parse::<u64>().ok().map(|l| l.clamp(1, 10_000)) else {
                echo(&w, browse.lock().unwrap().limit);
                return;
            };
            {
                let mut st = browse.lock().unwrap();
                st.limit = l;
                st.page = 0;
            }
            echo(&w, l);
            run_browse(0);
        });
    }

    {
        let panes = panes.clone();
        let run_browse = run_browse.clone();
        window.on_p1_set_limit(move |text| {
            let lower = text.trim().to_ascii_lowercase();
            let limit = if lower.is_empty()
                || lower == "0"
                || lower == "all"
                || lower == "none"
                || lower.contains("no limit")
            {
                0
            } else {
                let digits: String = text.chars().filter(|c| c.is_ascii_digit()).collect();
                let Some(limit) = digits.parse::<u64>().ok().map(|n| n.clamp(1, 10_000)) else {
                    return;
                };
                limit
            };
            let mut browse = panes[1].browse.lock().unwrap();
            browse.limit = limit;
            browse.page = 0;
            drop(browse);
            run_browse(1);
        });
    }
    {
        let panes = panes.clone();
        let run_browse = run_browse.clone();
        window.on_p1_prev_page(move || {
            let mut browse = panes[1].browse.lock().unwrap();
            if browse.page == 0 {
                return;
            }
            browse.page -= 1;
            drop(browse);
            run_browse(1);
        });
    }
    {
        let panes = panes.clone();
        let run_browse = run_browse.clone();
        window.on_p1_next_page(move || {
            panes[1].browse.lock().unwrap().page += 1;
            run_browse(1);
        });
    }
    {
        let run_browse = run_browse.clone();
        window.on_p1_refresh_page(move || run_browse(1));
    }
    // ----- right pane: Mongo browse filter (mirrors group 0) -----
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        let run_browse = run_browse.clone();
        window.on_p1_apply_mongo_filter(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let raw = w.get_p1_mongo_filter().to_string();
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                if let Err(e) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    set_p_status_error(&w, 1, true);
                    set_p_result_status(
                        &w,
                        1,
                        SharedString::from(format!("invalid filter JSON: {e}")),
                    );
                    return;
                }
            }
            {
                let mut st = panes[1].browse.lock().unwrap();
                st.mongo_filter = trimmed.to_string();
                st.page = 0;
            }
            run_browse(1);
        });
    }

    // ----- Mongo filter box autocomplete -----
    //
    // The filter box holds a bare filter document, so it can't reuse the
    // editor's completion path (see `completion::suggest_mongo_filter` for
    // why). State is single-copy across both panes because only the focused
    // field can be typing, and `FilterField` paints its list only while
    // focused.
    //
    // ponytail: the whole field text is treated as "before the cursor".
    // `TextInput` does expose `cursor-position`, but plumbing it up three
    // component layers buys correctness only for editing mid-string, and
    // filters are typed left to right. Pass the caret through if that changes.
    {
        // Char length of the partial word to replace on accept, plus the
        // candidate labels currently on offer.
        let ctx: Rc<RefCell<(usize, Vec<String>)>> = Rc::new(RefCell::new((0, Vec::new())));

        let set_items = {
            let ctx = ctx.clone();
            move |w: &MainWindow, word_len: usize, cands: Vec<completion::Candidate>| {
                if cands.is_empty() {
                    *ctx.borrow_mut() = (0, Vec::new());
                    w.set_filter_completion_items(ModelRc::from(Rc::new(
                        VecModel::<PaletteItem>::default(),
                    )));
                    return;
                }
                let items: Vec<PaletteItem> = cands
                    .iter()
                    .map(|c| PaletteItem {
                        label: c.label.clone().into(),
                        kind: c.kind.clone().into(),
                        sub: c.sub.clone().into(),
                        local: false,
                        color: theme::accent_or_default(""),
                        has_custom_color: false,
                        env_tag_label: SharedString::default(),
                        env_tag_color: theme::accent_or_default(""),
                        group: SharedString::default(),
                        expanded: false,
                        is_group_end: false,
                        depth: 0,
                    })
                    .collect();
                *ctx.borrow_mut() = (word_len, cands.iter().map(|c| c.label.clone()).collect());
                w.set_filter_completion_items(ModelRc::from(Rc::new(VecModel::from(items))));
                w.set_filter_completion_selected(0);
            }
        };

        // Splice the chosen candidate over the partial word and close the popup.
        let apply_choice = {
            let ctx = ctx.clone();
            move |w: &MainWindow, pane: usize, idx: i32| {
                let (word_len, labels) = ctx.borrow().clone();
                let Some(label) = labels.get(idx.max(0) as usize).cloned() else {
                    return;
                };
                let text = if pane == 0 {
                    w.get_mongo_filter().to_string()
                } else {
                    w.get_p1_mongo_filter().to_string()
                };
                let keep: String = text
                    .chars()
                    .take(text.chars().count().saturating_sub(word_len))
                    .collect();
                let next = SharedString::from(format!("{keep}{label}"));
                if pane == 0 {
                    w.set_mongo_filter(next);
                } else {
                    w.set_p1_mongo_filter(next);
                }
                *ctx.borrow_mut() = (0, Vec::new());
                w.set_filter_completion_items(ModelRc::from(Rc::new(
                    VecModel::<PaletteItem>::default(),
                )));
            }
        };

        {
            let weak = window.as_weak();
            let panes = panes.clone();
            let completion_nodes = completion_nodes.clone();
            let set_items = set_items.clone();
            window.on_mongo_filter_edited(move |pane, text| {
                let Some(w) = weak.upgrade() else {
                    return;
                };
                let pane = pane.max(0) as usize;
                // Fields come from the collection this pane is browsing.
                let collection = panes[pane]
                    .browse
                    .lock()
                    .unwrap()
                    .table
                    .as_ref()
                    .map(|t| t.name.clone())
                    .unwrap_or_default();
                let (word_len, cands) = completion::suggest_mongo_filter(
                    text.as_str(),
                    &completion_nodes.lock().unwrap(),
                    &collection,
                );
                set_items(&w, word_len, cands);
            });
        }

        {
            let weak = window.as_weak();
            let apply_choice = apply_choice.clone();
            window.on_filter_completion_choose(move |pane, idx| {
                if let Some(w) = weak.upgrade() {
                    apply_choice(&w, pane.max(0) as usize, idx);
                }
            });
        }

        {
            let weak = window.as_weak();
            window.on_filter_completion_key(move |pane, key| {
                let Some(w) = weak.upgrade() else {
                    return false;
                };
                let n = w.get_filter_completion_items().row_count() as i32;
                if n == 0 {
                    return false;
                }
                let sel = w.get_filter_completion_selected();
                match key.as_str() {
                    // Wrap around, matching the editor popup.
                    "\u{f700}" => {
                        w.set_filter_completion_selected((sel - 1).rem_euclid(n));
                        true
                    }
                    "\u{f701}" => {
                        w.set_filter_completion_selected((sel + 1).rem_euclid(n));
                        true
                    }
                    "\t" | "\n" | "\r" => {
                        w.invoke_filter_completion_choose(pane, sel);
                        true
                    }
                    "\u{1b}" => {
                        w.set_filter_completion_items(ModelRc::from(Rc::new(VecModel::<
                            PaletteItem,
                        >::default(
                        ))));
                        true
                    }
                    _ => false,
                }
            });
        }
    }

    // ----- Mongo browse filter bar (Compass-style filter document) -----
    {
        let weak = window.as_weak();
        let browse = browse.clone();
        let run_browse = run_browse.clone();
        let guard_pending = guard_pending.clone();
        window.on_apply_mongo_filter(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if guard_pending(&w) {
                return;
            }
            let raw = w.get_mongo_filter().to_string();
            let trimmed = raw.trim();
            // Empty clears the filter; otherwise it must be a JSON document.
            if !trimmed.is_empty() {
                if let Err(e) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    w.set_status_error(true);
                    w.set_result_status(SharedString::from(format!("invalid filter JSON: {e}")));
                    return;
                }
            }
            {
                let mut st = browse.lock().unwrap();
                st.mongo_filter = trimmed.to_string();
                st.page = 0;
            }
            run_browse(0);
        });
    }

    // ----- Mongo JSON tree: fold/unfold a branch -----
    {
        let weak = window.as_weak();
        window.on_toggle_doc_node(move |path| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            DOC_TREES.with(|s| {
                let mut st = s.borrow_mut();
                let (full, collapsed) = &mut st[0];
                let p = path.to_string();
                if !collapsed.remove(&p) {
                    collapsed.insert(p);
                }
                push_doc_tree(&w, 0, full, collapsed);
            });
        });
    }
    {
        let weak = window.as_weak();
        window.on_p1_toggle_doc_node(move |path| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            DOC_TREES.with(|s| {
                let mut st = s.borrow_mut();
                let (full, collapsed) = &mut st[1];
                let p = path.to_string();
                if !collapsed.remove(&p) {
                    collapsed.insert(p);
                }
                push_doc_tree(&w, 1, full, collapsed);
            });
        });
    }

    // ----- ⌘R: refresh what the user is looking at -----
    {
        let weak = window.as_weak();
        window.on_refresh_result(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if active_tab_kind(&w) == "table" {
                // guarded against pending edits inside refresh-page
                w.invoke_refresh_page();
            } else if !w.get_query_text().trim().is_empty() {
                w.invoke_run_query();
            }
        });
    }

    // ----- toggle a schema header (expand/collapse) -----
    {
        let weak = window.as_weak();
        let raw_nodes = raw_nodes.clone();
        let expanded_tables = expanded_tables.clone();
        let loaded_dbs = loaded_dbs.clone();
        let collapsed_categories = collapsed_categories.clone();
        let cur_engine = cur_engine.clone();
        let current = current.clone();
        let rt = rt.clone();
        let sidebar_filter = sidebar_filter.clone();
        let collapsed_history_groups = collapsed_history_groups.clone();
        let rebuild_query_tree = rebuild_query_tree.clone();
        window.on_toggle_schema_node(move |label| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let label = label.to_string();

            // History tab's date-bucket headers are a separate tree
            // (`query_tree`, not the Items `schema_tree`) with their own
            // collapse set, so they're handled here before anything else.
            if w.get_sidebar_mode() == 2 {
                let mut c = collapsed_history_groups.borrow_mut();
                if !c.remove(&label) {
                    c.insert(label);
                }
                drop(c);
                rebuild_query_tree("");
                return;
            }

            let engine = *cur_engine.borrow();

            // Mongo/Redis/Cassandra: database (or keyspace) headers open an
            // opt-in set (default closed) and load their leaves (collections /
            // keys / tables) lazily on first expand.
            if matches!(
                engine,
                Some(rdb_connstore::Engine::Mongo)
                    | Some(rdb_connstore::Engine::Redis)
                    | Some(rdb_connstore::Engine::Cassandra)
            ) {
                let leaf_kind = match engine {
                    Some(rdb_connstore::Engine::Redis) => "key",
                    Some(rdb_connstore::Engine::Cassandra) => "table",
                    _ => "collection",
                };
                let now_open = {
                    let mut e = expanded_tables.lock().unwrap();
                    if e.remove(&label) {
                        false
                    } else {
                        e.insert(label.clone());
                        true
                    }
                };
                // Fetch this database's collections once, on first open.
                let need_fetch = now_open && {
                    let mut l = loaded_dbs.lock().unwrap();
                    if l.contains(&label) {
                        false
                    } else {
                        l.insert(label.clone());
                        true
                    }
                };
                if need_fetch {
                    let driver = current.clone();
                    let raw_nodes = raw_nodes.clone();
                    let expanded_tables = expanded_tables.clone();
                    let loaded_dbs = loaded_dbs.clone();
                    let sidebar_filter = sidebar_filter.clone();
                    let weak2 = weak.clone();
                    let db = label.clone();
                    rt.spawn(async move {
                        let drv = {
                            let guard = driver.lock().await;
                            guard.as_ref().map(|(_, d)| d.clone())
                        };
                        let containers = match drv {
                            Some(drv) => drv.containers(&db).await.unwrap_or_default(),
                            None => return,
                        };
                        let rows = {
                            let mut nodes = raw_nodes.lock().unwrap();
                            if let Some(pos) = nodes
                                .iter()
                                .position(|n| n.kind == "database" && n.label == db)
                            {
                                for (k, c) in containers.into_iter().enumerate() {
                                    nodes.insert(
                                        pos + 1 + k,
                                        model::VmTreeNode {
                                            label: c.name,
                                            kind: leaf_kind.into(),
                                        },
                                    );
                                }
                            }
                            schema_display_rows(
                                &nodes,
                                &expanded_tables.lock().unwrap(),
                                &HashSet::new(),
                                &loaded_dbs.lock().unwrap(),
                                engine,
                                &sidebar_filter.lock().unwrap(),
                            )
                        };
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = weak2.upgrade() {
                                w.set_schema_tree(ModelRc::from(Rc::new(VecModel::from(rows))));
                            }
                        });
                    });
                    return;
                }
                // No fetch needed (collapse, or already loaded): rebuild now.
                let nodes = raw_nodes.lock().unwrap();
                let rows = schema_display_rows(
                    &nodes,
                    &expanded_tables.lock().unwrap(),
                    &HashSet::new(),
                    &loaded_dbs.lock().unwrap(),
                    engine,
                    &sidebar_filter.lock().unwrap(),
                );
                w.set_schema_tree(ModelRc::from(Rc::new(VecModel::from(rows))));
                return;
            }

            // SQL: category headers collapse-toggle (open by default).
            {
                let mut c = collapsed_categories.borrow_mut();
                if !c.remove(&label) {
                    c.insert(label);
                }
            }
            let nodes = raw_nodes.lock().unwrap();
            let rows = schema_display_rows(
                &nodes,
                &expanded_tables.lock().unwrap(),
                &collapsed_categories.borrow(),
                &loaded_dbs.lock().unwrap(),
                engine,
                &sidebar_filter.lock().unwrap(),
            );
            w.set_schema_tree(ModelRc::from(Rc::new(VecModel::from(rows))));
        });
    }

    // ----- expand a table's columns inline (single-click) -----
    {
        let weak = window.as_weak();
        let raw_nodes = raw_nodes.clone();
        let expanded_tables = expanded_tables.clone();
        let loaded_dbs = loaded_dbs.clone();
        let collapsed_categories = collapsed_categories.clone();
        let cur_engine = cur_engine.clone();
        let sidebar_filter = sidebar_filter.clone();
        window.on_toggle_fields(move |db, label| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let engine = *cur_engine.borrow();
            // ponytail: only SQL tables carry inline fields. Mongo/Redis/Cassandra
            // leaves have none, so single-click keeps opening them — delegate to the
            // existing open-table handler instead of duplicating its browse setup.
            if !matches!(
                engine,
                Some(rdb_connstore::Engine::Postgres)
                    | Some(rdb_connstore::Engine::MySql)
                    | Some(rdb_connstore::Engine::Sqlite)
            ) {
                w.invoke_open_table(db, label);
                return;
            }
            let label = label.to_string();
            {
                let mut e = expanded_tables.lock().unwrap();
                if !e.remove(&label) {
                    e.insert(label);
                }
            }
            let nodes = raw_nodes.lock().unwrap();
            let rows = schema_display_rows(
                &nodes,
                &expanded_tables.lock().unwrap(),
                &collapsed_categories.borrow(),
                &loaded_dbs.lock().unwrap(),
                engine,
                &sidebar_filter.lock().unwrap(),
            );
            w.set_schema_tree(ModelRc::from(Rc::new(VecModel::from(rows))));
        });
    }

    // ----- sidebar tree filter -----
    {
        let weak = window.as_weak();
        let raw_nodes = raw_nodes.clone();
        let expanded_tables = expanded_tables.clone();
        let loaded_dbs = loaded_dbs.clone();
        let collapsed_categories = collapsed_categories.clone();
        let cur_engine = cur_engine.clone();
        let sidebar_filter = sidebar_filter.clone();
        window.on_filter_tree(move |text| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            *sidebar_filter.lock().unwrap() = text.to_string();
            let nodes = raw_nodes.lock().unwrap();
            let rows = schema_display_rows(
                &nodes,
                &expanded_tables.lock().unwrap(),
                &collapsed_categories.borrow(),
                &loaded_dbs.lock().unwrap(),
                *cur_engine.borrow(),
                &sidebar_filter.lock().unwrap(),
            );
            w.set_schema_tree(ModelRc::from(Rc::new(VecModel::from(rows))));
        });
    }
}
