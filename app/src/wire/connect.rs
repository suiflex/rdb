//! Connecting to a database: the background health poll that reddens the
//! breadcrumb dot when a live connection drops, the connect handler itself
//! (driver work on tokio, schema pushed back to the UI), cancelling an
//! in-flight connect, and grid column drag-resize.
//!
//! Split out of `main`; the handler bodies are unchanged.

use std::collections::HashSet;
use std::rc::Rc;

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::*;

/// Decides what a connection switch does with the open tabs, before
/// touching any UI or shared state.
struct TabRestorePlan {
    tabs: Vec<WorkspaceTab>,
    active: Option<String>,
    active_p1: Option<String>,
    active_group: usize,
    /// A surviving SQL tab stays active across the switch, so its last
    /// result is still meaningful — leave it on screen instead of blanking.
    /// Only a fresh restore (first connect / DB switch) clears.
    standby: bool,
}

fn plan_tab_restore(
    tabs_restored: &Rc<Cell<bool>>,
    workspace_tabs: &Arc<Mutex<Vec<WorkspaceTab>>>,
    active_tab_id: &Arc<Mutex<Option<String>>>,
    active_group1_tab_id: &Arc<Mutex<Option<String>>>,
    query_number: &Arc<std::sync::atomic::AtomicUsize>,
) -> TabRestorePlan {
    let restore = should_restore_query_tabs(tabs_restored.get());
    tabs_restored.set(true);
    let (tabs, active, active_p1, active_group) = if restore {
        let (tabs, active, active_p1, active_group, max_number) = load_query_tabs();
        // Never let a freshly-minted tab reuse a number a restored tab
        // already holds — `fetch_max` only ever raises the counter.
        query_number.fetch_max(max_number, std::sync::atomic::Ordering::Relaxed);
        (tabs, active, active_p1, active_group)
    } else {
        // Retain every open tab across the switch so the workspace behaves
        // like a set of persistent documents — the SQL scratch tabs keep
        // their results ("standby") and the connection-scoped table/
        // collection tabs stay open too, their last data now a snapshot
        // until the user hits Refresh against the new connection. Only the
        // in-flight loading flag is cleared so no tab is left showing a
        // stuck spinner.
        let mut kept: Vec<WorkspaceTab> = std::mem::take(&mut *workspace_tabs.lock().unwrap());
        for t in &mut kept {
            t.loading = false;
        }
        // Picking a connection changes what NEW actions target (new tab,
        // browse-from-sidebar) — it must never rewrite a tab that's already
        // open. Each tab is permanently locked to the connection it was
        // created against (routed by its own `connection_id` through
        // `driver_pool`); reassigning the focused one here is what made
        // switching connections look like it dragged the open query tab
        // along with it.
        let active = active_tab_id
            .lock()
            .unwrap()
            .clone()
            .filter(|id| kept.iter().any(|t| t.id == *id))
            .or_else(|| kept.first().map(|t| t.id.clone()));
        // Connection switch keeps the in-memory focus + right-group tab.
        let active_p1 = active_group1_tab_id
            .lock()
            .unwrap()
            .clone()
            .filter(|id| kept.iter().any(|t| t.id == *id));
        (kept, active, active_p1, 0usize)
    };
    let standby = !restore && active.is_some();
    TabRestorePlan {
        tabs,
        active,
        active_p1,
        active_group,
        standby,
    }
}

/// Connect + fetch the initial schema, bounded by `timeout_secs` so an
/// unreachable host can't spin forever (the Cancel button aborts sooner).
async fn attempt_connect(
    engine: rdb_connstore::Engine,
    cfg: rdb_connstore::Result<rdb_core::conn::ConnConfig>,
    nosql_limit: usize,
    timeout_secs: u64,
) -> Result<(AnyDriver, rdb_core::schema::Schema, Option<String>), rdb_core::error::RdbError> {
    let attempt = async {
        let cfg = cfg.map_err(|e| rdb_core::error::RdbError::Connection(e.to_string()))?;
        let driver = AnyDriver::connect(engine, &cfg).await?;
        // Apply the NoSQL collection cap before any schema fetch so the
        // first sidebar load already honors it (Mongo only).
        driver.set_collection_limit(nosql_limit);
        // MongoDB: when the connection names a database, scope the sidebar
        // to it (matching the schema switcher) instead of listing every
        // database on the server.
        let scoped_db = if matches!(engine, rdb_connstore::Engine::Mongo) {
            cfg.database.clone().filter(|d| !d.is_empty())
        } else {
            None
        };
        let schema = match &scoped_db {
            Some(db) => driver.schema_for(db).await?,
            None => driver.schema().await?,
        };
        Ok::<_, rdb_core::error::RdbError>((driver, schema, scoped_db))
    };
    match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), attempt).await {
        Ok(r) => r,
        Err(_) => Err(rdb_core::error::RdbError::Connection(
            "connection timed out".into(),
        )),
    }
}

