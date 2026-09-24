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

#[allow(clippy::too_many_arguments)]
fn plan_tab_restore(
    tabs_restored: &Rc<Cell<bool>>,
    workspace_tabs: &Arc<Mutex<Vec<WorkspaceTab>>>,
    active_tab_id: &Arc<Mutex<Option<String>>>,
    active_group1_tab_id: &Arc<Mutex<Option<String>>>,
    query_number: &Arc<std::sync::atomic::AtomicUsize>,
    connection_id: &str,
    scoped: bool,
    badge: &ConnBadgeInfo,
) -> TabRestorePlan {
    let visible = |tab: &WorkspaceTab| tab_visible_for_connection(tab, connection_id, scoped);
    // Only a tab this connection owns may take the focus. The focused tab *is*
    // the active connection (`restore_tab_for_pane`), so landing on another
    // connection's tab would drag the whole context — sidebar tree, engine,
    // driver — straight back to it, and the connect the user just asked for
    // would appear to do nothing. A tab bound to nothing yet is fair game: it
    // latches onto whatever it first runs against.
    let ownable = |tab: &WorkspaceTab| {
        visible(tab)
            && tab
                .connection_id
                .as_deref()
                .is_none_or(|id| id == connection_id)
    };
    let restore = should_restore_query_tabs(tabs_restored.get());
    tabs_restored.set(true);
    let (mut tabs, active, active_p1, mut active_group) = if restore {
        let (tabs, disk_active, disk_active_p1, active_group, max_number) = load_query_tabs();
        query_number.fetch_max(max_number, std::sync::atomic::Ordering::Relaxed);
        let active = disk_active
            .filter(|id| tabs.iter().any(|tab| tab.id == *id && ownable(tab)))
            .or_else(|| {
                tabs.iter()
                    .find(|tab| ownable(tab))
                    .map(|tab| tab.id.clone())
            });
        let active_p1 =
            disk_active_p1.filter(|id| tabs.iter().any(|tab| tab.id == *id && ownable(tab)));
        (tabs, active, active_p1, active_group)
    } else {
        // Retain every open tab across the switch. In scoped mode the renderer
        // hides tabs for other connections, but their state remains available
        // when the user switches back.
        let mut kept: Vec<WorkspaceTab> = std::mem::take(&mut *workspace_tabs.lock().unwrap());
        for t in &mut kept {
            t.loading = false;
        }
        let active = active_tab_id
            .lock()
            .unwrap()
            .clone()
            .filter(|id| kept.iter().any(|tab| tab.id == *id && ownable(tab)))
            .or_else(|| {
                kept.iter()
                    .find(|tab| ownable(tab))
                    .map(|tab| tab.id.clone())
            });
        let active_p1 = active_group1_tab_id
            .lock()
            .unwrap()
            .clone()
            .filter(|id| kept.iter().any(|tab| tab.id == *id && ownable(tab)));
        (kept, active, active_p1, 0usize)
    };
    // Nothing this connection can land on: give it an empty tab of its own
    // rather than leaving the focus on a foreign one.
    let created = active.is_none();
    let active = active.or_else(|| {
        let number = query_number.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let id = format!("query:{connection_id}:{number}");
        let mut tab = WorkspaceTab::sql(id.clone(), number);
        tab.connection_id = Some(connection_id.to_string());
        tab.engine = badge.engine.clone();
        tab.connection_name = badge.name.clone();
        tab.color = badge.color;
        tab.has_custom_color = badge.has_custom_color;
        tabs.push(tab);
        // The fresh tab is in the left group; landing on the right one would
        // focus a group that has nothing to show for this connection.
        active_group = 0;
        Some(id)
    });
    // A brand-new empty tab has no result worth preserving, so it clears like
    // a fresh restore does.
    let standby = !restore && active.is_some() && !created;
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
    pooled: Option<Arc<AnyDriver>>,
) -> Result<(Arc<AnyDriver>, rdb_core::schema::Schema, Option<String>), rdb_core::error::RdbError> {
    let attempt = async {
        let cfg = cfg.map_err(|e| rdb_core::error::RdbError::Connection(e.to_string()))?;
        // Switching back to a connection that is still open (the rail) reuses
        // its driver instead of a new handshake; a dead one falls through to
        // a fresh connect.
        if let Some(driver) = pooled {
            if let Ok((schema, scoped_db)) = initial_schema(&driver, engine, &cfg).await {
                return Ok((driver, schema, scoped_db));
            }
        }
        let driver = AnyDriver::connect(engine, &cfg).await?;
        // Apply the NoSQL collection cap before any schema fetch so the
        // first sidebar load already honors it (Mongo only).
        driver.set_collection_limit(nosql_limit);
        let (schema, scoped_db) = initial_schema(&driver, engine, &cfg).await?;
        Ok::<_, rdb_core::error::RdbError>((Arc::new(driver), schema, scoped_db))
    };
    match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), attempt).await {
        Ok(r) => r,
        Err(_) => Err(rdb_core::error::RdbError::Connection(
            "connection timed out".into(),
        )),
    }
}

