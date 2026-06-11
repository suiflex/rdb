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
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use dbm_connstore::{ConnStore, SavedConnection};
use slint::{Model, ModelRc, SharedString, VecModel};

use dispatch::AnyDriver;

/// Load saved connections from connstore. Returns an empty list on any error
/// (no config yet, no keychain, etc.) so the app always launches.
fn load_saved() -> Vec<SavedConnection> {
    fn inner() -> dbm_connstore::Result<Vec<SavedConnection>> {
        let path = ConnStore::default_path()?;
        let dir = path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        let backend = dbm_connstore::secret::select_backend(&dir)?;
        let store = ConnStore::load(path, backend)?;
        Ok(store.list().to_vec())
    }
    inner().unwrap_or_default()
}

/// Rebuild a `ConnConfig` (password injected from the secret backend) for a
/// saved connection id. Returns the error as a string for UI display.
fn conn_config_for(id: &str) -> Result<dbm_core::conn::ConnConfig, String> {
    let path = ConnStore::default_path().map_err(|e| e.to_string())?;
    let dir = path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let backend = dbm_connstore::secret::select_backend(&dir).map_err(|e| e.to_string())?;
    let store = ConnStore::load(path, backend).map_err(|e| e.to_string())?;
    store.conn_config_for(id).map_err(|e| e.to_string())
}

/// Rebuild the tab model titles "Query 1..N".
fn set_tab_titles(w: &MainWindow, count: usize) {
    let items: Vec<TabItem> = (1..=count)
        .map(|n| TabItem { title: format!("Query {n}").into() })
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

    // (engine, driver) so run-query can parse text for the right paradigm.
    let current: Arc<tokio::sync::Mutex<Option<(dbm_connstore::Engine, AnyDriver)>>> =
        Arc::new(tokio::sync::Mutex::new(None));

    // Load saved connections (sync; passwords loaded lazily at connect time).
    let saved: Vec<SavedConnection> = load_saved();

    // Build the ConnItem model for the sidebar.
    let conn_items: Vec<ConnItem> = saved
        .iter()
        .map(|s| ConnItem {
            id: s.id.clone().into(),
            name: s.name.clone().into(),
            engine: AnyDriver::label(s.engine).into(),
            color: theme::accent_or_default(s.color.as_deref().unwrap_or("")),
            supported: AnyDriver::is_supported(s.engine),
        })
        .collect();
    let conn_model: Rc<VecModel<ConnItem>> = Rc::new(VecModel::from(conn_items));
    window.set_connections(ModelRc::from(conn_model));
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
        let saved = saved.clone();
        let current = current.clone();
        window.on_connect_clicked(move |idx| {
            let i = idx as usize;
            let Some(sc) = saved.get(i).cloned() else { return; };
            // Reflect selection + accent immediately.
            if let Some(w) = weak.upgrade() {
                w.set_selected_conn(idx);
                w.global::<Theme>()
                    .set_accent(theme::accent_or_default(sc.color.as_deref().unwrap_or("")));
                w.set_status_conn(SharedString::from(sc.name.clone()));
            }
            let weak2 = weak.clone();
            let store_driver = current.clone();
            rt.spawn(async move {
                let result = async {
                    let cfg = conn_config_for(&sc.id).map_err(dbm_core::error::DbmError::Connection)?;
                    let driver = AnyDriver::connect(sc.engine, &cfg).await?;
                    let schema = driver.schema().await?;
                    Ok::<_, dbm_core::error::DbmError>((driver, schema))
                }
                .await;

                match result {
                    Ok((driver, schema)) => {
                        *store_driver.lock().await = Some((sc.engine, driver));
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
            let Some(w) = weak.upgrade() else { return; };
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
                    None => Err(dbm_core::error::DbmError::Connection("not connected".into())),
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
            let Some(w) = weak.upgrade() else { return; };
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
            let Some(w) = weak.upgrade() else { return; };
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
            let Some(w) = weak.upgrade() else { return; };
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
        let saved = saved.clone();
        window.on_toggle_palette(move || {
            if let Some(w) = weak.upgrade() {
                let opening = !w.get_palette_open();
                w.set_palette_open(opening);
                if opening {
                    let mut items: Vec<PaletteItem> = saved
                        .iter()
                        .map(|s| PaletteItem {
                            label: s.name.clone().into(),
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
        let saved = saved.clone();
        window.on_palette_filter(move |q| {
            if let Some(w) = weak.upgrade() {
                let needle = q.to_lowercase();
                let mut items: Vec<PaletteItem> = saved
                    .iter()
                    .filter(|s| s.name.to_lowercase().contains(&needle))
                    .map(|s| PaletteItem {
                        label: s.name.clone().into(),
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

    window.run()
}
