//! rdbs-app: Slint desktop binary.
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
use std::collections::HashSet;
use std::rc::Rc;
use std::sync::Arc;

use slint::{Model, ModelRc, SharedString, VecModel};

use dispatch::AnyDriver;

/// Label used for connections with no explicit group.
const UNGROUPED: &str = "Ungrouped";

/// Build the grouped sidebar row model: a header row per group followed by its
/// connection rows (unless the group is collapsed). `index` on each connection
/// row is its position in the store list, so connect/edit callbacks stay correct
/// regardless of grouping or ordering. `collapsed` holds the set of group labels
/// currently folded shut.
fn build_conn_items(
    store: &rdbs_connstore::ConnStore,
    collapsed: &HashSet<String>,
    filter: &str,
) -> Vec<ConnItem> {
    let needle = filter.trim().to_lowercase();
    // Bucket store indices by group label, preserving first-seen group order.
    let mut order: Vec<String> = Vec::new();
    let mut buckets: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();
    for (i, sc) in store.list().iter().enumerate() {
        if !needle.is_empty() && !sc.name.to_lowercase().contains(&needle) {
            continue;
        }
        let g = sc
            .group
            .as_deref()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(UNGROUPED)
            .to_string();
        if !buckets.contains_key(&g) {
            order.push(g.clone());
        }
        buckets.entry(g).or_default().push(i);
    }

    let mut rows: Vec<ConnItem> = Vec::new();
    for g in &order {
        // Ungrouped connections list flat with no header and are never collapsed,
        // so an empty "Ungrouped" header never shows when the user has no groups.
        let is_ungrouped = g == UNGROUPED;
        let expanded = is_ungrouped || !collapsed.contains(g);
        if !is_ungrouped {
            rows.push(ConnItem {
                id: SharedString::default(),
                name: g.clone().into(),
                engine: SharedString::default(),
                color: theme::accent_or_default(""),
                is_header: true,
                expanded,
                index: -1,
                group: g.clone().into(),
            });
        }
        if !expanded {
            continue;
        }
        for &i in &buckets[g] {
            let s = &store.list()[i];
            rows.push(ConnItem {
                id: s.id.clone().into(),
                name: s.name.clone().into(),
                engine: AnyDriver::label(s.engine).into(),
                color: theme::accent_or_default(s.color.as_deref().unwrap_or("")),
                is_header: false,
                expanded: true,
                index: i as i32,
                group: g.clone().into(),
            });
        }
    }
    rows
}

/// Top-level sidebar categories per engine (TablePlus-style). The label is also
/// the toggle key, so `on_toggle_schema_node` can tell a category click from a
/// table click. The first entry is the "primary" category that holds the
/// engine's containers. ponytail: SQL keeps the Views/Functions placeholders.
fn sidebar_categories(engine: Option<rdbs_connstore::Engine>) -> &'static [&'static str] {
    match engine {
        // Mongo/Redis use the nested database→leaf path, not these categories.
        Some(rdbs_connstore::Engine::Mongo) => &["Collections"],
        _ => &["Tables", "Views", "Functions"],
    }
}

/// Build the collapsible schema sidebar rows from the raw flat node list.
///
/// Layout is a two-level tree under fixed category headers:
///   - category (depth 0)  — collapsed when its label is in `collapsed_cats`
///   - container (depth 1)  — a table/collection/keyspace, opens its data
///   - field (depth 2)      — a column, shown only when its container is expanded
///
/// The current schema model only produces table-like containers, so every
/// container lands under "Tables"; "Views"/"Functions" render as empty
/// collapsible headers until schema introspection grows those kinds.
fn schema_display_rows(
    nodes: &[model::VmTreeNode],
    expanded_tables: &HashSet<String>,
    collapsed_cats: &HashSet<String>,
    loaded_dbs: &HashSet<String>,
    engine: Option<rdbs_connstore::Engine>,
    filter: &str,
) -> Vec<TreeNode> {
    // Mongo (database→collection) and Redis (database→key) both render as a
    // collapsible database header nesting its own lazily-loaded leaves.
    // `expanded_tables` is the set of OPEN databases (default closed).
    match engine {
        Some(rdbs_connstore::Engine::Mongo) => {
            return nested_display_rows(nodes, expanded_tables, loaded_dbs, "collection", filter);
        }
        Some(rdbs_connstore::Engine::Redis) => {
            return nested_display_rows(nodes, expanded_tables, loaded_dbs, "key", filter);
        }
        _ => {}
    }

    let needle = filter.to_lowercase();
    let filtering = !needle.is_empty();
    let matches = |label: &str| !filtering || label.to_lowercase().contains(&needle);
    let categories = sidebar_categories(engine);
    let primary = categories[0];
    // Matching containers all live under the primary category today.
    let container_count = nodes
        .iter()
        .filter(|n| {
            matches!(n.kind.as_str(), "table" | "collection" | "keyspace") && matches(&n.label)
        })
        .count() as i32;
    let mut rows: Vec<TreeNode> = Vec::new();
    for &cat in categories {
        // While filtering, categories are forced open so matches stay visible.
        let cat_open = filtering || !collapsed_cats.contains(cat);
        rows.push(TreeNode {
            label: cat.into(),
            depth: 0,
            kind: "category".into(),
            expanded: cat_open,
            db: SharedString::default(),
            count: if cat == primary { container_count } else { 0 },
        });
        if !cat_open || cat != primary {
            continue;
        }
        // Walk the flat nodes: containers + (when expanded) their fields.
        let mut show_fields = false;
        for n in nodes {
            let is_container = matches!(n.kind.as_str(), "table" | "collection" | "keyspace");
            if n.kind == "field" {
                if !show_fields {
                    continue;
                }
                rows.push(TreeNode {
                    label: n.label.clone().into(),
                    depth: 2,
                    kind: "field".into(),
                    expanded: false,
                    db: SharedString::default(),
                    count: 0,
                });
            } else if is_container {
                if !matches(&n.label) {
                    show_fields = false;
                    continue;
                }
                show_fields = expanded_tables.contains(&n.label);
                rows.push(TreeNode {
                    label: n.label.clone().into(),
                    depth: 1,
                    kind: n.kind.clone().into(),
                    expanded: show_fields,
                    db: SharedString::default(),
                    count: 0,
                });
            } else {
                // database row: categories replace it; reset field visibility
                show_fields = false;
            }
        }
    }
    rows
}

