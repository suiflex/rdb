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

fn qualified_table_ident(
    db: &str,
    schema: &str,
    label: &str,
    eng: rdb_connstore::Engine,
) -> String {
    if !db.is_empty() {
        format!("{}.{}", quote_ident(db, eng), quote_ident(label, eng))
    } else if !schema.is_empty()
        && matches!(
            eng,
            rdb_connstore::Engine::Postgres
                | rdb_connstore::Engine::Mssql
                | rdb_connstore::Engine::Oracle
        )
    {
        format!("{}.{}", quote_ident(schema, eng), quote_ident(label, eng))
    } else {
        quote_ident(label, eng)
    }
}

fn copy_to_meta(w: &MainWindow, text: &str, ok_msg: &'static str) {
    let msg = if clip_set(text) {
        ok_msg
    } else {
        "copy failed"
    };
    w.set_results_meta(msg.into());
}
fn sync_table_structure(
    w: &MainWindow,
    raw_nodes: &Arc<std::sync::Mutex<Vec<model::VmTreeNode>>>,
    table_label: &str,
) {
    let mut fields = Vec::new();
    if let Ok(nodes) = raw_nodes.lock() {
        let mut in_table = false;
        for n in nodes.iter() {
            let is_container = matches!(n.kind.as_str(), "table" | "collection" | "keyspace");
            if is_container {
                in_table = n.label == table_label;
            } else if n.kind == "field" && in_table {
                let (name, rest) = n.label.split_once(':').unwrap_or((&n.label, ""));
                let ty = rest
                    .trim()
                    .trim_end_matches("PK")
                    .trim_end_matches("FK")
                    .trim();
                fields.push(StructField {
                    name: name.trim().into(),
                    type_name: ty.into(),
                    nullable: !rest.contains("PK"),
                });
            } else if n.kind != "field" {
                in_table = false;
            }
        }
    }
    if !fields.is_empty() {
        w.set_structure_columns(ModelRc::from(Rc::new(VecModel::from(fields))));
    }
}

struct ImportOutcome {
    summary: String,
    has_error: bool,
    log_entries: Vec<String>,
}

fn format_failed_indices(indices: &[usize]) -> String {
    let shown: Vec<String> = indices.iter().take(5).map(|i| i.to_string()).collect();
    if indices.len() > 5 {
        format!("{} (+{} more)", shown.join(", "), indices.len() - 5)
    } else {
        shown.join(", ")
    }
}

#[derive(Clone)]
struct PendingImport {
    db: String,
    sheet: export::ImportSheet,
    target_cols: Vec<String>,
}

fn schema_table_names(raw_nodes: &Arc<std::sync::Mutex<Vec<model::VmTreeNode>>>) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(nodes) = raw_nodes.lock() {
        for n in nodes.iter() {
            if matches!(n.kind.as_str(), "table" | "collection" | "keyspace")
                && !out.contains(&n.label)
            {
                out.push(n.label.clone());
            }
        }
    }
    out
}

fn table_column_names(
    raw_nodes: &Arc<std::sync::Mutex<Vec<model::VmTreeNode>>>,
    table_label: &str,
) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(nodes) = raw_nodes.lock() {
        let mut in_table = false;
        for n in nodes.iter() {
            let is_container = matches!(n.kind.as_str(), "table" | "collection" | "keyspace");
            if is_container {
                in_table = n.label == table_label;
            } else if n.kind == "field" && in_table {
                let col = completion::field_name(&n.label).to_string();
                if !col.is_empty() {
                    out.push(col);
                }
            } else if n.kind != "field" {
                in_table = false;
            }
        }
    }
    out
}

fn sync_import_cols_and_summary(
    w: &MainWindow,
    sheet: &export::ImportSheet,
    target_cols: &[String],
    table_col_count: usize,
) {
    let cols_model: Vec<ImportCol> = sheet
        .headers
        .iter()
        .zip(target_cols.iter())
        .map(|(fc, tc)| ImportCol {
            file_col: fc.clone().into(),
            target_col: tc.clone().into(),
        })
        .collect();
    let matched = target_cols
        .iter()
        .filter(|tc| !tc.is_empty() && tc.as_str() != "Do not import")
        .count();
    w.set_import_cols(ModelRc::from(Rc::new(VecModel::from(cols_model))));
    w.set_import_summary(
        format!(
            "{} {} columns, table {} columns, matched {} columns",
            sheet.format_label,
            sheet.headers.len(),
            table_col_count,
            matched
        )
        .into(),
    );
}