/// The first sidebar schema. MongoDB: when the connection names a database,
/// scope the sidebar to it (matching the schema switcher) instead of listing
/// every database on the server.
async fn initial_schema(
    driver: &AnyDriver,
    engine: rdb_connstore::Engine,
    cfg: &rdb_core::conn::ConnConfig,
) -> Result<(rdb_core::schema::Schema, Option<String>), rdb_core::error::RdbError> {
    let scoped_db = if matches!(engine, rdb_connstore::Engine::Mongo) {
        cfg.database.clone().filter(|d| !d.is_empty())
    } else {
        None
    };
    let schema = match &scoped_db {
        Some(db) => driver.schema_for(db).await?,
        None => driver.schema().await?,
    };
    Ok((schema, scoped_db))
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
    driver: Arc<AnyDriver>,
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
    current_connection_id: Arc<Mutex<Option<String>>>,
    painted_connection: Arc<Mutex<Option<String>>>,
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
    *slot = Some((engine, driver.clone()));
    drop(slot);
    driver_pool
        .write()
        .await
        .insert(connection_id.clone(), (engine, driver.clone()));
    // Now genuinely connected — the sidebar dot and "another connection is
    // still around" checks read this, not tab scoping (see `connected_ids`
    // on `AppState`).
    connected_ids.lock().unwrap().insert(connection_id.clone());
    // The pool and the live-ids set above are facts about the connection and
    // are recorded either way. Everything below repaints the workspace *as*
    // this connection, and a tab switch is a context switch now, so the user
    // may have moved to another one while this schema was in flight. Painting
    // anyway drops the other connection's tree, autocomplete and schema list
    // on top of the workspace they are actually looking at.
    if current_connection_id.lock().unwrap().as_deref() != Some(connection_id.as_str()) {
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = weak.upgrade() {
                // It did connect, so nothing should be left spinning; the
                // sidebar dot for it goes live off `connected_ids` above.
                w.set_connecting(false);
                w.set_tree_loading(false);
                w.invoke_refresh_connections();
            }
        });
        return;
    }
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
    let sql_capable = rdb_connstore::Engine::language(engine).is_statement_text();
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
        // What the workspace now shows, for `ConnContext`'s snapshot on the
        // way out.
        *painted_connection.lock().unwrap() = Some(connection_id);
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