/// Build a database→leaf tree for engines that browse per-database (Mongo
/// collections, Redis keys). Each database is a depth-0 collapsible header (open
/// when its name is in `expanded_dbs`, default closed); its leaves are depth-1
/// rows tagged with the owning database and emitted with `leaf_kind`. An open
/// database with no leaf rows gets a non-clickable hint row so the header never
/// looks stuck: `(loading…)` until its fetch lands in `loaded_dbs`, then
/// `(empty)` if it really is empty.
fn nested_display_rows(
    nodes: &[model::VmTreeNode],
    expanded_dbs: &HashSet<String>,
    loaded_dbs: &HashSet<String>,
    leaf_kind: &str,
    filter: &str,
) -> Vec<TreeNode> {
    let needle = filter.to_lowercase();
    let filtering = !needle.is_empty();
    let matches = |label: &str| !filtering || label.to_lowercase().contains(&needle);

    // Group the flat list into (database, leaves) so headers can carry a
    // matching-leaf count badge before their rows are emitted.
    let mut groups: Vec<(String, Vec<&model::VmTreeNode>)> = Vec::new();
    for n in nodes {
        match n.kind.as_str() {
            "database" => groups.push((n.label.clone(), Vec::new())),
            "collection" | "table" | "keyspace" | "key" => {
                if let Some((_, leaves)) = groups.last_mut() {
                    if matches(&n.label) {
                        leaves.push(n);
                    }
                }
            }
            _ => {}
        }
    }

    let mut rows: Vec<TreeNode> = Vec::new();
    for (db, leaves) in &groups {
        // While filtering, loaded databases are forced open so matches show.
        let db_open =
            expanded_dbs.contains(db) || (filtering && loaded_dbs.contains(db) && !leaves.is_empty());
        rows.push(TreeNode {
            label: db.clone().into(),
            depth: 0,
            kind: "database".into(),
            expanded: db_open,
            db: db.clone().into(),
            count: leaves.len() as i32,
        });
        if !db_open {
            continue;
        }
        if leaves.is_empty() {
            // Loading/empty hint so an open header never looks stuck. While
            // filtering an already-loaded db, empty means "no match".
            rows.push(TreeNode {
                label: if !loaded_dbs.contains(db) {
                    "(loading…)".into()
                } else if filtering {
                    "(no match)".into()
                } else {
                    "(empty)".into()
                },
                depth: 1,
                kind: "hint".into(),
                expanded: false,
                db: db.clone().into(),
                count: 0,
            });
            continue;
        }
        for n in leaves {
            rows.push(TreeNode {
                label: n.label.clone().into(),
                depth: 1,
                kind: leaf_kind.into(),
                expanded: false,
                db: db.clone().into(),
                count: 0,
            });
        }
    }
    rows
}

/// Live browse-mode state: which container is open and where the page window
/// sits. Shared (Arc<Mutex>) so async count/pk fetches can update it.
#[derive(Default, Clone)]
struct BrowseState {
    table: Option<rdbs_core::write::TableRef>,
    page: u64,
    limit: u64,
    total: Option<u64>,
    pk_cols: Vec<String>,
}

/// 1-based display bounds of the current page window plus prev/next
/// availability. `shown` is how many rows the page actually returned.
fn page_bounds(page: u64, limit: u64, total: Option<u64>, shown: u64) -> (u64, u64, bool, bool) {
    let start = if shown == 0 { 0 } else { page * limit + 1 };
    let end = page * limit + shown;
    let can_prev = page > 0;
    let can_next = match total {
        Some(t) => end < t,
        None => shown == limit, // full page and unknown total: assume more
    };
    (start, end, can_prev, can_next)
}

/// Engine-appropriate browse text for one page of a container. The text also
/// lands in the editor, so it stays a valid editor query for that engine.
fn browse_text(
    engine: rdbs_connstore::Engine,
    table: &rdbs_core::write::TableRef,
    page: u64,
    limit: u64,
) -> String {
    let offset = page * limit;
    match engine {
        rdbs_connstore::Engine::Postgres => {
            let schema = table.schema.as_deref().unwrap_or("public");
            let q = |s: &str| s.replace('"', "\"\"");
            format!(
                "SELECT * FROM \"{}\".\"{}\" LIMIT {limit} OFFSET {offset}",
                q(schema),
                q(&table.name)
            )
        }
        rdbs_connstore::Engine::MySql => {
            format!(
                "SELECT * FROM `{}` LIMIT {limit} OFFSET {offset}",
                table.name.replace('`', "``")
            )
        }
        rdbs_connstore::Engine::Mongo => {
            let db = table
                .database
                .as_deref()
                .map(|d| format!("\"database\":\"{d}\","))
                .unwrap_or_default();
            format!(
                "{{\"collection\":\"{}\",{db}\"op\":\"find\",\"body\":{{}},\"limit\":{limit},\"skip\":{offset}}}",
                table.name
            )
        }
        rdbs_connstore::Engine::Redis => {
            format!("BROWSE {} {offset} {limit}", table.name)
        }
    }
}