fn refresh_import_wizard_mapping(
    w: &MainWindow,
    pending: &mut PendingImport,
    raw_nodes: &Arc<std::sync::Mutex<Vec<model::VmTreeNode>>>,
) {
    let target_table = w.get_import_target_table().to_string();
    let create_table = w.get_import_create_table();
    let mode = w.get_import_match_mode().to_string();
    let tcols = table_column_names(raw_nodes, &target_table);

    let mut options: Vec<SharedString> = vec!["Do not import".into()];
    for tc in &tcols {
        options.push(tc.clone().into());
    }
    if create_table {
        for fc in &pending.sheet.headers {
            let s = SharedString::from(fc.as_str());
            if !options.contains(&s) {
                options.push(s);
            }
        }
    }
    w.set_import_target_col_options(ModelRc::from(Rc::new(VecModel::from(options))));

    pending.target_cols =
        export::match_import_columns(&pending.sheet.headers, &tcols, &mode, create_table);
    let table_col_count = if create_table {
        pending.sheet.headers.len()
    } else {
        tcols.len()
    };
    sync_import_cols_and_summary(w, &pending.sheet, &pending.target_cols, table_col_count);
}

fn is_duplicate_db_error(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("duplicate")
        || lower.contains("unique constraint")
        || lower.contains("23505")
        || lower.contains("1062")
}

fn is_transactional_import_engine(engine: rdb_connstore::Engine) -> bool {
    matches!(
        engine,
        rdb_connstore::Engine::Postgres
            | rdb_connstore::Engine::MySql
            | rdb_connstore::Engine::MariaDb
            | rdb_connstore::Engine::Sqlite
            | rdb_connstore::Engine::Mssql
            | rdb_connstore::Engine::Oracle
    )
}

async fn execute_import_ops(
    engine: rdb_connstore::Engine,
    driver: &AnyDriver,
    ops: Vec<rdb_core::write::WriteOp>,
    insert_option: &str,
) -> ImportOutcome {
    let total = ops.len();
    let stop_on_error = insert_option == "STOP ON ERROR";
    let skip_duplicates = insert_option == "SKIP DUPLICATES";

    // Atomic batch transaction only when STOP ON ERROR is selected on an engine
    // whose `commit(&ops)` rolls back on failure. Non-transactional engines
    // (Mongo, Cassandra, Redis, Valkey, ClickHouse) and CONTINUE/SKIP modes
    // execute per-row so committed rows are never re-applied.
    if stop_on_error && is_transactional_import_engine(engine) {
        return match driver.commit(&ops).await {
            Ok(_) => ImportOutcome {
                summary: format!("imported {total} row(s)"),
                has_error: false,
                log_entries: vec![format!("-- imported {total} row(s) in batch")],
            },
            Err(e) => {
                let clean = editor::strip_error_marker(&e.to_string()).into_owned();
                ImportOutcome {
                    summary: format!(
                        "error: import aborted (rolled back {total} row(s)) — {clean}"
                    ),
                    has_error: true,
                    log_entries: vec![format!("-- [import rolled back] {clean}")],
                }
            }
        };
    }

    let mut ok_count = 0usize;
    let mut skipped_dups = 0usize;
    let mut failed_rows = Vec::new();
    let mut first_err: Option<(usize, String)> = None;
    let mut log_entries = Vec::new();

    for (idx, op) in ops.iter().enumerate() {
        let row_num = idx + 1;
        let single = std::slice::from_ref(op);
        match driver.commit(single).await {
            Ok(_) => ok_count += 1,
            Err(e) => {
                let clean = editor::strip_error_marker(&e.to_string()).into_owned();
                if skip_duplicates && is_duplicate_db_error(&clean) {
                    skipped_dups += 1;
                    continue;
                }
                let stmt = dispatch::write_statements(engine, single)
                    .into_iter()
                    .next()
                    .unwrap_or_default();
                failed_rows.push(row_num);
                log_entries.push(format!("-- [import error row {row_num}] {clean}\n{stmt}"));
                if first_err.is_none() {
                    first_err = Some((row_num, clean));
                }
                if stop_on_error {
                    break;
                }
            }
        }
    }

    if failed_rows.is_empty() {
        let summary = if skipped_dups > 0 {
            format!("imported {ok_count}/{total} row(s) ({skipped_dups} duplicate(s) skipped)")
        } else {
            format!("imported {ok_count} row(s)")
        };
        return ImportOutcome {
            summary,
            has_error: false,
            log_entries,
        };
    }

    let fail_count = failed_rows.len();
    let rows_list = format_failed_indices(&failed_rows);
    let (_, detail) = first_err.unwrap_or_else(|| (1, "unknown error".into()));
    ImportOutcome {
        summary: format!(
            "error: imported {ok_count}/{total} row(s); {fail_count} failed (row {rows_list}: {detail})"
        ),
        has_error: true,
        log_entries,
    }
}