/// Files the workspace's current state under `connection_id`, so coming back
/// to it later is a restore rather than a reconnect.
///
/// A connection still loading has nothing worth filing: the entry is dropped
/// instead, and the next switch to it fetches for real.
#[allow(clippy::too_many_arguments)]
fn snapshot_conn_context(
    w: &MainWindow,
    contexts: &ConnContexts,
    connection_id: &str,
    raw_nodes: &Arc<Mutex<Vec<model::VmTreeNode>>>,
    completion_nodes: &Arc<Mutex<Vec<model::VmTreeNode>>>,
    fn_defs: &Arc<Mutex<HashMap<String, String>>>,
    expanded_tables: &Arc<Mutex<HashSet<String>>>,
    loaded_dbs: &Arc<Mutex<HashSet<String>>>,
    collapsed_categories: &Rc<RefCell<HashSet<String>>>,
) {
    let nodes = raw_nodes.lock().unwrap().clone();
    if w.get_tree_loading() || nodes.is_empty() {
        contexts.borrow_mut().remove(connection_id);
        return;
    }
    let ctx = ConnContext {
        nodes,
        completion: completion_nodes.lock().unwrap().clone(),
        fn_defs: fn_defs.lock().unwrap().clone(),
        expanded: expanded_tables.lock().unwrap().clone(),
        loaded: loaded_dbs.lock().unwrap().clone(),
        collapsed_cats: collapsed_categories.borrow().clone(),
        schema_names: w.get_schema_list().iter().collect(),
        schema_current: w.get_schema_name(),
        db_names: w.get_db_list().iter().collect(),
        fields: w.get_structure_columns().iter().collect(),
        sql_capable: w.get_sql_capable(),
        new_tab_label: w.get_new_tab_label(),
    };
    contexts.borrow_mut().insert(connection_id.to_string(), ctx);
}

/// Paints the workspace as `ctx`'s connection: sidebar tree, autocomplete
/// data, schema and database pickers, structure columns. Entirely local.
#[allow(clippy::too_many_arguments)]
fn restore_conn_context(
    w: &MainWindow,
    ctx: &ConnContext,
    engine: rdb_connstore::Engine,
    raw_nodes: &Arc<Mutex<Vec<model::VmTreeNode>>>,
    completion_nodes: &Arc<Mutex<Vec<model::VmTreeNode>>>,
    fn_defs: &Arc<Mutex<HashMap<String, String>>>,
    expanded_tables: &Arc<Mutex<HashSet<String>>>,
    loaded_dbs: &Arc<Mutex<HashSet<String>>>,
    collapsed_categories: &Rc<RefCell<HashSet<String>>>,
    sidebar_filter: &Arc<Mutex<String>>,
) {
    *raw_nodes.lock().unwrap() = ctx.nodes.clone();
    *completion_nodes.lock().unwrap() = ctx.completion.clone();
    *fn_defs.lock().unwrap() = ctx.fn_defs.clone();
    *expanded_tables.lock().unwrap() = ctx.expanded.clone();
    *loaded_dbs.lock().unwrap() = ctx.loaded.clone();
    *collapsed_categories.borrow_mut() = ctx.collapsed_cats.clone();
    let rows = schema_display_rows(
        &ctx.nodes,
        &ctx.expanded,
        &ctx.collapsed_cats,
        &ctx.loaded,
        Some(engine),
        &sidebar_filter.lock().unwrap().clone(),
    );
    w.set_schema_tree(ModelRc::from(Rc::new(VecModel::from(rows))));
    w.set_schema_list(ModelRc::from(Rc::new(VecModel::from(
        ctx.schema_names.clone(),
    ))));
    w.set_schema_name(ctx.schema_current.clone());
    w.set_db_list(ModelRc::from(Rc::new(VecModel::from(ctx.db_names.clone()))));
    w.set_structure_columns(ModelRc::from(Rc::new(VecModel::from(ctx.fields.clone()))));
    w.set_sql_capable(ctx.sql_capable);
    w.set_new_tab_label(ctx.new_tab_label.clone());
    w.set_tree_loading(false);
}