/// The database/schema selector's entries and its starting value.
///
/// Postgres browses namespaces, not databases: the selector must say "public",
/// never the db name (a `"dbname"."table"` query would fail). Scoped Mongo
/// still lists every database so the switcher can reach them, and starts on
/// the one it scoped to; everything else defaults to "public" when present,
/// else the first name.
fn build_schema_picker_names(
    engine: rdb_connstore::Engine,
    scoped_db: Option<&str>,
    pg_schemas: Vec<SharedString>,
    db_names: &[SharedString],
    schema: &rdb_core::schema::Schema,
) -> (Vec<SharedString>, SharedString) {
    let mut schema_names: Vec<SharedString> = if matches!(engine, rdb_connstore::Engine::Postgres) {
        if pg_schemas.is_empty() {
            vec![SharedString::from("public")]
        } else {
            pg_schemas
        }
    } else if scoped_db.is_some() && !db_names.is_empty() {
        db_names.to_vec()
    } else {
        schema
            .databases
            .iter()
            .map(|d| SharedString::from(d.name.clone()))
            .collect()
    };
    if schema_names.is_empty() {
        schema_names.push(SharedString::from("public"));
    }
    let schema_current = match scoped_db {
        Some(db) => SharedString::from(db),
        None => schema_names
            .iter()
            .find(|s| s.as_str() == "public")
            .unwrap_or(&schema_names[0])
            .clone(),
    };
    (schema_names, schema_current)
}