async fn execute_import_sql(
    engine: rdb_connstore::Engine,
    driver: &AnyDriver,
    stmts: Vec<String>,
    insert_option: &str,
) -> ImportOutcome {
    let total = stmts.len();
    let stop_on_error = insert_option == "STOP ON ERROR";
    let mut ok_count = 0usize;
    let mut failed_stmts = Vec::new();
    let mut first_err: Option<(usize, String)> = None;
    let mut log_entries = Vec::new();

    for (idx, stmt) in stmts.iter().enumerate() {
        let stmt_num = idx + 1;
        let outcome = match query_parse::parse_query(engine, stmt) {
            Ok(q) => driver
                .query(&q)
                .await
                .map(|_| ())
                .map_err(|e| e.to_string()),
            Err(e) => Err(e),
        };
        match outcome {
            Ok(()) => ok_count += 1,
            Err(raw_err) => {
                let clean = editor::strip_error_marker(&raw_err).into_owned();
                failed_stmts.push(stmt_num);
                log_entries.push(format!("-- [import error stmt {stmt_num}] {clean}\n{stmt}"));
                if first_err.is_none() {
                    first_err = Some((stmt_num, clean));
                }
                if stop_on_error {
                    break;
                }
            }
        }
    }

    if failed_stmts.is_empty() {
        ImportOutcome {
            summary: format!("imported {ok_count} statement(s)"),
            has_error: false,
            log_entries: vec![format!("-- imported {ok_count} statement(s)")],
        }
    } else {
        let fail_count = failed_stmts.len();
        let stmts_list = format_failed_indices(&failed_stmts);
        let (_, detail) = first_err.unwrap_or_else(|| (1, "unknown error".into()));
        ImportOutcome {
            summary: format!(
                "error: imported {ok_count}/{total} statement(s); {fail_count} failed (stmt {stmts_list}: {detail})"
            ),
            has_error: true,
            log_entries,
        }
    }
}

fn apply_import_status(w: &MainWindow, summary: &str, has_error: bool) {
    w.set_status_error(has_error);
    w.set_result_status(SharedString::from(summary));
    w.set_results_meta(SharedString::from(summary));
}