/// Point the app's whole connection context at `connection_id`: the engine,
/// the `current` driver slot, the sidebar tree, the completion data and the
/// schema picker.
///
/// Built once and handed to `restore_tab_for_pane`, which is what makes "the
/// focused tab's connection *is* the active connection" true rather than
/// aspirational. Before this, a tab switch moved `current_connection_id` and
/// the topbar while everything else stayed pinned to whichever connection was
/// last picked from the rail — so the sidebar listed one database's tables
/// while opening one of them bound the new tab to another database, which
/// across two different engines produces a tab that simply cannot run.
///
/// No handshake: `spawn_connect_task` with `reuse_pooled` takes the driver the
/// pool already holds and only re-reads the schema.
pub(crate) fn build_activate_connection(state: &AppState) -> WindowConnFn {
    let AppState {
        rt,
        store,
        current,
        driver_pool,
        connected_ids,
        raw_nodes,
        completion_nodes,
        fn_defs,
        expanded_tables,
        loaded_dbs,
        collapsed_categories,
        connect_handle,
        cur_engine,
        current_connection_id,
        conn_contexts,
        painted_connection,
        sidebar_filter,
        ..
    } = state.clone();
    // `connect_clicked` paints, which restores a tab, which lands back here.
    // While this is set, activation does the synchronous part and gets out of
    // its own way.
    let reconnecting = Rc::new(Cell::new(false));
    // Connections already auto-connected once for a tab. An unreachable host
    // costs a 15s timeout (25s over SSH) per attempt, and without this every
    // click on its tab would pay that again. Cleared by an explicit connect or
    // disconnect, so retrying by hand always works.
    let auto_attempted: Rc<RefCell<HashSet<String>>> = Rc::new(RefCell::new(HashSet::new()));
    Rc::new(move |w: &MainWindow, connection_id: &str| {
        let Some(sc) = store
            .borrow()
            .list()
            .iter()
            .find(|c| c.id == connection_id)
            .cloned()
        else {
            return;
        };
        // Synchronous, and the part that matters most: every
        // `let Some(engine) = *cur_engine.borrow()` guard (open table, browse
        // SQL, add column) and the format/comment/stream forks read this on
        // the very next click, long before any schema comes back.
        *cur_engine.borrow_mut() = Some(sc.engine);
        // The tab's connection is not open. Half-switching to it is what left
        // the workspace contradicting itself: the topbar, the rail selection
        // and every "what does a new action target" pointer moved to a
        // connection with no driver, while the sidebar tree and the driver
        // slot stayed on the live one — so New Query and opening a table
        // aimed somewhere that could not answer. Open it instead, which is
        // also what makes the rail honest: a tile is shown for the selected
        // connection whether or not it is live, so a selection that never
        // becomes live is a tile that appears and vanishes with tab focus.
        if !connected_ids.lock().unwrap().contains(connection_id) {
            if reconnecting.get()
                || !auto_attempted
                    .borrow_mut()
                    .insert(connection_id.to_string())
            {
                return;
            }
            let Some(idx) = store
                .borrow()
                .list()
                .iter()
                .position(|c| c.id == connection_id)
            else {
                return;
            };
            reconnecting.set(true);
            // The whole real connect path: tab restore, spinner, pooled
            // driver reuse. It repaints, which restores a tab, which reaches
            // this closure again — hence the guard.
            w.invoke_connect_clicked(idx as i32);
            reconnecting.set(false);
            return;
        }
        // It answered, so a later drop is worth auto-connecting through again.
        auto_attempted.borrow_mut().remove(connection_id);
        // Put the connection being left behind in the drawer before opening
        // another one over the top of it.
        let outgoing = painted_connection.lock().unwrap().clone();
        if let Some(prev) = outgoing.filter(|prev| prev != connection_id) {
            snapshot_conn_context(
                w,
                &conn_contexts,
                &prev,
                &raw_nodes,
                &completion_nodes,
                &fn_defs,
                &expanded_tables,
                &loaded_dbs,
                &collapsed_categories,
            );
        }
        // Already open: everything the workspace shows about it is in memory,
        // so this is a swap. No connect, no schema read, and above all no
        // claim on the `current` driver slot that the sidebar's lazy expand
        // would then have to queue behind.
        let cached = conn_contexts.borrow().get(connection_id).cloned();
        if let Some(ctx) = cached {
            restore_conn_context(
                w,
                &ctx,
                sc.engine,
                &raw_nodes,
                &completion_nodes,
                &fn_defs,
                &expanded_tables,
                &loaded_dbs,
                &collapsed_categories,
                &sidebar_filter,
            );
            *painted_connection.lock().unwrap() = Some(connection_id.to_string());
            // The legacy single-driver slot still backs anything that has no
            // tab to resolve against; point it at this connection's pooled
            // driver without blocking the UI on the lock.
            let cid = connection_id.to_string();
            let current = current.clone();
            let driver_pool = driver_pool.clone();
            rt.spawn(async move {
                let entry = driver_pool.read().await.get(&cid).cloned();
                *current.lock().await = entry;
            });
            return;
        }
        let cfg = store.borrow().conn_config_for(&sc.id);
        // Nothing cached for it (first activation of a connection this
        // session): fall through to the real fetch. The tree about to be
        // replaced describes the previous connection.
        expanded_tables.lock().unwrap().clear();
        loaded_dbs.lock().unwrap().clear();
        *collapsed_categories.borrow_mut() = default_collapsed_cats();
        w.set_tree_loading(true);
        spawn_connect_task(
            rt.clone(),
            w.as_weak(),
            &sc,
            cfg,
            current.clone(),
            driver_pool.clone(),
            connected_ids.clone(),
            raw_nodes.clone(),
            completion_nodes.clone(),
            fn_defs.clone(),
            expanded_tables.clone(),
            loaded_dbs.clone(),
            connect_handle.clone(),
            current_connection_id.clone(),
            painted_connection.clone(),
            true,
        );
    })
}

