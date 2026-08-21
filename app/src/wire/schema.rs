//! Schema-level UI: the Open Database (⌘⇧O) and Open Connection (⌘O) modals,
//! the sidebar schema switcher, creating a database or schema from the
//! breadcrumb, and the new-table designer.
//!
//! Split out of `main`; the handler bodies are unchanged.

use std::rc::Rc;

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::*;

pub(crate) fn wire(window: &MainWindow, state: &AppState, fns: &AppFns) {
    let AppState {
        rt,
        panes,
        current,
        cur_engine,
        raw_nodes,
        expanded_tables,
        loaded_dbs,
        db_override,
        table_cols,
        query_console,
        completion_nodes,
        collapsed_categories,
        workspace_tabs,
        active_tab_id,
        ..
    } = state.clone();
    let AppFns {
        load_editor_text, ..
    } = fns.clone();
    let browse = panes[0].browse.clone();
    let displayed_grid = panes[0].displayed_grid.clone();
    let results = panes[0].results.clone();
    let ed_state = panes[0].ed_state.clone();

    // ----- Open database (⌘⇧O) / Open Connection (⌘O) modals -----
    {
        let weak = window.as_weak();
        window.on_open_db_modal(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            // Real databases enumerated on connect and cached in `db-list`. Empty
            // for engines that can't switch (single/implicit database).
            let items: Vec<PaletteItem> = w
                .get_db_list()
                .iter()
                .map(|d| PaletteItem {
                    label: d,
                    kind: "database".into(),
                    sub: SharedString::default(),
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
            if items.is_empty() {
                return;
            }
            w.set_db_items(ModelRc::from(Rc::new(VecModel::from(group_palette_items(
                items,
            )))));
            w.set_db_modal_open(true);
        });
    }
    {
        let weak = window.as_weak();
        let db_override = db_override.clone();
        window.on_db_choose(move |idx| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            // `db-items` is always the single-bucket wrap (never real
            // top-level groups), so its one `PaletteGroup.rows` is the flat
            // list `idx` was always meant to index into.
            let Some(it) = w
                .get_db_items()
                .row_data(0)
                .and_then(|g| g.rows.row_data(idx.max(0) as usize))
            else {
                return;
            };
            w.set_db_modal_open(false);
            // Switching database means reconnecting with a new dbname (a pg
            // connection is bound to one database). Stash the target and re-run
            // the connect path for the current connection; it resets tabs/schema
            // and reloads the tree against the new database.
            *db_override.lock().unwrap() = Some(it.label.to_string());
            w.invoke_connect_clicked(w.get_selected_conn());
        });
    }
    // ----- schema switcher: sidebar "schema: …" selector -----
    {
        let weak = window.as_weak();
        window.on_select_schema(move |_current| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let items: Vec<PaletteItem> = w
                .get_schema_list()
                .iter()
                .map(|s| PaletteItem {
                    label: s,
                    kind: "database".into(),
                    sub: SharedString::default(),
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
            if items.is_empty() {
                return;
            }
            w.set_schema_items(ModelRc::from(Rc::new(VecModel::from(group_palette_items(
                items,
            )))));
            w.set_schema_modal_open(true);
        });
    }
    {
        let weak = window.as_weak();
        let rt = rt.clone();
        let current = current.clone();
        let raw_nodes = raw_nodes.clone();
        let completion_nodes = completion_nodes.clone();
        let cur_engine = cur_engine.clone();
        let expanded_tables = expanded_tables.clone();
        let collapsed_categories = collapsed_categories.clone();
        let loaded_dbs = loaded_dbs.clone();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let browse = browse.clone();
        let results = results.clone();
        let displayed_grid = displayed_grid.clone();
        let load_editor_text = load_editor_text.clone();
        let query_console = query_console.clone();
        let ed_state = ed_state.clone();
        window.on_schema_choose(move |idx| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            w.set_schema_modal_open(false);
            // Same single-bucket wrap as `db-items` — see comment there.
            let Some(it) = w
                .get_schema_items()
                .row_data(0)
                .and_then(|g| g.rows.row_data(idx.max(0) as usize))
            else {
                return;
            };
            let schema_name = it.label.to_string();
            w.set_schema_name(it.label.clone());
            w.set_bc_schema(it.label);
            // Every tab survives a schema switch. A table tab carries its own
            // fully-qualified TableRef (database + schema + name), so it stays
            // valid and re-queryable no matter which schema is active now —
            // closing it was throwing away work the user still wanted, which is
            // not what switching schema asks for. Snapshot the live editor into
            // the active tab first (the user may not have run/switched since
            // typing).
            let prev_active = active_tab_id.lock().unwrap().clone();
            let keep_active: Option<String> = {
                let mut tabs = workspace_tabs.lock().unwrap();
                if let Some(id) = prev_active.clone() {
                    if let Some(tab) = tabs.iter_mut().find(|t| t.id == id) {
                        tab.query_text = ed_state.borrow().text();
                    }
                }
                prev_active
                    .clone()
                    .filter(|id| tabs.iter().any(|t| &t.id == id))
                    .or_else(|| tabs.first().map(|t| t.id.clone()))
            };
            // Standby: the active tab survives, so its result is still valid —
            // leave the grid up instead of blanking it.
            let standby = keep_active.is_some() && keep_active == prev_active;
            *active_tab_id.lock().unwrap() = keep_active.clone();
            // Reset the live browse state only when the tab staying active is
            // not a table tab. A surviving table tab keeps browsing its own
            // TableRef, so wiping page/limit/filters under it would break its
            // pagination for a switch that never concerned it.
            let active_is_table = {
                let tabs = workspace_tabs.lock().unwrap();
                keep_active
                    .as_ref()
                    .and_then(|id| tabs.iter().find(|t| &t.id == id))
                    .is_some_and(|t| t.table.is_some())
            };
            if !active_is_table {
                // Nothing is being browsed any more, so drop the browse state
                // and the active-table marker together — the marker is what
                // gates Refresh and the "Filter Rows" affordance, and a table
                // tab that stays active still needs both.
                w.set_active_table(SharedString::default());
                let limit = browse.lock().unwrap().limit;
                *browse.lock().unwrap() = BrowseState {
                    limit,
                    ..Default::default()
                };
            }
            {
                let tabs = workspace_tabs.lock().unwrap();
                set_workspace_tabs(&w, &tabs, keep_active.as_deref());
                let text = keep_active
                    .as_ref()
                    .and_then(|id| tabs.iter().find(|t| t.id == *id))
                    .map(|t| t.query_text.clone())
                    .unwrap_or_default();
                load_editor_text(0, &text);
            }
            if !standby {
                results.lock().unwrap().clear();
                *displayed_grid.lock().unwrap() = None;
                clear_grid(&w, 0);
            }
            expanded_tables.lock().unwrap().clear();
            *collapsed_categories.borrow_mut() = default_collapsed_cats();
            let engine = *cur_engine.borrow();
            let Some(engine) = engine else {
                return;
            };
            // Refetch the table tree for the chosen schema off the event loop.
            let weak2 = weak.clone();
            let current = current.clone();
            let raw_nodes = raw_nodes.clone();
            let completion_nodes = completion_nodes.clone();
            let expanded_tables = expanded_tables.clone();
            let loaded_dbs = loaded_dbs.clone();
            let query_console = query_console.clone();
            // Show the tree spinner until the refetch below finishes (any exit).
            w.set_tree_loading(true);
            let clear_loading = {
                let weak = weak.clone();
                move || {
                    let weak = weak.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(w) = weak.upgrade() {
                            w.set_tree_loading(false);
                        }
                    });
                }
            };
            rt.spawn(async move {
                let driver = {
                    let guard = current.lock().await;
                    guard.as_ref().map(|(_, d)| d.clone())
                };
                let Some(driver) = driver else {
                    clear_loading();
                    return;
                };
                let Ok(schema) = driver.schema_for(&schema_name).await else {
                    clear_loading();
                    return;
                };
                // Completion previously stayed seeded from whatever schema was
                // scoped at connect (possibly empty, if the connection had no
                // default database) — refresh it for the schema/database the
                // user just switched to.
                let seed = build_completion_seed(&driver, engine, &schema_name, &schema).await;
                *completion_nodes.lock().unwrap() = seed;
                let nodes = model::to_tree_model(&schema);
                // Nested engines (Mongo/Redis/Cassandra) now scope to the one
                // chosen database; open it so its collections show at once.
                let nested = matches!(
                    engine,
                    rdb_connstore::Engine::Mongo
                        | rdb_connstore::Engine::Redis
                        | rdb_connstore::Engine::Cassandra
                );
                let (exp, loaded) = if nested {
                    let mut e = expanded_tables.lock().unwrap();
                    let mut l = loaded_dbs.lock().unwrap();
                    e.insert(schema_name.clone());
                    l.insert(schema_name.clone());
                    (e.clone(), l.clone())
                } else {
                    (HashSet::new(), HashSet::new())
                };
                let rows = schema_display_rows(
                    &nodes,
                    &exp,
                    &default_collapsed_cats(),
                    &loaded,
                    Some(engine),
                    "",
                );
                *raw_nodes.lock().unwrap() = nodes;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak2.upgrade() {
                        sync_query_console(&w, &query_console);
                        w.set_schema_tree(ModelRc::from(Rc::new(VecModel::from(rows))));
                        let empty_cols: Vec<StructField> = Vec::new();
                        w.set_structure_columns(ModelRc::from(Rc::new(VecModel::from(empty_cols))));
                        w.set_tree_loading(false);
                    }
                });
            });
        });
    }
    // ----- create database / schema from the breadcrumb "New…" prompt -----
    {
        let weak = window.as_weak();
        let rt = rt.clone();
        let current = current.clone();
        let cur_engine = cur_engine.clone();
        window.on_create_commit(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let kind = w.get_create_kind().to_string();
            let name = w.get_create_text().trim().to_string();
            if name.is_empty() {
                w.set_create_error(SharedString::from("Name can't be empty"));
                return;
            }
            let Some(engine) = *cur_engine.borrow() else {
                w.set_create_error(SharedString::from("Not connected"));
                return;
            };
            let ident = quote_ident(&name, engine);
            let sql = match kind.as_str() {
                "database" => format!("CREATE DATABASE {ident}"),
                "schema" => format!("CREATE SCHEMA {ident}"),
                _ => return,
            };
            w.set_create_error(SharedString::default());
            let selected = w.get_selected_conn();
            let weak2 = weak.clone();
            let current = current.clone();
            rt.spawn(async move {
                let driver = {
                    let guard = current.lock().await;
                    guard.as_ref().map(|(_, d)| d.clone())
                };
                let res = match driver {
                    Some(driver) => driver
                        .query(&rdb_core::query::Query::Sql(sql))
                        .await
                        .map(|_| ()),
                    None => Err(rdb_core::error::RdbError::Query("not connected".into())),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak2.upgrade() {
                        match res {
                            // Reconnect to refresh the db/schema lists + tree from
                            // the server — the just-created object then shows up.
                            Ok(()) => {
                                w.set_create_modal_open(false);
                                w.invoke_connect_clicked(selected);
                            }
                            Err(e) => w.set_create_error(SharedString::from(format!("{e}"))),
                        }
                    }
                });
            });
        });
    }
    // ----- new-table designer: Rust owns the column rows so add/remove go
    // through callbacks; the dialog inputs two-way bind into the row fields -----
    {
        let weak = window.as_weak();
        let cur_engine = cur_engine.clone();
        let table_cols = table_cols.clone();
        window.on_new_table(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let Some(engine) = *cur_engine.borrow() else {
                return;
            };
            // Seed with a sensible primary key so the common case is one click.
            table_cols.set_vec(vec![TableCol {
                name: "id".into(),
                type_name: default_pk_type(engine).into(),
                nullable: false,
                pk: true,
            }]);
            w.set_table_name(SharedString::default());
            w.set_table_error(SharedString::default());
            w.set_table_modal_open(true);
        });
    }
    {
        let cur_engine = cur_engine.clone();
        let table_cols = table_cols.clone();
        window.on_table_add_col(move || {
            let ty = cur_engine.borrow().map(default_col_type).unwrap_or("text");
            table_cols.push(TableCol {
                name: SharedString::default(),
                type_name: ty.into(),
                nullable: true,
                pk: false,
            });
        });
    }
    {
        let table_cols = table_cols.clone();
        window.on_table_remove_col(move |i| {
            let i = i.max(0) as usize;
            if i < table_cols.row_count() {
                table_cols.remove(i);
            }
        });
    }
    {
        let table_cols = table_cols.clone();
        window.on_table_set_col_name(move |i, t| {
            let i = i.max(0) as usize;
            if let Some(mut c) = table_cols.row_data(i) {
                c.name = t;
                table_cols.set_row_data(i, c);
            }
        });
    }
    {
        let table_cols = table_cols.clone();
        window.on_table_set_col_type(move |i, t| {
            let i = i.max(0) as usize;
            if let Some(mut c) = table_cols.row_data(i) {
                c.type_name = t;
                table_cols.set_row_data(i, c);
            }
        });
    }
    {
        let table_cols = table_cols.clone();
        window.on_table_toggle_null(move |i| {
            let i = i.max(0) as usize;
            if let Some(mut c) = table_cols.row_data(i) {
                c.nullable = !c.nullable;
                table_cols.set_row_data(i, c);
            }
        });
    }
    {
        let table_cols = table_cols.clone();
        window.on_table_toggle_pk(move |i| {
            let i = i.max(0) as usize;
            if let Some(mut c) = table_cols.row_data(i) {
                c.pk = !c.pk;
                // A primary key can't be null; clear the flag to keep the DDL valid.
                if c.pk {
                    c.nullable = false;
                }
                table_cols.set_row_data(i, c);
            }
        });
    }
    {
        let weak = window.as_weak();
        let rt = rt.clone();
        let current = current.clone();
        let cur_engine = cur_engine.clone();
        let table_cols = table_cols.clone();
        window.on_create_table_commit(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let Some(engine) = *cur_engine.borrow() else {
                return;
            };
            let cols: Vec<ColSpec> = table_cols
                .iter()
                .map(|c| ColSpec {
                    name: c.name.to_string(),
                    ty: c.type_name.to_string(),
                    nullable: c.nullable,
                    pk: c.pk,
                })
                .collect();
            // Postgres browses namespaces, so qualify with the current schema;
            // other engines scope to the connection's database already.
            let schema = matches!(engine, rdb_connstore::Engine::Postgres)
                .then(|| w.get_schema_name().to_string());
            let sql = match build_create_table(
                schema.as_deref(),
                w.get_table_name().as_ref(),
                &cols,
                engine,
            ) {
                Ok(s) => s,
                Err(e) => {
                    w.set_table_error(SharedString::from(e));
                    return;
                }
            };
            w.set_table_error(SharedString::default());
            let selected = w.get_selected_conn();
            let weak2 = weak.clone();
            let current = current.clone();
            rt.spawn(async move {
                let driver = {
                    let guard = current.lock().await;
                    guard.as_ref().map(|(_, d)| d.clone())
                };
                let res = match driver {
                    Some(driver) => driver
                        .query(&rdb_core::query::Query::Sql(sql))
                        .await
                        .map(|_| ()),
                    None => Err(rdb_core::error::RdbError::Query("not connected".into())),
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak2.upgrade() {
                        match res {
                            Ok(()) => {
                                w.set_table_modal_open(false);
                                // Reconnect to reload the tree so the new table shows.
                                w.invoke_connect_clicked(selected);
                            }
                            Err(e) => w.set_table_error(SharedString::from(format!("{e}"))),
                        }
                    }
                });
            });
        });
    }
}
