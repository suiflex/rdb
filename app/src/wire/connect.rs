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

pub(crate) fn wire(window: &MainWindow, state: &AppState, fns: &AppFns) {
    let AppState {
        rt,
        store,
        panes,
        current,
        cur_engine,
        collapsed,
        raw_nodes,
        completion_nodes,
        expanded_tables,
        loaded_dbs,
        collapsed_categories,
        workspace_tabs,
        active_tab_id,
        active_group1_tab_id,
        current_connection_id,
        db_override,
        query_console,
        query_number,
        last_view,
        connect_handle,
        fn_defs,
        conn_modal_map,
        tabs_restored,
        ..
    } = state.clone();
    let AppFns {
        load_editor_text,
        save_active_tab,
        restore_tab,
        save_p1_tab,
        ..
    } = fns.clone();
    let browse = panes[0].browse.clone();
    let edit_buf = panes[0].edit_buf.clone();
    let results = panes[0].results.clone();

    // ----- background health poll: flip the breadcrumb dot red when a live
    // connection stops answering, green again when it recovers -----
    // ponytail: one fixed 10s loop for the lifetime of the app; make the
    // interval configurable only if asked.
    {
        let weak = window.as_weak();
        let current = current.clone();
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
        let rt = rt.clone();
        let store = store.clone();
        let completion_nodes = completion_nodes.clone();
        let current = current.clone();
        let raw_nodes = raw_nodes.clone();
        let expanded_tables = expanded_tables.clone();
        let loaded_dbs = loaded_dbs.clone();
        let collapsed_categories = collapsed_categories.clone();
        let cur_engine = cur_engine.clone();
        let load_editor_text = load_editor_text.clone();
        let fn_defs = fn_defs.clone();
        let connect_handle = connect_handle.clone();
        let browse = browse.clone();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let active_group1_tab_id = active_group1_tab_id.clone();
        let current_connection_id = current_connection_id.clone();
        let query_console = query_console.clone();
        let results = results.clone();
        let last_view = last_view.clone();
        let edit_buf = edit_buf.clone();
        let db_override = db_override.clone();
        let tabs_restored = tabs_restored.clone();
        let query_number = query_number.clone();
        let restore_tab = restore_tab.clone();
        let save_active_tab = save_active_tab.clone();
        let save_p1_tab = save_p1_tab.clone();
        window.on_connect_clicked(move |idx| {
            // Flush the live editor text + results of the active tab(s) into the
            // workspace model before it is rebuilt for the new connection. Without
            // this a connection switch reads stale/empty tab text (the query looked
            // duplicated or lost) and dropped the results of the inactive tabs.
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
            // First connect of the session (or a database switch) restores the
            // persisted SQL scratch tabs; a later switch to another connection
            // keeps the SQL scratch tabs so the user can hop between connections
            // without losing their queries.
            let restore = should_restore_query_tabs(tabs_restored.get());
            tabs_restored.set(true);
            let (init_tabs, init_active, init_active_p1, init_active_group) = if restore {
                let (tabs, active, active_p1, active_group, max_number) = load_query_tabs();
                // Never let a freshly-minted tab reuse a number a restored tab
                // already holds — `fetch_max` only ever raises the counter.
                query_number.fetch_max(max_number, std::sync::atomic::Ordering::Relaxed);
                (tabs, active, active_p1, active_group)
            } else {
                // Retain every open tab across the switch so the workspace
                // behaves like a set of persistent documents — the SQL scratch
                // tabs keep their results ("standby") and the connection-scoped
                // table/collection tabs stay open too, their last data now a
                // snapshot until the user hits Refresh against the new
                // connection. Only the in-flight loading flag is cleared so no
                // tab is left showing a stuck spinner.
                let mut kept: Vec<WorkspaceTab> =
                    std::mem::take(&mut *workspace_tabs.lock().unwrap())
                        .into_iter()
                        .collect();
                for t in &mut kept {
                    t.loading = false;
                }
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
            // Standby: a surviving SQL tab stays active across the switch, so its
            // last result is still meaningful — leave it on screen instead of
            // blanking. Only a fresh restore (first connect / DB switch) clears.
            let standby = !restore && init_active.is_some();
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
                {
                    let tabs = workspace_tabs.lock().unwrap();
                    set_workspace_tabs(&w, &tabs, init_active.as_deref());
                }
                if !standby {
                    clear_grid(&w, 0);
                }
                sync_query_console(&w, &query_console);
                w.set_selected_conn(idx);
                // Show progress + clear any prior failure immediately.
                w.set_connecting(true);
                // Dim the sidebar tree while the new schema loads so a
                // connection/db switch isn't a silent, frozen-looking reload.
                w.set_tree_loading(true);
                w.set_conn_status(SharedString::from("connecting"));
                w.set_picker_error(SharedString::default());
                w.global::<Theme>()
                    .set_accent(theme::accent_or_default(sc.color.as_deref().unwrap_or("")));
                w.set_status_conn(SharedString::from(sc.name.clone()));
                w.set_bc_conn(SharedString::from(sc.name.clone()));
                w.set_active_env_tag_label(theme::env_tag_label(sc.env_tag).into());
                w.set_active_env_tag_color(
                    theme::env_tag_color(sc.env_tag)
                        .unwrap_or_else(|| theme::accent_or_default("")),
                );
                w.set_bc_db(SharedString::from(
                    db_ovr
                        .clone()
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
                // Load the restored active tab's SQL, else empty (the engine hint
                // shows as a ghost placeholder rendered by CodeEditor).
                let init_text = init_active
                    .as_deref()
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
                w.set_p1_filter_operators(ModelRc::from(Rc::new(VecModel::from(
                    filter_operators(sc.engine),
                ))));
                w.set_p1_filter_op(SharedString::from("="));
                // Re-present the active tab's stored result so a connection
                // switch keeps the last result visible instead of blanking the
                // grid. No-op (clears) when the tab has no stored result, e.g.
                // the disk-restored tabs on the first connect of the session.
                let active_idx = init_active.as_deref().and_then(|id| {
                    workspace_tabs
                        .lock()
                        .unwrap()
                        .iter()
                        .position(|t| t.id == id)
                });
                if let Some(idx) = active_idx {
                    restore_tab(&w, idx);
                }
                // Restore the right group's active tab, then land on the group
                // that was focused last session.
                if let Some(p1_id) = init_active_p1.as_deref() {
                    let p1_idx = workspace_tabs
                        .lock()
                        .unwrap()
                        .iter()
                        .position(|t| t.id == p1_id);
                    if let Some(idx) = p1_idx {
                        restore_tab(&w, idx);
                    }
                }
                w.set_active_pane(init_active_group as i32);
            }
            // Fresh connection: nothing browsed, nothing expanded.
            *cur_engine.borrow_mut() = Some(sc.engine);
            expanded_tables.lock().unwrap().clear();
            loaded_dbs.lock().unwrap().clear();
            *collapsed_categories.borrow_mut() = default_collapsed_cats();
            let weak2 = weak.clone();
            let store_driver = current.clone();
            // Claim the driver slot synchronously, before any query/browse
            // task spawned after this click can observe a stale None: those
            // tasks await this lock and resolve once the connect lands.
            let claimed = current.clone().try_lock_owned().ok();
            let engine = sc.engine;
            let raw_nodes = raw_nodes.clone();
            let completion_nodes = completion_nodes.clone();
            let fn_defs = fn_defs.clone();
            let expanded_tables = expanded_tables.clone();
            let loaded_dbs = loaded_dbs.clone();
            // NoSQL collection cap to push onto the fresh connection (Mongo only).
            let nosql_limit = weak
                .upgrade()
                .map(|w| w.get_nosql_collection_limit().max(1) as usize)
                .unwrap_or(200);
            let handle = rt.spawn(async move {
                let mut slot = match claimed {
                    Some(g) => g,
                    None => store_driver.clone().lock_owned().await,
                };
                *slot = None;
                // Bound the attempt so an unreachable host can't spin forever;
                // the Cancel button aborts sooner.
                let attempt = async {
                    let cfg =
                        cfg.map_err(|e| rdb_core::error::RdbError::Connection(e.to_string()))?;
                    let driver = AnyDriver::connect(engine, &cfg).await?;
                    // Apply the NoSQL collection cap before any schema fetch so
                    // the first sidebar load already honors it (Mongo only).
                    driver.set_collection_limit(nosql_limit);
                    // MongoDB: when the connection names a database, scope the
                    // sidebar to it (matching the schema switcher) instead of
                    // listing every database on the server.
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
                let result =
                    match tokio::time::timeout(std::time::Duration::from_secs(15), attempt).await {
                        Ok(r) => r,
                        Err(_) => Err(rdb_core::error::RdbError::Connection(
                            "connection timed out".into(),
                        )),
                    };

                match result {
                    Ok((driver, schema, scoped_db)) => {
                        // Postgres: list real namespaces so the sidebar schema
                        // switcher offers more than "public". Engine-specific SQL
                        // lives in the driver, not here.
                        let pg_schemas: Vec<SharedString> =
                            if matches!(engine, rdb_connstore::Engine::Postgres) {
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
                        // Databases on the server, backing the breadcrumb switcher.
                        // Empty for engines that can't switch database.
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
                        let nodes = model::to_tree_model(&schema);
                        let fields = model::to_structure_model(&schema);
                        // Scoped Mongo tree holds only the selected database: open
                        // it and mark it loaded so its collections show at once
                        // (mirrors the schema switcher).
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
                        // Stash raw nodes for later expand/collapse rebuilds, and
                        // render the initial view (Functions collapsed, Tables open,
                        // fields hidden). Matches the reseed done on connect above;
                        // collapsed_categories itself is !Send so can't cross here.
                        let rows = schema_display_rows(
                            &nodes,
                            &exp,
                            &default_collapsed_cats(),
                            &loaded,
                            Some(engine),
                            "",
                        );
                        // Postgres browses namespaces, not databases: the
                        // selector must say "public", never the db name (a
                        // `"dbname"."table"` query would fail).
                        let mut schema_names: Vec<SharedString> =
                            if matches!(engine, rdb_connstore::Engine::Postgres) {
                                if pg_schemas.is_empty() {
                                    vec![SharedString::from("public")]
                                } else {
                                    pg_schemas
                                }
                            } else if scoped_db.is_some() && !db_names.is_empty() {
                                // Mongo scoped its tree to one database: still list
                                // every database so the switcher can reach them.
                                db_names.clone()
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
                        // Scoped Mongo starts on its selected database; otherwise
                        // default to "public" when present, else the first name.
                        let schema_current = match &scoped_db {
                            Some(db) => SharedString::from(db.clone()),
                            None => schema_names
                                .iter()
                                .find(|s| s.as_str() == "public")
                                .unwrap_or(&schema_names[0])
                                .clone(),
                        };
                        // Format only makes sense for text query languages.
                        let sql_capable = matches!(
                            rdb_connstore::Engine::language(engine),
                            rdb_connstore::QueryLanguage::Sql | rdb_connstore::QueryLanguage::Cql
                        );
                        *raw_nodes.lock().unwrap() = nodes;
                        // Seed autocomplete with the active schema's tables plus a
                        // bare node for every other schema name, so `schema.`
                        // autocompletes immediately. The remaining schemas' tables
                        // and columns fill in from the background load below.
                        let all_schema_names: Vec<String> =
                            schema_names.iter().map(|s| s.to_string()).collect();
                        {
                            let mut seed = build_completion_seed(
                                &driver,
                                engine,
                                schema_current.as_str(),
                                &schema,
                            )
                            .await;
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
                            for db in &schema.databases {
                                for f in &db.functions {
                                    defs.insert(f.name.clone(), f.definition.clone());
                                }
                            }
                        }
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = weak2.upgrade() {
                                w.set_schema_tree(ModelRc::from(Rc::new(VecModel::from(rows))));
                                w.set_sql_capable(sql_capable);
                                w.set_new_tab_label(
                                    crate::query_parse::language_label(engine).into(),
                                );
                                w.set_schema_name(schema_current);
                                w.set_schema_list(ModelRc::from(Rc::new(VecModel::from(
                                    schema_names,
                                ))));
                                w.set_db_list(ModelRc::from(Rc::new(VecModel::from(db_names))));
                                let sfields: Vec<StructField> = fields
                                    .into_iter()
                                    .map(|f| StructField {
                                        name: f.name.into(),
                                        type_name: f.type_name.into(),
                                        nullable: f.nullable,
                                    })
                                    .collect();
                                w.set_structure_columns(ModelRc::from(Rc::new(VecModel::from(
                                    sfields,
                                ))));
                                w.set_status_latency(SharedString::from("connected"));
                                w.set_conn_status(SharedString::from("connected"));
                                w.set_picker_error(SharedString::default());
                                w.set_connecting(false);
                                w.set_tree_loading(false);
                                // Swap the picker for the workspace.
                                w.set_connected(true);
                            }
                        });
                        // Load every other schema's tables so cross-schema
                        // `schema.table` autocompletes. Runs after the sidebar
                        // (active schema) already rendered; the popup just gains
                        // more names as this fills. Fetched concurrently, one
                        // task per schema, instead of sequentially.
                        if matches!(engine, rdb_connstore::Engine::Postgres)
                            && all_schema_names.len() > 1
                        {
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
                    Err(e) => {
                        eprintln!("connect failed: {e}");
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = weak2.upgrade() {
                                // Stay on the picker; surface the failure there.
                                w.set_connected(false);
                                w.set_connecting(false);
                                w.set_tree_loading(false);
                                // `e` is `RdbError`, whose own `Display` already
                                // reads "connection failed: …" — don't prefix it
                                // again.
                                w.set_picker_error(SharedString::from(format!("{e}")));
                            }
                        });
                    }
                }
            });
            // Abort any still-running connect from a previous selection before
            // tracking the new one: switching connections mid-connect otherwise
            // leaks the old task, which keeps holding the driver lock and hangs
            // the UI with no way to cancel.
            if let Some(old) = connect_handle.borrow_mut().take() {
                old.abort();
            }
            *connect_handle.borrow_mut() = Some(handle);
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