/// A connect task that panicked still has to clear the picker's
/// "Connecting…" state; nothing else will, and the task it was waiting on is
/// gone. An abort is not a failure — Cancel and a superseding connect both
/// use it — so that reports nothing.
///
/// Debug builds only: `panic = "abort"` in the release profile takes the whole
/// process down before the join can ever resolve.
fn panic_to_connection_error(e: tokio::task::JoinError) -> Option<rdb_core::error::RdbError> {
    e.is_panic()
        .then(|| rdb_core::error::RdbError::Connection("connect failed unexpectedly".into()))
}

/// Passes an abort through to the task it wraps, so watching a task from
/// another one leaves `abort()` on the watcher meaning what it meant before.
struct AbortOnDrop(tokio::task::JoinHandle<()>);

impl Drop for AbortOnDrop {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// `on_connect_clicked`: flush the active tab, resolve the picked
/// connection, reset workspace state for it, paint the UI immediately,
/// then spawn the driver connect on tokio.
fn handle_connect_clicked(state: &AppState, fns: &AppFns, weak: slint::Weak<MainWindow>, idx: i32) {
    let AppState {
        rt,
        store,
        panes,
        settings,
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
        painted_connection,
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
        &sc.id,
        settings.borrow().get().ui_state.query_tabs_by_connection,
        &connection_badge_info(&store.borrow(), &sc.id),
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
        current_connection_id,
        painted_connection,
        // A database switch needs a driver on the new database, never the
        // pooled one.
        db_ovr.is_none(),
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
    w.set_query_scope_connection(SharedString::from(sc.id.clone()));
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
    current_connection_id: Arc<Mutex<Option<String>>>,
    painted_connection: Arc<Mutex<Option<String>>>,
    reuse_pooled: bool,
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
    let inner = rt.spawn(async move {
        let slot = match claimed {
            Some(g) => g,
            None => store_driver.clone().lock_owned().await,
        };
        let timeout_secs = if cfg.as_ref().ok().and_then(|c| c.ssh.as_ref()).is_some() {
            25
        } else {
            15
        };
        let pooled = if reuse_pooled {
            let pool = driver_pool.read().await;
            pool.get(&connection_id).map(|(_, d)| d.clone())
        } else {
            None
        };
        match attempt_connect(engine, cfg, nosql_limit, timeout_secs, pooled).await {
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
                    current_connection_id,
                    painted_connection,
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
    // Watch that task, so a panic inside it still clears "Connecting…"
    // instead of leaving the picker spinning on a task that is already gone.
    // The stored handle is this watcher, and `AbortOnDrop` passes an abort
    // through to the connect itself, so Cancel behaves exactly as before.
    let watch_weak = weak.clone();
    let handle = rt.spawn(async move {
        let mut connect = AbortOnDrop(inner);
        if let Err(join) = (&mut connect.0).await {
            if let Some(err) = panic_to_connection_error(join) {
                finish_connect_failure(watch_weak, err);
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
        collapsed,
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
        // No idle eviction here any more: a pooled connection used to be
        // closed once no open tab referenced it, which pulled it out of the
        // connections rail on the next tick (a third connection with no tab
        // of its own vanished as soon as you switched away). Open connections
        // now stay until they are disconnected explicitly.
        rt.spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(10)).await;
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
        let connected_ids = connected_ids.clone();
        window.on_open_conn_modal(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            fill_conn_modal(
                &w,
                &store.borrow(),
                &collapsed.borrow(),
                &connected_ids,
                &conn_modal_map,
            );
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

#[cfg(test)]
mod panic_to_connection_error_tests {
    use super::panic_to_connection_error;

    // The panic case has no test: this binary cannot unwind, so provoking a
    // real panic aborts the whole test process ("failed to initiate panic")
    // rather than handing back a `JoinError` — the same reason CLAUDE.md warns
    // that a failing test here aborts instead of reporting. What that case
    // does is one `is_panic()` call; the case worth pinning is the other one,
    // where reporting anything at all would be wrong.

    /// Cancel and a superseding connect both abort the task; neither is a
    /// failure worth putting in front of the user.
    #[tokio::test]
    async fn an_aborted_task_reports_nothing() {
        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        });
        handle.abort();
        let join = handle.await.expect_err("the task was aborted");
        assert!(panic_to_connection_error(join).is_none());
    }
}

#[cfg(test)]
mod plan_tab_restore_tests {
    use super::*;

    /// `tabs_restored` pre-set, so the plan takes the in-memory path and no
    /// test touches the on-disk tab file.
    fn plan(
        tabs: Vec<WorkspaceTab>,
        active: Option<&str>,
        connection_id: &str,
        scoped: bool,
    ) -> TabRestorePlan {
        plan_tab_restore(
            &Rc::new(Cell::new(true)),
            &Arc::new(Mutex::new(tabs)),
            &Arc::new(Mutex::new(active.map(str::to_string))),
            &Arc::new(Mutex::new(None)),
            &Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            connection_id,
            scoped,
            &ConnBadgeInfo::default(),
        )
    }

    fn tab(id: &str, connection_id: Option<&str>) -> WorkspaceTab {
        let mut t = WorkspaceTab::sql(id.to_string(), 1);
        t.connection_id = connection_id.map(str::to_string);
        t
    }

    /// The focused tab is the active connection, so landing on another
    /// connection's tab would pull the context straight back off the
    /// connection the user just picked.
    #[test]
    fn connecting_mints_a_tab_when_every_open_one_belongs_elsewhere() {
        for scoped in [false, true] {
            let plan = plan(
                vec![tab("a1", Some("conn-a"))],
                Some("a1"),
                "conn-b",
                scoped,
            );
            let active = plan.active.expect("a connection always lands somewhere");
            let landed = plan
                .tabs
                .iter()
                .find(|t| t.id == active)
                .expect("the active id names a tab in the plan");
            assert_eq!(landed.connection_id.as_deref(), Some("conn-b"));
            // The other connection's tab survives the switch.
            assert!(plan.tabs.iter().any(|t| t.id == "a1"));
            // A tab minted empty has no result to keep on screen.
            assert!(!plan.standby);
        }
    }

    #[test]
    fn connecting_lands_on_a_tab_it_already_owns() {
        let plan = plan(
            vec![tab("a1", Some("conn-a")), tab("b1", Some("conn-b"))],
            Some("a1"),
            "conn-b",
            false,
        );
        assert_eq!(plan.active.as_deref(), Some("b1"));
        assert_eq!(plan.tabs.len(), 2);
        assert!(plan.standby);
    }

    /// A tab that has never run is bound to nothing and latches onto the
    /// first connection it runs against, so it is a fine place to land.
    #[test]
    fn an_unbound_tab_is_reused_rather_than_replaced() {
        let plan = plan(vec![tab("q1", None)], Some("q1"), "conn-b", false);
        assert_eq!(plan.active.as_deref(), Some("q1"));
        assert_eq!(plan.tabs.len(), 1);
    }
}