/// Rebuild the tab model titles "Query 1..N".
fn set_tab_titles(w: &MainWindow, count: usize) {
    let items: Vec<TabItem> = (1..=count)
        .map(|n| TabItem {
            title: format!("Query {n}").into(),
        })
        .collect();
    w.set_tabs(ModelRc::from(Rc::new(VecModel::from(items))));
}

/// Push a flat `GridModel` into the window's grid columns/cells properties.
fn push_grid(w: &MainWindow, g: &model::GridModel) {
    let cols: Vec<GridColumn> = g
        .columns
        .iter()
        .map(|c| GridColumn {
            name: c.name.clone().into(),
            type_name: c.type_name.clone().into(),
        })
        .collect();
    let mut flat: Vec<GridCell> = Vec::new();
    for row in &g.rows {
        for cell in row {
            flat.push(GridCell {
                text: cell.text.clone().into(),
                is_null: cell.is_null,
                state: 0,
            });
        }
    }
    w.set_grid_col_count(cols.len() as i32);
    w.set_grid_columns(ModelRc::from(Rc::new(VecModel::from(cols))));
    w.set_grid_cells(ModelRc::from(Rc::new(VecModel::from(flat))));
}

/// Clear the tabular grid properties (used by non-tabular result kinds).
fn clear_grid(w: &MainWindow) {
    w.set_grid_col_count(0);
    w.set_grid_columns(ModelRc::from(Rc::new(VecModel::<GridColumn>::default())));
    w.set_grid_cells(ModelRc::from(Rc::new(VecModel::<GridCell>::default())));
}

/// Return a copy of `g` keeping only rows where some cell matches `needle`
/// (already lowercased). An empty needle keeps every row.
fn filter_grid(g: &model::GridModel, needle: &str) -> model::GridModel {
    if needle.is_empty() {
        return g.clone();
    }
    model::GridModel {
        columns: g.columns.clone(),
        rows: g
            .rows
            .iter()
            .filter(|row| row.iter().any(|c| c.text.to_lowercase().contains(needle)))
            .cloned()
            .collect(),
    }
}