/// Publishes the driver, builds the sidebar tree and autocomplete seed,
/// pushes it to the UI, then keeps loading every other Postgres schema in
/// the background so cross-schema completion fills in without blocking the
/// first paint.
#[allow(clippy::too_many_arguments)]
async fn finish_connect_success(
    weak: slint::Weak<MainWindow>,
    engine: rdb_connstore::Engine,
    driver: AnyDriver,
    schema: rdb_core::schema::Schema,
    scoped_db: Option<String>,
    connection_id: String,
    mut slot: tokio::sync::OwnedMutexGuard<Option<(rdb_connstore::Engine, Arc<AnyDriver>)>>,
    store_driver: DriverSlot,
    driver_pool: DriverPool,
    connected_ids: Arc<Mutex<HashSet<String>>>,
    expanded_tables: Arc<Mutex<HashSet<String>>>,
    loaded_dbs: Arc<Mutex<HashSet<String>>>,
    raw_nodes: Arc<Mutex<Vec<model::VmTreeNode>>>,
    completion_nodes: Arc<Mutex<Vec<model::VmTreeNode>>>,
    fn_defs: Arc<Mutex<HashMap<String, String>>>,
) {
    // Postgres: list real namespaces so the sidebar schema switcher offers
    // more than "public". Engine-specific SQL lives in the driver, not here.
    let pg_schemas: Vec<SharedString> = if matches!(engine, rdb_connstore::Engine::Postgres) {
        driver
            .list_schemas()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(SharedString::from)
            .collect()
    } else {
        Vec::new()
    };
    // Databases on the server, backing the breadcrumb switcher. Empty for
    // engines that can't switch database.
    let db_names: Vec<SharedString> = driver
        .list_databases()
        .await
        .unwrap_or_default()
        .into_iter()
        .map(SharedString::from)
        .collect();
    let driver = Arc::new(driver);
    *slot = Some((engine, driver.clone()));
    drop(slot);
    driver_pool
        .write()
        .await
        .insert(connection_id.clone(), (engine, driver.clone()));
    // Now genuinely connected — the sidebar dot and "another connection is
    // still around" checks read this, not tab scoping (see `connected_ids`
    // on `AppState`).
    connected_ids.lock().unwrap().insert(connection_id);
    let nodes = model::to_tree_model(&schema);
    let fields = model::to_structure_model(&schema);
    // Scoped Mongo tree holds only the selected database: open it and mark
    // it loaded so its collections show at once (mirrors the schema
    // switcher).
    let (exp, loaded) = match &scoped_db {
        Some(db) => {
            let mut e = expanded_tables.lock().unwrap();
            let mut l = loaded_dbs.lock().unwrap();
            e.insert(db.clone());
            l.insert(db.clone());
            (e.clone(), l.clone())
        }
        None => (HashSet::new(), HashSet::new()),
    };
    // Stash raw nodes for later expand/collapse rebuilds, and render the
    // initial view (Functions collapsed, Tables open, fields hidden).
    // Matches the reseed done on connect above; collapsed_categories itself
    // is !Send so can't cross here.
    let rows = schema_display_rows(
        &nodes,
        &exp,
        &default_collapsed_cats(),
        &loaded,
        Some(engine),
        "",
    );
    let (schema_names, schema_current) =
        build_schema_picker_names(engine, scoped_db.as_deref(), pg_schemas, &db_names, &schema);
    let sql_capable = matches!(
        rdb_connstore::Engine::language(engine),
        rdb_connstore::QueryLanguage::Sql | rdb_connstore::QueryLanguage::Cql
    );
    *raw_nodes.lock().unwrap() = nodes;
    // Seed autocomplete with the active schema's tables plus a bare node for
    // every other schema name, so `schema.` autocompletes immediately. The
    // remaining schemas' tables and columns fill in from the background
    // load below.
    let all_schema_names: Vec<String> = schema_names.iter().map(|s| s.to_string()).collect();
    {
        let mut seed =
            build_completion_seed(&driver, engine, schema_current.as_str(), &schema).await;
        for name in &all_schema_names {
            if name != schema_current.as_str() {
                seed.push(model::VmTreeNode {
                    label: name.clone(),
                    kind: "database".into(),
                });
            }
        }
        *completion_nodes.lock().unwrap() = seed;
    }
    {
        let mut defs = fn_defs.lock().unwrap();
        defs.clear();
        defs.extend(schema.databases.iter().flat_map(|db| {
            db.functions
                .iter()
                .map(|f| (f.name.clone(), f.definition.clone()))
        }));
    }
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(w) = weak.upgrade() {
            w.set_schema_tree(ModelRc::from(Rc::new(VecModel::from(rows))));
            w.set_sql_capable(sql_capable);
            w.set_new_tab_label(crate::query_parse::language_label(engine).into());
            w.set_schema_name(schema_current);
            w.set_schema_list(ModelRc::from(Rc::new(VecModel::from(schema_names))));
            w.set_db_list(ModelRc::from(Rc::new(VecModel::from(db_names))));
            let sfields: Vec<StructField> = fields
                .into_iter()
                .map(|f| StructField {
                    name: f.name.into(),
                    type_name: f.type_name.into(),
                    nullable: f.nullable,
                })
                .collect();
            w.set_structure_columns(ModelRc::from(Rc::new(VecModel::from(sfields))));
            w.set_status_latency(SharedString::from("connected"));
            w.set_conn_status(SharedString::from("connected"));
            w.set_picker_error(SharedString::default());
            w.set_connecting(false);
            w.set_tree_loading(false);
            // Swap the picker for the workspace.
            w.set_connected(true);
            // Sidebar dot now shows this connection live, even before any
            // tab has run a query against it.
            w.invoke_refresh_connections();
        }
    });
    // Load every other schema's tables so cross-schema `schema.table`
    // autocompletes. Runs after the sidebar (active schema) already
    // rendered; the popup just gains more names as this fills. Fetched
    // concurrently, one task per schema, instead of sequentially.
    if matches!(engine, rdb_connstore::Engine::Postgres) && all_schema_names.len() > 1 {
        let driver = {
            let guard = store_driver.lock().await;
            guard.as_ref().map(|(_, d)| d.clone())
        };
        if let Some(driver) = driver {
            let handles: Vec<_> = all_schema_names
                .iter()
                .cloned()
                .map(|name| {
                    let driver = driver.clone();
                    tokio::spawn(async move {
                        driver
                            .schema_for(&name)
                            .await
                            .ok()
                            .map(|s| model::to_completion_nodes(&name, &s))
                    })
                })
                .collect();
            let mut all = Vec::new();
            for handle in handles {
                if let Ok(Some(nodes)) = handle.await {
                    all.extend(nodes);
                }
            }
            if !all.is_empty() {
                *completion_nodes.lock().unwrap() = all;
            }
        }
    }
}

