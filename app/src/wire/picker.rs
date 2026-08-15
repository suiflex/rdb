//! The connection picker and sidebar groups: selecting a connection into the
//! detail panel, the split-pane tab handlers, collapsing/deleting/renaming
//! groups, the search box, favourites, drag-reorder between groups,
//! disconnect, reconnect, and opening an external link.
//!
//! Split out of `main`; the handler bodies are unchanged.

use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::*;

pub(crate) fn wire(window: &MainWindow, state: &AppState, fns: &AppFns) {
    let AppState {
        rt,
        store,
        settings,
        panes,
        current,
        cur_engine,
        collapsed,
        conn_filter,
        raw_nodes,
        expanded_tables,
        loaded_dbs,
        collapsed_categories,
        workspace_tabs,
        active_tab_id,
        active_group1_tab_id,
        current_connection_id,
        query_number,
        conn_modal_map,
        ..
    } = state.clone();
    let AppFns {
        load_editor_text,
        save_active_tab,
        restore_tab,
        save_p1_tab,
        restore_p1_tab,
        ..
    } = fns.clone();

    // ----- connections screen: selection fills the right detail panel -----
    let fill_detail = {
        let weak = window.as_weak();
        let store = store.clone();
        move |idx: i32| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let store = store.borrow();
            let Some(s) = store.list().get(idx as usize) else {
                w.set_selected_conn(-1);
                return;
            };
            w.set_selected_conn(idx);
            w.set_sel_name(s.name.clone().into());
            w.set_sel_engine(AnyDriver::badge(s.engine).into());
            w.set_sel_color(theme::accent_or_default(s.color.as_deref().unwrap_or("")));
            w.set_sel_has_custom_color(s.color.is_some());
            let label = AnyDriver::label(s.engine);
            let gsub = group_sub_label(s.group.as_deref());
            let sub = if gsub.is_empty() {
                label.to_string()
            } else {
                format!("{label} · {gsub}")
            };
            w.set_sel_sub(sub.into());
            w.set_sel_local(s.local);
            w.set_sel_env_tag_label(theme::env_tag_label(s.env_tag).into());
            w.set_sel_env_tag_color(
                theme::env_tag_color(s.env_tag).unwrap_or_else(|| theme::accent_or_default("")),
            );
            let ssl = match s.sslmode {
                rdb_core::conn::SslMode::Disable => "disable",
                rdb_core::conn::SslMode::Prefer => "prefer",
                rdb_core::conn::SslMode::Require => "require",
            };
            let mut rows = vec![
                KvRow {
                    k: "Host".into(),
                    v: s.host.clone().into(),
                },
                KvRow {
                    k: "Port".into(),
                    v: s.port.to_string().into(),
                },
            ];
            if let Some(db) = &s.database {
                rows.push(KvRow {
                    k: "Database".into(),
                    v: db.clone().into(),
                });
            }
            rows.push(KvRow {
                k: "User".into(),
                v: s.user.clone().into(),
            });
            rows.push(KvRow {
                k: "SSL".into(),
                v: ssl.into(),
            });
            if mock::mock_mode() && s.engine == rdb_connstore::Engine::Postgres {
                rows.push(KvRow {
                    k: "Server".into(),
                    v: "PostgreSQL 16.14".into(),
                });
            }
            w.set_sel_rows(ModelRc::from(Rc::new(VecModel::from(rows))));
            let tags: Vec<SharedString> = s.tags.iter().map(|t| t.as_str().into()).collect();
            w.set_sel_tags(ModelRc::from(Rc::new(VecModel::from(tags))));
            let footer = if mock::mock_mode() {
                "Terakhir terhubung · 2 menit lalu · RDB 1.2.0 open source".to_string()
            } else {
                format!("RDB {} open source", env!("CARGO_PKG_VERSION"))
            };
            w.set_sel_footer(footer.into());
        }
    };
    {
        let fill_detail = fill_detail.clone();
        window.on_select_conn(fill_detail);
    }
    // Mock mode boots with the reference selection ("bot ai tele").
    if mock::mock_mode() {
        let idx = store
            .borrow()
            .list()
            .iter()
            .position(|s| s.name == "bot ai tele")
            .map(|i| i as i32)
            .unwrap_or(-1);
        fill_detail(idx);
    }

    // RDB_SCREEN drives the app to a reference state for screenshots and the
    // e2e harness: "workspace" connects + opens emiten; "sql" opens + runs the
    // saved query; the rest open a specific modal/view.
    if let Ok(screen) = std::env::var("RDB_SCREEN") {
        let idx = store
            .borrow()
            .list()
            .iter()
            .position(|s| s.name == "bot ai tele")
            .map(|i| i as i32)
            .unwrap_or(0);
        // "connections" IS the pre-connect screen; connecting would swap it
        // for the workspace before the shot fires.
        if screen != "connections" {
            let weak = window.as_weak();
            let t1 = Box::leak(Box::new(slint::Timer::default()));
            t1.start(
                slint::TimerMode::SingleShot,
                std::time::Duration::from_millis(250),
                move || {
                    if let Some(w) = weak.upgrade() {
                        w.invoke_connect_clicked(idx);
                    }
                },
            );
        }
        if screen.starts_with("sql") {
            let weak = window.as_weak();
            let t2 = Box::leak(Box::new(slint::Timer::default()));
            t2.start(
                slint::TimerMode::SingleShot,
                std::time::Duration::from_millis(1800),
                move || {
                    if let Some(w) = weak.upgrade() {
                        w.set_sidebar_mode(1);
                        w.invoke_open_query("emiten-per-sektor".into(), 0);
                    }
                },
            );
            let weak = window.as_weak();
            let t3 = Box::leak(Box::new(slint::Timer::default()));
            t3.start(
                slint::TimerMode::SingleShot,
                std::time::Duration::from_millis(2300),
                move || {
                    if let Some(w) = weak.upgrade() {
                        w.invoke_run_query();
                    }
                },
            );
        }
        if screen == "modal-db"
            || screen == "modal-conn"
            || screen == "modal-add-mongo"
            || screen == "function"
            || screen == "palette"
        {
            let weak = window.as_weak();
            let which = screen.clone();
            let t4 = Box::leak(Box::new(slint::Timer::default()));
            t4.start(
                slint::TimerMode::SingleShot,
                std::time::Duration::from_millis(1800),
                move || {
                    if let Some(w) = weak.upgrade() {
                        match which.as_str() {
                            "modal-db" => w.invoke_open_db_modal(),
                            "modal-conn" => w.invoke_open_conn_modal(),
                            "modal-add-mongo" => {
                                w.invoke_open_add_form();
                                w.set_f_engine("MongoDB".into());
                                w.set_f_port("27017".into());
                                w.set_f_import_url("mongodb://root:secret@10.1.237.31:32343/admin?authMechanism=DEFAULT&replicaSet=production-rs".into());
                            }
                            "palette" => w.invoke_toggle_palette(),
                            _ => w.invoke_open_function("uuid_generate_v3".into()),
                        }
                    }
                },
            );
        }
        if screen.starts_with("workspace") {
            let weak = window.as_weak();
            let table = if screen.starts_with("workspace-users") {
                "users"
            } else {
                "emiten"
            };
            let t2 = Box::leak(Box::new(slint::Timer::default()));
            t2.start(
                slint::TimerMode::SingleShot,
                std::time::Duration::from_millis(1800),
                move || {
                    if let Some(w) = weak.upgrade() {
                        w.invoke_open_table("".into(), table.into());
                    }
                },
            );
        }

        // E2E editing scenarios layered on the base screens above. Fixed
        // delays race the async connect/browse pipeline, so each step fires
        // when its precondition is observable, polling every 100ms.
        let when = |cond: Rc<dyn Fn(&MainWindow) -> bool>, act: Rc<dyn Fn(&MainWindow)>| {
            let weak = window.as_weak();
            let t: &'static slint::Timer = Box::leak(Box::new(slint::Timer::default()));
            t.start(
                slint::TimerMode::Repeated,
                std::time::Duration::from_millis(100),
                move || {
                    let Some(w) = weak.upgrade() else {
                        t.stop();
                        return;
                    };
                    if cond(&w) {
                        t.stop();
                        act(&w);
                    }
                },
            );
        };
        use slint::Model as _;
        // grid loaded with an editable page (pk fetched)
        let grid_ready: Rc<dyn Fn(&MainWindow) -> bool> =
            Rc::new(|w| w.get_grid_cells().row_count() > 0 && !w.get_grid_read_only());
        let has_pending: Rc<dyn Fn(&MainWindow) -> bool> = Rc::new(|w| w.get_pending_count() > 0);
        if matches!(
            screen.as_str(),
            "workspace-dirty" | "workspace-guard" | "workspace-commit" | "workspace-tabnav"
        ) {
            when(
                grid_ready.clone(),
                Rc::new(|w| w.invoke_cell_edited(0, 2, "Bayan EDITED".into())),
            );
        }
        if screen == "workspace-active-commit" {
            when(
                grid_ready.clone(),
                Rc::new(|w| {
                    w.invoke_edit_cell(0, 2);
                    w.set_editing_value("Bayan ACTIVE SAVE".into());
                    w.invoke_commit_edits();
                }),
            );
        }
        if screen == "workspace-detail-commit" {
            when(
                grid_ready.clone(),
                Rc::new(|w| {
                    w.invoke_stage_cell(
                        0,
                        2,
                        "Bayan Resources — long detail value saved from the right panel".into(),
                    );
                    w.invoke_commit_edits();
                }),
            );
        }
        if screen == "workspace-long-edit" {
            when(
                grid_ready.clone(),
                Rc::new(|w| {
                    w.invoke_cell_edited(
                        0,
                        2,
                        "Bayan Resources — a deliberately long inline value remains visible while editing, wraps inside a bounded overlay, and still supports cursor navigation all the way to the final character."
                            .into(),
                    );
                    w.invoke_edit_cell(0, 2);
                }),
            );
        }
        if screen == "workspace-pointer-edit" {
            when(
                grid_ready.clone(),
                Rc::new(|w| {
                    use slint::platform::{PointerEventButton, WindowEvent};
                    let position = slint::LogicalPosition::new(650.0, 164.0);
                    for _ in 0..2 {
                        w.window().dispatch_event(WindowEvent::PointerPressed {
                            position,
                            button: PointerEventButton::Left,
                        });
                        w.window().dispatch_event(WindowEvent::PointerReleased {
                            position,
                            button: PointerEventButton::Left,
                        });
                    }
                    assert_eq!((w.get_editing_row(), w.get_editing_col()), (0, 2));
                }),
            );
        }
        if screen == "workspace-dirty" {
            // second pending change: a delete-marked row
            when(
                has_pending.clone(),
                Rc::new(|w| {
                    w.set_selected_row(2);
                    w.invoke_mark_delete();
                }),
            );
        }
        if screen == "workspace-guard" {
            // navigation with pending edits must be refused with a message
            when(has_pending.clone(), Rc::new(|w| w.invoke_next_page()));
        }
        if screen == "workspace-commit" {
            // full CRUD loop: buffer → WriteOps → mock commit → refetch
            when(has_pending.clone(), Rc::new(|w| w.invoke_commit_edits()));
        }
        if screen == "workspace-tabnav" {
            // Tab stores the edited cell and opens the neighbour's editor
            when(
                has_pending.clone(),
                Rc::new(|w| w.invoke_cell_advance(0, 3, "TABBED".into(), true)),
            );
        }
        if screen == "workspace-sql" {
            // Real UI transition: table browse → global SQL button. The fresh
            // query tab must not inherit the table request's loading state.
            when(
                Rc::new(|w| w.get_active_table() == "emiten"),
                Rc::new(|w| w.invoke_new_tab()),
            );
        }
        if screen == "workspace-tabflow" {
            when(
                Rc::new(|w| w.get_active_table() == "emiten" && w.get_tabs().row_count() == 1),
                Rc::new(|w| w.invoke_open_table("".into(), "referral_sources".into())),
            );
            when(
                Rc::new(|w| {
                    w.get_active_table() == "referral_sources" && w.get_tabs().row_count() == 1
                }),
                Rc::new(|w| {
                    w.invoke_pin_table("".into(), "referral_sources".into());
                    w.invoke_open_table("".into(), "sectors".into());
                }),
            );
            when(
                Rc::new(|w| w.get_active_table() == "sectors" && w.get_tabs().row_count() == 2),
                Rc::new(|w| w.invoke_new_tab()),
            );
        }
        if screen == "workspace-filter" {
            when(
                grid_ready.clone(),
                Rc::new(|w| {
                    w.set_data_filter_open(true);
                    w.set_filter_col("name".into());
                    w.set_filter_op("ILIKE".into());
                    w.set_grid_filter("mitra".into());
                    w.invoke_apply_filter();
                }),
            );
        }
        if screen == "workspace-limit" {
            when(
                grid_ready.clone(),
                Rc::new(|w| w.invoke_set_limit("25".into())),
            );
        }
        if screen == "workspace-insert" {
            when(grid_ready.clone(), Rc::new(|w| w.invoke_add_row()));
        }
        if screen == "workspace-users-bool" || screen == "workspace-users-date" {
            when(grid_ready.clone(), Rc::new(|w| w.invoke_add_row()));
            let date = screen == "workspace-users-date";
            when(
                has_pending.clone(),
                Rc::new(move |w| {
                    let rows = w.get_grid_cells().row_count() / w.get_grid_col_count() as usize;
                    w.invoke_edit_cell(rows.saturating_sub(1) as i32, if date { 4 } else { 3 });
                }),
            );
        }
        if screen == "sql-select" {
            // ⌘A select-all: the whole query gets the selection tint
            when(
                Rc::new(|w| !w.get_query_text().trim().is_empty()),
                Rc::new(|w| {
                    w.invoke_editor_key("a".into(), true, false, false);
                }),
            );
        }
        if screen == "sql-empty" {
            // a query with zero rows shows the empty state, not a blank pane
            let load = load_editor_text.clone();
            when(
                Rc::new(|w| !w.get_results_meta().is_empty()),
                Rc::new(move |w| {
                    load(0, "SELECT * FROM emiten OFFSET 99999");
                    w.invoke_run_query();
                }),
            );
        }
        if screen == "sql-find" {
            // ⌘F find bar: highlights the first match of a term
            when(
                Rc::new(|w| !w.get_query_text().trim().is_empty()),
                Rc::new(|w| {
                    w.invoke_toggle_find();
                    w.set_find_text("sector".into());
                    w.invoke_find_changed("sector".into());
                }),
            );
        }
        if screen == "sql-multi" {
            // multi-statement run: status reads "N statements · …"
            let load = load_editor_text.clone();
            when(
                Rc::new(|w| !w.get_results_meta().is_empty()),
                Rc::new(move |w| {
                    load(0, "SELECT 1;\nSELECT * FROM emiten LIMIT 5;");
                    w.invoke_run_query();
                }),
            );
        }
    }

    // Last result view kept in memory so the client-side filter (Feature C)
    // can re-derive the visible rows without re-querying. Arc<Mutex<>> (not Rc)
    // so it can cross into the Send event-loop closure from the query task.

    {
        let restore_p1_tab = restore_p1_tab.clone();
        let save_p1_tab = save_p1_tab.clone();
        let weak = window.as_weak();
        window.on_select_p1_tab(move |index| {
            if let Some(w) = weak.upgrade() {
                save_p1_tab(&w);
                restore_p1_tab(&w, index.max(0) as usize);
            }
        });
    }
    {
        let weak = window.as_weak();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let active_group1_tab_id = active_group1_tab_id.clone();
        let current_connection_id = current_connection_id.clone();
        let query_number = query_number.clone();
        let save_p1_tab = save_p1_tab.clone();
        let restore_p1_tab = restore_p1_tab.clone();
        let store = store.clone();
        window.on_new_tab_in_group(move |group| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if group == 0 {
                w.invoke_new_tab();
                return;
            }
            save_p1_tab(&w);
            let number = query_number.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            let connection = current_connection_id
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_default();
            let id = format!("query:{connection}:{number}");
            let badge = connection_badge_info(&store.borrow(), &connection);
            let right_index = {
                let mut tabs = workspace_tabs.lock().unwrap();
                let mut tab = WorkspaceTab::sql(id.clone(), number);
                tab.group = 1;
                tab.connection_id = (!connection.is_empty()).then(|| connection.clone());
                tab.engine = badge.engine;
                tab.connection_name = badge.name;
                tab.color = badge.color;
                tab.has_custom_color = badge.has_custom_color;
                tabs.push(tab);
                let left = active_tab_id.lock().unwrap().clone();
                set_workspace_tabs(&w, &tabs, left.as_deref());
                save_query_tabs(&w, &tabs, left.as_deref());
                tabs.iter().filter(|tab| tab.group == 1).count() - 1
            };
            *active_group1_tab_id.lock().unwrap() = Some(id);
            restore_p1_tab(&w, right_index);
        });
    }
    {
        let weak = window.as_weak();
        let tabs = workspace_tabs.clone();
        let save_active_tab = save_active_tab.clone();
        let save_p1_tab = save_p1_tab.clone();
        let restore_tab = restore_tab.clone();
        let restore_p1_tab = restore_p1_tab.clone();
        let active_group1_tab_id = active_group1_tab_id.clone();
        let active_tab_id = active_tab_id.clone();
        window.on_move_tab_group(move |index, target| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let target = target.clamp(0, 1) as usize;
            save_active_tab(&w);
            save_p1_tab(&w);
            let source = if target == 1 { 0 } else { 1 };
            // Whichever tab was active in each pane before this move, so a
            // split (or moving one back) doesn't jump focus to the first tab
            // in that pane — it only needs to change when the tab that just
            // moved WAS the active one.
            let prev_left = active_tab_id.lock().unwrap().clone();
            let prev_right = active_group1_tab_id.lock().unwrap().clone();
            let moved_id = {
                let mut tabs = tabs.lock().unwrap();
                if source == 0 && tabs.iter().filter(|tab| tab.group == 0).count() == 1 {
                    return;
                }
                let Some(tab) = tabs
                    .iter_mut()
                    .filter(|tab| tab.group == source)
                    .nth(index.max(0) as usize)
                else {
                    return;
                };
                tab.group = target;
                tab.id.clone()
            };
            let (left_index, right_index, left_id, right_id) = {
                let tabs = tabs.lock().unwrap();
                // The pane the tab just landed in follows it; the other pane
                // keeps its previously-active tab if it's still there.
                let left_id = if target == 0 {
                    Some(moved_id.clone())
                } else {
                    prev_left.filter(|id| tabs.iter().any(|t| t.group == 0 && &t.id == id))
                }
                .or_else(|| tabs.iter().find(|t| t.group == 0).map(|t| t.id.clone()));
                let right_id = if target == 1 {
                    Some(moved_id.clone())
                } else {
                    prev_right.filter(|id| tabs.iter().any(|t| t.group == 1 && &t.id == id))
                }
                .or_else(|| tabs.iter().find(|t| t.group == 1).map(|t| t.id.clone()));
                let left_index = left_id
                    .as_ref()
                    .and_then(|id| tabs.iter().position(|t| &t.id == id));
                let right_index = right_id.as_ref().and_then(|id| {
                    tabs.iter()
                        .filter(|t| t.group == 1)
                        .position(|t| &t.id == id)
                });
                set_workspace_tabs(&w, &tabs, left_id.as_deref());
                (left_index, right_index, left_id, right_id)
            };
            *active_tab_id.lock().unwrap() = left_id;
            if let Some(index) = left_index {
                restore_tab(&w, index);
            }
            if let (Some(id), Some(index)) = (right_id, right_index) {
                *active_group1_tab_id.lock().unwrap() = Some(id);
                restore_p1_tab(&w, index);
            } else {
                *active_group1_tab_id.lock().unwrap() = None;
                w.set_p1_active_tab(-1);
            }
        });
    }

    // ----- toggle a sidebar group's collapsed state (Feature A) -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        let settings = settings.clone();
        let conn_modal_map = conn_modal_map.clone();
        window.on_toggle_group(move |g| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let g = g.to_string();
            {
                let mut c = collapsed.borrow_mut();
                if !c.remove(&g) {
                    c.insert(g);
                }
            }
            // Persist the new collapsed set (best-effort; a write failure must
            // not break the UI).
            let groups: Vec<String> = collapsed.borrow().iter().cloned().collect();
            let _ = settings
                .borrow_mut()
                .update(|s| s.ui_state.collapsed_groups = groups);
            w.set_connections(build_sidebar_model(
                &store.borrow(),
                &collapsed.borrow(),
                &conn_filter.borrow(),
            ));
            // Keep the ⌘O modal in sync too, whichever surface triggered this.
            if w.get_conn_modal_open() {
                let (items, map) =
                    build_conn_palette_items(&store.borrow(), &collapsed.borrow(), "");
                *conn_modal_map.borrow_mut() = map;
                w.set_conn_items(ModelRc::from(Rc::new(VecModel::from(group_palette_items(
                    items,
                )))));
            }
        });
    }

    // ----- delete a group: its connections + descendant subfolders promote
    // one level up (a top-level folder's members fall back to Ungrouped,
    // same as before nesting existed) -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        let settings = settings.clone();
        window.on_group_delete(move |g| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let g = g.to_string();
            {
                let mut st = store.borrow_mut();
                let members: Vec<(String, String)> = st
                    .list()
                    .iter()
                    .filter_map(|s| {
                        let grp = s.group.as_deref()?;
                        rdb_connstore::is_descendant(grp, &g)
                            .then(|| (s.id.clone(), grp.to_string()))
                    })
                    .collect();
                for (id, grp) in members {
                    if let Some(mut sc) = st.get(&id).cloned() {
                        sc.group = cascade_delete_group(&g, &grp);
                        let _ = st.update(sc);
                    }
                }
            }
            collapsed
                .borrow_mut()
                .retain(|p| !rdb_connstore::is_descendant(p, &g));
            let groups: Vec<String> = collapsed.borrow().iter().cloned().collect();
            let _ = settings
                .borrow_mut()
                .update(|s| s.ui_state.collapsed_groups = groups);
            w.set_connections(build_sidebar_model(
                &store.borrow(),
                &collapsed.borrow(),
                &conn_filter.borrow(),
            ));
        });
    }

    // ----- rename a group: every member connection and descendant subfolder
    // follows, prefix-replaced -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        let settings = settings.clone();
        window.on_group_rename(move |old, new| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let old = old.to_string();
            let Some(new) = rdb_connstore::normalize_group_path(&new) else {
                return;
            };
            // Also rejects a no-op rename (new == old counts as its own
            // descendant) and renaming a folder into its own subtree, which
            // would otherwise create a cycle.
            if rdb_connstore::is_descendant(&new, &old) {
                return;
            }
            {
                let mut st = store.borrow_mut();
                let members: Vec<(String, String)> = st
                    .list()
                    .iter()
                    .filter_map(|s| {
                        let grp = s.group.as_deref()?;
                        rdb_connstore::is_descendant(grp, &old)
                            .then(|| (s.id.clone(), grp.to_string()))
                    })
                    .collect();
                for (id, grp) in members {
                    if let Some(mut sc) = st.get(&id).cloned() {
                        sc.group = Some(cascade_rename_group(&old, &new, &grp));
                        let _ = st.update(sc);
                    }
                }
            }
            // Carry the collapsed state over so renaming doesn't silently
            // re-expand a group (or one of its subfolders) the user had
            // folded shut.
            {
                let mut c = collapsed.borrow_mut();
                let renamed: Vec<String> = c
                    .iter()
                    .filter(|p| rdb_connstore::is_descendant(p, &old))
                    .cloned()
                    .collect();
                for p in renamed {
                    c.remove(&p);
                    c.insert(cascade_rename_group(&old, &new, &p));
                }
            }
            let groups: Vec<String> = collapsed.borrow().iter().cloned().collect();
            let _ = settings
                .borrow_mut()
                .update(|s| s.ui_state.collapsed_groups = groups);
            w.set_connections(build_sidebar_model(
                &store.borrow(),
                &collapsed.borrow(),
                &conn_filter.borrow(),
            ));
        });
    }

    // ----- connection-picker search (filter the connection list) -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        window.on_conn_filter(move |t| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            *conn_filter.borrow_mut() = t.to_string();
            w.set_connections(build_sidebar_model(
                &store.borrow(),
                &collapsed.borrow(),
                &conn_filter.borrow(),
            ));
        });
    }

    // ----- star/unstar a saved connection -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        window.on_toggle_favorite(move |idx| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let id = {
                let s = store.borrow();
                match s.list().get(idx as usize) {
                    Some(c) => (c.id.clone(), c.favorite),
                    None => return,
                }
            };
            let (id, was_fav) = id;
            let _ = store.borrow_mut().set_favorite(&id, !was_fav);
            w.set_connections(build_sidebar_model(
                &store.borrow(),
                &collapsed.borrow(),
                &conn_filter.borrow(),
            ));
        });
    }

    // ----- drag-reorder within a group, or drop onto a different group -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        window.on_reorder_conn(move |from_idx, delta, drop_y| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let group_key = |c: &rdb_connstore::SavedConnection| {
                c.group
                    .as_deref()
                    .filter(|g| !g.trim().is_empty())
                    .unwrap_or(UNGROUPED)
                    .to_string()
            };

            // Cross-group drop: the release point landed on a different
            // group's header or row than the dragged connection's own group.
            let rendered =
                build_conn_items(&store.borrow(), &collapsed.borrow(), &conn_filter.borrow());
            if let Some(target_group) = row_group_at_y(&rendered, drop_y) {
                let from_id = {
                    let s = store.borrow();
                    let Some(from) = s.list().get(from_idx as usize) else {
                        return;
                    };
                    if group_key(from) == target_group {
                        None
                    } else {
                        Some(from.id.clone())
                    }
                };
                if let Some(id) = from_id {
                    let sc = store.borrow().get(&id).cloned();
                    if let Some(mut sc) = sc {
                        sc.group = if target_group == UNGROUPED {
                            None
                        } else {
                            Some(target_group)
                        };
                        let _ = store.borrow_mut().update(sc);
                    }
                    w.set_connections(build_sidebar_model(
                        &store.borrow(),
                        &collapsed.borrow(),
                        &conn_filter.borrow(),
                    ));
                    return;
                }
            }

            // Same-group reorder: resolve the row-step delta against the
            // dragged connection's own group, using the same (favorite desc,
            // order asc) display order as the builder.
            if delta == 0 {
                return;
            }
            let (from_id, target_vec_idx) = {
                let s = store.borrow();
                let list = s.list();
                let Some(from) = list.get(from_idx as usize) else {
                    return;
                };
                let g = group_key(from);
                let mut members: Vec<usize> = list
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| group_key(c) == g)
                    .map(|(i, _)| i)
                    .collect();
                members.sort_by_key(|&i| (!list[i].favorite, list[i].order));
                let Some(pos) = members.iter().position(|&i| i == from_idx as usize) else {
                    return;
                };
                let target_pos =
                    (pos as i64 + delta as i64).clamp(0, members.len() as i64 - 1) as usize;
                if target_pos == pos {
                    return;
                }
                (from.id.clone(), members[target_pos])
            };
            let _ = store.borrow_mut().reorder(&from_id, target_vec_idx);
            w.set_connections(build_sidebar_model(
                &store.borrow(),
                &collapsed.borrow(),
                &conn_filter.borrow(),
            ));
        });
    }

    // ----- disconnect: drop the driver and return to the picker -----
    {
        let weak = window.as_weak();
        let current = current.clone();
        let rt = rt.clone();
        let cur_engine = cur_engine.clone();
        let expanded_tables = expanded_tables.clone();
        let loaded_dbs = loaded_dbs.clone();
        let collapsed_categories = collapsed_categories.clone();
        let raw_nodes = raw_nodes.clone();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let current_connection_id = current_connection_id.clone();
        let panes = panes.clone();
        window.on_disconnect(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            // Stop any in-flight query first: disconnecting must not leave a query
            // running on the server. Fire the same cancels the Cancel buttons use
            // for both panes — aborting the task drops its Arc<AnyDriver> clone so
            // the connection closes and the server terminates the query.
            for p in [0usize, 1] {
                if let Some(c) = panes[p].stream_cancel.borrow().as_ref() {
                    c.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                if let Some(h) = panes[p].query_abort.borrow_mut().take() {
                    h.abort();
                }
                set_p_query_running(&w, p, false);
                set_p_streaming(&w, p, false);
            }
            w.set_connected(false);
            w.set_selected_conn(-1);
            w.set_active_table(SharedString::default());
            w.set_status_conn(SharedString::from("no connection"));
            w.set_status_latency(SharedString::default());
            w.set_schema_tree(ModelRc::from(Rc::new(VecModel::<TreeNode>::default())));
            w.set_structure_columns(ModelRc::from(Rc::new(VecModel::<StructField>::default())));
            // Keep the SQL scratch tabs (they are connection-agnostic) so a later
            // reconnect can restore them; drop only the connection-scoped table /
            // function tabs whose data belongs to the connection being left.
            {
                let mut tabs = workspace_tabs.lock().unwrap();
                tabs.retain(|t| t.kind == "sql");
                for t in tabs.iter_mut() {
                    t.loading = false;
                }
                let keep_active = active_tab_id
                    .lock()
                    .unwrap()
                    .clone()
                    .filter(|id| tabs.iter().any(|t| t.id == *id))
                    .or_else(|| tabs.first().map(|t| t.id.clone()));
                *active_tab_id.lock().unwrap() = keep_active.clone();
                set_workspace_tabs(&w, &tabs, keep_active.as_deref());
            }
            *current_connection_id.lock().unwrap() = None;
            clear_grid(&w, 0);
            *cur_engine.borrow_mut() = None;
            expanded_tables.lock().unwrap().clear();
            loaded_dbs.lock().unwrap().clear();
            *collapsed_categories.borrow_mut() = default_collapsed_cats();
            raw_nodes.lock().unwrap().clear();
            let current = current.clone();
            rt.spawn(async move {
                *current.lock().await = None;
            });
        });
    }

    // ----- reconnect: retry the current connection after a health drop -----
    {
        let weak = window.as_weak();
        window.on_reconnect(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            // The health poll leaves selected_conn pointing at the live
            // connection, so replay the connect path against it.
            let idx = w.get_selected_conn();
            if idx >= 0 {
                w.invoke_connect_clicked(idx);
            }
        });
    }

    // ----- open an external link (Product Hunt / GitHub) in the browser -----
    {
        window.on_open_url(move |u| {
            let _ = open::that(u.as_str());
        });
    }
}
