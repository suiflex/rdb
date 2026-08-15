//! Running queries from the UI: the Run button, re-running or inserting a
//! history entry, the saved-query list and its context menu, running into the
//! right split pane, the Explain button and the Format button.
//!
//! The runners themselves (`run_sql`, `run_stream`, `run_browse`) stay in
//! `main` — they are built from the driver slot and handed here through
//! [`AppFns`]. Split out of `main`; the handler bodies are unchanged.

use slint::{ComponentHandle, SharedString};

use crate::*;

pub(crate) fn wire(window: &MainWindow, state: &AppState, fns: &AppFns) {
    let AppState {
        store,
        panes,
        cur_engine,
        workspace_tabs,
        active_tab_id,
        current_connection_id,
        recent_queries,
        saved_queries,
        history_cap,
        ..
    } = state.clone();
    let AppFns {
        rebuild_query_tree,
        run_sql,
        run_stream,
        sync_editor,
        ..
    } = fns.clone();
    let ed_state = panes[0].ed_state.clone();

    // ----- run query (editor) -----
    {
        let weak = window.as_weak();
        let run_sql = run_sql.clone();
        let run_stream = run_stream.clone();
        let cur_engine = cur_engine.clone();
        let ed_state = ed_state.clone();
        let recent_queries = recent_queries.clone();
        let history_cap = history_cap.clone();
        let rebuild_query_tree = rebuild_query_tree.clone();
        let panes = panes.clone();
        let store = store.clone();
        let current_connection_id = current_connection_id.clone();
        window.on_run_query(move || {
            if let Some(w) = weak.upgrade() {
                // ⌘⏎ / Run: the highlighted selection, else just the statement
                // under the cursor — so a buffer with several statements runs
                // only the one the user is editing (⌘A then Run for all).
                let text = {
                    let ed = ed_state.borrow();
                    // Where that text sits in the buffer, so a driver error
                    // position reported against it maps to the right line.
                    panes[0].pending_run_origin.set(ed.run_origin());
                    ed.selected_text()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| ed.current_statement())
                };
                if text.is_empty() {
                    return;
                }
                record_recent(
                    &recent_queries,
                    &text,
                    history_cap.get(),
                    cur_engine.borrow().map(AnyDriver::badge).map(String::from),
                    resolve_conn_color(&current_connection_id, &store),
                );
                if w.get_sidebar_mode() == 2 {
                    rebuild_query_tree("");
                }
                // A manual query result has no row identity — never editable.
                // (The browse path re-enables editing after its PK fetch.)
                if active_tab_kind(&w) != "table" {
                    w.set_grid_read_only(true);
                }
                // SQL engines never carry an injected LIMIT: a bare SELECT streams
                // the rows in progressively (cancelable, no artificial cap), and a
                // statement with its own LIMIT / a write runs buffered as written.
                let stream = active_tab_kind(&w) != "table"
                    && cur_engine
                        .borrow()
                        .map(|e| is_bare_select(e, &text))
                        .unwrap_or(false);
                if stream {
                    run_stream(0, text);
                } else {
                    // 2+ statements → one result tab each.
                    if active_tab_kind(&w) != "table" && editor::split_statements(&text).len() >= 2
                    {
                        panes[0]
                            .split_results
                            .store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    run_sql(0, text);
                }
                if w.get_sidebar_mode() != 0 {
                    rebuild_query_tree("");
                }
            }
        });
    }

    // ----- History: re-run an entry directly (fresh result, editor untouched) -----
    {
        let weak = window.as_weak();
        let run_sql = run_sql.clone();
        let run_stream = run_stream.clone();
        let cur_engine = cur_engine.clone();
        let recent_queries = recent_queries.clone();
        window.on_rerun_query(move |idx| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let Some(text) = recent_queries
                .borrow()
                .get(idx.max(0) as usize)
                .map(|e| e.sql.clone())
            else {
                return;
            };
            if text.trim().is_empty() {
                return;
            }
            // Same stream/buffered rule as a manual run; the result replaces the
            // grid but the editor buffer is left alone.
            if active_tab_kind(&w) != "table" {
                w.set_grid_read_only(true);
            }
            let stream = active_tab_kind(&w) != "table"
                && cur_engine
                    .borrow()
                    .map(|e| is_bare_select(e, &text))
                    .unwrap_or(false);
            if stream {
                run_stream(0, text);
            } else {
                run_sql(0, text);
            }
        });
    }

    // ----- History context menu: insert, run, copy, save to queries, or
    // remove one entry. -----
    {
        let weak = window.as_weak();
        let recent_queries = recent_queries.clone();
        let saved_queries = saved_queries.clone();
        let rebuild_query_tree = rebuild_query_tree.clone();
        window.on_history_action(move |idx, action| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let idx = idx.max(0) as usize;
            match action {
                0 => w.invoke_insert_query(idx as i32),
                1 => w.invoke_rerun_query(idx as i32),
                2 => {
                    if let Some(entry) = recent_queries.borrow().get(idx) {
                        clip_set(&entry.sql);
                    }
                }
                3 => {
                    // Promote a history entry into the curated Saved list.
                    let Some(sql) = recent_queries.borrow().get(idx).map(|e| e.sql.clone()) else {
                        return;
                    };
                    let name = derive_query_name(&sql, &saved_queries.borrow());
                    saved_queries.borrow_mut().push((name, sql));
                    if !mock::mock_mode() {
                        save_saved(&saved_queries.borrow());
                    }
                    rebuild_query_tree("");
                }
                4 if remove_recent(&recent_queries, idx) => {
                    if !mock::mock_mode() {
                        save_recent(&recent_queries.borrow());
                    }
                    rebuild_query_tree("");
                }
                _ => {}
            }
        });
    }

    // ----- History: append an entry to the editor (never replaces its text) -----
    {
        let weak = window.as_weak();
        let ed_state = ed_state.clone();
        let sync_editor = sync_editor.clone();
        let recent_queries = recent_queries.clone();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        window.on_insert_query(move |idx| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let Some(text) = recent_queries
                .borrow()
                .get(idx.max(0) as usize)
                .map(|e| e.sql.clone())
            else {
                return;
            };
            {
                let mut ed = ed_state.borrow_mut();
                let end_line = ed.lines.len().saturating_sub(1) as i32;
                let end_col = ed.lines.last().map(|l| l.chars().count()).unwrap_or(0) as i32;
                ed.move_to(end_line, end_col, false);
                // Blank line between what's there and the appended statement.
                let prefix = if ed.text().trim().is_empty() {
                    ""
                } else {
                    "\n\n"
                };
                ed.insert(&format!("{prefix}{text}"));
            }
            sync_editor(0);
            // Persist the grown buffer to the active tab.
            if let Some(id) = active_tab_id.lock().unwrap().clone() {
                let new_text = ed_state.borrow().text();
                let mut tabs = workspace_tabs.lock().unwrap();
                if let Some(tab) = tabs.iter_mut().find(|t| t.id == id) {
                    tab.query_text = new_text;
                }
            }
            let _ = w;
        });
    }

    // ----- Saved query ▶: run it directly (fresh result, editor untouched) -----
    {
        let weak = window.as_weak();
        let run_sql = run_sql.clone();
        let run_stream = run_stream.clone();
        let cur_engine = cur_engine.clone();
        let saved = saved_queries.clone();
        window.on_run_saved_query(move |idx| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let Some(text) = saved
                .borrow()
                .get(idx.max(0) as usize)
                .map(|(_, s)| s.clone())
            else {
                return;
            };
            if text.trim().is_empty() {
                return;
            }
            if active_tab_kind(&w) != "table" {
                w.set_grid_read_only(true);
            }
            let stream = active_tab_kind(&w) != "table"
                && cur_engine
                    .borrow()
                    .map(|e| is_bare_select(e, &text))
                    .unwrap_or(false);
            if stream {
                run_stream(0, text);
            } else {
                run_sql(0, text);
            }
        });
    }

    // ----- Saved query context menu: run, open in new tab, insert, copy,
    // delete (persisted). -----
    {
        let weak = window.as_weak();
        let saved = saved_queries.clone();
        let ed_state = ed_state.clone();
        let sync_editor = sync_editor.clone();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let rebuild_query_tree = rebuild_query_tree.clone();
        window.on_query_action(move |idx, action| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let idx = idx.max(0) as usize;
            let Some((name, sql)) = saved.borrow().get(idx).cloned() else {
                return;
            };
            match action {
                0 => w.invoke_run_saved_query(idx as i32),
                1 => {
                    // Force a fresh tab, then load the query into it.
                    w.invoke_new_tab();
                    w.invoke_open_query(SharedString::from(name), idx as i32);
                }
                2 => {
                    // Append to the editor, never clobbering what's there.
                    {
                        let mut ed = ed_state.borrow_mut();
                        let end_line = ed.lines.len().saturating_sub(1) as i32;
                        let end_col =
                            ed.lines.last().map(|l| l.chars().count()).unwrap_or(0) as i32;
                        ed.move_to(end_line, end_col, false);
                        let prefix = if ed.text().trim().is_empty() {
                            ""
                        } else {
                            "\n\n"
                        };
                        ed.insert(&format!("{prefix}{sql}"));
                    }
                    sync_editor(0);
                    if let Some(id) = active_tab_id.lock().unwrap().clone() {
                        let new_text = ed_state.borrow().text();
                        let mut tabs = workspace_tabs.lock().unwrap();
                        if let Some(tab) = tabs.iter_mut().find(|t| t.id == id) {
                            tab.query_text = new_text;
                        }
                    }
                }
                3 => {
                    clip_set(&sql);
                }
                4 => {
                    let removed = {
                        let mut list = saved.borrow_mut();
                        if idx < list.len() {
                            list.remove(idx);
                            true
                        } else {
                            false
                        }
                    };
                    if removed {
                        if !mock::mock_mode() {
                            save_saved(&saved.borrow());
                        }
                        rebuild_query_tree("");
                    }
                }
                _ => {}
            }
        });
    }

    // ----- run query in the right split pane -----
    {
        let run_sql = run_sql.clone();
        let run_stream = run_stream.clone();
        let panes = panes.clone();
        let cur_engine = cur_engine.clone();
        let weak = window.as_weak();
        let recent_queries = recent_queries.clone();
        let history_cap = history_cap.clone();
        let rebuild_query_tree = rebuild_query_tree.clone();
        let store = store.clone();
        let current_connection_id = current_connection_id.clone();
        let run_p1 = move || {
            let text = {
                let ed = panes[1].ed_state.borrow();
                panes[1].pending_run_origin.set(ed.run_origin());
                ed.selected_text()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| ed.current_statement())
            };
            if text.is_empty() {
                return;
            }
            record_recent(
                &recent_queries,
                &text,
                history_cap.get(),
                cur_engine.borrow().map(AnyDriver::badge).map(String::from),
                resolve_conn_color(&current_connection_id, &store),
            );
            if weak.upgrade().is_some_and(|w| w.get_sidebar_mode() == 2) {
                rebuild_query_tree("");
            }
            let stream = weak
                .upgrade()
                .map(|w| {
                    cur_engine
                        .borrow()
                        .map(|e| is_bare_select(e, &text))
                        .unwrap_or(false)
                        && active_tab_kind(&w) != "table"
                })
                .unwrap_or(false);
            if stream {
                run_stream(1, text);
            } else {
                run_sql(1, text);
            }
        };
        window.on_p1_run(run_p1);
    }

    // ----- Explain button: run EXPLAIN for the editor SQL -----
    {
        let weak = window.as_weak();
        let run_sql = run_sql.clone();
        let panes = panes.clone();
        window.on_explain_query(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if !w.get_sql_capable() {
                return;
            }
            let text = panes[0].ed_state.borrow().current_statement();
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return;
            }
            if active_tab_kind(&w) != "table" {
                w.set_grid_read_only(true);
            }
            if trimmed.to_uppercase().starts_with("EXPLAIN") {
                run_sql(0, trimmed.to_string());
            } else {
                run_sql(0, format!("EXPLAIN {trimmed}"));
            }
        });
    }
    {
        let weak = window.as_weak();
        let run_sql = run_sql.clone();
        let panes = panes.clone();
        window.on_p1_explain_query(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if !w.get_sql_capable() {
                return;
            }
            let text = panes[1].ed_state.borrow().current_statement();
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return;
            }
            w.set_p1_grid_read_only(true);
            run_sql(
                1,
                if trimmed.to_uppercase().starts_with("EXPLAIN") {
                    trimmed.to_string()
                } else {
                    format!("EXPLAIN {trimmed}")
                },
            );
        });
    }

    // ----- Format button: tidy the editor SQL in place -----
    {
        let panes = panes.clone();
        let sync_editor = sync_editor.clone();
        let cur_engine = cur_engine.clone();
        window.on_format_sql(move || {
            let language = cur_engine
                .borrow()
                .map(rdb_connstore::Engine::language)
                .unwrap_or(rdb_connstore::QueryLanguage::Sql);
            let changed = {
                let mut ed = panes[0].ed_state.borrow_mut();
                let stmt = ed.current_statement();
                match format::dispatch(language, &stmt) {
                    Some(formatted) if !stmt.trim().is_empty() => {
                        ed.replace_current_statement(&formatted);
                        true
                    }
                    _ => false,
                }
            };
            if changed {
                sync_editor(0);
            }
        });
    }
    {
        let panes = panes.clone();
        let sync_editor = sync_editor.clone();
        let cur_engine = cur_engine.clone();
        window.on_p1_format_sql(move || {
            let language = cur_engine
                .borrow()
                .map(rdb_connstore::Engine::language)
                .unwrap_or(rdb_connstore::QueryLanguage::Sql);
            let changed = {
                let mut ed = panes[1].ed_state.borrow_mut();
                let stmt = ed.current_statement();
                match format::dispatch(language, &stmt) {
                    Some(formatted) if !stmt.trim().is_empty() => {
                        ed.replace_current_statement(&formatted);
                        true
                    }
                    _ => false,
                }
            };
            if changed {
                sync_editor(1);
            }
        });
    }
}