/// A failed connect stays on the picker and surfaces the error there.
fn finish_connect_failure(weak: slint::Weak<MainWindow>, e: rdb_core::error::RdbError) {
    eprintln!("connect failed: {e}");
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(w) = weak.upgrade() {
            w.set_connected(false);
            w.set_connecting(false);
            w.set_tree_loading(false);
            // `e`'s own `Display` already reads "connection failed: …" —
            // don't prefix it again.
            w.set_picker_error(SharedString::from(format!("{e}")));
        }
    });
}

/// `on_connect_clicked`: flush the active tab, resolve the picked
/// connection, reset workspace state for it, paint the UI immediately,
/// then spawn the driver connect on tokio.
fn handle_connect_clicked(state: &AppState, fns: &AppFns, weak: slint::Weak<MainWindow>, idx: i32) {
    let AppState {
        rt,
        store,
        panes,
        current,
        driver_pool,
        cur_engine,
        raw_nodes,
        completion_nodes,
        expanded_tables,
        loaded_dbs,
        collapsed_categories,
        workspace_tabs,
        active_tab_id,
        active_group1_tab_id,
        current_connection_id,
        connected_ids,
        db_override,
        query_console,
        query_number,
        last_view,
        connect_handle,
        fn_defs,
        tabs_restored,
        ..
    } = state.clone();
    let AppFns {
        load_editor_text,
        save_active_tab,
        restore_tab,
        save_p1_tab,
        sync_editor,
        ..
    } = fns.clone();
    let browse = panes[0].browse.clone();
    let edit_buf = panes[0].edit_buf.clone();
    let results = panes[0].results.clone();

    // Flush the active tab(s) into the workspace model first, or a switch
    // reads stale/empty text and drops inactive tabs' results.
    if let Some(w) = weak.upgrade() {
        save_active_tab(&w);
        save_p1_tab(&w);
    }
    let i = idx as usize;
    // One-shot: set by the database switcher, empty for a fresh picker
    // connect (which then uses the connection's saved database).
    let db_ovr = db_override.lock().unwrap().take();
    let (sc, cfg) = {
        let st = store.borrow();
        let Some(sc) = st.list().get(i).cloned() else {
            return;
        };
        let cfg = st.conn_config_for(&sc.id).map(|mut c| {
            if let Some(db) = db_ovr.clone() {
                c.database = Some(db);
            }
            c
        });
        (sc, cfg)
    };
    *current_connection_id.lock().unwrap() = Some(sc.id.clone());
    // Usually keeps the tabs `main` already restored at startup; falls back
    // to a disk read only if startup found none. Either way, switching
    // connections doesn't lose open queries.
    let TabRestorePlan {
        tabs: init_tabs,
        active: init_active,
        active_p1: init_active_p1,
        active_group: init_active_group,
        standby,
    } = plan_tab_restore(
        &tabs_restored,
        &workspace_tabs,
        &active_tab_id,
        &active_group1_tab_id,
        &query_number,
    );
    *workspace_tabs.lock().unwrap() = init_tabs;
    *active_tab_id.lock().unwrap() = init_active.clone();
    *active_group1_tab_id.lock().unwrap() = init_active_p1.clone();
    if !standby {
        results.lock().unwrap().clear();
        *last_view.lock().unwrap() = None;
        edit_buf.lock().unwrap().clear();
    }
    *browse.lock().unwrap() = BrowseState {
        limit: default_browse_limit(sc.engine),
        ..Default::default()
    };
    query_console.lock().unwrap().clear();
    // Reflect selection + accent immediately.
    if let Some(w) = weak.upgrade() {
        paint_connect_ui(
            &w,
            &sc,
            db_ovr.as_deref(),
            idx,
            init_active.as_deref(),
            init_active_p1.as_deref(),
            init_active_group,
            standby,
            &store,
            &workspace_tabs,
            &query_console,
            &load_editor_text,
            &restore_tab,
        );
        *current_connection_id.lock().unwrap() = Some(sc.id.clone());
    }
    // Fresh connection: nothing browsed, nothing expanded.
    *cur_engine.borrow_mut() = Some(sc.engine);
    // Editors were lexed before the engine was known; repaint both panes
    // so a tab with no engine of its own isn't stuck on the old dialect.
    sync_editor(0);
    sync_editor(1);
    expanded_tables.lock().unwrap().clear();
    loaded_dbs.lock().unwrap().clear();
    *collapsed_categories.borrow_mut() = default_collapsed_cats();

    spawn_connect_task(
        rt,
        weak,
        &sc,
        cfg,
        current,
        driver_pool,
        connected_ids,
        raw_nodes,
        completion_nodes,
        fn_defs,
        expanded_tables,
        loaded_dbs,
        connect_handle,
    );
}

