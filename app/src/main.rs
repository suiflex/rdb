//! dbm-app: Slint desktop binary.
//!
//! Async bridge pattern (canonical):
//!   - A `tokio` multi-thread runtime (`Arc<Runtime>`) lives for the whole
//!     program; the Slint event loop owns the main thread via `window.run()`.
//!   - UI callbacks call `rt.spawn(async { ... })` to do driver I/O off the UI
//!     thread.
//!   - Results return to the UI with `slint::invoke_from_event_loop(move || {
//!     if let Some(w) = weak.upgrade() { /* set_* properties */ } })`, where
//!     `weak = window.as_weak()` was cloned into the task.
//!   - Shared connected state lives in `Arc<tokio::sync::Mutex<Option<AnyDriver>>>`.

slint::include_modules!();

mod dispatch;
mod model;
mod query_parse;
mod theme;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use slint::{Model, ModelRc, SharedString, VecModel};

use dispatch::AnyDriver;

/// Rebuild the tab model titles "Query 1..N".
fn set_tab_titles(w: &MainWindow, count: usize) {
    let items: Vec<TabItem> = (1..=count)
        .map(|n| TabItem {
            title: format!("Query {n}").into(),
        })
        .collect();
    w.set_tabs(ModelRc::from(Rc::new(VecModel::from(items))));
}

/// Push a `GridModel` into the window's grid/json/status properties.
fn apply_grid(w: &MainWindow, g: model::GridModel) {
    if g.is_documents {
        w.set_is_documents(true);
        w.set_doc_json(SharedString::from(g.json));
        w.set_result_status(SharedString::default());
        return;
    }
    w.set_is_documents(false);
    if !g.status.is_empty() {
        // Affected: no grid, just a status toast.
        w.set_grid_col_count(0);
        w.set_grid_columns(ModelRc::from(Rc::new(VecModel::<GridColumn>::default())));
        w.set_grid_cells(ModelRc::from(Rc::new(VecModel::<GridCell>::default())));
        w.set_result_status(SharedString::from(g.status));
        return;
    }
    let cols: Vec<GridColumn> = g
        .columns
        .iter()
        .map(|c| GridColumn {
            name: c.name.clone().into(),
            type_name: c.type_name.clone().into(),
        })
        .collect();
    let col_count = cols.len() as i32;
    let mut flat: Vec<GridCell> = Vec::new();
    for row in &g.rows {
        for cell in row {
            flat.push(GridCell {
                text: cell.text.clone().into(),
                is_null: cell.is_null,
            });
        }
    }
    w.set_grid_col_count(col_count);
    w.set_grid_columns(ModelRc::from(Rc::new(VecModel::from(cols))));
    w.set_grid_cells(ModelRc::from(Rc::new(VecModel::from(flat))));
    w.set_result_status(SharedString::from(format!("{} rows", g.rows.len())));
}