fn open_import_wizard_for_table(
    w: &MainWindow,
    db_arg: SharedString,
    label_arg: SharedString,
    raw_nodes: Arc<std::sync::Mutex<Vec<model::VmTreeNode>>>,
    pending_import: Arc<std::sync::Mutex<Option<PendingImport>>>,
) {
    let prompt = format!("Import into {} (CSV, JSON, SQL)", label_arg);
    pick_file_via_dialog(w, prompt, move |w, picked| {
        let (path, contents) = match picked {
            Ok(pair) => pair,
            Err(msg) => {
                w.set_results_meta(msg.into());
                return;
            }
        };
        let sheet = match export::parse_import_sheet(&path, &contents) {
            Ok(s) => s,
            Err(e) => {
                let msg = format!("error: import failed — {e}");
                apply_import_status(w, &msg, true);
                return;
            }
        };

        let mut tables = schema_table_names(&raw_nodes);
        if !label_arg.is_empty() && !tables.iter().any(|t| t == label_arg.as_str()) {
            tables.insert(0, label_arg.to_string());
        }
        let table_opts: Vec<SharedString> =
            tables.iter().cloned().map(SharedString::from).collect();
        w.set_import_table_options(ModelRc::from(Rc::new(VecModel::from(table_opts))));
        w.set_import_target_table(label_arg.clone());
        w.set_import_new_table_name(
            std::path::Path::new(&sheet.file_name)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("imported_table")
                .into(),
        );
        w.set_import_create_table(false);
        w.set_import_wizard_title(format!("Import {} Wizard", sheet.format_label).into());
        w.set_import_file_info(
            format!(
                "{} - {}",
                sheet.file_name,
                export::format_byte_size(sheet.size_bytes)
            )
            .into(),
        );
        w.set_import_preview_caption(
            format!("First 100 rows of the {} file", sheet.format_label).into(),
        );
        w.set_import_match_mode("Match Columns by Name and Order".into());
        w.set_import_encoding("utf8".into());
        w.set_import_insert_option("STOP ON ERROR".into());
        w.set_import_error(SharedString::default());

        let preview_rows: Vec<ImportPreviewRow> = sheet
            .rows
            .iter()
            .take(100)
            .map(|r| {
                let cells: Vec<SharedString> = r
                    .iter()
                    .map(|c| match c {
                        rdb_core::result::Cell::Null => SharedString::from("NULL"),
                        other => SharedString::from(other.render()),
                    })
                    .collect();
                ImportPreviewRow {
                    cells: ModelRc::from(Rc::new(VecModel::from(cells))),
                }
            })
            .collect();
        w.set_import_preview_rows(ModelRc::from(Rc::new(VecModel::from(preview_rows))));

        let mut pending = PendingImport {
            db: db_arg.to_string(),
            sheet,
            target_cols: Vec::new(),
        };
        refresh_import_wizard_mapping(w, &mut pending, &raw_nodes);
        if let Ok(mut guard) = pending_import.lock() {
            *guard = Some(pending);
        }
        w.set_import_modal_open(true);
    });
}