/// Push a `ResultView` into the window, selecting the per-kind result region
/// via `result-kind` (0 Table, 1 Documents, 2 KeyValue, 3 Affected).
fn apply_result(w: &MainWindow, view: model::ResultView) {
    // Reset the KeyValue rows by default; the KeyValue arm repopulates them.
    let clear_kv =
        |w: &MainWindow| w.set_kv_rows(ModelRc::from(Rc::new(VecModel::<KvRow>::default())));
    match view {
        model::ResultView::Table(g) => {
            w.set_result_kind(0);
            w.set_doc_json(SharedString::default());
            clear_kv(w);
            push_grid(w, &g);
            w.set_result_status(SharedString::from(format!("{} rows", g.rows.len())));
        }
        model::ResultView::Documents(d) => {
            w.set_result_kind(1);
            w.set_doc_json(SharedString::from(d.json));
            clear_kv(w);
            push_grid(w, &d.grid);
            w.set_result_status(SharedString::from(format!(
                "{} documents",
                d.grid.rows.len()
            )));
        }
        model::ResultView::KeyValue(kv) => {
            w.set_result_kind(2);
            w.set_doc_json(SharedString::default());
            clear_grid(w);
            let n = kv.rows.len();
            let rows: Vec<KvRow> = kv
                .rows
                .iter()
                .map(|(k, c)| KvRow {
                    key: k.clone().into(),
                    value: GridCell {
                        text: c.text.clone().into(),
                        is_null: c.is_null,
                        state: 0,
                    },
                })
                .collect();
            w.set_kv_rows(ModelRc::from(Rc::new(VecModel::from(rows))));
            w.set_result_status(SharedString::from(format!("{n} keys")));
        }
        model::ResultView::Affected(status) => {
            w.set_result_kind(3);
            w.set_doc_json(SharedString::default());
            clear_grid(w);
            clear_kv(w);
            w.set_result_status(SharedString::from(status));
        }
    }
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
    let store: Rc<RefCell<rdbs_connstore::ConnStore>> = Rc::new(RefCell::new(
        rdbs_connstore::ConnStore::open_default().unwrap_or_else(|_| {
            let dir = std::env::temp_dir().join("dbm");
            let _ = std::fs::create_dir_all(&dir);
            let backend = rdbs_connstore::secret::select_backend(&dir).expect("secret backend");
            rdbs_connstore::ConnStore::new(dir.join("connections.json"), backend)
        }),
    ));

    // (engine, driver) so run-query can parse text for the right paradigm.
    let current: Arc<tokio::sync::Mutex<Option<(rdbs_connstore::Engine, AnyDriver)>>> =
        Arc::new(tokio::sync::Mutex::new(None));

    // Set of group labels the user has collapsed in the sidebar.
    let collapsed: Rc<RefCell<HashSet<String>>> = Rc::new(RefCell::new(HashSet::new()));
    // Current connection-picker search text.
    let conn_filter: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));

    // Schema sidebar state: raw flat nodes from the last connect (Send, so the
    // connect task can fill it) + the set of expanded table labels.
    let raw_nodes: Arc<std::sync::Mutex<Vec<model::VmTreeNode>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    // Expanded sidebar nodes. For Mongo this is the set of OPEN databases
    // (default closed → collections load lazily on expand). Arc<Mutex> so the
    // lazy-load task can read it off the UI thread.
    let expanded_tables: Arc<std::sync::Mutex<HashSet<String>>> =
        Arc::new(std::sync::Mutex::new(HashSet::new()));
    // Mongo databases whose collections have already been fetched.
    let loaded_dbs: Arc<std::sync::Mutex<HashSet<String>>> =
        Arc::new(std::sync::Mutex::new(HashSet::new()));
    // Sidebar category headers the user has collapsed (Tables/Views/Functions).
    let collapsed_categories: Rc<RefCell<HashSet<String>>> = Rc::new(RefCell::new(HashSet::new()));
    // Sidebar tree filter text. Arc<Mutex> so lazy-load tasks can read it.
    let sidebar_filter: Arc<std::sync::Mutex<String>> =
        Arc::new(std::sync::Mutex::new(String::new()));
    // Browse-mode pagination state (open container + page window + pk).
    let browse: Arc<std::sync::Mutex<BrowseState>> =
        Arc::new(std::sync::Mutex::new(BrowseState {
            limit: 300,
            ..Default::default()
        }));
    // Engine of the live connection, kept on the UI thread so table clicks can
    // build an engine-appropriate "browse this table" query synchronously.
    let cur_engine: Rc<RefCell<Option<rdbs_connstore::Engine>>> = Rc::new(RefCell::new(None));

    // Reusable sidebar rebuild: buckets the store's list into grouped rows.
    let rebuild_sidebar = {
        let weak = window.as_weak();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let items =
                build_conn_items(&store.borrow(), &collapsed.borrow(), &conn_filter.borrow());
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

    // Last result view kept in memory so the client-side filter (Feature C)
    // can re-derive the visible rows without re-querying. Arc<Mutex<>> (not Rc)
    // so it can cross into the Send event-loop closure from the query task.
    let last_view: Arc<std::sync::Mutex<Option<model::ResultView>>> =
        Arc::new(std::sync::Mutex::new(None));

    // ----- toggle a sidebar group's collapsed state (Feature A) -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        window.on_toggle_group(move |g| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let g = g.to_string();
            {
                let mut c = collapsed.borrow_mut();
                if !c.remove(&g) {
                    c.insert(g);
                }
            }
            let items =
                build_conn_items(&store.borrow(), &collapsed.borrow(), &conn_filter.borrow());
            w.set_connections(ModelRc::from(Rc::new(VecModel::from(items))));
        });
    }

    // ----- connection-picker search (filter the connection list) -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        window.on_conn_filter(move |t| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            *conn_filter.borrow_mut() = t.to_string();
            let items =
                build_conn_items(&store.borrow(), &collapsed.borrow(), &conn_filter.borrow());
            w.set_connections(ModelRc::from(Rc::new(VecModel::from(items))));
        });
    }

    // ----- disconnect: drop the driver and return to the picker -----
    {
        let weak = window.as_weak();
        let current = current.clone();
        let rt = rt.clone();
        let cur_engine = cur_engine.clone();
        let expanded_tables = expanded_tables.clone();
        let loaded_dbs = loaded_dbs.clone();
        let collapsed_categories = collapsed_categories.clone();
        let raw_nodes = raw_nodes.clone();
        window.on_disconnect(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            w.set_connected(false);
            w.set_selected_conn(-1);
            w.set_active_table(SharedString::default());
            w.set_status_conn(SharedString::from("no connection"));
            w.set_status_latency(SharedString::default());
            w.set_schema_tree(ModelRc::from(Rc::new(VecModel::<TreeNode>::default())));
            w.set_structure_columns(ModelRc::from(Rc::new(VecModel::<StructField>::default())));
            *cur_engine.borrow_mut() = None;
            expanded_tables.lock().unwrap().clear();
            loaded_dbs.lock().unwrap().clear();
            collapsed_categories.borrow_mut().clear();
            raw_nodes.lock().unwrap().clear();
            let current = current.clone();
            rt.spawn(async move {
                *current.lock().await = None;
            });
        });
    }

    // ----- apply client-side row filter to the last result (Feature C) -----
    {
        let weak = window.as_weak();
        let last_view = last_view.clone();
        window.on_apply_filter(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let needle = w.get_grid_filter().to_string().to_lowercase();
            let guard = last_view.lock().unwrap();
            let Some(v) = guard.as_ref() else {
                return;
            };
            // Filter the row-bearing views; Affected has nothing to filter.
            let filtered = match v {
                model::ResultView::Table(g) => model::ResultView::Table(filter_grid(g, &needle)),
                model::ResultView::Documents(d) => model::ResultView::Documents(model::DocModel {
                    json: d.json.clone(),
                    grid: filter_grid(&d.grid, &needle),
                }),
                model::ResultView::KeyValue(kv) => model::ResultView::KeyValue(model::KvModel {
                    rows: kv
                        .rows
                        .iter()
                        .filter(|(k, c)| {
                            needle.is_empty()
                                || k.to_lowercase().contains(&needle)
                                || c.text.to_lowercase().contains(&needle)
                        })
                        .cloned()
                        .collect(),
                }),
                model::ResultView::Affected(_) => return,
            };
            apply_result(&w, filtered);
        });
    }

    // ----- connect: spawn driver work on tokio, push schema back to UI -----
    {
        let weak = window.as_weak();
        let rt = rt.clone();
        let store = store.clone();
        let current = current.clone();
        let raw_nodes = raw_nodes.clone();
        let expanded_tables = expanded_tables.clone();
        let loaded_dbs = loaded_dbs.clone();
        let collapsed_categories = collapsed_categories.clone();
        let cur_engine = cur_engine.clone();
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
                w.set_active_table(SharedString::default());
            }
            // Fresh connection: nothing browsed, nothing expanded.
            *cur_engine.borrow_mut() = Some(sc.engine);
            expanded_tables.lock().unwrap().clear();
            loaded_dbs.lock().unwrap().clear();
            collapsed_categories.borrow_mut().clear();
            let weak2 = weak.clone();
            let store_driver = current.clone();
            let engine = sc.engine;
            let raw_nodes = raw_nodes.clone();
            rt.spawn(async move {
                let result = async {
                    let cfg =
                        cfg.map_err(|e| rdbs_core::error::RdbsError::Connection(e.to_string()))?;
                    let driver = AnyDriver::connect(engine, &cfg).await?;
                    let schema = driver.schema().await?;
                    Ok::<_, rdbs_core::error::RdbsError>((driver, schema))
                }
                .await;

                match result {
                    Ok((driver, schema)) => {
                        *store_driver.lock().await = Some((engine, driver));
                        let nodes = model::to_tree_model(&schema);
                        let fields = model::to_structure_model(&schema);
                        // Stash raw nodes for later expand/collapse rebuilds, and
                        // render the initial view (categories open, fields hidden).
                        let rows = schema_display_rows(
                            &nodes,
                            &HashSet::new(),
                            &HashSet::new(),
                            &HashSet::new(),
                            Some(engine),
                            "",
                        );
                        // Real database/schema names from introspection; fall back
                        // to "public" only when the driver exposed none.
                        let mut schema_names: Vec<SharedString> = schema
                            .databases
                            .iter()
                            .map(|d| SharedString::from(d.name.clone()))
                            .collect();
                        if schema_names.is_empty() {
                            schema_names.push(SharedString::from("public"));
                        }
                        let schema_current = schema_names[0].clone();
                        // SQL editor only makes sense for the SQL engines.
                        let sql_capable = matches!(
                            engine,
                            rdbs_connstore::Engine::Postgres | rdbs_connstore::Engine::MySql
                        );
                        *raw_nodes.lock().unwrap() = nodes;
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = weak2.upgrade() {
                                w.set_schema_tree(ModelRc::from(Rc::new(VecModel::from(rows))));
                                w.set_sql_capable(sql_capable);
                                w.set_schema_name(schema_current);
                                w.set_schema_list(ModelRc::from(Rc::new(VecModel::from(
                                    schema_names,
                                ))));
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
                                w.set_picker_error(SharedString::default());
                                // Swap the picker for the workspace.
                                w.set_connected(true);
                            }
                        });
                    }
                    Err(e) => {
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(w) = weak2.upgrade() {
                                // Stay on the picker; surface the failure there.
                                w.set_connected(false);
                                w.set_picker_error(SharedString::from(format!(
                                    "connection failed: {e}"
                                )));
                            }
                        });
                    }
                }
            });
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

    // ----- shared query runner: parse text for the live engine, run it, push
    // the result (grid / documents / error) back to the UI. Used by both the
    // Run button and table clicks. -----
    let run_sql: Rc<dyn Fn(String)> = {
        let weak = window.as_weak();
        let rt = rt.clone();
        let current = current.clone();
        let last_view = last_view.clone();
        let browse = browse.clone();
        Rc::new(move |sql: String| {
            let weak2 = weak.clone();
            let current = current.clone();
            let last_view = last_view.clone();
            let browse = browse.clone();
            rt.spawn(async move {
                let guard = current.lock().await;
                let outcome = match guard.as_ref() {
                    Some((engine, driver)) => {
                        match crate::query_parse::parse_query(*engine, &sql) {
                            Ok(q) => driver.query(&q).await,
                            Err(msg) => Err(rdbs_core::error::RdbsError::Query(msg)),
                        }
                    }
                    None => Err(rdbs_core::error::RdbsError::Connection(
                        "not connected".into(),
                    )),
                };
                let view = outcome.as_ref().ok().map(model::to_result_view);
                let err = outcome.err();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak2.upgrade() {
                        match (view, err) {
                            (Some(v), _) => {
                                // Only tabular-shaped views have resizable columns.
                                let ncols = match &v {
                                    model::ResultView::Table(g) => g.columns.len(),
                                    model::ResultView::Documents(d) => d.grid.columns.len(),
                                    _ => 0,
                                };
                                // Cache for client-side filtering, then reset the
                                // filter input so the full result shows first.
                                let shown = match &v {
                                    model::ResultView::Table(g) => g.rows.len(),
                                    model::ResultView::Documents(d) => d.grid.rows.len(),
                                    model::ResultView::KeyValue(kv) => kv.rows.len(),
                                    model::ResultView::Affected(_) => 0,
                                } as u64;
                                *last_view.lock().unwrap() = Some(v.clone());
                                w.set_grid_filter(SharedString::default());
                                apply_result(&w, v);
                                // Fresh result: reset every column to a default width.
                                let widths: Vec<f32> = vec![140.0; ncols];
                                w.set_grid_col_widths(ModelRc::from(Rc::new(VecModel::from(
                                    widths,
                                ))));
                                // Browse mode: refresh the pagination footer.
                                let st = browse.lock().unwrap().clone();
                                if st.table.is_some() {
                                    let (start, end, prev, next) =
                                        page_bounds(st.page, st.limit, st.total, shown);
                                    w.set_page_start(start as i32);
                                    w.set_page_end(end as i32);
                                    w.set_total_rows(st.total.map(|t| t as i32).unwrap_or(-1));
                                    w.set_can_prev(prev);
                                    w.set_can_next(next);
                                }
                            }
                            (None, Some(e)) => {
                                *last_view.lock().unwrap() = None;
                                apply_result(
                                    &w,
                                    model::ResultView::Affected(format!("error: {e}")),
                                );
                            }
                            _ => {}
                        }
                    }
                });
            });
        })
    };

    // ----- run query (editor) -----
    {
        let weak = window.as_weak();
        let run_sql = run_sql.clone();
        window.on_run_query(move || {
            if let Some(w) = weak.upgrade() {
                run_sql(w.get_query_text().to_string());
            }
        });
    }

    // ----- open table: build an engine-appropriate "browse" query, show it in
    // the editor, and run it (TablePlus-style click-to-view). -----
    // Shared page runner: build the browse text for the current state, mirror
    // it in the editor, and run it. Used by open-table and the footer nav.
    let run_browse: Rc<dyn Fn()> = {
        let weak = window.as_weak();
        let cur_engine = cur_engine.clone();
        let browse = browse.clone();
        let run_sql = run_sql.clone();
        Rc::new(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let Some(engine) = *cur_engine.borrow() else {
                return;
            };
            let st = browse.lock().unwrap().clone();
            let Some(table) = st.table else {
                return;
            };
            let text = browse_text(engine, &table, st.page, st.limit);
            w.set_query_text(SharedString::from(text.clone()));
            run_sql(text);
        })
    };

    {
        let weak = window.as_weak();
        let cur_engine = cur_engine.clone();
        let browse = browse.clone();
        let current = current.clone();
        let rt = rt.clone();
        let run_browse = run_browse.clone();
        window.on_open_table(move |db, label| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let Some(engine) = *cur_engine.borrow() else {
                return;
            };
            let label = label.to_string();
            let db = db.to_string();
            let table = rdbs_core::write::TableRef {
                database: (!db.is_empty()).then(|| db.clone()),
                schema: matches!(engine, rdbs_connstore::Engine::Postgres)
                    .then(|| w.get_schema_name().to_string()),
                name: label.clone(),
            };
            // Fresh container: page 0, keep the user's limit, forget totals.
            {
                let mut st = browse.lock().unwrap();
                st.table = Some(table.clone());
                st.page = 0;
                st.total = None;
                st.pk_cols.clear();
            }
            w.set_active_table(SharedString::from(label));
            w.set_show_structure(false);
            w.set_total_rows(-1);
            w.set_grid_read_only(true);
            run_browse();

            // Fetch total + primary key off-thread; footer updates when done.
            let weak2 = weak.clone();
            let current = current.clone();
            let browse = browse.clone();
            rt.spawn(async move {
                let guard = current.lock().await;
                let Some((_, driver)) = guard.as_ref() else {
                    return;
                };
                let total = driver.count(&table).await.ok();
                let pk = driver.primary_key(&table).await.unwrap_or_default();
                let (page, limit) = {
                    let mut st = browse.lock().unwrap();
                    // Ignore a late reply if the user already opened another table.
                    if st.table.as_ref() != Some(&table) {
                        return;
                    }
                    st.total = total;
                    st.pk_cols = pk.clone();
                    (st.page, st.limit)
                };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak2.upgrade() {
                        w.set_total_rows(total.map(|t| t as i32).unwrap_or(-1));
                        w.set_grid_read_only(pk.is_empty());
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

    // ----- pagination footer: prev / next / refresh / limit -----
    {
        let browse = browse.clone();
        let run_browse = run_browse.clone();
        window.on_prev_page(move || {
            {
                let mut st = browse.lock().unwrap();
                if st.page == 0 {
                    return;
                }
                st.page -= 1;
            }
            run_browse();
        });
    }
    {
        let browse = browse.clone();
        let run_browse = run_browse.clone();
        window.on_next_page(move || {
            browse.lock().unwrap().page += 1;
            run_browse();
        });
    }
    {
        let weak = window.as_weak();
        let browse = browse.clone();
        let current = current.clone();
        let rt = rt.clone();
        let run_browse = run_browse.clone();
        window.on_refresh_page(move || {
            run_browse();
            // Re-count in the background so the total tracks external writes.
            let table = browse.lock().unwrap().table.clone();
            let Some(table) = table else { return };
            let weak2 = weak.clone();
            let current = current.clone();
            let browse = browse.clone();
            rt.spawn(async move {
                let guard = current.lock().await;
                let Some((_, driver)) = guard.as_ref() else {
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
        let browse = browse.clone();
        let run_browse = run_browse.clone();
        window.on_set_limit(move |text| {
            let parsed = text.trim().parse::<u64>().ok();
            let Some(l) = parsed.map(|l| l.clamp(1, 10_000)) else {
                return;
            };
            {
                let mut st = browse.lock().unwrap();
                st.limit = l;
                st.page = 0;
            }
            run_browse();
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
        window.on_toggle_schema_node(move |label| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let engine = *cur_engine.borrow();
            let label = label.to_string();

            // Mongo/Redis: database headers open an opt-in set (default closed)
            // and load their leaves (collections / keys) lazily on first expand.
            if matches!(
                engine,
                Some(rdbs_connstore::Engine::Mongo) | Some(rdbs_connstore::Engine::Redis)
            ) {
                let leaf_kind = if engine == Some(rdbs_connstore::Engine::Redis) {
                    "key"
                } else {
                    "collection"
                };
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
                let need_fetch = now_open && {
                    let mut l = loaded_dbs.lock().unwrap();
                    if l.contains(&label) {
                        false
                    } else {
                        l.insert(label.clone());
                        true
                    }
                };
                if need_fetch {
                    let driver = current.clone();
                    let raw_nodes = raw_nodes.clone();
                    let expanded_tables = expanded_tables.clone();
                    let loaded_dbs = loaded_dbs.clone();
                    let sidebar_filter = sidebar_filter.clone();
                    let weak2 = weak.clone();
                    let db = label.clone();
                    rt.spawn(async move {
                        let containers = {
                            let guard = driver.lock().await;
                            match &*guard {
                                Some((_, drv)) => drv.containers(&db).await.unwrap_or_default(),
                                None => return,
                            }
                        };
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

    // ----- sidebar tree filter -----
    {
        let weak = window.as_weak();
        let raw_nodes = raw_nodes.clone();
        let expanded_tables = expanded_tables.clone();
        let loaded_dbs = loaded_dbs.clone();
        let collapsed_categories = collapsed_categories.clone();
        let cur_engine = cur_engine.clone();
        let sidebar_filter = sidebar_filter.clone();
        window.on_filter_tree(move |text| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            *sidebar_filter.lock().unwrap() = text.to_string();
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

    // ----- cell selection (click in the grid) -----
    {
        let weak = window.as_weak();
        window.on_select_cell(move |r, c| {
            if let Some(w) = weak.upgrade() {
                w.set_selected_row(r);
                w.set_selected_col(c);
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
    fn label_to_engine(label: &str) -> rdbs_connstore::Engine {
        match label {
            "MySQL" => rdbs_connstore::Engine::MySql,
            "Redis" => rdbs_connstore::Engine::Redis,
            "MongoDB" => rdbs_connstore::Engine::Mongo,
            _ => rdbs_connstore::Engine::Postgres,
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
            w.set_f_import_url(SharedString::default());
            w.set_form_error(SharedString::default());
            w.set_test_result(SharedString::default());
            w.set_test_ok(false);
            w.set_test_busy(false);
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
                rdbs_core::conn::SslMode::Disable => "Disable",
                rdbs_core::conn::SslMode::Prefer => "Prefer",
                rdbs_core::conn::SslMode::Require => "Require",
            }));
            w.set_f_color(SharedString::from(
                sc.color.unwrap_or_else(|| "#3b82f6".into()),
            ));
            w.set_f_import_url(SharedString::default());
            w.set_form_error(SharedString::default());
            w.set_test_result(SharedString::default());
            w.set_test_ok(false);
            w.set_test_busy(false);
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
    // import URL -> parse and fill form fields for review.
    {
        let weak = window.as_weak();
        window.on_form_import_url(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let raw = w.get_f_import_url().to_string();
            match rdbs_connstore::parse_conn_url(&raw) {
                Ok(parsed) => {
                    if let Some(engine) = parsed.engine {
                        w.set_f_engine(SharedString::from(AnyDriver::label(engine)));
                    }
                    if let Some(host) = parsed.host {
                        w.set_f_host(SharedString::from(host));
                    }
                    // Port from the URL wins; otherwise apply the engine default
                    // (mirrors form_engine_changed) without clobbering a URL port.
                    if let Some(port) = parsed.port {
                        w.set_f_port(SharedString::from(port.to_string()));
                    } else if let Some(engine) = parsed.engine {
                        w.set_f_port(SharedString::from(default_port(AnyDriver::label(engine))));
                    }
                    if let Some(user) = parsed.user {
                        w.set_f_user(SharedString::from(user));
                    }
                    if let Some(password) = parsed.password {
                        w.set_f_password(SharedString::from(password));
                    }
                    if let Some(database) = parsed.database {
                        w.set_f_database(SharedString::from(database));
                    }
                    if let Some(sslmode) = parsed.sslmode {
                        w.set_f_sslmode(SharedString::from(match sslmode {
                            rdbs_core::conn::SslMode::Disable => "Disable",
                            rdbs_core::conn::SslMode::Prefer => "Prefer",
                            rdbs_core::conn::SslMode::Require => "Require",
                        }));
                    }
                    w.set_form_error(SharedString::default());
                }
                Err(e) => {
                    w.set_form_error(SharedString::from(format!("import failed: {e}")));
                }
            }
        });
    }
    // cancel
    {
        let weak = window.as_weak();
        window.on_form_cancel(move || {
            if let Some(w) = weak.upgrade() {
                // Clear any in-flight test state so the form is never stuck on
                // "Testing connection…" when reopened.
                w.set_test_busy(false);
                w.set_test_result(SharedString::default());
                w.set_form_open(false);
            }
        });
    }
    // test connection: build a config straight from the form fields (not the
    // store) so unsaved edits are exercised, open a real connection, then drop
    // it. Result reported in the form's test-result line.
    {
        let weak = window.as_weak();
        let rt = rt.clone();
        window.on_form_test_conn(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let host = w.get_f_host().to_string();
            if host.trim().is_empty() {
                w.set_test_ok(false);
                w.set_test_result(SharedString::from("host is required"));
                return;
            }
            let port: u16 = match w.get_f_port().to_string().parse() {
                Ok(p) if p != 0 => p,
                _ => {
                    w.set_test_ok(false);
                    w.set_test_result(SharedString::from("port must be a number 1-65535"));
                    return;
                }
            };
            let engine = label_to_engine(w.get_f_engine().as_ref());
            let sslmode = match w.get_f_sslmode().to_string().as_str() {
                "Disable" => rdbs_core::conn::SslMode::Disable,
                "Require" => rdbs_core::conn::SslMode::Require,
                _ => rdbs_core::conn::SslMode::Prefer,
            };
            let database = {
                let d = w.get_f_database().to_string();
                if d.is_empty() {
                    None
                } else {
                    Some(d)
                }
            };
            let password = {
                let p = w.get_f_password().to_string();
                if p.is_empty() {
                    None
                } else {
                    Some(p)
                }
            };
            let cfg = rdbs_core::conn::ConnConfig {
                host,
                port,
                user: w.get_f_user().to_string(),
                database,
                password,
                sslmode,
            };

            w.set_test_busy(true);
            w.set_test_ok(false);
            w.set_test_result(SharedString::default());
            w.set_form_error(SharedString::default());

            let weak2 = weak.clone();
            rt.spawn(async move {
                let attempt = async {
                    let driver = AnyDriver::connect(engine, &cfg).await?;
                    driver.ping().await?;
                    Ok::<_, rdbs_core::error::RdbsError>(())
                };
                // ponytail: timeout-bounded, no hard abort; add CancellationToken
                // if a true cancel button is ever needed. Bounds a hung connect so
                // "Testing connection…" always resolves.
                let result =
                    match tokio::time::timeout(std::time::Duration::from_secs(8), attempt).await {
                        Ok(r) => r,
                        Err(_) => Err(rdbs_core::error::RdbsError::Connection(
                            "connection timed out".into(),
                        )),
                    };
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak2.upgrade() {
                        w.set_test_busy(false);
                        match result {
                            Ok(()) => {
                                w.set_test_ok(true);
                                w.set_test_result(SharedString::from("connection ok"));
                            }
                            Err(e) => {
                                w.set_test_ok(false);
                                w.set_test_result(SharedString::from(format!(
                                    "connection failed: {e}"
                                )));
                            }
                        }
                    }
                });
            });
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
            let collapsed = collapsed.clone();
            let conn_filter = conn_filter.clone();
            move || {
                if let Some(w) = weak.upgrade() {
                    let items = build_conn_items(
                        &store.borrow(),
                        &collapsed.borrow(),
                        &conn_filter.borrow(),
                    );
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
                "Disable" => rdbs_core::conn::SslMode::Disable,
                "Require" => rdbs_core::conn::SslMode::Require,
                _ => rdbs_core::conn::SslMode::Prefer,
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

            let result: rdbs_connstore::Result<()> = (|| {
                let mut st = store.borrow_mut();
                let conn_id = if id.is_empty() {
                    let mut sc = rdbs_connstore::SavedConnection::new(
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
                        .ok_or_else(|| rdbs_connstore::ConnStoreError::NotFound(id.clone()))?;
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
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
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
            let items =
                build_conn_items(&store.borrow(), &collapsed.borrow(), &conn_filter.borrow());
            w.set_connections(ModelRc::from(Rc::new(VecModel::from(items))));
        });
    }

    window.run()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes() -> Vec<model::VmTreeNode> {
        vec![
            model::VmTreeNode {
                label: "users".into(),
                kind: "table".into(),
            },
            model::VmTreeNode {
                label: "orders".into(),
                kind: "table".into(),
            },
            model::VmTreeNode {
                label: "audit_log".into(),
                kind: "table".into(),
            },
        ]
    }

    #[test]
    fn sidebar_filter_keeps_matching_containers_and_counts() {
        let rows = schema_display_rows(
            &nodes(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            Some(rdbs_connstore::Engine::Postgres),
            "or",
        );
        let tables: Vec<_> = rows.iter().filter(|r| r.kind == "table").collect();
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].label.as_str(), "orders");
        let header = rows.iter().find(|r| r.label.as_str() == "Tables").unwrap();
        assert_eq!(header.count, 1);
    }

    #[test]
    fn sidebar_filter_empty_keeps_all_with_total_count() {
        let rows = schema_display_rows(
            &nodes(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            Some(rdbs_connstore::Engine::Postgres),
            "",
        );
        assert_eq!(rows.iter().filter(|r| r.kind == "table").count(), 3);
        let header = rows.iter().find(|r| r.label.as_str() == "Tables").unwrap();
        assert_eq!(header.count, 3);
    }

    #[test]
    fn page_bounds_first_page_full() {
        // 300 shown of 1000 total: 1–300, no prev, next available
        assert_eq!(
            page_bounds(0, 300, Some(1000), 300),
            (1, 300, false, true)
        );
    }

    #[test]
    fn page_bounds_last_partial_page() {
        // page 3 of 1000 rows at limit 300 → 901–1000, prev only
        assert_eq!(
            page_bounds(3, 300, Some(1000), 100),
            (901, 1000, true, false)
        );
    }

    #[test]
    fn page_bounds_unknown_total_assumes_more_on_full_page() {
        assert_eq!(page_bounds(0, 300, None, 300), (1, 300, false, true));
        assert_eq!(page_bounds(0, 300, None, 120), (1, 120, false, false));
    }

    #[test]
    fn page_bounds_empty_page_shows_zero() {
        assert_eq!(page_bounds(0, 300, Some(0), 0), (0, 0, false, false));
    }

    #[test]
    fn browse_text_per_engine() {
        let t = rdbs_core::write::TableRef {
            database: None,
            schema: Some("public".into()),
            name: "users".into(),
        };
        assert_eq!(
            browse_text(rdbs_connstore::Engine::Postgres, &t, 1, 300),
            "SELECT * FROM \"public\".\"users\" LIMIT 300 OFFSET 300"
        );
        assert_eq!(
            browse_text(rdbs_connstore::Engine::MySql, &t, 0, 50),
            "SELECT * FROM `users` LIMIT 50 OFFSET 0"
        );
        assert_eq!(
            browse_text(rdbs_connstore::Engine::Redis, &t, 2, 100),
            "BROWSE users 200 100"
        );
        let m = rdbs_core::write::TableRef {
            database: Some("shop".into()),
            schema: None,
            name: "orders".into(),
        };
        assert_eq!(
            browse_text(rdbs_connstore::Engine::Mongo, &m, 1, 50),
            "{\"collection\":\"orders\",\"database\":\"shop\",\"op\":\"find\",\"body\":{},\"limit\":50,\"skip\":50}"
        );
    }

    #[test]
    fn sidebar_filter_is_case_insensitive() {
        let rows = schema_display_rows(
            &nodes(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            Some(rdbs_connstore::Engine::Postgres),
            "USERS",
        );
        assert_eq!(rows.iter().filter(|r| r.kind == "table").count(), 1);
    }
}