/// Paints the topbar/sidebar/editor into their "connecting" state and
/// restores whichever tab the switch landed on. Split out of
/// `handle_connect_clicked` since it's pure UI work, no driver/tokio
/// involved.
#[allow(clippy::too_many_arguments)]
fn paint_connect_ui(
    w: &MainWindow,
    sc: &rdb_connstore::SavedConnection,
    db_ovr: Option<&str>,
    conn_idx: i32,
    init_active: Option<&str>,
    init_active_p1: Option<&str>,
    init_active_group: usize,
    standby: bool,
    store: &Rc<RefCell<rdb_connstore::ConnStore>>,
    workspace_tabs: &Arc<Mutex<Vec<WorkspaceTab>>>,
    query_console: &Arc<Mutex<Vec<String>>>,
    load_editor_text: &PaneTextFn,
    restore_tab: &WindowPaneFn,
) {
    {
        let tabs = workspace_tabs.lock().unwrap();
        set_workspace_tabs(w, &tabs, init_active);
    }
    if !standby {
        clear_grid(w, 0);
    }
    sync_query_console(w, query_console);
    w.set_selected_conn(conn_idx);
    // Show progress + clear any prior failure immediately.
    w.set_connecting(true);
    // Dim the sidebar tree while the new schema loads so a connection/db
    // switch isn't a silent, frozen-looking reload.
    w.set_tree_loading(true);
    w.set_conn_status(SharedString::from("connecting"));
    w.set_picker_error(SharedString::default());
    w.global::<Theme>()
        .set_accent(theme::accent_or_default(sc.color.as_deref().unwrap_or("")));
    w.set_status_conn(SharedString::from(sc.name.clone()));
    w.set_bc_conn(SharedString::from(sc.name.clone()));
    w.set_active_env_tag_label(theme::env_tag_label(sc.env_tag).into());
    w.set_active_env_tag_color(
        theme::env_tag_color(sc.env_tag).unwrap_or_else(|| theme::accent_or_default("")),
    );
    w.set_bc_db(SharedString::from(
        db_ovr
            .map(str::to_string)
            .or_else(|| sc.database.clone())
            .unwrap_or_default(),
    ));
    w.set_bc_schema(SharedString::from(
        if matches!(sc.engine, rdb_connstore::Engine::Postgres) {
            "public"
        } else {
            ""
        },
    ));
    // Load the restored active tab's SQL, else empty (the engine hint shows
    // as a ghost placeholder rendered by CodeEditor).
    let init_text = init_active
        .and_then(|id| {
            workspace_tabs
                .lock()
                .unwrap()
                .iter()
                .find(|t| t.id == id)
                .map(|t| t.query_text.clone())
        })
        .unwrap_or_default();
    load_editor_text(0, &init_text);
    w.set_editor_placeholder(SharedString::from(if mock::mock_mode() {
        ""
    } else {
        crate::query_parse::editor_hint(sc.engine)
    }));
    w.set_active_table(SharedString::default());
    // Seed the browse page size from the engine default (Mongo = 20).
    let dft_limit = default_browse_limit(sc.engine);
    w.set_limit_text(SharedString::from(dft_limit.to_string()));
    w.set_filter_operators(ModelRc::from(Rc::new(VecModel::from(filter_operators(
        sc.engine,
    )))));
    w.set_filter_op(SharedString::from("="));
    // Mirror the operator list to the right split pane's filter row.
    w.set_p1_filter_operators(ModelRc::from(Rc::new(VecModel::from(filter_operators(
        sc.engine,
    )))));
    w.set_p1_filter_op(SharedString::from("="));
    // Keep the active tab's last result visible instead of blanking the
    // grid. No-op if the tab has no stored result yet (first connect).
    let active_idx = init_active.and_then(|id| {
        workspace_tabs
            .lock()
            .unwrap()
            .iter()
            .position(|t| t.id == id)
    });
    if let Some(idx) = active_idx {
        restore_tab(w, idx);
    }
    // Restore the right group's active tab, then land on the group that was
    // focused last session.
    if let Some(p1_id) = init_active_p1 {
        let p1_idx = workspace_tabs
            .lock()
            .unwrap()
            .iter()
            .position(|t| t.id == p1_id);
        if let Some(idx) = p1_idx {
            restore_tab(w, idx);
        }
    }
    w.set_active_pane(init_active_group as i32);
    // restore_tab() above re-syncs chrome to the repainted tab's own
    // connection as a side effect. If that tab belongs to a different
    // connection, its side effect drags the topbar back; reapply the
    // picked connection last so it wins.
    sync_conn_chrome(w, &store.borrow(), Some(&sc.id));
}