pub(crate) fn wire(window: &MainWindow, state: &AppState, fns: &AppFns) {
    let AppState {
        rt,
        store,
        panes,
        current,
        driver_pool,
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
        let driver_pool = driver_pool.clone();
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
        let raw_nodes_for_open = state.raw_nodes.clone();
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
                sync_table_structure(&w, &raw_nodes_for_open, &label);
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
                    folded_heads: Vec::new(),
                    caret: Default::default(),
                    prod_confirmed: HashSet::new(),
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
            sync_table_structure(&w, &raw_nodes_for_open, &label);
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
            let driver_pool = driver_pool.clone();
            let browse = browse.clone();
            let edit_buf = edit_buf.clone();
            let workspace_tabs = workspace_tabs.clone();
            let active_tab_id = active_tab_id.clone();
            let query_console = query_console.clone();
            let table_connection_id = (!connection_id.is_empty()).then(|| connection_id.clone());
            rt.spawn(async move {
                let picked =
                    resolve_driver(&driver_pool, &current, table_connection_id.as_deref()).await;
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
        let driver_pool = driver_pool.clone();
        let rt = rt.clone();
        let run_browse = run_browse.clone();
        let guard_pending = guard_pending.clone();
        let active_tab_id = active_tab_id.clone();
        let workspace_tabs = workspace_tabs.clone();
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
            let driver_pool = driver_pool.clone();
            let browse = browse.clone();
            // The focused tab's own connection, not whatever `current` is —
            // switching focus between already-open tabs never touches `current`.
            let tab_connection_id = focused_tab_connection_id(&active_tab_id, &workspace_tabs);
            rt.spawn(async move {
                let driver = resolve_driver(&driver_pool, &current, tab_connection_id.as_deref())
                    .await
                    .map(|(_, d)| d);
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
            // Same as the left pane: the popup has had its answer.
            w.set_filter_completion_items(ModelRc::from(Rc::new(
                VecModel::<PaletteItem>::default(),
            )));
            // Same as the left pane: validation is left to the query path.
            let raw = w.get_p1_mongo_filter().to_string();
            let trimmed = raw.trim();
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
                    // Tab takes the candidate; Enter runs the filter. Enter
                    // used to take the candidate too, so it never reached the
                    // field's submit: the first press rewrote the text and the
                    // filter only ran on a second press — which reads as Enter
                    // doing nothing, and mangles an already-complete filter.
                    "\t" => {
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
            // The suggestion popup has had its answer either way.
            w.set_filter_completion_items(ModelRc::from(Rc::new(
                VecModel::<PaletteItem>::default(),
            )));
            let raw = w.get_mongo_filter().to_string();
            let trimmed = raw.trim();
            // Deliberately unvalidated here. The filter is spliced into the
            // browse query and parsed by `query_parse` (json5, so bare keys and
            // single quotes work like they do in the editor); a bad document
            // comes back through the normal query error path, which browse mode
            // actually renders. Rejecting it here instead set a status string
            // that nothing painted, so Enter looked like a no-op.
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
            if let Some(leaf_kind) = nested_leaf_kind(engine) {
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
                let need_fetch = now_open && loaded_dbs.lock().unwrap().insert(label.clone());
                if need_fetch {
                    let driver = current.clone();
                    let driver_pool = driver_pool.clone();
                    let raw_nodes = raw_nodes.clone();
                    let expanded_tables = expanded_tables.clone();
                    let loaded_dbs = loaded_dbs.clone();
                    let sidebar_filter = sidebar_filter.clone();
                    let weak2 = weak.clone();
                    let db = label.clone();
                    // The tree belongs to a connection, so resolve its driver
                    // through the pool like every other action does. Taking
                    // the single `current` slot instead meant this queued
                    // behind whatever held it — a connect holds it across its
                    // whole schema read — and an expanding database just span
                    // until that finished, or until it timed out.
                    let tree_connection_id = current_connection_id.lock().unwrap().clone();
                    let current_connection_id = current_connection_id.clone();
                    rt.spawn(async move {
                        let drv =
                            resolve_driver(&driver_pool, &driver, tree_connection_id.as_deref())
                                .await
                                .map(|(_, d)| d);
                        let containers = match drv {
                            Some(drv) => drv.containers(&db).await.unwrap_or_default(),
                            None => return,
                        };
                        // `raw_nodes` is whichever connection the sidebar shows
                        // *now*. If that changed while this was in flight, these
                        // containers belong to a tree that is no longer there.
                        // ponytail: the db stays marked loaded in the snapshot
                        // left behind, so it reopens empty until re-expanded.
                        if *current_connection_id.lock().unwrap() != tree_connection_id {
                            return;
                        }
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
    // ----- sidebar schema tree: table context menu actions & Import Wizard -----
    let pending_import: Arc<std::sync::Mutex<Option<PendingImport>>> =
        Arc::new(std::sync::Mutex::new(None));
    {
        let weak = window.as_weak();
        let raw_nodes = state.raw_nodes.clone();
        let pending_import = pending_import.clone();
        window.on_import_table_changed(move |_tbl| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if let Ok(mut guard) = pending_import.lock() {
                if let Some(pending) = guard.as_mut() {
                    refresh_import_wizard_mapping(&w, pending, &raw_nodes);
                }
            }
        });
    }
    {
        let weak = window.as_weak();
        let raw_nodes = state.raw_nodes.clone();
        let pending_import = pending_import.clone();
        window.on_import_create_toggled(move |_create| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if let Ok(mut guard) = pending_import.lock() {
                if let Some(pending) = guard.as_mut() {
                    refresh_import_wizard_mapping(&w, pending, &raw_nodes);
                }
            }
        });
    }
    {
        let weak = window.as_weak();
        let raw_nodes = state.raw_nodes.clone();
        let pending_import = pending_import.clone();
        window.on_import_match_mode_changed(move |_mode| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if let Ok(mut guard) = pending_import.lock() {
                if let Some(pending) = guard.as_mut() {
                    refresh_import_wizard_mapping(&w, pending, &raw_nodes);
                }
            }
        });
    }
    {
        let weak = window.as_weak();
        let raw_nodes = state.raw_nodes.clone();
        let pending_import = pending_import.clone();
        window.on_import_col_target_changed(move |col_idx, target_col| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let Ok(mut guard) = pending_import.lock() else {
                return;
            };
            let Some(pending) = guard.as_mut() else {
                return;
            };
            if let Some(slot) = pending.target_cols.get_mut(col_idx.max(0) as usize) {
                *slot = target_col.to_string();
            }
            let tcols = table_column_names(&raw_nodes, &w.get_import_target_table());
            let table_col_count = if w.get_import_create_table() {
                pending.sheet.headers.len()
            } else {
                tcols.len()
            };
            sync_import_cols_and_summary(&w, &pending.sheet, &pending.target_cols, table_col_count);
        });
    }
    {
        let weak = window.as_weak();
        let cur_engine = cur_engine.clone();
        let rt = rt.clone();
        let current = current.clone();
        let driver_pool = state.driver_pool.clone();
        let current_connection_id = state.current_connection_id.clone();
        let query_console = state.query_console.clone();
        let pending_import = pending_import.clone();
        window.on_import_commit(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let Some(pending) = pending_import.lock().ok().and_then(|g| g.clone()) else {
                return;
            };
            let eng = (*cur_engine.borrow()).unwrap_or(rdb_connstore::Engine::Postgres);
            let schema = w.get_schema_name().to_string();
            let create_table = w.get_import_create_table();
            let target_table = if create_table {
                w.get_import_new_table_name().trim().to_string()
            } else {
                w.get_import_target_table().trim().to_string()
            };
            if target_table.is_empty() {
                w.set_import_error("Table name is required".into());
                return;
            }
            let table_ref = rdb_core::write::TableRef {
                database: (!pending.db.is_empty()).then(|| pending.db.clone()),
                schema: matches!(eng, rdb_connstore::Engine::Postgres)
                    .then(|| schema.clone())
                    .filter(|s| !s.is_empty()),
                name: target_table.clone(),
            };
            let ops = if pending.sheet.sql_statements.is_empty() {
                match export::build_import_ops_from_sheet(
                    &pending.sheet,
                    &table_ref,
                    &pending.target_cols,
                ) {
                    Ok(ops) => ops,
                    Err(e) => {
                        w.set_import_error(e.into());
                        return;
                    }
                }
            } else {
                Vec::new()
            };
            let sql_statements = pending.sheet.sql_statements.clone();
            let insert_option = w.get_import_insert_option().to_string();
            let create_ddl = if create_table && sql_statements.is_empty() {
                let qual = qualified_table_ident(&pending.db, &schema, &target_table, eng);
                let cols_ddl: Vec<String> = pending
                    .target_cols
                    .iter()
                    .filter(|tc| !tc.is_empty() && tc.as_str() != "Do not import")
                    .map(|tc| format!("{} TEXT", quote_ident(tc, eng)))
                    .collect();
                Some(format!("CREATE TABLE {qual} ({});", cols_ddl.join(", ")))
            } else {
                None
            };
            w.set_import_error(SharedString::default());
            let conn_id = current_connection_id.lock().ok().and_then(|g| g.clone());
            let db_arg = SharedString::from(pending.db.clone());
            let label_arg = SharedString::from(target_table);
            let query_console = query_console.clone();
            let weak_done = w.as_weak();
            let rt_inner = rt.clone();
            let current = current.clone();
            let driver_pool = driver_pool.clone();
            rt.spawn(async move {
                let Some((engine, driver)) =
                    resolve_driver(&driver_pool, &current, conn_id.as_deref()).await
                else {
                    return;
                };
                if let Some(ddl) = create_ddl {
                    append_query_console(&query_console, ddl.clone());
                    if let Ok(q) = query_parse::parse_query(engine, &ddl) {
                        if let Err(e) = driver.query(&q).await {
                            let clean = editor::strip_error_marker(&e.to_string()).into_owned();
                            let _ = slint::invoke_from_event_loop(move || {
                                if let Some(w) = weak_done.upgrade() {
                                    w.set_import_error(
                                        format!("CREATE TABLE failed: {clean}").into(),
                                    );
                                }
                            });
                            return;
                        }
                    }
                }
                let outcome = if !sql_statements.is_empty() {
                    execute_import_sql(engine, &driver, sql_statements, &insert_option).await
                } else {
                    execute_import_ops(engine, &driver, ops, &insert_option).await
                };
                for entry in outcome.log_entries {
                    append_query_console(&query_console, entry);
                }
                let summary = outcome.summary;
                let has_error = outcome.has_error;
                let stop_failed = has_error && insert_option == "STOP ON ERROR";
                let weak_after = weak_done.clone();
                let summary_after = summary.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(w) = weak_done.upgrade() else {
                        return;
                    };
                    sync_query_console(&w, &query_console);
                    if stop_failed {
                        w.set_import_error(summary.clone().into());
                        apply_import_status(&w, &summary, true);
                    } else {
                        w.set_import_modal_open(false);
                        w.invoke_open_table(db_arg, label_arg);
                        apply_import_status(&w, &summary, has_error);
                    }
                });
                if has_error && !stop_failed {
                    let _ = rt_inner;
                    tokio::time::sleep(std::time::Duration::from_millis(320)).await;
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = weak_after.upgrade() {
                            apply_import_status(&w, &summary_after, true);
                        }
                    });
                }
            });
        });
    }
    {
        let weak = window.as_weak();
        let cur_engine = cur_engine.clone();
        let load_editor_text = load_editor_text.clone();
        let raw_nodes = state.raw_nodes.clone();
        let pending_import = pending_import.clone();
        window.on_table_action(move |db, label, action| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let eng = (*cur_engine.borrow()).unwrap_or(rdb_connstore::Engine::Postgres);
            let schema = w.get_schema_name().to_string();
            let qual = qualified_table_ident(&db, &schema, &label, eng);
            match action {
                0 => {
                    w.invoke_open_table(db.clone(), label.clone());
                    w.invoke_pin_table(db, label);
                }
                1 => {
                    w.invoke_open_table(db.clone(), label.clone());
                    w.invoke_pin_table(db, label);
                    w.set_show_structure(false);
                    w.set_show_structure(true);
                }
                3 => copy_to_meta(&w, &label, "copied table name"),
                4 => copy_to_meta(&w, &qual, "copied qualified table name"),
                5 => {
                    let sql = format!("SELECT\n    *\nFROM\n    {qual}\nLIMIT\n    100;");
                    copy_to_meta(&w, &sql, "copied SELECT script");
                }
                7 => {
                    open_import_wizard_for_table(
                        &w,
                        db,
                        label,
                        raw_nodes.clone(),
                        pending_import.clone(),
                    );
                }
                8 => {
                    w.invoke_open_table(db, label);
                    w.invoke_export_results(0);
                }
                9 => w.invoke_new_table(),
                11 => {
                    let sql = format!("TRUNCATE TABLE {qual};");
                    w.invoke_new_tab();
                    load_editor_text(0, &sql);
                }
                _ => {}
            }
        });
    }
    // ----- sidebar schema tree: copy column name(s) from a field row -----
    {
        let weak = window.as_weak();
        let raw_nodes = raw_nodes.clone();
        let cur_engine = cur_engine.clone();
        window.on_copy_tree_field(move |table, field_label, action| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let target_col = completion::field_name(&field_label).to_string();
            if target_col.is_empty() {
                return;
            }
            let mut names = Vec::new();
            if let Ok(nodes) = raw_nodes.lock() {
                let mut in_table = false;
                for n in nodes.iter() {
                    let is_container =
                        matches!(n.kind.as_str(), "table" | "collection" | "keyspace");
                    if is_container {
                        in_table = n.label == table.as_str();
                    } else if n.kind == "field" && in_table {
                        let col = completion::field_name(&n.label).to_string();
                        if !col.is_empty() {
                            names.push(col);
                        }
                    } else if n.kind != "field" {
                        in_table = false;
                    }
                }
            }
            if names.is_empty() {
                names.push(target_col.clone());
            }
            let col_idx = names.iter().position(|c| c == &target_col).unwrap_or(0);
            let Some(text) = crate::wire::split_pane::column_names_text(
                &names,
                col_idx,
                action,
                *cur_engine.borrow(),
            ) else {
                return;
            };
            let msg = if clip_set(&text) {
                "copied column name(s)"
            } else {
                "copy failed"
            };
            w.set_results_meta(msg.into());
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
        let rebuild_query_tree = rebuild_query_tree.clone();
        window.on_filter_tree(move |text| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            *sidebar_filter.lock().unwrap() = text.to_string();
            // Queries and History share the field; their list reads the
            // filter back out of `sidebar_filter`. The Items tree below is
            // rebuilt either way, so switching back to it shows the same
            // filter the field does.
            if w.get_sidebar_mode() != 0 {
                rebuild_query_tree("");
            }
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
