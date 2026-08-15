//! The query runners: the buffered runner (parse the editor text for the live
//! engine, run it, push the result back) and the streaming runner used by
//! "No limit", which pulls rows from the driver in batches.
//!
//! `build` rather than `wire`: this installs no callbacks, it constructs the
//! two closures every query-triggering handler calls. `main` hands them on
//! through `AppFns`.
//!
//! Split out of `main`; the bodies are unchanged.

use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::*;

pub(crate) fn build(window: &MainWindow, state: &AppState) -> (PaneSqlFn, PaneSqlFn) {
    let AppState {
        rt,
        store,
        panes,
        current,
        workspace_tabs,
        active_tab_id,
        active_group1_tab_id,
        current_connection_id,
        query_console,
        last_view,
        ..
    } = state.clone();

    // ----- shared query runner: parse text for the live engine, run it, push
    // the result (grid / documents / error) back to the UI. Used by both the
    // Run button and table clicks. -----
    #[allow(clippy::type_complexity)]
    let run_sql: Rc<dyn Fn(usize, String)> = {
        let weak = window.as_weak();
        let rt = rt.clone();
        let current = current.clone();
        let current_connection_id = current_connection_id.clone();
        let store = store.clone();
        let last_view = last_view.clone();
        let panes = panes.clone();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let active_group1_tab_id = active_group1_tab_id.clone();
        let query_console = query_console.clone();
        Rc::new(move |pane: usize, sql: String| {
            // Per-pane live state; pane 1 (right split) uses its own buffers so a
            // run in one pane never disturbs the other.
            let browse = panes[pane].browse.clone();
            let edit_buf = panes[pane].edit_buf.clone();
            let displayed_grid = panes[pane].displayed_grid.clone();
            let hidden_cols = panes[pane].hidden_cols.clone();
            let sort_state = panes[pane].sort_state.clone();
            let col_order = panes[pane].col_order.clone();
            let col_filters = panes[pane].col_filters.clone();
            let results = panes[pane].results.clone();
            let active_result = panes[pane].active_result.clone();
            let result_new_tab = panes[pane].result_new_tab.clone();
            let error_mark_state = panes[pane].error_mark.clone();
            let run_origin = panes[pane].pending_run_origin.take();
            // New run clears any previous error highlight right away, so a
            // successful re-run never leaves stale red on the editor — the
            // (view/err) branches below only re-arm it on a fresh failure.
            *error_mark_state.lock().unwrap() = None;
            if let Some(w) = weak.upgrade() {
                set_p_error_mark(&w, pane, None);
            }
            let split_results = panes[pane].split_results.clone();
            let active_id = if pane == 1 {
                &active_group1_tab_id
            } else {
                &active_tab_id
            };
            let Some(target_id) = active_id.lock().unwrap().clone() else {
                return;
            };
            if let Some(tab) = workspace_tabs
                .lock()
                .unwrap()
                .iter_mut()
                .find(|tab| tab.id == target_id)
            {
                tab.loading = true;
                tab.query_text = sql.clone();
            }
            let weak2 = weak.clone();
            let current = current.clone();
            let last_view = last_view.clone();
            let browse = browse.clone();
            let edit_buf = edit_buf.clone();
            let displayed_grid = displayed_grid.clone();
            let hidden_cols = hidden_cols.clone();
            let sort_state = sort_state.clone();
            let col_order = col_order.clone();
            let col_filters = col_filters.clone();
            let results = results.clone();
            let active_result = active_result.clone();
            let workspace_tabs = workspace_tabs.clone();
            let active_tab_id = active_tab_id.clone();
            let active_group1_tab_id = active_group1_tab_id.clone();
            let query_console = query_console.clone();
            // ⌘\ set this; consume it so the next plain run replaces again.
            let new_tab = result_new_tab.swap(false, std::sync::atomic::Ordering::SeqCst);
            // A multi-statement run sets this; consume it likewise.
            let split = split_results.swap(false, std::sync::atomic::Ordering::SeqCst);
            // Currently selected database (top dropdown). Mongo line queries with
            // no `use(...)` run against it, matching what the user sees browsing.
            let mut cur_db = String::new();
            if let Some(w) = weak.upgrade() {
                set_p_query_running(&w, pane, true);
                // Don't force the SQL console open on every run — the eye toggle
                // owns its visibility. Re-opening it here ignored a user who just
                // hid it. The console still updates in place when it is open.
                cur_db = w.get_schema_name().to_string();
            }
            // Snapshot which connection is running THIS query — not read back
            // later from `current_connection_id`, since the user can switch the
            // active connection while the query is in flight.
            let query_connection_id = current_connection_id.lock().unwrap().clone();
            let query_badge = query_connection_id
                .as_deref()
                .map(|cid| connection_badge_info(&store.borrow(), cid))
                .unwrap_or_default();
            let started = std::time::Instant::now();
            let jh = rt.spawn(async move {
                let picked = {
                    let guard = current.lock().await;
                    guard.as_ref().map(|(e, d)| (*e, d.clone()))
                };
                let queue_ms = started.elapsed().as_millis() as u64;
                let driver_started = std::time::Instant::now();
                // Multi-statement: SQL engines split on top-level `;` and run
                // each in order, stopping at the first error. The last result
                // is what the grid shows (TablePlus semantics). Redis/Mongo
                // take the whole text as a single command.
                let mut split_views: Vec<model::ResultView> = Vec::new();
                let mut split_verbs: Vec<Option<&'static str>> = Vec::new();
                let stmt_offsets = editor::statement_offsets(&sql);
                let (outcome, n_stmts, last_stmt) = match picked.as_ref() {
                    Some((engine, driver)) => {
                        let stmts = if matches!(
                            engine,
                            rdb_connstore::Engine::Postgres
                                | rdb_connstore::Engine::MySql
                                | rdb_connstore::Engine::Sqlite
                                | rdb_connstore::Engine::Cassandra
                        ) {
                            editor::split_statements(&sql)
                        } else {
                            vec![sql.clone()]
                        };
                        let n = stmts.len().max(1);
                        // SQL engines are never auto-capped — the user's SELECT runs
                        // as written (bare reads take the streaming path instead).
                        // NoSQL keeps the row-limit control's value. Browse text
                        // carries its own LIMIT either way, so cap_select no-ops it.
                        let row_limit = if matches!(
                            engine,
                            rdb_connstore::Engine::Postgres
                                | rdb_connstore::Engine::MySql
                                | rdb_connstore::Engine::Sqlite
                                | rdb_connstore::Engine::Cassandra
                        ) {
                            0
                        } else {
                            browse.lock().unwrap().limit
                        };
                        let mut out = Err(rdb_core::error::RdbError::Query("empty query".into()));
                        for (i, s) in stmts.iter().enumerate() {
                            let s = cap_select(*engine, s, row_limit);
                            append_query_console(&query_console, s.clone());
                            out = match crate::query_parse::parse_query(*engine, &s) {
                                Ok(mut q) => {
                                    // Fill the selected database for a Mongo query
                                    // that didn't name one via `use(...)`.
                                    if let rdb_core::query::Query::Mongo(op) = &mut q {
                                        if op.database.is_none() && !cur_db.is_empty() {
                                            op.database = Some(cur_db.clone());
                                        }
                                    }
                                    driver.query(&q).await
                                }
                                Err(msg) => Err(rdb_core::error::RdbError::Query(msg)),
                            };
                            if let Err(e) = &out {
                                // Point the user at the offending statement.
                                if n > 1 {
                                    out = Err(rdb_core::error::RdbError::Query(format!(
                                        "statement {}/{n}: {e}",
                                        i + 1
                                    )));
                                }
                                break;
                            }
                            // Run Selection multi-tab: keep each statement's result.
                            if split {
                                if let Ok(rs) = &out {
                                    split_views.push(model::to_result_view(rs));
                                    split_verbs.push(model::sql_verb(&s));
                                }
                            }
                        }
                        (out, n, stmts.last().cloned())
                    }
                    None => (
                        Err(rdb_core::error::RdbError::Connection(
                            "not connected".into(),
                        )),
                        1,
                        None,
                    ),
                };
                let driver_ms = driver_started.elapsed().as_millis() as u64;
                let model_started = std::time::Instant::now();
                let view = outcome.as_ref().ok().map(model::to_result_view);
                let model_ms = model_started.elapsed().as_millis() as u64;
                let elapsed_ms = started.elapsed().as_millis().max(1) as u64;
                // Only a genuine `Ok(rs)` reaches here — the "error: ..." Affected
                // variant used elsewhere for failures is built on a separate path
                // that never calls `to_result_view`, so any Affected we see below
                // is always a real "N rows affected" from a successful mutation.
                let view = view.map(|v| match v {
                    model::ResultView::Affected(status) => {
                        let verb = last_stmt.as_deref().and_then(model::sql_verb);
                        model::ResultView::Affected(model::format_affected(
                            &status,
                            verb,
                            &model::format_latency(elapsed_ms),
                        ))
                    }
                    other => other,
                });
                let err = outcome.err();
                // The failing statement's error position/line is relative to
                // that statement's trimmed text; `error_spot` shifts it by the
                // statement's own byte offset so it lands on the right line.
                let error_spot = err
                    .as_ref()
                    .and_then(|e| editor::error_spot(&e.to_string(), &sql, &stmt_offsets));
                // `sql` is only the fragment Run sent; shift its line span back
                // into full-buffer coordinates.
                let error_mark_value = error_spot.as_ref().map(|s| mark_from(s, run_origin));
                // A hand-typed `SELECT ... FROM t` result has no row identity by
                // default. When it's an unambiguous single-table select (no
                // join/union/aggregate — see single_table_name) and the table's
                // PK columns come back in the result, treat it the same as a
                // browsed table: editable, PK-aware. Anything ambiguous just
                // stays read-only, same as before this existed.
                let pk_hint: Option<(rdb_core::write::TableRef, Vec<String>)> = if let (
                    Some((engine, driver)),
                    Some(model::ResultView::Table(g)),
                    Some(stmt),
                ) =
                    (picked.as_ref(), view.as_ref(), last_stmt.as_deref())
                {
                    if matches!(
                        engine,
                        rdb_connstore::Engine::Postgres
                            | rdb_connstore::Engine::MySql
                            | rdb_connstore::Engine::Sqlite
                    ) {
                        if let Some((qualifier, name)) = crate::query_parse::single_table_name(stmt)
                        {
                            // The query's own `schema.table`/`db.table` prefix wins
                            // when it named one explicitly; otherwise fall back to
                            // the connection's active schema/database selection.
                            let qualifier =
                                qualifier.or_else(|| (!cur_db.is_empty()).then(|| cur_db.clone()));
                            let table = rdb_core::write::TableRef {
                                database: (!matches!(engine, rdb_connstore::Engine::Postgres))
                                    .then(|| qualifier.clone())
                                    .flatten(),
                                schema: matches!(engine, rdb_connstore::Engine::Postgres)
                                    .then(|| qualifier.clone())
                                    .flatten(),
                                name,
                            };
                            match driver.primary_key(&table).await {
                                Ok(pk)
                                    if !pk.is_empty()
                                        && pk
                                            .iter()
                                            .all(|k| g.columns.iter().any(|c| &c.name == k)) =>
                                {
                                    Some((table, pk))
                                }
                                _ => None,
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak2.upgrade() {
                        sync_query_console(&w, &query_console);
                        let active_id = if pane == 1 {
                            &active_group1_tab_id
                        } else {
                            &active_tab_id
                        };
                        let is_active = active_id.lock().unwrap().as_deref() == Some(&target_id);
                        if is_active {
                            set_p_query_running(&w, pane, false);
                        }
                        match (view, err) {
                            (Some(v), _) => {
                                *error_mark_state.lock().unwrap() = None;
                                set_p_error_mark(&w, pane, None);
                                // Run Selection over 2+ statements: store one
                                // result tab per statement and show the first.
                                if split && split_views.len() >= 2 {
                                    let stored: Vec<StoredResult> = split_views
                                        .iter()
                                        .zip(split_verbs.iter())
                                        .map(|(vw, verb)| {
                                            let rows = match vw {
                                                model::ResultView::Table(g) => g.rows.len(),
                                                model::ResultView::Documents(d) => {
                                                    d.grid.rows.len()
                                                }
                                                model::ResultView::Affected(_) => 0,
                                            }
                                                as u64;
                                            let vw = match vw {
                                                model::ResultView::Affected(status) => {
                                                    model::ResultView::Affected(
                                                        model::format_affected(
                                                            status,
                                                            *verb,
                                                            &model::format_latency(elapsed_ms),
                                                        ),
                                                    )
                                                }
                                                other => other.clone(),
                                            };
                                            StoredResult {
                                                view: Arc::new(vw),
                                                meta: query_timing_meta(
                                                    rows, 1, elapsed_ms, queue_ms, driver_ms,
                                                    model_ms,
                                                ),
                                                latency: model::format_latency(elapsed_ms),
                                                grid: GridState::default(),
                                                connection_id: query_connection_id.clone(),
                                                engine: query_badge.engine.clone(),
                                                connection_name: query_badge.name.clone(),
                                                color: query_badge.color,
                                                has_custom_color: query_badge.has_custom_color,
                                            }
                                        })
                                        .collect();
                                    let (first, all) = {
                                        let mut tabs = workspace_tabs.lock().unwrap();
                                        let Some(tab) =
                                            tabs.iter_mut().find(|tab| tab.id == target_id)
                                        else {
                                            return;
                                        };
                                        tab.loading = false;
                                        tab.results = stored;
                                        tab.active_result = 0;
                                        tab.view = tab.results.first().cloned();
                                        (tab.results[0].clone(), tab.results.clone())
                                    };
                                    if !is_active {
                                        return;
                                    }
                                    *active_result.lock().unwrap() = 0;
                                    set_result_tabs(&w, pane, &all, 0);
                                    *results.lock().unwrap() = all;
                                    present_view(
                                        &w,
                                        pane,
                                        &first.view,
                                        &first.meta,
                                        &first.latency,
                                        &last_view,
                                        &displayed_grid,
                                        &hidden_cols,
                                        &sort_state,
                                        &col_order,
                                        &col_filters,
                                        &edit_buf,
                                        &browse,
                                    );
                                    return;
                                }
                                let shown = match &v {
                                    model::ResultView::Table(g) => g.rows.len(),
                                    model::ResultView::Documents(d) => d.grid.rows.len(),
                                    model::ResultView::Affected(_) => 0,
                                } as u64;
                                let meta = query_timing_meta(
                                    shown, n_stmts, elapsed_ms, queue_ms, driver_ms, model_ms,
                                );
                                let latency = model::format_latency(elapsed_ms);
                                let sr = StoredResult {
                                    view: Arc::new(v.clone()),
                                    meta: meta.clone(),
                                    latency: latency.clone(),
                                    grid: GridState::default(),
                                    connection_id: query_connection_id.clone(),
                                    engine: query_badge.engine.clone(),
                                    connection_name: query_badge.name.clone(),
                                    color: query_badge.color,
                                    has_custom_color: query_badge.has_custom_color,
                                };
                                let mut tabs = workspace_tabs.lock().unwrap();
                                let Some(tab) = tabs.iter_mut().find(|tab| tab.id == target_id)
                                else {
                                    return;
                                };
                                tab.loading = false;
                                tab.view = Some(sr.clone());
                                let is_browse = tab.kind == "table";
                                if is_browse {
                                    tab.results.clear();
                                    tab.active_result = 0;
                                } else {
                                    // ⌘\ opens a new result tab; snapshot the
                                    // outgoing result's live grid (sort/filters/
                                    // search) so returning to it keeps its state.
                                    if new_tab && is_active {
                                        if let Some(r) = tab.results.get_mut(tab.active_result) {
                                            let mut hidden: Vec<usize> = hidden_cols
                                                .lock()
                                                .unwrap()
                                                .iter()
                                                .copied()
                                                .collect();
                                            hidden.sort_unstable();
                                            r.grid = GridState {
                                                col_filters: col_filters.lock().unwrap().clone(),
                                                sort: *sort_state.lock().unwrap(),
                                                hidden,
                                                col_order: col_order.lock().unwrap().clone(),
                                                col_widths: get_p_col_widths(&w, pane),
                                                grid_filter: w.get_grid_filter().to_string(),
                                                filter_col: w.get_filter_col().to_string(),
                                                filter_op: w.get_filter_op().to_string(),
                                            };
                                        }
                                    }
                                    store_result(
                                        &mut tab.results,
                                        &mut tab.active_result,
                                        sr,
                                        new_tab,
                                    );
                                }
                                let tab_results = tab.results.clone();
                                let tab_active = tab.active_result;
                                if !is_active {
                                    return;
                                }
                                *results.lock().unwrap() = tab_results;
                                *active_result.lock().unwrap() = tab_active;
                                if is_browse {
                                    set_result_tabs(&w, pane, &[], 0);
                                } else {
                                    set_result_tabs(&w, pane, &results.lock().unwrap(), tab_active);
                                }
                                present_view(
                                    &w,
                                    pane,
                                    &v,
                                    &meta,
                                    &latency,
                                    &last_view,
                                    &displayed_grid,
                                    &hidden_cols,
                                    &sort_state,
                                    &col_order,
                                    &col_filters,
                                    &edit_buf,
                                    &browse,
                                );
                                // present_view seeds edit_buf from `browse`, which
                                // only carries a table for the table-browse path.
                                // For a hand-typed single-table SELECT, apply the
                                // PK hint computed above instead so the grid can
                                // go from view-only to editable.
                                if !is_browse {
                                    if let Some((table, pk)) = pk_hint {
                                        edit_buf.lock().unwrap().table = Some(table);
                                        edit_buf.lock().unwrap().pk_cols = pk;
                                    }
                                    let editable = !edit_buf.lock().unwrap().pk_cols.is_empty();
                                    set_p_read_only(&w, pane, !editable);
                                }
                            }
                            (None, Some(e)) => {
                                if let Some(tab) = workspace_tabs
                                    .lock()
                                    .unwrap()
                                    .iter_mut()
                                    .find(|tab| tab.id == target_id)
                                {
                                    tab.loading = false;
                                }
                                if !is_active {
                                    return;
                                }
                                *last_view.lock().unwrap() = None;
                                *error_mark_state.lock().unwrap() = error_mark_value;
                                set_p_error_mark(&w, pane, error_mark_value);
                                apply_result(
                                    &w,
                                    pane,
                                    &model::ResultView::Affected(format!(
                                        "error: {}",
                                        editor::strip_error_marker(&e.to_string())
                                    )),
                                );
                            }
                            _ => {}
                        }
                    }
                });
            });
            // ponytail: task-abort is a client-side cancel — it frees the pane and
            // the connection guard immediately; the server statement may keep
            // running until the connection notices. Add Client::cancel_token() for
            // a true server-side cancel if that ever matters.
            *panes[pane].query_abort.borrow_mut() = Some(jh.abort_handle());
        })
    };

    // ----- streaming run ("No limit"): pull rows from the driver in batches and
    // append them to the grid live, so a `SELECT * FROM huge_table` shows up
    // progressively and stays cancelable instead of freezing. The query text
    // reaches the driver verbatim — clean log, no injected LIMIT. A UI-thread
    // timer drains batches into the grid; an off-thread task does the fetch. -----
    let run_stream: Rc<dyn Fn(usize, String)> = {
        let weak = window.as_weak();
        let rt = rt.clone();
        let current = current.clone();
        let current_connection_id = current_connection_id.clone();
        let store = store.clone();
        let query_console = query_console.clone();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let active_group1_tab_id = active_group1_tab_id.clone();
        let panes = panes.clone();
        let last_view = last_view.clone();
        Rc::new(move |pane, sql: String| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            // New stream clears any previous error highlight (same reset as
            // run_sql) so a correct re-run that streams drops the stale red.
            *panes[pane].error_mark.lock().unwrap() = None;
            let error_mark_state = panes[pane].error_mark.clone();
            let run_origin = panes[pane].pending_run_origin.take();
            let sql_for_error = sql.clone();
            set_p_error_mark(&w, pane, None);
            // Read on the UI thread now — the producer/consumer task below
            // runs off it and can't touch `w`.
            let cur_db = w.get_schema_name().to_string();
            let results = panes[pane].results.clone();
            let active_result = panes[pane].active_result.clone();
            let displayed_grid = panes[pane].displayed_grid.clone();
            let hidden_cols = panes[pane].hidden_cols.clone();
            let sort_state = panes[pane].sort_state.clone();
            let col_order = panes[pane].col_order.clone();
            let col_filters = panes[pane].col_filters.clone();
            let edit_buf = panes[pane].edit_buf.clone();
            let browse = panes[pane].browse.clone();
            let stream_cancel = panes[pane].stream_cancel.clone();
            let stream_timer = panes[pane].stream_timer.clone();
            let result_new_tab = panes[pane].result_new_tab.clone();
            let active_id = if pane == 1 {
                &active_group1_tab_id
            } else {
                &active_tab_id
            };
            let Some(target_id) = active_id.lock().unwrap().clone() else {
                return;
            };
            let new_tab = result_new_tab.swap(false, std::sync::atomic::Ordering::SeqCst);
            // Stop any in-flight stream, then arm a fresh cancel flag.
            if let Some(prev) = stream_cancel.borrow().as_ref() {
                prev.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
            *stream_cancel.borrow_mut() = Some(cancel.clone());

            // Log the RAW sql (clean `SELECT * FROM t`, no injected LIMIT).
            append_query_console(&query_console, sql.clone());
            sync_query_console(&w, &query_console);
            if let Some(tab) = workspace_tabs
                .lock()
                .unwrap()
                .iter_mut()
                .find(|t| t.id == target_id)
            {
                tab.loading = true;
                tab.query_text = sql.clone();
            }
            set_p_query_running(&w, pane, true);
            set_p_streaming(&w, pane, true);
            set_p_read_only(&w, pane, true);
            clear_grid(&w, pane);
            set_p_result_status(&w, pane, SharedString::from("streaming…"));
            set_p_results_meta(&w, pane, SharedString::default());

            // Snapshot which connection is running THIS stream, same reasoning
            // as run_sql: the active connection can change before it finishes.
            let query_connection_id = current_connection_id.lock().unwrap().clone();
            let query_badge = query_connection_id
                .as_deref()
                .map(|cid| connection_badge_info(&store.borrow(), cid))
                .unwrap_or_default();

            // UI-thread accumulator (for filter/sort/export after the stream) and
            // the live cell model we push each batch into.
            let (ui_tx, ui_rx) = std::sync::mpsc::channel::<StreamMsg>();
            let accum: Rc<std::cell::RefCell<model::GridModel>> =
                Rc::new(std::cell::RefCell::new(model::GridModel::default()));
            let cells_model: Rc<std::cell::RefCell<Option<Rc<VecModel<GridCell>>>>> =
                Rc::new(std::cell::RefCell::new(None));

            // Drain timer: append whatever arrived this tick, on the UI thread.
            let timer = slint::Timer::default();
            {
                let weak = weak.clone();
                let accum = accum.clone();
                let cells_model = cells_model.clone();
                let last_view = last_view.clone();
                let displayed_grid = displayed_grid.clone();
                let hidden_cols = hidden_cols.clone();
                let sort_state = sort_state.clone();
                let col_order = col_order.clone();
                let col_filters = col_filters.clone();
                let edit_buf = edit_buf.clone();
                let browse = browse.clone();
                let workspace_tabs = workspace_tabs.clone();
                let results = results.clone();
                let active_result = active_result.clone();
                let active_tab_id = active_tab_id.clone();
                let active_group1_tab_id = active_group1_tab_id.clone();
                let stream_timer_stop = stream_timer.clone();
                let target_id = target_id.clone();
                let query_connection_id = query_connection_id.clone();
                let query_badge = query_badge.clone();
                timer.start(
                    slint::TimerMode::Repeated,
                    std::time::Duration::from_millis(50),
                    move || {
                        let Some(w) = weak.upgrade() else {
                            return;
                        };
                        loop {
                            match ui_rx.try_recv() {
                                Ok(StreamMsg::Meta(cols)) => {
                                    let gcols: Vec<GridColumn> = cols
                                        .iter()
                                        .map(|c| GridColumn {
                                            name: c.name.clone().into(),
                                            type_name: c.type_name.clone().into(),
                                        })
                                        .collect();
                                    set_p_col_count(&w, pane, gcols.len() as i32);
                                    set_p_columns(
                                        &w,
                                        pane,
                                        ModelRc::from(Rc::new(VecModel::from(gcols))),
                                    );
                                    let vm = Rc::new(VecModel::<GridCell>::default());
                                    set_p_cells(&w, pane, ModelRc::from(vm.clone()));
                                    *cells_model.borrow_mut() = Some(vm);
                                    *accum.borrow_mut() = model::GridModel {
                                        columns: cols,
                                        rows: Vec::new(),
                                    };
                                }
                                Ok(StreamMsg::Batch(rows)) => {
                                    if let Some(vm) = cells_model.borrow().as_ref() {
                                        let mut acc = accum.borrow_mut();
                                        for row in &rows {
                                            let mut vmrow = Vec::with_capacity(row.len());
                                            for cell in row {
                                                let is_null =
                                                    matches!(cell, rdb_core::result::Cell::Null);
                                                let text = cell.render();
                                                vm.push(GridCell {
                                                    text: text.clone().into(),
                                                    is_null,
                                                    state: 0,
                                                });
                                                vmrow.push(model::VmCell { text, is_null });
                                            }
                                            acc.rows.push(vmrow);
                                        }
                                        set_p_result_status(
                                            &w,
                                            pane,
                                            SharedString::from(format!(
                                                "loaded {}",
                                                acc.rows.len()
                                            )),
                                        );
                                    }
                                }
                                Ok(StreamMsg::Done {
                                    capped,
                                    elapsed_ms,
                                    pk_hint,
                                }) => {
                                    let g = accum.borrow().clone();
                                    let n = g.rows.len();
                                    let latency = model::format_latency(elapsed_ms);
                                    let meta = format!(
                                        "{n} rows{} · {latency}",
                                        if capped { " (capped)" } else { "" }
                                    );
                                    let view = Arc::new(model::ResultView::Table(g));
                                    let sr = StoredResult {
                                        view: view.clone(),
                                        meta: meta.clone(),
                                        latency: latency.clone(),
                                        grid: GridState::default(),
                                        connection_id: query_connection_id.clone(),
                                        engine: query_badge.engine.clone(),
                                        connection_name: query_badge.name.clone(),
                                        color: query_badge.color,
                                        has_custom_color: query_badge.has_custom_color,
                                    };
                                    let (tab_results, tab_active, is_browse) = {
                                        let mut tabs = workspace_tabs.lock().unwrap();
                                        let Some(tab) = tabs.iter_mut().find(|t| t.id == target_id)
                                        else {
                                            return;
                                        };
                                        tab.loading = false;
                                        tab.view = Some(sr.clone());
                                        store_result(
                                            &mut tab.results,
                                            &mut tab.active_result,
                                            sr.clone(),
                                            new_tab,
                                        );
                                        (
                                            tab.results.clone(),
                                            tab.active_result,
                                            tab.kind == "table",
                                        )
                                    };
                                    let active_id = if pane == 1 {
                                        &active_group1_tab_id
                                    } else {
                                        &active_tab_id
                                    };
                                    if active_id.lock().unwrap().as_deref() == Some(&target_id) {
                                        *results.lock().unwrap() = tab_results;
                                        *active_result.lock().unwrap() = tab_active;
                                        set_result_tabs(
                                            &w,
                                            pane,
                                            &results.lock().unwrap(),
                                            tab_active,
                                        );
                                        present_view(
                                            &w,
                                            pane,
                                            &view,
                                            &meta,
                                            &latency,
                                            &last_view,
                                            &displayed_grid,
                                            &hidden_cols,
                                            &sort_state,
                                            &col_order,
                                            &col_filters,
                                            &edit_buf,
                                            &browse,
                                        );
                                        set_p_query_running(&w, pane, false);
                                        set_p_streaming(&w, pane, false);
                                        // A bare `SELECT` with no LIMIT streams instead of
                                        // going through run_sql, so it needs the same PK
                                        // lookup to go from view-only to editable. pk_hint
                                        // was computed inside the producer/consumer task
                                        // before Done was sent, so applying it here lands
                                        // atomically with present_view — no window where
                                        // the grid looks interactive but table/pk_cols
                                        // hasn't caught up (see the run_stream race this
                                        // replaced).
                                        if !is_browse {
                                            if let Some((table, pk)) = pk_hint {
                                                let mut b = edit_buf.lock().unwrap();
                                                b.table = Some(table);
                                                b.pk_cols = pk;
                                            }
                                            let editable =
                                                !edit_buf.lock().unwrap().pk_cols.is_empty();
                                            set_p_read_only(&w, pane, !editable);
                                        }
                                    }
                                    if let Some(t) = stream_timer_stop.borrow().as_ref() {
                                        t.stop();
                                    }
                                    return;
                                }
                                Ok(StreamMsg::Err(e)) => {
                                    if let Some(tab) = workspace_tabs
                                        .lock()
                                        .unwrap()
                                        .iter_mut()
                                        .find(|t| t.id == target_id)
                                    {
                                        tab.loading = false;
                                    }
                                    let active_id = if pane == 1 {
                                        &active_group1_tab_id
                                    } else {
                                        &active_tab_id
                                    };
                                    if active_id.lock().unwrap().as_deref() == Some(&target_id) {
                                        // A streamed run fails the same way a
                                        // buffered one does, so it arms the
                                        // editor's error line the same way.
                                        let mark = editor::error_spot(
                                            &e,
                                            &sql_for_error,
                                            &editor::statement_offsets(&sql_for_error),
                                        )
                                        .as_ref()
                                        .map(|s| mark_from(s, run_origin));
                                        *error_mark_state.lock().unwrap() = mark;
                                        set_p_error_mark(&w, pane, mark);
                                        apply_result(
                                            &w,
                                            pane,
                                            &model::ResultView::Affected(format!(
                                                "error: {}",
                                                editor::strip_error_marker(&e)
                                            )),
                                        );
                                        set_p_query_running(&w, pane, false);
                                        set_p_streaming(&w, pane, false);
                                    }
                                    if let Some(t) = stream_timer_stop.borrow().as_ref() {
                                        t.stop();
                                    }
                                    return;
                                }
                                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                                    set_p_query_running(&w, pane, false);
                                    set_p_streaming(&w, pane, false);
                                    if let Some(t) = stream_timer_stop.borrow().as_ref() {
                                        t.stop();
                                    }
                                    return;
                                }
                            }
                        }
                    },
                );
            }
            *stream_timer.borrow_mut() = Some(timer);

            // Producer task: stream from the driver, forward Send batches to the
            // UI channel. The driver is cloned out of the mutex up front (the
            // mongodb/pg client is internally pooled), so the whole stream runs
            // without holding the lock; Cancel or a new run stops it at the next
            // batch.
            let sql_for_pk = sql.clone();
            let q = rdb_core::query::Query::Sql(sql);
            let current = current.clone();
            rt.spawn(async move {
                let t0 = std::time::Instant::now();
                let picked = {
                    let guard = current.lock().await;
                    guard.as_ref().map(|(e, d)| (*e, d.clone()))
                };
                let driver = picked.as_ref().map(|(_, d)| d.clone());
                let (ctx, mut crx) = tokio::sync::mpsc::channel::<rdb_core::result::StreamItem>(4);
                let cancel_prod = cancel.clone();
                let producer = async move {
                    match driver {
                        Some(driver) => {
                            driver
                                .query_stream(&q, STREAM_BATCH, cancel_prod, ctx)
                                .await
                        }
                        None => Err(rdb_core::error::RdbError::Connection(
                            "not connected".into(),
                        )),
                    }
                };
                let ui_tx2 = ui_tx.clone();
                let cancel_cons = cancel.clone();
                let mut col_names: Vec<String> = Vec::new();
                let consumer = async move {
                    let mut total = 0usize;
                    let mut capped = false;
                    while let Some(item) = crx.recv().await {
                        match item {
                            rdb_core::result::StreamItem::Meta(cols) => {
                                let vmcols: Vec<model::VmColumn> = cols
                                    .iter()
                                    .map(|c| model::VmColumn {
                                        name: c.name.clone(),
                                        type_name: c.type_name.clone(),
                                    })
                                    .collect();
                                col_names = vmcols.iter().map(|c| c.name.clone()).collect();
                                if ui_tx2.send(StreamMsg::Meta(vmcols)).is_err() {
                                    cancel_cons.store(true, std::sync::atomic::Ordering::SeqCst);
                                    break;
                                }
                            }
                            rdb_core::result::StreamItem::Batch(rows) => {
                                total += rows.len();
                                if ui_tx2.send(StreamMsg::Batch(rows)).is_err() {
                                    cancel_cons.store(true, std::sync::atomic::Ordering::SeqCst);
                                    break;
                                }
                                if total >= STREAM_SOFT_CAP {
                                    capped = true;
                                    cancel_cons.store(true, std::sync::atomic::Ordering::SeqCst);
                                }
                            }
                        }
                    }
                    (capped, col_names)
                };
                let (pres, (capped, col_names)) = tokio::join!(producer, consumer);
                let elapsed_ms = t0.elapsed().as_millis().max(1) as u64;
                match pres {
                    Ok(()) => {
                        // Same heuristic-then-primary_key lookup as run_sql, done here
                        // (inside the same task, before Done ships) so it lands
                        // atomically with the result — see the StreamMsg::Done doc
                        // comment for why this replaced a separately spawned task.
                        let pk_hint: Option<(rdb_core::write::TableRef, Vec<String>)> =
                            if let Some((engine, driver)) = picked.as_ref() {
                                if matches!(
                                    engine,
                                    rdb_connstore::Engine::Postgres
                                        | rdb_connstore::Engine::MySql
                                        | rdb_connstore::Engine::Sqlite
                                ) {
                                    if let Some((qualifier, name)) =
                                        crate::query_parse::single_table_name(&sql_for_pk)
                                    {
                                        let qualifier = qualifier.or_else(|| {
                                            (!cur_db.is_empty()).then(|| cur_db.clone())
                                        });
                                        let table = rdb_core::write::TableRef {
                                            database: (!matches!(
                                                engine,
                                                rdb_connstore::Engine::Postgres
                                            ))
                                            .then(|| qualifier.clone())
                                            .flatten(),
                                            schema: matches!(
                                                engine,
                                                rdb_connstore::Engine::Postgres
                                            )
                                            .then(|| qualifier.clone())
                                            .flatten(),
                                            name,
                                        };
                                        match driver.primary_key(&table).await {
                                            Ok(pk)
                                                if !pk.is_empty()
                                                    && pk.iter().all(|k| col_names.contains(k)) =>
                                            {
                                                Some((table, pk))
                                            }
                                            _ => None,
                                        }
                                    } else {
                                        None
                                    }
                                } else {
                                    None
                                }
                            } else {
                                None
                            };
                        let _ = ui_tx.send(StreamMsg::Done {
                            capped,
                            elapsed_ms,
                            pk_hint,
                        });
                    }
                    Err(e) => {
                        let _ = ui_tx.send(StreamMsg::Err(e.to_string()));
                    }
                }
            });
        })
    };

    (run_sql, run_stream)
}