/// Claims the driver slot, connects on tokio, and publishes the result
/// back to the UI. Aborts any still-running connect from a previous
/// selection first, so switching mid-connect can't leak the old task.
#[allow(clippy::too_many_arguments)]
fn spawn_connect_task(
    rt: Arc<tokio::runtime::Runtime>,
    weak: slint::Weak<MainWindow>,
    sc: &rdb_connstore::SavedConnection,
    cfg: rdb_connstore::Result<rdb_core::conn::ConnConfig>,
    current: DriverSlot,
    driver_pool: DriverPool,
    connected_ids: Arc<Mutex<HashSet<String>>>,
    raw_nodes: Arc<Mutex<Vec<model::VmTreeNode>>>,
    completion_nodes: Arc<Mutex<Vec<model::VmTreeNode>>>,
    fn_defs: Arc<Mutex<HashMap<String, String>>>,
    expanded_tables: Arc<Mutex<HashSet<String>>>,
    loaded_dbs: Arc<Mutex<HashSet<String>>>,
    connect_handle: Rc<RefCell<Option<tokio::task::JoinHandle<()>>>>,
) {
    let weak2 = weak.clone();
    let store_driver = current.clone();
    // Claim the driver slot now, so a query/browse task spawned right
    // after this click can't observe a stale None; it awaits this lock
    // instead and resolves once connect lands.
    let claimed = current.clone().try_lock_owned().ok();
    let engine = sc.engine;
    let connection_id = sc.id.clone();
    // NoSQL collection cap to push onto the fresh connection (Mongo only).
    let nosql_limit = weak
        .upgrade()
        .map(|w| w.get_nosql_collection_limit().max(1) as usize)
        .unwrap_or(200);
    let handle = rt.spawn(async move {
        let slot = match claimed {
            Some(g) => g,
            None => store_driver.clone().lock_owned().await,
        };
        let timeout_secs = if cfg.as_ref().ok().and_then(|c| c.ssh.as_ref()).is_some() {
            25
        } else {
            15
        };
        match attempt_connect(engine, cfg, nosql_limit, timeout_secs).await {
            Ok((driver, schema, scoped_db)) => {
                finish_connect_success(
                    weak2,
                    engine,
                    driver,
                    schema,
                    scoped_db,
                    connection_id,
                    slot,
                    store_driver,
                    driver_pool,
                    connected_ids,
                    expanded_tables,
                    loaded_dbs,
                    raw_nodes,
                    completion_nodes,
                    fn_defs,
                )
                .await;
            }
            Err(e) => {
                // Drop the claimed lock without touching its content — a
                // failed reconnect must leave whatever driver was already
                // `current` untouched.
                drop(slot);
                finish_connect_failure(weak2, e);
            }
        }
    });
    // Abort any still-running connect first, or switching mid-connect
    // leaks the old task and hangs the UI holding the driver lock.
    if let Some(old) = connect_handle.borrow_mut().take() {
        old.abort();
    }
    *connect_handle.borrow_mut() = Some(handle);
}

