//! Command palette and the settings-modal toggles (theme, update check,
//! sidebar side, font size, history retention, NoSQL collection cap, query tab
//! scope, auto table alias, query-error highlight).
//!
//! Split out of `main`; the handler bodies are unchanged.

use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::*;

pub(crate) fn wire(window: &MainWindow, state: &AppState, fns: &AppFns) {
    let AppState {
        store,
        settings,
        history_cap,
        panes,
        recent_queries,
        saved_queries,
        current,
        cur_engine,
        rt,
        raw_nodes,
        expanded_tables,
        loaded_dbs,
        workspace_tabs,
        active_tab_id,
        active_group1_tab_id,
        ..
    } = state.clone();
    let restore_tab = fns.restore_tab.clone();
    let rebuild_query_tree = fns.rebuild_query_tree.clone();

    // ----- palette toggle -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        let saved_queries = saved_queries.clone();
        let recent_queries = recent_queries.clone();
        window.on_toggle_palette(move || {
            if let Some(w) = weak.upgrade() {
                let opening = !w.get_palette_open();
                w.set_palette_open(opening);
                if opening {
                    let names = build_palette_conn_names(&store.borrow());
                    let (items, actions) = build_palette_items(
                        &names,
                        &w,
                        &saved_queries.borrow(),
                        &recent_queries.borrow(),
                        "",
                    );
                    w.set_palette_items(ModelRc::from(Rc::new(VecModel::from(
                        group_palette_items(items),
                    ))));
                    PALETTE_ACTIONS.with(|s| *s.borrow_mut() = actions);
                }
            }
        });
    }

    // ----- palette filter -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        let saved_queries = saved_queries.clone();
        let recent_queries = recent_queries.clone();
        window.on_palette_filter(move |q| {
            if let Some(w) = weak.upgrade() {
                let names = build_palette_conn_names(&store.borrow());
                let (items, actions) = build_palette_items(
                    &names,
                    &w,
                    &saved_queries.borrow(),
                    &recent_queries.borrow(),
                    &q.to_lowercase(),
                );
                w.set_palette_items(ModelRc::from(Rc::new(VecModel::from(group_palette_items(
                    items,
                )))));
                PALETTE_ACTIONS.with(|s| *s.borrow_mut() = actions);
            }
        });
    }

    // ----- palette choose -----
    {
        let weak = window.as_weak();
        window.on_palette_choose(move |idx| {
            if let Some(w) = weak.upgrade() {
                w.set_palette_open(false);
                let action = PALETTE_ACTIONS
                    .with(|s| s.borrow().get(idx.max(0) as usize).cloned())
                    .unwrap_or(PaletteAction::None);
                match action {
                    PaletteAction::None => {}
                    PaletteAction::Connect(i) => w.invoke_connect_clicked(i as i32),
                    PaletteAction::OpenTable(db, label) => w.invoke_open_table(db, label),
                    PaletteAction::OpenFunction(name) => w.invoke_open_function(name),
                    PaletteAction::OpenSavedQuery(name, idx) => {
                        w.invoke_open_query(SharedString::from(name), idx as i32)
                    }
                    PaletteAction::OpenRecent(idx) => {
                        w.invoke_open_query(SharedString::default(), idx as i32)
                    }
                }
            }
        });
    }

    // ----- theme mode -----
    //
    // Both handlers write only the mode. Darkness is derived in `tokens.slint`
    // from the mode plus the live OS appearance, and writing that derived
    // property from here would replace its binding permanently — severing the
    // link to the system theme with no way back. `Theme.dark` is read-only for
    // exactly that reason.
    let apply_mode = {
        let settings = settings.clone();
        move |w: &MainWindow, mode: rdb_connstore::ThemeMode| {
            w.global::<Theme>().set_mode(mode.to_index());
            let _ = settings.borrow_mut().update(|s| s.theme = mode);
        }
    };

    // Header button: walks System -> Light -> Dark -> System, so every state is
    // reachable without opening Settings.
    {
        let weak = window.as_weak();
        let apply_mode = apply_mode.clone();
        window.on_cycle_theme(move || {
            if let Some(w) = weak.upgrade() {
                let next = (w.global::<Theme>().get_mode() + 1) % 3;
                apply_mode(&w, rdb_connstore::ThemeMode::from_index(next));
            }
        });
    }

    // Settings modal: picks a mode directly.
    {
        let weak = window.as_weak();
        let apply_mode = apply_mode.clone();
        window.on_set_theme_mode(move |i| {
            if let Some(w) = weak.upgrade() {
                apply_mode(&w, rdb_connstore::ThemeMode::from_index(i));
            }
        });
    }

    // ----- settings: check-for-updates toggle -----
    {
        let weak = window.as_weak();
        let settings = settings.clone();
        window.on_set_update_check(move |v| {
            let _ = settings.borrow_mut().update(|s| s.update_check = v);
            if let Some(w) = weak.upgrade() {
                w.set_update_check_enabled(v);
            }
        });
    }

    // ----- settings: sidebar-on-the-right toggle -----
    {
        let settings = settings.clone();
        window.on_set_sidebar_side(move |v| {
            let _ = settings
                .borrow_mut()
                .update(|s| s.ui_state.sidebar_right = v);
        });
    }
    // ----- settings: connection-scoped query tabs toggle -----
    {
        let weak = window.as_weak();
        let settings = settings.clone();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let active_group1_tab_id = active_group1_tab_id.clone();
        let restore_tab = restore_tab.clone();
        let restore_p1_tab = fns.restore_p1_tab.clone();
        window.on_set_query_tabs_by_connection(move |enabled| {
            let _ = settings
                .borrow_mut()
                .update(|s| s.ui_state.query_tabs_by_connection = enabled);
            let Some(w) = weak.upgrade() else {
                return;
            };
            w.set_query_tabs_by_connection(enabled);
            let (next, right_next) = {
                let tabs = workspace_tabs.lock().unwrap();
                let active = active_tab_id.lock().unwrap().clone();
                let next = active
                    .filter(|id| {
                        tabs.iter()
                            .find(|tab| tab.id == *id)
                            .is_some_and(|tab| workspace_tab_visible(&w, tab))
                    })
                    .or_else(|| {
                        tabs.iter()
                            .find(|tab| workspace_tab_visible(&w, tab))
                            .map(|tab| tab.id.clone())
                    });
                let right_active = active_group1_tab_id.lock().unwrap().clone();
                let right_next = right_active
                    .filter(|id| {
                        tabs.iter()
                            .find(|tab| tab.id == *id)
                            .is_some_and(|tab| tab.group == 1 && workspace_tab_visible(&w, tab))
                    })
                    .or_else(|| {
                        tabs.iter()
                            .find(|tab| tab.group == 1 && workspace_tab_visible(&w, tab))
                            .map(|tab| tab.id.clone())
                    });
                (next, right_next)
            };
            *active_tab_id.lock().unwrap() = next.clone();
            *active_group1_tab_id.lock().unwrap() = right_next.clone();
            let (index, right_index) = {
                let tabs = workspace_tabs.lock().unwrap();
                let index = next
                    .as_deref()
                    .and_then(|id| tabs.iter().position(|tab| tab.id == id));
                let right_index = right_next.as_deref().and_then(|id| {
                    tabs.iter()
                        .filter(|tab| tab.group == 1 && workspace_tab_visible(&w, tab))
                        .position(|tab| tab.id == id)
                });
                set_workspace_tabs(&w, &tabs, next.as_deref());
                (index, right_index)
            };
            if let Some(index) = index {
                restore_tab(&w, index);
            } else {
                clear_grid(&w, 0);
                w.set_results_meta(SharedString::default());
            }
            if let Some(index) = right_index {
                restore_p1_tab(&w, index);
            } else {
                w.set_p1_active_tab(-1);
            }
        });
    }

    // ----- app-wide zoom (⌘+ / ⌘−) -----
    {
        let weak = window.as_weak();
        let settings = settings.clone();
        window.on_zoom_step(move |step| {
            let current = settings.borrow().get().editor.font_size as i32;
            let level = clamp_font_size(current + step);
            let _ = settings
                .borrow_mut()
                .update(|s| s.editor.font_size = level as u16);
            if let Some(w) = weak.upgrade() {
                apply_zoom(&w, level);
            }
        });
    }

    // ----- settings: history retention limit -----
    {
        let weak = window.as_weak();
        let settings = settings.clone();
        let history_cap = history_cap.clone();
        let recent_queries = recent_queries.clone();
        let rebuild_query_tree = rebuild_query_tree.clone();
        window.on_set_history_max_entries(move |value| {
            let cap = match value {
                25 | 50 | 100 | 200 => value as usize,
                _ => RECENT_CAP,
            };
            history_cap.set(cap);
            recent_queries.borrow_mut().truncate(cap);
            let _ = settings
                .borrow_mut()
                .update(|s| s.editor.history_max_entries = cap as u16);
            if !mock::mock_mode() {
                save_recent(&recent_queries.borrow());
            }
            if let Some(w) = weak.upgrade() {
                w.set_history_max_entries(cap as i32);
                rebuild_query_tree("");
            }
        });
    }
    // ----- NoSQL collection-limit setting (MongoDB sidebar cap) -----
    {
        let weak = window.as_weak();
        let rt = rt.clone();
        let current = current.clone();
        let raw_nodes = raw_nodes.clone();
        let expanded_tables = expanded_tables.clone();
        let loaded_dbs = loaded_dbs.clone();
        let cur_engine = cur_engine.clone();
        let settings = settings.clone();
        window.on_set_nosql_collection_limit(move |value| {
            let n = match value {
                50 | 100 | 200 | 500 | 1000 => value as usize,
                _ => 200,
            };
            let _ = settings
                .borrow_mut()
                .update(|s| s.nosql_collection_limit = n as u32);
            let Some(w) = weak.upgrade() else {
                return;
            };
            w.set_nosql_collection_limit(n as i32);
            // Only MongoDB has a sidebar collection cap; nothing else to refresh.
            if !matches!(*cur_engine.borrow(), Some(rdb_connstore::Engine::Mongo)) {
                return;
            }
            let schema_name = w.get_schema_name().to_string();
            if schema_name.is_empty() {
                return;
            }
            // Push the new cap onto the live driver and refetch the open
            // database's collections so the change shows immediately.
            w.set_tree_loading(true);
            let weak2 = weak.clone();
            let current = current.clone();
            let raw_nodes = raw_nodes.clone();
            let expanded_tables = expanded_tables.clone();
            let loaded_dbs = loaded_dbs.clone();
            rt.spawn(async move {
                let clear_loading = move |weak: slint::Weak<MainWindow>| {
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = weak.upgrade() {
                            w.set_tree_loading(false);
                        }
                    });
                };
                let driver = {
                    let guard = current.lock().await;
                    guard.as_ref().map(|(_, d)| d.clone())
                };
                let Some(driver) = driver else {
                    clear_loading(weak2);
                    return;
                };
                driver.set_collection_limit(n);
                let Ok(schema) = driver.schema_for(&schema_name).await else {
                    clear_loading(weak2);
                    return;
                };
                let nodes = model::to_tree_model(&schema);
                let (exp, loaded) = {
                    let mut e = expanded_tables.lock().unwrap();
                    let mut l = loaded_dbs.lock().unwrap();
                    e.insert(schema_name.clone());
                    l.insert(schema_name.clone());
                    (e.clone(), l.clone())
                };
                let rows = schema_display_rows(
                    &nodes,
                    &exp,
                    &default_collapsed_cats(),
                    &loaded,
                    Some(rdb_connstore::Engine::Mongo),
                    "",
                );
                *raw_nodes.lock().unwrap() = nodes;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak2.upgrade() {
                        w.set_schema_tree(ModelRc::from(Rc::new(VecModel::from(rows))));
                        w.set_tree_loading(false);
                    }
                });
            });
        });
    }

    // ----- settings: auto-table-alias toggle -----
    {
        let settings = settings.clone();
        window.on_set_auto_table_alias(move |v| {
            let _ = settings
                .borrow_mut()
                .update(|s| s.editor.auto_table_alias = v);
        });
    }

    // ----- settings: query-error-highlight toggle -----
    {
        let settings = settings.clone();
        let panes = panes.clone();
        let w = window.as_weak();
        window.on_set_error_highlight(move |v| {
            let _ = settings
                .borrow_mut()
                .update(|s| s.editor.error_highlight = v);
            let Some(w) = w.upgrade() else { return };
            // Repaint both panes from the marks already held in pane state so
            // the change shows without waiting for the next failed run.
            for pane in 0..2 {
                let mark = *panes[pane].error_mark.lock().unwrap();
                set_p_error_mark(&w, pane, mark);
            }
        });
    }
}
