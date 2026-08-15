//! Command palette and the settings-modal toggles (theme, update check,
//! sidebar side, font size, history retention, NoSQL collection cap, auto table
//! alias, query-error highlight).
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
        ..
    } = state.clone();
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

    // ----- light/dark toggle -----
    {
        let weak = window.as_weak();
        let settings = settings.clone();
        window.on_toggle_theme(move || {
            if let Some(w) = weak.upgrade() {
                let t = w.global::<Theme>();
                let now = t.get_dark();
                t.set_dark(!now);
                let _ = settings
                    .borrow_mut()
                    .update(|s| s.theme = rdb_connstore::ThemeMode::from_dark(!now));
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

    // ----- app-wide font size -----
    {
        let weak = window.as_weak();
        let settings = settings.clone();
        window.on_set_font_size(move |value| {
            let size = clamp_font_size(value);
            let _ = settings
                .borrow_mut()
                .update(|s| s.editor.font_size = size as u16);
            if let Some(w) = weak.upgrade() {
                w.global::<Tokens>().set_font_base(size as f32);
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