pub(crate) fn wire(window: &MainWindow, state: &AppState, fns: &AppFns) {
    let AppState {
        rt,
        store,
        current,
        driver_pool,
        collapsed,
        workspace_tabs,
        current_connection_id,
        connected_ids,
        connect_handle,
        conn_modal_map,
        ..
    } = state.clone();

    // ----- background health poll: flip the breadcrumb dot red when a live
    // connection stops answering, green again when it recovers -----
    // ponytail: one fixed 10s loop for the lifetime of the app; make the
    // interval configurable only if asked.
    {
        let weak = window.as_weak();
        let current = current.clone();
        let driver_pool = driver_pool.clone();
        let workspace_tabs = workspace_tabs.clone();
        let current_connection_id = current_connection_id.clone();
        let connected_ids = connected_ids.clone();
        rt.spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                // Close every pooled connection no open tab references anymore
                // (plus whichever one is actively focused, even between its
                // last tab closing and a new one opening). Piggybacked on this
                // existing tick rather than a second timer — up to 10s of a
                // closed tab's connection lingering is a fine trade for one
                // fewer moving part.
                let evicted: HashSet<String> = {
                    let mut live = live_connection_ids(&workspace_tabs.lock().unwrap());
                    if let Some(id) = current_connection_id.lock().unwrap().clone() {
                        live.insert(id);
                    }
                    // Single pass: `retain` decides what stays, and the ids it
                    // drops are exactly the ones eviction needs downstream —
                    // no separate filter pass over the same keys beforehand.
                    let mut evicted = HashSet::new();
                    driver_pool.write().await.retain(|id, _| {
                        let keep = live.contains(id);
                        if !keep {
                            evicted.insert(id.clone());
                        }
                        keep
                    });
                    evicted
                };
                if !evicted.is_empty() {
                    // A connection can go from "connected" to evicted without
                    // ever going through the explicit disconnect handler
                    // (every tab that named it just got closed) — keep the
                    // sidebar dot's source of truth in step here too.
                    {
                        let mut ids = connected_ids.lock().unwrap();
                        ids.retain(|id| !evicted.contains(id));
                    }
                    let weak = weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = weak.upgrade() {
                            w.invoke_refresh_connections();
                        }
                    });
                }
                // None = no driver (picker); Some(ok) = pinged a live connection.
                // Clone the driver out of the mutex before pinging so a slow ping
                // never blocks an in-flight query.
                let driver = {
                    let guard = current.lock().await;
                    guard.as_ref().map(|(_, d)| d.clone())
                };
                let alive = match driver {
                    Some(driver) => Some(driver.ping().await.is_ok()),
                    None => None,
                };
                let Some(ok) = alive else { continue };
                let weak = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak.upgrade() {
                        // Only touch a live workspace; never override "connecting".
                        if w.get_connected() {
                            w.set_conn_status(SharedString::from(if ok {
                                "connected"
                            } else {
                                "error"
                            }));
                        }
                    }
                });
            }
        });
    }
    {
        let weak = window.as_weak();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_modal_map = conn_modal_map.clone();
        window.on_open_conn_modal(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let (items, map) = build_conn_palette_items(&store.borrow(), &collapsed.borrow(), "");
            *conn_modal_map.borrow_mut() = map;
            w.set_conn_items(ModelRc::from(Rc::new(VecModel::from(group_palette_items(
                items,
            )))));
            w.set_conn_modal_open(true);
        });
    }
    {
        let weak = window.as_weak();
        let conn_modal_map = conn_modal_map.clone();
        window.on_conn_choose(move |idx| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let store_idx = conn_modal_map
                .borrow()
                .get(idx.max(0) as usize)
                .copied()
                .unwrap_or(-1);
            w.set_conn_modal_open(false);
            if store_idx >= 0 {
                w.invoke_connect_clicked(store_idx);
            }
        });
    }

    // ----- connect: spawn driver work on tokio, push schema back to UI -----
    {
        let weak = window.as_weak();
        let state = state.clone();
        let fns = fns.clone();
        window.on_connect_clicked(move |idx| {
            handle_connect_clicked(&state, &fns, weak.clone(), idx);
        });
    }

    // ----- cancel an in-flight connect -----
    {
        let weak = window.as_weak();
        let connect_handle = connect_handle.clone();
        window.on_cancel_connect(move || {
            if let Some(h) = connect_handle.borrow_mut().take() {
                h.abort();
            }
            if let Some(w) = weak.upgrade() {
                w.set_connecting(false);
                w.set_connected(false);
                w.set_picker_error(SharedString::from("connection cancelled"));
            }
        });
    }

    // ----- drag-resize a grid column: add the drag delta to its width -----
    {
        let weak = window.as_weak();
        window.on_resize_grid_col(move |i, delta| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let mut v: Vec<f32> = w.get_grid_col_widths().iter().collect();
            let idx = i as usize;
            if idx < v.len() {
                v[idx] = (v[idx] + delta).clamp(60.0, 1000.0);
                w.set_grid_col_widths(ModelRc::from(Rc::new(VecModel::from(v))));
            }
        });
    }
}