fn main() -> Result<(), slint::PlatformError> {
    // tokio multi-thread runtime on background threads; the Slint event loop
    // owns the main thread. Async results return via invoke_from_event_loop.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    let rt = Arc::new(rt);

    let window = MainWindow::new()?;

    // One store for the app lifetime; all CRUD + password ops go through it.
    let store: Rc<RefCell<dbm_connstore::ConnStore>> = Rc::new(RefCell::new(
        dbm_connstore::ConnStore::open_default().unwrap_or_else(|_| {
            let dir = std::env::temp_dir().join("dbm");
            let _ = std::fs::create_dir_all(&dir);
            let backend = dbm_connstore::secret::select_backend(&dir).expect("secret backend");
            dbm_connstore::ConnStore::new(dir.join("connections.json"), backend)
        }),
    ));

    // (engine, driver) so run-query can parse text for the right paradigm.
    let current: Arc<tokio::sync::Mutex<Option<(dbm_connstore::Engine, AnyDriver)>>> =
        Arc::new(tokio::sync::Mutex::new(None));

    // Reusable sidebar rebuild: maps the store's list into the ConnItem model.
    let rebuild_sidebar = {
        let weak = window.as_weak();
        let store = store.clone();
        move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let items: Vec<ConnItem> = store
                .borrow()
                .list()
                .iter()
                .map(|s| ConnItem {
                    id: s.id.clone().into(),
                    name: s.name.clone().into(),
                    engine: AnyDriver::label(s.engine).into(),
                    color: theme::accent_or_default(s.color.as_deref().unwrap_or("")),
                    supported: AnyDriver::is_supported(s.engine),
                })
                .collect();
            w.set_connections(ModelRc::from(Rc::new(VecModel::from(items))));
        }
    };
    rebuild_sidebar();
    window.set_schema_tree(ModelRc::from(Rc::new(VecModel::<TreeNode>::default())));

    // Per-tab query text. MVP: switching tabs swaps the editor text.
    let tab_texts: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(vec![String::new()]));
    window.set_tabs(ModelRc::from(Rc::new(VecModel::from(vec![TabItem {
        title: "Query 1".into(),
    }]))));

    // ----- connect: spawn driver work on tokio, push schema back to UI -----
    {
        let weak = window.as_weak();
        let rt = rt.clone();
        let store = store.clone();
        let current = current.clone();
        window.on_connect_clicked(move |idx| {
            let i = idx as usize;
            let (sc, cfg) = {
                let st = store.borrow();
                let Some(sc) = st.list().get(i).cloned() else {
                    return;
                };
                let cfg = st.conn_config_for(&sc.id);
                (sc, cfg)
            };
            // Reflect selection + accent immediately.
            if let Some(w) = weak.upgrade() {
                w.set_selected_conn(idx);
                w.global::<Theme>()
                    .set_accent(theme::accent_or_default(sc.color.as_deref().unwrap_or("")));
                w.set_status_conn(SharedString::from(sc.name.clone()));
                w.set_query_text(SharedString::from(crate::query_parse::editor_hint(
                    sc.engine,
                )));
            }
            let weak2 = weak.clone();
            let store_driver = current.clone();
            let engine = sc.engine;
            rt.spawn(async move {
                let result = async {
                    let cfg =
                        cfg.map_err(|e| dbm_core::error::DbmError::Connection(e.to_string()))?;
                    let driver = AnyDriver::connect(engine, &cfg).await?;
                    let schema = driver.schema().await?;
                    Ok::<_, dbm_core::error::DbmError>((driver, schema))
                }
                .await;

                match result {
                    Ok((driver, schema)) => {
                        *store_driver.lock().await = Some((engine, driver));
                        let nodes = model::to_tree_model(&schema);
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = weak2.upgrade() {
                                let rows: Vec<TreeNode> = nodes
                                    .into_iter()
                                    .map(|n| TreeNode {
                                        label: n.label.into(),
                                        depth: n.depth,
                                        kind: n.kind.into(),
                                    })
                                    .collect();
                                w.set_schema_tree(ModelRc::from(Rc::new(VecModel::from(rows))));
                                w.set_status_latency(SharedString::from("connected"));
                            }
                        });
                    }
                    Err(e) => {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = weak2.upgrade() {
                                w.set_status_latency(SharedString::from(format!("error: {e}")));
                            }
                        });
                    }
                }
            });
        });
    }

    // ----- run query -----
    {
        let weak = window.as_weak();
        let rt = rt.clone();
        let current = current.clone();
        window.on_run_query(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let sql = w.get_query_text().to_string();
            let weak2 = weak.clone();
            let current = current.clone();
            rt.spawn(async move {
                let guard = current.lock().await;
                let outcome = match guard.as_ref() {
                    Some((engine, driver)) => {
                        match crate::query_parse::parse_query(*engine, &sql) {
                            Ok(q) => driver.query(&q).await,
                            Err(msg) => Err(dbm_core::error::DbmError::Query(msg)),
                        }
                    }
                    None => Err(dbm_core::error::DbmError::Connection(
                        "not connected".into(),
                    )),
                };
                let grid = outcome.as_ref().ok().map(model::to_grid_model);
                let err = outcome.err();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak2.upgrade() {
                        match (grid, err) {
                            (Some(g), _) => apply_grid(&w, g),
                            (None, Some(e)) => {
                                w.set_is_documents(false);
                                w.set_result_status(SharedString::from(format!("error: {e}")));
                            }
                            _ => {}
                        }
                    }
                });
            });
        });
    }

    // ----- new tab -----
    {
        let weak = window.as_weak();
        let tab_texts = tab_texts.clone();
        window.on_new_tab(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let active = w.get_active_tab() as usize;
            {
                let mut t = tab_texts.borrow_mut();
                if let Some(slot) = t.get_mut(active) {
                    *slot = w.get_query_text().to_string();
                }
                t.push(String::new());
            }
            let count = tab_texts.borrow().len();
            set_tab_titles(&w, count);
            w.set_active_tab((count - 1) as i32);
            w.set_query_text(SharedString::default());
        });
    }

    // ----- close tab -----
    {
        let weak = window.as_weak();
        let tab_texts = tab_texts.clone();
        window.on_close_tab(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let mut t = tab_texts.borrow_mut();
            if t.len() <= 1 {
                return;
            }
            let active = w.get_active_tab() as usize;
            let remove_at = active.min(t.len() - 1);
            t.remove(remove_at);
            let new_active = active.saturating_sub(1).min(t.len() - 1);
            let text = t[new_active].clone();
            let count = t.len();
            drop(t);
            set_tab_titles(&w, count);
            w.set_active_tab(new_active as i32);
            w.set_query_text(SharedString::from(text));
        });
    }

    // ----- select tab -----
    {
        let weak = window.as_weak();
        let tab_texts = tab_texts.clone();
        window.on_select_tab(move |idx| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let i = idx as usize;
            let mut t = tab_texts.borrow_mut();
            if i >= t.len() {
                return;
            }
            let active = w.get_active_tab() as usize;
            if let Some(slot) = t.get_mut(active) {
                *slot = w.get_query_text().to_string();
            }
            let text = t[i].clone();
            drop(t);
            w.set_active_tab(idx);
            w.set_query_text(SharedString::from(text));
        });
    }

    // ----- row nav (j/k) -----
    {
        let weak = window.as_weak();
        window.on_move_row(move |delta| {
            if let Some(w) = weak.upgrade() {
                let n = w.get_grid_col_count();
                let total = if n > 0 {
                    (w.get_grid_cells().row_count() as i32) / n
                } else {
                    0
                };
                if total > 0 {
                    let next = (w.get_selected_row() + delta).clamp(0, total - 1);
                    w.set_selected_row(next);
                }
            }
        });
    }

    // ----- palette toggle -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        window.on_toggle_palette(move || {
            if let Some(w) = weak.upgrade() {
                let opening = !w.get_palette_open();
                w.set_palette_open(opening);
                if opening {
                    let names: Vec<String> = store
                        .borrow()
                        .list()
                        .iter()
                        .map(|s| s.name.clone())
                        .collect();
                    let mut items: Vec<PaletteItem> = names
                        .iter()
                        .map(|n| PaletteItem {
                            label: n.clone().into(),
                            kind: "connection".into(),
                        })
                        .collect();
                    for n in w.get_schema_tree().iter().filter(|n| n.kind == "table") {
                        items.push(PaletteItem {
                            label: n.label.clone(),
                            kind: "table".into(),
                        });
                    }
                    w.set_palette_items(ModelRc::from(Rc::new(VecModel::from(items))));
                }
            }
        });
    }

    // ----- palette filter -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        window.on_palette_filter(move |q| {
            if let Some(w) = weak.upgrade() {
                let needle = q.to_lowercase();
                let names: Vec<String> = store
                    .borrow()
                    .list()
                    .iter()
                    .map(|s| s.name.clone())
                    .collect();
                let mut items: Vec<PaletteItem> = names
                    .iter()
                    .filter(|n| n.to_lowercase().contains(&needle))
                    .map(|n| PaletteItem {
                        label: n.clone().into(),
                        kind: "connection".into(),
                    })
                    .collect();
                for n in w
                    .get_schema_tree()
                    .iter()
                    .filter(|n| n.kind == "table" && n.label.to_lowercase().contains(&needle))
                {
                    items.push(PaletteItem {
                        label: n.label.clone(),
                        kind: "table".into(),
                    });
                }
                w.set_palette_items(ModelRc::from(Rc::new(VecModel::from(items))));
            }
        });
    }

    // ----- palette choose (MVP: just closes) -----
    {
        let weak = window.as_weak();
        window.on_palette_choose(move |_idx| {
            if let Some(w) = weak.upgrade() {
                w.set_palette_open(false);
            }
        });
    }

    // ----- light/dark toggle -----
    {
        let weak = window.as_weak();
        window.on_toggle_theme(move || {
            if let Some(w) = weak.upgrade() {
                let t = w.global::<Theme>();
                let now = t.get_dark();
                t.set_dark(!now);
            }
        });
    }

    // ----- connection form (add / edit / delete) -----
    fn default_port(engine_label: &str) -> &'static str {
        match engine_label {
            "MySQL" => "3306",
            "Redis" => "6379",
            "MongoDB" => "27017",
            _ => "5432",
        }
    }
    fn label_to_engine(label: &str) -> dbm_connstore::Engine {
        match label {
            "MySQL" => dbm_connstore::Engine::MySql,
            "Redis" => dbm_connstore::Engine::Redis,
            "MongoDB" => dbm_connstore::Engine::Mongo,
            _ => dbm_connstore::Engine::Postgres,
        }
    }
    let editing_id: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));

    // open add form
    {
        let weak = window.as_weak();
        let editing_id = editing_id.clone();
        window.on_open_add_form(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            *editing_id.borrow_mut() = String::new();
            w.set_form_edit_mode(false);
            w.set_f_name(SharedString::default());
            w.set_f_engine(SharedString::from("PostgreSQL"));
            w.set_f_host(SharedString::from("localhost"));
            w.set_f_port(SharedString::from("5432"));
            w.set_f_user(SharedString::default());
            w.set_f_database(SharedString::default());
            w.set_f_password(SharedString::default());
            w.set_f_sslmode(SharedString::from("Prefer"));
            w.set_f_color(SharedString::from("#3b82f6"));
            w.set_form_error(SharedString::default());
            w.set_form_open(true);
        });
    }
    // open edit form
    {
        let weak = window.as_weak();
        let store = store.clone();
        let editing_id = editing_id.clone();
        window.on_open_edit_form(move |idx| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let st = store.borrow();
            let Some(sc) = st.list().get(idx as usize).cloned() else {
                return;
            };
            *editing_id.borrow_mut() = sc.id.clone();
            w.set_form_edit_mode(true);
            w.set_f_name(SharedString::from(sc.name));
            w.set_f_engine(SharedString::from(AnyDriver::label(sc.engine)));
            w.set_f_host(SharedString::from(sc.host));
            w.set_f_port(SharedString::from(sc.port.to_string()));
            w.set_f_user(SharedString::from(sc.user));
            w.set_f_database(SharedString::from(sc.database.unwrap_or_default()));
            w.set_f_password(SharedString::default());
            w.set_f_sslmode(SharedString::from(match sc.sslmode {
                dbm_core::conn::SslMode::Disable => "Disable",
                dbm_core::conn::SslMode::Prefer => "Prefer",
                dbm_core::conn::SslMode::Require => "Require",
            }));
            w.set_f_color(SharedString::from(
                sc.color.unwrap_or_else(|| "#3b82f6".into()),
            ));
            w.set_form_error(SharedString::default());
            w.set_form_open(true);
        });
    }
    // engine changed -> default port if port empty/default-ish
    {
        let weak = window.as_weak();
        window.on_form_engine_changed(move |label| {
            if let Some(w) = weak.upgrade() {
                let cur = w.get_f_port().to_string();
                if cur.is_empty() || ["5432", "3306", "6379", "27017"].contains(&cur.as_str()) {
                    w.set_f_port(SharedString::from(default_port(&label)));
                }
            }
        });
    }
    // cancel
    {
        let weak = window.as_weak();
        window.on_form_cancel(move || {
            if let Some(w) = weak.upgrade() {
                w.set_form_open(false);
            }
        });
    }
    // save (add or update)
    {
        let weak = window.as_weak();
        let store = store.clone();
        let editing_id = editing_id.clone();
        let rebuild = {
            let weak = window.as_weak();
            let store = store.clone();
            move || {
                if let Some(w) = weak.upgrade() {
                    let items: Vec<ConnItem> = store
                        .borrow()
                        .list()
                        .iter()
                        .map(|s| ConnItem {
                            id: s.id.clone().into(),
                            name: s.name.clone().into(),
                            engine: AnyDriver::label(s.engine).into(),
                            color: theme::accent_or_default(s.color.as_deref().unwrap_or("")),
                            supported: AnyDriver::is_supported(s.engine),
                        })
                        .collect();
                    w.set_connections(ModelRc::from(Rc::new(VecModel::from(items))));
                }
            }
        };
        window.on_form_save(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let name = w.get_f_name().to_string();
            let host = w.get_f_host().to_string();
            if name.trim().is_empty() || host.trim().is_empty() {
                w.set_form_error(SharedString::from("name and host are required"));
                return;
            }
            let port: u16 = match w.get_f_port().to_string().parse() {
                Ok(p) if p != 0 => p,
                _ => {
                    w.set_form_error(SharedString::from("port must be a number 1-65535"));
                    return;
                }
            };
            let engine = label_to_engine(w.get_f_engine().as_ref());
            let sslmode = match w.get_f_sslmode().to_string().as_str() {
                "Disable" => dbm_core::conn::SslMode::Disable,
                "Require" => dbm_core::conn::SslMode::Require,
                _ => dbm_core::conn::SslMode::Prefer,
            };
            let database = {
                let d = w.get_f_database().to_string();
                if d.is_empty() {
                    None
                } else {
                    Some(d)
                }
            };
            let color = Some(w.get_f_color().to_string());
            let password = w.get_f_password().to_string();
            let id = editing_id.borrow().clone();

            let result: dbm_connstore::Result<()> = (|| {
                let mut st = store.borrow_mut();
                let conn_id = if id.is_empty() {
                    let mut sc = dbm_connstore::SavedConnection::new(
                        name,
                        engine,
                        host,
                        port,
                        w.get_f_user().to_string(),
                    );
                    sc.database = database;
                    sc.sslmode = sslmode;
                    sc.color = color;
                    let cid = sc.id.clone();
                    st.add(sc)?;
                    cid
                } else {
                    let mut sc = st
                        .get(&id)
                        .cloned()
                        .ok_or_else(|| dbm_connstore::ConnStoreError::NotFound(id.clone()))?;
                    sc.name = name;
                    sc.engine = engine;
                    sc.host = host;
                    sc.port = port;
                    sc.user = w.get_f_user().to_string();
                    sc.database = database;
                    sc.sslmode = sslmode;
                    sc.color = color;
                    st.update(sc)?;
                    id.clone()
                };
                if !password.is_empty() {
                    st.set_password(&conn_id, &password)?;
                }
                Ok(())
            })();

            match result {
                Ok(()) => {
                    w.set_form_open(false);
                    rebuild();
                }
                Err(e) => {
                    w.set_form_error(SharedString::from(format!("save failed: {e}")));
                }
            }
        });
    }
    // delete
    {
        let weak = window.as_weak();
        let store = store.clone();
        let editing_id = editing_id.clone();
        window.on_form_delete(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let id = editing_id.borrow().clone();
            if id.is_empty() {
                w.set_form_open(false);
                return;
            }
            {
                let mut st = store.borrow_mut();
                let _ = st.delete_password(&id);
                let _ = st.remove(&id);
            }
            w.set_form_open(false);
            w.set_selected_conn(-1);
            w.set_schema_tree(ModelRc::from(Rc::new(VecModel::<TreeNode>::default())));
            let items: Vec<ConnItem> = store
                .borrow()
                .list()
                .iter()
                .map(|s| ConnItem {
                    id: s.id.clone().into(),
                    name: s.name.clone().into(),
                    engine: AnyDriver::label(s.engine).into(),
                    color: theme::accent_or_default(s.color.as_deref().unwrap_or("")),
                    supported: AnyDriver::is_supported(s.engine),
                })
                .collect();
            w.set_connections(ModelRc::from(Rc::new(VecModel::from(items))));
        });
    }

    window.run()
}
