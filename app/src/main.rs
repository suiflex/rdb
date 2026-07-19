//! rdb-app: Slint desktop binary.
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

mod completion;
mod dispatch;
mod editor;
mod export;
mod mock;
mod model;
mod query_parse;
#[cfg(feature = "mock")]
mod shot;
mod sql_format;
mod theme;
mod update;

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
    store: &rdb_connstore::ConnStore,
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
        // Ungrouped connections list flat (no header) when they are the only
        // bucket; once real groups exist they get their own UNGROUPED header.
        let is_ungrouped = g == UNGROUPED && order.len() == 1;
        let expanded = is_ungrouped || !collapsed.contains(g);
        if !is_ungrouped {
            rows.push(ConnItem {
                id: SharedString::default(),
                name: g.to_uppercase().into(),
                engine: SharedString::default(),
                color: theme::accent_or_default(""),
                is_header: true,
                expanded,
                index: -1,
                group: g.clone().into(),
                subline: SharedString::default(),
                local: false,
                count: buckets[g].len() as i32,
                favorite: false,
            });
        }
        if !expanded {
            continue;
        }
        // Favorites float to the top of their group, then explicit sort order.
        // Stable sort keeps store insertion order for equal keys (order == 0).
        let mut idxs = buckets[g].clone();
        idxs.sort_by_key(|&i| {
            let s = &store.list()[i];
            (!s.favorite, s.order)
        });
        for &i in &idxs {
            let s = &store.list()[i];
            let subline = match &s.database {
                Some(db) => format!("{} : {}", s.host, db),
                None => s.host.clone(),
            };
            rows.push(ConnItem {
                id: s.id.clone().into(),
                name: s.name.clone().into(),
                engine: AnyDriver::badge(s.engine).into(),
                color: theme::accent_or_default(s.color.as_deref().unwrap_or("")),
                is_header: false,
                expanded: true,
                index: i as i32,
                group: g.clone().into(),
                subline: subline.into(),
                local: s.local,
                count: 0,
                favorite: s.favorite,
            });
        }
    }
    rows
}

/// Top-level sidebar categories per engine (TablePlus-style). The label is also
/// the toggle key, so `on_toggle_schema_node` can tell a category click from a
/// table click. The first entry is the "primary" category that holds the
/// engine's containers. ponytail: SQL keeps the Views/Functions placeholders.
fn sidebar_categories(engine: Option<rdb_connstore::Engine>) -> &'static [&'static str] {
    match engine {
        // Mongo/Redis use the nested database→leaf path, not these categories.
        Some(rdb_connstore::Engine::Mongo) => &["Collections"],
        _ => &["Functions", "Tables"],
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
/// Sidebar categories collapsed on a fresh connection / schema switch.
/// Functions start closed so the tables list is what you land on.
fn default_collapsed_cats() -> HashSet<String> {
    HashSet::from(["Functions".to_string()])
}

fn schema_display_rows(
    nodes: &[model::VmTreeNode],
    expanded_tables: &HashSet<String>,
    collapsed_cats: &HashSet<String>,
    loaded_dbs: &HashSet<String>,
    engine: Option<rdb_connstore::Engine>,
    filter: &str,
) -> Vec<TreeNode> {
    // Mongo (database→collection) and Redis (database→key) both render as a
    // collapsible database header nesting its own lazily-loaded leaves.
    // `expanded_tables` is the set of OPEN databases (default closed).
    match engine {
        Some(rdb_connstore::Engine::Mongo) => {
            return nested_display_rows(nodes, expanded_tables, loaded_dbs, "collection", filter);
        }
        Some(rdb_connstore::Engine::Redis) => {
            return nested_display_rows(nodes, expanded_tables, loaded_dbs, "key", filter);
        }
        // Cassandra nests keyspace→table like Mongo nests database→collection.
        Some(rdb_connstore::Engine::Cassandra) => {
            return nested_display_rows(nodes, expanded_tables, loaded_dbs, "table", filter);
        }
        _ => {}
    }

    let needle = filter.to_lowercase();
    let filtering = !needle.is_empty();
    let matches = |label: &str| !filtering || label.to_lowercase().contains(&needle);
    let categories = sidebar_categories(engine);
    // Matching containers live under "Tables"; functions under "Functions".
    let container_count = nodes
        .iter()
        .filter(|n| {
            matches!(n.kind.as_str(), "table" | "collection" | "keyspace") && matches(&n.label)
        })
        .count() as i32;
    let function_count = nodes
        .iter()
        .filter(|n| n.kind == "function" && matches(&n.label))
        .count() as i32;
    let mut rows: Vec<TreeNode> = Vec::new();
    for &cat in categories {
        let is_fn_cat = cat == "Functions";
        if is_fn_cat && function_count == 0 && !filtering {
            continue; // engines without routines skip the header entirely
        }
        // While filtering, categories are forced open so matches stay visible.
        let cat_open = filtering || !collapsed_cats.contains(cat);
        rows.push(TreeNode {
            label: cat.into(),
            depth: 0,
            kind: "category".into(),
            expanded: cat_open,
            db: SharedString::default(),
            count: if is_fn_cat {
                function_count
            } else {
                container_count
            },
        });
        if !cat_open {
            continue;
        }
        if is_fn_cat {
            for n in nodes.iter().filter(|n| n.kind == "function") {
                if !matches(&n.label) {
                    continue;
                }
                rows.push(TreeNode {
                    label: n.label.clone().into(),
                    depth: 1,
                    kind: "function".into(),
                    expanded: false,
                    db: SharedString::default(),
                    count: 0,
                });
            }
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
            } else if n.kind != "function" {
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
        let db_open = expanded_dbs.contains(db)
            || (filtering && loaded_dbs.contains(db) && !leaves.is_empty());
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
    table: Option<rdb_core::write::TableRef>,
    page: u64,
    limit: u64,
    total: Option<u64>,
    pk_cols: Vec<String>,
    /// Compass-style Mongo filter document (raw JSON, already validated). Empty
    /// = browse all. Ignored for non-Mongo engines.
    mongo_filter: String,
    /// Per-column server-side filters as (column-name, raw-expr) for SQL engines.
    /// Empty expr = no filter on that column. Pushed into the browse query's
    /// WHERE clause so filtering hits the DB, not just the fetched page.
    col_filters: Vec<(String, String)>,
}

/// Default browse page size per engine. Mongo documents are fat, so a Mongo
/// collection opens 20 at a time (matching Compass) instead of the SQL default.
/// ponytail: per-engine default only; the stepper + paging handle the rest.
fn default_browse_limit(engine: rdb_connstore::Engine) -> u64 {
    match engine {
        rdb_connstore::Engine::Mongo => 20,
        _ => 300,
    }
}

/// Operators exposed by the active engine. The filter itself is evaluated on
/// the fetched page, but the vocabulary mirrors what that engine accepts.
fn filter_operators(engine: rdb_connstore::Engine) -> Vec<SharedString> {
    use rdb_connstore::Engine;
    let ops: &[&str] = match engine {
        Engine::Postgres => &[
            "=",
            "<>",
            ">",
            ">=",
            "<",
            "<=",
            "LIKE",
            "ILIKE",
            "IN",
            "IS NULL",
            "IS NOT NULL",
        ],
        Engine::MySql | Engine::Sqlite => &[
            "=",
            "<>",
            ">",
            ">=",
            "<",
            "<=",
            "LIKE",
            "IN",
            "IS NULL",
            "IS NOT NULL",
        ],
        Engine::Cassandra => &[
            "=",
            "<>",
            ">",
            ">=",
            "<",
            "<=",
            "IN",
            "IS NULL",
            "IS NOT NULL",
        ],
        Engine::Redis | Engine::Mongo => &[
            "=",
            "<>",
            ">",
            ">=",
            "<",
            "<=",
            "IN",
            "IS NULL",
            "IS NOT NULL",
        ],
    };
    ops.iter().copied().map(SharedString::from).collect()
}

#[cfg(target_os = "macos")]
fn install_macos_app_icon() {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    let data = NSData::with_bytes(include_bytes!("../assets/icon@512.png"));
    let Some(icon) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    let app = NSApplication::sharedApplication(main_thread);
    // SAFETY: `icon` is a live NSImage and AppKit is called on the main thread.
    unsafe { app.setApplicationIconImage(Some(&icon)) };
}

#[cfg(not(target_os = "macos"))]
fn install_macos_app_icon() {}

/// One cached SQL result, shown as a result tab. ⌘⏎ replaces the active one;
/// ⌘\ appends a new one.
#[derive(Clone)]
struct StoredResult {
    view: model::ResultView,
    meta: String,
    latency: String,
}

/// The live editor + result state a single query pane owns independently. A SQL
/// tab can show up to two of these side by side (dual-pane split), so everything
/// that must not be shared between panes — the editor buffer, folds, completion,
/// find hits, result set, grid view/sort/filter, and streaming — lives here.
struct PaneRuntime {
    ed_state: Rc<RefCell<editor::EditorState>>,
    folded_heads: Rc<RefCell<HashSet<usize>>>,
    completion_ctx: Rc<RefCell<(usize, Vec<String>)>>,
    find_hits: Rc<RefCell<Vec<(usize, usize, usize)>>>,
    edit_buf: Arc<std::sync::Mutex<model::EditBuffer>>,
    results: Arc<std::sync::Mutex<Vec<StoredResult>>>,
    active_result: Arc<std::sync::Mutex<usize>>,
    result_new_tab: Arc<std::sync::atomic::AtomicBool>,
    displayed_grid: Arc<std::sync::Mutex<Option<model::GridModel>>>,
    browse: Arc<std::sync::Mutex<BrowseState>>,
    hidden_cols: Arc<std::sync::Mutex<HashSet<usize>>>,
    sort_state: Arc<std::sync::Mutex<(i32, bool)>>,
    col_order: Arc<std::sync::Mutex<Vec<usize>>>,
    col_filters: Arc<std::sync::Mutex<Vec<String>>>,
    stream_cancel: Rc<RefCell<Option<Arc<std::sync::atomic::AtomicBool>>>>,
    stream_timer: Rc<RefCell<Option<slint::Timer>>>,
}

impl PaneRuntime {
    fn new() -> Self {
        Self {
            ed_state: Rc::new(RefCell::new(editor::EditorState::from_text(""))),
            folded_heads: Rc::new(RefCell::new(HashSet::new())),
            completion_ctx: Rc::new(RefCell::new((0, Vec::new()))),
            find_hits: Rc::new(RefCell::new(Vec::new())),
            edit_buf: Arc::new(std::sync::Mutex::new(model::EditBuffer::default())),
            results: Arc::new(std::sync::Mutex::new(Vec::new())),
            active_result: Arc::new(std::sync::Mutex::new(0)),
            result_new_tab: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            displayed_grid: Arc::new(std::sync::Mutex::new(None)),
            browse: Arc::new(std::sync::Mutex::new(BrowseState {
                limit: 300,
                ..Default::default()
            })),
            hidden_cols: Arc::new(std::sync::Mutex::new(HashSet::new())),
            sort_state: Arc::new(std::sync::Mutex::new((-1, true))),
            col_order: Arc::new(std::sync::Mutex::new(Vec::new())),
            col_filters: Arc::new(std::sync::Mutex::new(Vec::new())),
            stream_cancel: Rc::new(RefCell::new(None)),
            stream_timer: Rc::new(RefCell::new(None)),
        }
    }
}

/// Rust-owned document state. Slint only renders this list; an empty list and
/// `active_tab_id == None` are a real empty workspace, not a synthetic query.
#[derive(Clone)]
struct WorkspaceTab {
    id: String,
    title: String,
    kind: String,
    query_text: String,
    table: Option<rdb_core::write::TableRef>,
    browse: BrowseState,
    results: Vec<StoredResult>,
    active_result: usize,
    view: Option<StoredResult>,
    indexes: Vec<(String, String)>,
    loading: bool,
    pinned: bool,
    // Workspace group that owns this tab: 0 is left, 1 is right.
    group: usize,
    // Dual-pane split state for this tab. Right-pane results stay in memory
    // like the left pane; only the SQL text is persisted to disk.
    split: bool,
    split_ratio: f32,
    pane1_query: String,
}

impl WorkspaceTab {
    fn sql(id: String, number: usize) -> Self {
        Self {
            id,
            title: format!("Query {number}"),
            kind: "sql".into(),
            query_text: String::new(),
            table: None,
            browse: BrowseState::default(),
            results: Vec::new(),
            active_result: 0,
            view: None,
            indexes: Vec::new(),
            loading: false,
            pinned: true,
            group: 0,
            split: false,
            split_ratio: 0.5,
            pane1_query: String::new(),
        }
    }

    fn is_preview(&self) -> bool {
        self.kind == "table" && !self.pinned
    }
}

fn table_tab_id(connection_id: &str, database: &str, schema: &str, table: &str) -> String {
    format!("table:{connection_id}:{database}:{schema}:{table}")
}

fn workspace_tab_index(tabs: &[WorkspaceTab], active_id: Option<&str>) -> Option<usize> {
    let id = active_id?;
    tabs.iter().position(|tab| tab.id == id)
}

fn replaceable_table_tab_index(tabs: &[WorkspaceTab], active_id: Option<&str>) -> Option<usize> {
    let index = workspace_tab_index(tabs, active_id)?;
    let tab = &tabs[index];
    tab.is_preview().then_some(index)
}

fn set_workspace_tabs(w: &MainWindow, tabs: &[WorkspaceTab], active_id: Option<&str>) {
    let left: Vec<&WorkspaceTab> = tabs.iter().filter(|tab| tab.group == 0).collect();
    let items: Vec<TabItem> = left
        .iter()
        .map(|tab| TabItem {
            kind: tab.kind.clone().into(),
            title: tab.title.clone().into(),
            preview: tab.is_preview(),
        })
        .collect();
    w.set_tabs(ModelRc::from(Rc::new(VecModel::from(items))));
    w.set_active_tab(
        active_id
            .and_then(|id| left.iter().position(|tab| tab.id == id))
            .map(|i| i as i32)
            .unwrap_or(-1),
    );
    let right: Vec<&WorkspaceTab> = tabs.iter().filter(|tab| tab.group == 1).collect();
    let p1_items: Vec<TabItem> = right
        .iter()
        .map(|tab| TabItem {
            kind: tab.kind.clone().into(),
            title: tab.title.clone().into(),
            preview: tab.is_preview(),
        })
        .collect();
    w.set_p1_tabs(ModelRc::from(Rc::new(VecModel::from(p1_items))));
    w.set_split(!right.is_empty());
    if right.is_empty() {
        w.set_p1_active_tab(-1);
    }
}

const QUERY_CONSOLE_CAP: usize = 200;

fn append_query_console(log: &Arc<std::sync::Mutex<Vec<String>>>, sql: impl Into<String>) {
    let sql = sql.into();
    let sql = sql.trim();
    if sql.is_empty() {
        return;
    }
    let mut entries = log.lock().unwrap();
    entries.push(sql.to_string());
    let extra = entries.len().saturating_sub(QUERY_CONSOLE_CAP);
    if extra > 0 {
        entries.drain(..extra);
    }
}

fn sync_query_console(w: &MainWindow, log: &Arc<std::sync::Mutex<Vec<String>>>) {
    let entries: Vec<SharedString> = log
        .lock()
        .unwrap()
        .iter()
        .cloned()
        .map(SharedString::from)
        .collect();
    w.set_query_console(ModelRc::from(Rc::new(VecModel::from(entries))));
}

/// 1-based display bounds of the current page window plus prev/next
/// availability. `shown` is how many rows the page actually returned.
/// Connect + ping, bounded to 8s so "Testing connection…" always resolves.
/// Returns the elapsed milliseconds on success.
// ponytail: timeout-bounded, no hard abort; add CancellationToken if a true
// cancel button is ever needed.
async fn try_connect(
    engine: rdb_connstore::Engine,
    cfg: rdb_core::conn::ConnConfig,
) -> Result<u64, rdb_core::error::RdbError> {
    let t0 = std::time::Instant::now();
    let attempt = async {
        let driver = AnyDriver::connect(engine, &cfg).await?;
        driver.ping().await?;
        Ok::<_, rdb_core::error::RdbError>(())
    };
    match tokio::time::timeout(std::time::Duration::from_secs(8), attempt).await {
        Ok(Ok(())) => Ok(t0.elapsed().as_millis().max(1) as u64),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(rdb_core::error::RdbError::Connection(
            "connection timed out".into(),
        )),
    }
}

/// Kind of the active tab ("table" | "sql" | "function"), empty when none.
fn active_tab_kind(w: &MainWindow) -> String {
    use slint::Model;
    if w.get_active_tab() < 0 {
        return String::new();
    }
    let tabs = w.get_tabs();
    let ti = w.get_active_tab() as usize;
    tabs.row_data(ti)
        .map(|t| t.kind.to_string())
        .unwrap_or_default()
}

/// Clipboard write; errors are ignored (no clipboard on some CI machines).
fn clip_set(s: &str) {
    use copypasta::ClipboardProvider;
    if let Ok(mut c) = copypasta::ClipboardContext::new() {
        let _ = c.set_contents(s.to_string());
    }
}

/// Clipboard read; None when empty or unavailable.
fn clip_get() -> Option<String> {
    use copypasta::ClipboardProvider;
    copypasta::ClipboardContext::new()
        .ok()?
        .get_contents()
        .ok()
        .filter(|s| !s.is_empty())
}

/// Max recent queries kept, in memory and on disk.
const RECENT_CAP: usize = 50;

/// Load persisted recent-query history; empty on a missing/unreadable file.
fn load_recent() -> Vec<String> {
    let Ok(path) = rdb_connstore::ConnStore::recent_queries_path() else {
        return Vec::new();
    };
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Persist the recent-query history (best-effort; I/O errors are ignored).
fn save_recent(list: &[String]) {
    let Ok(path) = rdb_connstore::ConnStore::recent_queries_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(list) {
        let _ = std::fs::write(path, json);
    }
}

/// Record an executed query at the head of the history (dedupe, cap
/// `RECENT_CAP`) and persist it, except in mock mode.
fn record_recent(list: &RefCell<Vec<String>>, text: &str) {
    let t = text.trim();
    if t.is_empty() {
        return;
    }
    let mut v = list.borrow_mut();
    v.retain(|s| s != t);
    v.insert(0, t.to_string());
    v.truncate(RECENT_CAP);
    if !mock::mock_mode() {
        save_recent(&v);
    }
}

/// Minimal on-disk shape of a query tab: identity + SQL text only. Results,
/// browse state and view are transient (and may hold DB data), so they are
/// never persisted — only SQL scratch tabs survive a restart.
#[derive(serde::Serialize, serde::Deserialize)]
struct PersistedTab {
    id: String,
    title: String,
    query_text: String,
    #[serde(default)]
    group: usize,
    #[serde(default)]
    split: bool,
    #[serde(default)]
    pane1_query: String,
    #[serde(default = "default_split_ratio")]
    split_ratio: f32,
}

fn default_split_ratio() -> f32 {
    0.5
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct PersistedTabs {
    tabs: Vec<PersistedTab>,
    active: Option<String>,
}

/// Persist the SQL tabs (kind == "sql") and the active tab id. Best-effort;
/// I/O errors are ignored. Skipped in mock mode.
fn save_query_tabs(tabs: &[WorkspaceTab], active: Option<&str>) {
    if mock::mock_mode() {
        return;
    }
    let Ok(path) = rdb_connstore::ConnStore::query_tabs_path() else {
        return;
    };
    let sql: Vec<PersistedTab> = tabs
        .iter()
        .filter(|t| t.kind == "sql")
        .map(|t| PersistedTab {
            id: t.id.clone(),
            title: t.title.clone(),
            query_text: t.query_text.clone(),
            group: t.group.min(1),
            split: t.split,
            pane1_query: t.pane1_query.clone(),
            split_ratio: t.split_ratio,
        })
        .collect();
    let active = active
        .filter(|id| sql.iter().any(|t| t.id == *id))
        .map(|s| s.to_string());
    let payload = PersistedTabs { tabs: sql, active };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(path, json);
    }
}

/// Load persisted SQL tabs into fresh `WorkspaceTab`s (empty results/browse).
/// Returns the tabs and the active tab id (if it still exists).
fn load_query_tabs() -> (Vec<WorkspaceTab>, Option<String>) {
    let Ok(path) = rdb_connstore::ConnStore::query_tabs_path() else {
        return (Vec::new(), None);
    };
    let Some(payload): Option<PersistedTabs> = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
    else {
        return (Vec::new(), None);
    };
    let tabs: Vec<WorkspaceTab> = payload
        .tabs
        .into_iter()
        .map(|p| {
            let mut t = WorkspaceTab::sql(p.id, 0);
            t.title = p.title;
            t.query_text = p.query_text;
            t.group = p.group.min(1);
            t.split = p.split;
            t.pane1_query = p.pane1_query;
            t.split_ratio = p.split_ratio.clamp(0.2, 0.8);
            t
        })
        .collect();
    let active = payload.active.filter(|id| tabs.iter().any(|t| t.id == *id));
    (tabs, active)
}

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
/// Default pixel width for a fresh result column, keyed on type then name.
fn default_col_width(name: &str, type_name: &str) -> f32 {
    if type_name == "bar" {
        return 520.0;
    }
    match name {
        "sector" => 420.0,
        "total" => 120.0,
        "name" | "email" => 265.0,
        "code" => 100.0,
        "short_name" => 115.0,
        "country" => 100.0,
        "exch" => 95.0,
        "ccy" => 64.0,
        _ => match type_name {
            "uuid" | "fk" => 185.0,
            "timestamptz" | "timestamp" => 190.0,
            "int4" | "int8" | "numeric" => 80.0,
            _ => 140.0,
        },
    }
}

/// Build a `WHERE` clause from per-column filters for SQL engines. Each entry is
/// (column-name, raw-expr); an empty expr is skipped. A leading comparison
/// operator (`=`, `<>`, `>=`, `<=`, `>`, `<`, `!=`) is honoured, otherwise the
/// expr is treated as a substring match via `contains_kw` (`LIKE`/`ILIKE`).
/// `quote` wraps/escapes the identifier for the dialect. Values are always
/// single-quoted with `'` doubled; the DB coerces to the column type.
/// ponytail: string literal + implicit cast; typed binds are a follow-up.
fn sql_where(
    cols: &[(String, String)],
    quote: impl Fn(&str) -> String,
    contains_kw: &str,
) -> String {
    let esc = |v: &str| v.replace('\'', "''");
    let ops = ["<=", ">=", "<>", "!=", "=", ">", "<"];
    let clauses: Vec<String> = cols
        .iter()
        .filter_map(|(name, raw)| {
            let raw = raw.trim();
            if raw.is_empty() {
                return None;
            }
            Some(match ops.iter().find(|op| raw.starts_with(**op)) {
                Some(op) => {
                    let val = raw[op.len()..].trim();
                    format!("{} {} '{}'", quote(name), op, esc(val))
                }
                None => format!("{} {} '%{}%'", quote(name), contains_kw, esc(raw)),
            })
        })
        .collect();
    if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    }
}

fn browse_text(
    engine: rdb_connstore::Engine,
    table: &rdb_core::write::TableRef,
    page: u64,
    limit: u64,
    // Mongo-only filter document (raw JSON, empty = all). Unused by SQL engines.
    filter: &str,
    // Per-column (name, expr) filters → WHERE clause. Unused by Mongo/Redis.
    col_filters: &[(String, String)],
) -> String {
    let offset = page * limit;
    match engine {
        rdb_connstore::Engine::Postgres => {
            let schema = table.schema.as_deref().unwrap_or("public");
            let q = |s: &str| s.replace('"', "\"\"");
            let where_sql = sql_where(col_filters, |c| format!("\"{}\"", q(c)), "ILIKE");
            format!(
                "SELECT * FROM \"{}\".\"{}\"{where_sql} LIMIT {limit} OFFSET {offset}",
                q(schema),
                q(&table.name)
            )
        }
        rdb_connstore::Engine::MySql => {
            let where_sql = sql_where(
                col_filters,
                |c| format!("`{}`", c.replace('`', "``")),
                "LIKE",
            );
            format!(
                "SELECT * FROM `{}`{where_sql} LIMIT {limit} OFFSET {offset}",
                table.name.replace('`', "``")
            )
        }
        rdb_connstore::Engine::Sqlite => {
            let where_sql = sql_where(
                col_filters,
                |c| format!("\"{}\"", c.replace('"', "\"\"")),
                "LIKE",
            );
            format!(
                "SELECT * FROM \"{}\"{where_sql} LIMIT {limit} OFFSET {offset}",
                table.name.replace('"', "\"\"")
            )
        }
        rdb_connstore::Engine::Cassandra => {
            // ponytail: CQL is LIMIT-only (no OFFSET); real paging state is a
            // follow-up. Keyspace travels in `database`.
            let q = |s: &str| s.replace('"', "\"\"");
            match table.database.as_deref() {
                Some(ks) if !ks.is_empty() => format!(
                    "SELECT * FROM \"{}\".\"{}\" LIMIT {limit}",
                    q(ks),
                    q(&table.name)
                ),
                _ => format!("SELECT * FROM \"{}\" LIMIT {limit}", q(&table.name)),
            }
        }
        rdb_connstore::Engine::Mongo => {
            let db = table
                .database
                .as_deref()
                .map(|d| format!("\"database\":\"{d}\","))
                .unwrap_or_default();
            let body = match filter.trim() {
                "" => "{}",
                f => f,
            };
            format!(
                "{{\"collection\":\"{}\",{db}\"op\":\"find\",\"body\":{body},\"limit\":{limit},\"skip\":{offset}}}",
                table.name
            )
        }
        rdb_connstore::Engine::Redis => {
            format!("BROWSE {} {offset} {limit}", table.name)
        }
    }
}

/// TablePlus-style row-limit guard for a manually-run statement: append
/// `LIMIT n` to a bare `SELECT`/`WITH` read so `SELECT * FROM huge_table` can't
/// buffer hundreds of thousands of rows into memory and freeze the grid. The
/// SQL is returned UNCHANGED when a cap doesn't apply — a non-SQL engine, a
/// write/DDL/EXPLAIN, a `SELECT ... INTO`, or a statement that already carries
/// its own `LIMIT` (the browse path and hand-written limits stay intact).
/// ponytail: word-scan, not a real SQL parser — a `limit`/`insert` inside a
/// subquery just skips the cap (safe: no double-LIMIT, no corruption). Pulling
/// the *full* millions is the streaming follow-up, not this guard.
/// True when `sql` is a single bare `SELECT`/`WITH` read on a SQL engine that
/// carries no row limit of its own — the shape it is safe to cap OR stream.
/// Writes, DDL, `EXPLAIN`, `SELECT ... INTO`, an existing `LIMIT`, and non-SQL
/// engines are all false (word-scan, not a real parser: a `limit`/`insert`
/// inside a subquery conservatively returns false, so we never mangle it).
fn is_bare_select(engine: rdb_connstore::Engine, sql: &str) -> bool {
    use rdb_connstore::Engine;
    if !matches!(
        engine,
        Engine::Postgres | Engine::MySql | Engine::Sqlite | Engine::Cassandra
    ) {
        return false;
    }
    let trimmed = sql.trim_end().trim_end_matches(';').trim_end();
    let words: Vec<&str> = trimmed
        .split(|c: char| !c.is_ascii_alphanumeric() && c != '_')
        .filter(|w| !w.is_empty())
        .collect();
    let first = words.first().copied().unwrap_or("");
    let is_read = first.eq_ignore_ascii_case("select") || first.eq_ignore_ascii_case("with");
    let has_limit = words.iter().any(|w| w.eq_ignore_ascii_case("limit"));
    let has_write = words.iter().any(|w| {
        w.eq_ignore_ascii_case("insert")
            || w.eq_ignore_ascii_case("update")
            || w.eq_ignore_ascii_case("delete")
            || w.eq_ignore_ascii_case("into")
    });
    is_read && !has_limit && !has_write
}

/// TablePlus-style row cap for a manually-run statement. `limit == 0` means
/// "No limit" (the streaming path owns that case), so the SQL is left as-is.
fn cap_select(engine: rdb_connstore::Engine, sql: &str, limit: u64) -> String {
    if limit == 0 || !is_bare_select(engine, sql) {
        return sql.to_string();
    }
    let trimmed = sql.trim_end().trim_end_matches(';').trim_end();
    format!("{trimmed} LIMIT {limit}")
}

#[cfg(test)]
mod cap_select_tests {
    use super::cap_select;
    use rdb_connstore::Engine;

    #[test]
    fn bare_select_gets_limit() {
        assert_eq!(
            cap_select(Engine::Postgres, "SELECT * from kelurahan_desa;", 300),
            "SELECT * from kelurahan_desa LIMIT 300"
        );
    }

    #[test]
    fn existing_limit_is_left_alone() {
        let sql = "SELECT * FROM t LIMIT 10";
        assert_eq!(cap_select(Engine::Postgres, sql, 300), sql);
    }

    #[test]
    fn writes_ddl_and_explain_untouched() {
        for sql in [
            "UPDATE t SET x = 1",
            "INSERT INTO t VALUES (1)",
            "DELETE FROM t",
            "EXPLAIN SELECT * FROM t",
            "SELECT * INTO backup FROM t",
        ] {
            assert_eq!(cap_select(Engine::Postgres, sql, 300), sql);
        }
    }

    #[test]
    fn cte_read_gets_limit_but_data_modifying_cte_does_not() {
        assert_eq!(
            cap_select(
                Engine::Postgres,
                "WITH x AS (SELECT 1) SELECT * FROM x",
                500
            ),
            "WITH x AS (SELECT 1) SELECT * FROM x LIMIT 500"
        );
        let write_cte = "WITH x AS (DELETE FROM t RETURNING *) SELECT * FROM x";
        assert_eq!(cap_select(Engine::Postgres, write_cte, 500), write_cte);
    }

    #[test]
    fn non_sql_engines_untouched() {
        let sql = "SELECT * FROM t";
        assert_eq!(cap_select(Engine::Redis, sql, 300), sql);
        assert_eq!(cap_select(Engine::Mongo, sql, 300), sql);
    }

    #[test]
    fn no_limit_zero_leaves_sql_untouched() {
        // 0 == "No limit" -> the streaming path handles it, SQL stays clean.
        let sql = "SELECT * FROM kelurahan_desa";
        assert_eq!(cap_select(Engine::Postgres, sql, 0), sql);
        assert!(super::is_bare_select(Engine::Postgres, sql));
        assert!(!super::is_bare_select(
            Engine::Postgres,
            "SELECT * FROM t LIMIT 5"
        ));
    }
}

/// Rows per streamed batch handed to the grid (`No limit` path).
const STREAM_BATCH: usize = 2_000;
/// Soft ceiling on streamed rows kept resident (R1 + soft-cap): once reached we
/// stop pulling so a runaway `SELECT *` on a billion-row table can't exhaust
/// RAM. Export handles the truly-complete set.
const STREAM_SOFT_CAP: usize = 200_000;

/// One message from the off-thread streaming producer to the UI-thread drain
/// timer. Carries plain `Send` data only (no Slint types cross the boundary).
enum StreamMsg {
    Meta(Vec<model::VmColumn>),
    Batch(Vec<rdb_core::result::Row>),
    Done { capped: bool, elapsed_ms: u64 },
    Err(String),
}

/// Push a flat `GridModel` into the window's grid columns/cells properties.
fn push_grid(w: &MainWindow, pane: usize, g: &model::GridModel) {
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
    set_p_col_count(w, pane, cols.len() as i32);
    set_p_columns(w, pane, ModelRc::from(Rc::new(VecModel::from(cols))));
    set_p_cells(w, pane, ModelRc::from(Rc::new(VecModel::from(flat))));
}

/// Update only the row cells (not columns/widths), so the per-column filter
/// inputs keep focus while the user types. Columns are assumed unchanged.
fn set_grid_cells_only(w: &MainWindow, pane: usize, g: &model::GridModel) {
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
    set_p_cells(w, pane, ModelRc::from(Rc::new(VecModel::from(flat))));
    set_p_result_status(
        w,
        pane,
        SharedString::from(format!("{} rows", g.rows.len())),
    );
}

/// Push `g` with the buffer's pending edits overlaid: changed cells show the
/// new text (state 1), delete-marked rows state 2, insert rows appended as
/// state-3 rows at the bottom.
fn paint_grid_with_edits(
    w: &MainWindow,
    pane: usize,
    g: &model::GridModel,
    buf: &model::EditBuffer,
) {
    let ncols = g.columns.len();
    let mut flat: Vec<GridCell> = Vec::with_capacity((g.rows.len() + buf.inserts.len()) * ncols);
    for (r, row) in g.rows.iter().enumerate() {
        let deleted = buf.deletes.contains(&r);
        for (c, cell) in row.iter().enumerate() {
            let (text, is_null, state) = match buf.changes.get(&(r, c)) {
                Some(t) => (
                    t.clone(),
                    t.eq_ignore_ascii_case("null"),
                    if deleted { 2 } else { 1 },
                ),
                None => (cell.text.clone(), cell.is_null, i32::from(deleted) * 2),
            };
            flat.push(GridCell {
                text: text.into(),
                is_null,
                state,
            });
        }
    }
    for ins in &buf.inserts {
        for c in 0..ncols {
            flat.push(GridCell {
                text: ins.get(c).cloned().unwrap_or_default().into(),
                is_null: false,
                state: 3,
            });
        }
    }
    set_p_cells(w, pane, ModelRc::from(Rc::new(VecModel::from(flat))));
}

/// The grid a view displays, if it has one (for edit-buffer bookkeeping).
fn view_grid(v: &model::ResultView) -> Option<model::GridModel> {
    match v {
        model::ResultView::Table(g) => Some(g.clone()),
        model::ResultView::Documents(d) => Some(d.grid.clone()),
        _ => None,
    }
}

fn guard_pending_edits(w: &MainWindow, edit_buf: &std::sync::Mutex<model::EditBuffer>) -> bool {
    let n = edit_buf.lock().unwrap().pending_count();
    if n == 0 {
        return false;
    }
    w.set_status_error(true);
    w.set_result_status(SharedString::from(format!(
        "{n} pending change(s) — ⌘S to commit, or Discard"
    )));
    true
}

/// Clear the tabular grid properties (used by non-tabular result kinds).
fn clear_grid(w: &MainWindow, pane: usize) {
    set_p_col_count(w, pane, 0);
    set_p_columns(
        w,
        pane,
        ModelRc::from(Rc::new(VecModel::<GridColumn>::default())),
    );
    set_p_cells(
        w,
        pane,
        ModelRc::from(Rc::new(VecModel::<GridCell>::default())),
    );
}

// ----- per-pane result setters: pane 0 writes the base properties, pane 1 the
// p1-* mirror. Only genuinely per-pane props are routed; tab-global props
// (browse paging, filter chrome, status latency) always use the base setter.
fn set_p_cells(w: &MainWindow, pane: usize, m: ModelRc<GridCell>) {
    if pane == 0 {
        w.set_grid_cells(m);
    } else {
        w.set_p1_cells(m);
    }
}
fn set_p_columns(w: &MainWindow, pane: usize, m: ModelRc<GridColumn>) {
    if pane == 0 {
        w.set_grid_columns(m);
    } else {
        w.set_p1_columns(m);
    }
}
fn set_p_col_count(w: &MainWindow, pane: usize, n: i32) {
    if pane == 0 {
        w.set_grid_col_count(n);
    } else {
        w.set_p1_col_count(n);
    }
}
fn set_p_col_widths(w: &MainWindow, pane: usize, m: ModelRc<f32>) {
    if pane == 0 {
        w.set_grid_col_widths(m);
    } else {
        w.set_p1_col_widths(m);
    }
}
fn set_p_result_kind(w: &MainWindow, pane: usize, k: i32) {
    if pane == 0 {
        w.set_result_kind(k);
    } else {
        w.set_p1_result_kind(k);
    }
}
fn set_p_result_status(w: &MainWindow, pane: usize, s: SharedString) {
    if pane == 0 {
        w.set_result_status(s);
    } else {
        w.set_p1_result_status(s);
    }
}
fn set_p_status_error(w: &MainWindow, pane: usize, b: bool) {
    if pane == 0 {
        w.set_status_error(b);
    } else {
        w.set_p1_status_error(b);
    }
}
fn set_p_results_meta(w: &MainWindow, pane: usize, s: SharedString) {
    if pane == 0 {
        w.set_results_meta(s);
    } else {
        w.set_p1_results_meta(s);
    }
}
fn set_p_chart_bars(w: &MainWindow, pane: usize, m: ModelRc<ChartBar>) {
    if pane == 0 {
        w.set_chart_bars(m);
    } else {
        w.set_p1_chart_bars(m);
    }
}
fn set_p_grid_sort(w: &MainWindow, pane: usize, col: i32, asc: bool) {
    if pane == 0 {
        w.set_grid_sort_col(col);
        w.set_grid_sort_asc(asc);
    } else {
        w.set_p1_grid_sort_col(col);
        w.set_p1_grid_sort_asc(asc);
    }
}
fn set_p_grid_col_filters(w: &MainWindow, pane: usize, m: ModelRc<SharedString>) {
    if pane == 0 {
        w.set_grid_col_filters(m);
    } else {
        w.set_p1_grid_col_filters(m);
    }
}
fn set_p_editing(w: &MainWindow, pane: usize, row: i32, col: i32) {
    if pane == 0 {
        w.set_editing_row(row);
        w.set_editing_col(col);
    } else {
        w.set_p1_editing_row(row);
        w.set_p1_editing_col(col);
    }
}
fn set_p_doc_tree(w: &MainWindow, pane: usize, m: ModelRc<DocRow>) {
    if pane == 0 {
        w.set_doc_tree(m);
    } else {
        w.set_p1_doc_tree(m);
    }
}
fn set_p_query_running(w: &MainWindow, pane: usize, b: bool) {
    if pane == 0 {
        w.set_query_running(b);
    } else {
        w.set_p1_query_running(b);
    }
}
fn set_p_streaming(w: &MainWindow, pane: usize, b: bool) {
    if pane == 0 {
        w.set_streaming(b);
    } else {
        w.set_p1_streaming(b);
    }
}
fn set_p_completion_items(w: &MainWindow, pane: usize, m: ModelRc<PaletteItem>) {
    if pane == 0 {
        w.set_completion_items(m);
    } else {
        w.set_p1_completion_items(m);
    }
}
fn set_p_completion_visible(w: &MainWindow, pane: usize, b: bool) {
    if pane == 0 {
        w.set_completion_visible(b);
    } else {
        w.set_p1_completion_visible(b);
    }
}
fn get_p_completion_visible(w: &MainWindow, pane: usize) -> bool {
    if pane == 0 {
        w.get_completion_visible()
    } else {
        w.get_p1_completion_visible()
    }
}
fn set_p_completion_selected(w: &MainWindow, pane: usize, i: i32) {
    if pane == 0 {
        w.set_completion_selected(i);
    } else {
        w.set_p1_completion_selected(i);
    }
}
fn get_p_completion_selected(w: &MainWindow, pane: usize) -> i32 {
    if pane == 0 {
        w.get_completion_selected()
    } else {
        w.get_p1_completion_selected()
    }
}
fn p_completion_count(w: &MainWindow, pane: usize) -> i32 {
    if pane == 0 {
        w.get_completion_items().row_count() as i32
    } else {
        w.get_p1_completion_items().row_count() as i32
    }
}
fn set_p_find_open(w: &MainWindow, pane: usize, b: bool) {
    if pane == 0 {
        w.set_find_open(b);
    } else {
        w.set_p1_find_open(b);
    }
}
fn get_p_find_open(w: &MainWindow, pane: usize) -> bool {
    if pane == 0 {
        w.get_find_open()
    } else {
        w.get_p1_find_open()
    }
}
fn set_p_find_text(w: &MainWindow, pane: usize, s: SharedString) {
    if pane == 0 {
        w.set_find_text(s);
    } else {
        w.set_p1_find_text(s);
    }
}
fn get_p_find_text(w: &MainWindow, pane: usize) -> String {
    if pane == 0 {
        w.get_find_text().to_string()
    } else {
        w.get_p1_find_text().to_string()
    }
}
fn set_p_find_status(w: &MainWindow, pane: usize, s: SharedString) {
    if pane == 0 {
        w.set_find_status(s);
    } else {
        w.set_p1_find_status(s);
    }
}
fn get_p_cursor(w: &MainWindow, pane: usize) -> (usize, usize) {
    if pane == 0 {
        (w.get_cursor_line() as usize, w.get_cursor_col() as usize)
    } else {
        (
            w.get_p1_cursor_line() as usize,
            w.get_p1_cursor_col() as usize,
        )
    }
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

/// List a table's indexes as (name, definition) via the engine catalog.
/// Empty for engines without one (Redis, Mongo) and on any query error.
async fn fetch_indexes(
    engine: rdb_connstore::Engine,
    driver: &AnyDriver,
    table: &rdb_core::write::TableRef,
) -> Vec<(String, String)> {
    let esc = |s: &str| s.replace('\'', "''");
    let sql = match engine {
        rdb_connstore::Engine::Postgres => format!(
            "SELECT indexname, indexdef FROM pg_indexes \
             WHERE tablename = '{}' AND schemaname = '{}' ORDER BY 1",
            esc(&table.name),
            esc(table.schema.as_deref().unwrap_or("public")),
        ),
        rdb_connstore::Engine::MySql => format!(
            "SELECT index_name, GROUP_CONCAT(column_name ORDER BY seq_in_index \
             SEPARATOR ', ') FROM information_schema.statistics \
             WHERE table_name = '{}' AND table_schema = \
             COALESCE(NULLIF('{}', ''), DATABASE()) GROUP BY index_name ORDER BY 1",
            esc(&table.name),
            esc(table.database.as_deref().unwrap_or("")),
        ),
        _ => return Vec::new(),
    };
    match driver.query(&rdb_core::query::Query::Sql(sql)).await {
        Ok(rdb_core::result::ResultSet::Tabular { rows, .. }) => rows
            .into_iter()
            .filter_map(|r| {
                let mut it = r.into_iter();
                Some((it.next()?.render(), it.next()?.render()))
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// Does one cell satisfy `op` against `needle` (already lowercased)? Numeric
/// comparison when both sides parse as f64, else case-insensitive text.
/// Shared by the global filter and the per-column filter row.
fn cell_matches(cell: &model::VmCell, op: &str, needle: &str) -> bool {
    use std::cmp::Ordering;
    match op {
        "is null" | "IS NULL" => cell.is_null,
        "not null" | "IS NOT NULL" => !cell.is_null,
        "=" => cell.text.to_lowercase() == needle,
        "≠" | "!=" | "<>" => cell.text.to_lowercase() != needle,
        ">" | "<" | ">=" | "<=" => {
            let ord = match (cell.text.parse::<f64>(), needle.parse::<f64>()) {
                (Ok(a), Ok(b)) => a.partial_cmp(&b),
                _ => Some(cell.text.to_lowercase().as_str().cmp(needle)),
            };
            match ord {
                Some(Ordering::Greater) => op == ">" || op == ">=",
                Some(Ordering::Less) => op == "<" || op == "<=",
                Some(Ordering::Equal) => op == ">=" || op == "<=",
                None => false,
            }
        }
        "IN" => needle
            .trim()
            .trim_matches(['(', ')'])
            .split(',')
            .map(|v| v.trim().trim_matches(['\'', '"']).to_lowercase())
            .any(|v| v == cell.text.to_lowercase()),
        "LIKE" | "ILIKE" => cell
            .text
            .to_lowercase()
            .contains(needle.trim().trim_matches(['\'', '"']).trim_matches('%')),
        _ => cell.text.to_lowercase().contains(needle),
    }
}

/// Row filter with an optional column + operator condition (`needle` already
/// lowercased). `col: None` falls back to the all-cell contains filter.
fn filter_grid_cond(
    g: &model::GridModel,
    needle: &str,
    col: Option<usize>,
    op: &str,
) -> model::GridModel {
    let Some(c) = col else {
        return filter_grid(g, needle);
    };
    // null checks ignore the value box; other operators with an empty value
    // keep every row (matches the plain filter's behavior)
    if needle.is_empty()
        && op != "is null"
        && op != "not null"
        && op != "IS NULL"
        && op != "IS NOT NULL"
    {
        return g.clone();
    }
    model::GridModel {
        columns: g.columns.clone(),
        rows: g
            .rows
            .iter()
            .filter(|r| r.get(c).is_some_and(|cell| cell_matches(cell, op, needle)))
            .cloned()
            .collect(),
    }
}

/// Sort rows by column `col` (an ORIGINAL column index). Numeric when both
/// cells parse as f64, else case-insensitive text. Nulls always sort last,
/// regardless of direction. `col < 0` or out of range leaves the grid as-is.
fn sort_grid(g: &model::GridModel, col: i32, asc: bool) -> model::GridModel {
    if col < 0 || col as usize >= g.columns.len() {
        return g.clone();
    }
    let c = col as usize;
    let mut rows = g.rows.clone();
    rows.sort_by(|a, b| {
        use std::cmp::Ordering;
        let (x, y) = (&a[c], &b[c]);
        match (x.is_null, y.is_null) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            _ => {}
        }
        let ord = match (x.text.parse::<f64>(), y.text.parse::<f64>()) {
            (Ok(m), Ok(n)) => m.partial_cmp(&n).unwrap_or(Ordering::Equal),
            _ => x.text.to_lowercase().cmp(&y.text.to_lowercase()),
        };
        if asc {
            ord
        } else {
            ord.reverse()
        }
    });
    model::GridModel {
        columns: g.columns.clone(),
        rows,
    }
}

/// Project `g`'s columns into display order `order` (ORIGINAL column indices),
/// dropping any index in `hidden`. Both columns and each row are projected the
/// same way, so this subsumes hide + reorder in one pass.
fn project_cols(
    g: &model::GridModel,
    order: &[usize],
    hidden: &HashSet<usize>,
) -> model::GridModel {
    let idx: Vec<usize> = order
        .iter()
        .copied()
        .filter(|i| *i < g.columns.len() && !hidden.contains(i))
        .collect();
    model::GridModel {
        columns: idx.iter().map(|&i| g.columns[i].clone()).collect(),
        rows: g
            .rows
            .iter()
            .map(|row| idx.iter().map(|&i| row[i].clone()).collect())
            .collect(),
    }
}

/// Build the on-screen grid from a base result grid: filter rows, sort rows,
/// then project columns (hide + reorder). Column index arguments are ORIGINAL
/// indices into `base`.
#[allow(clippy::too_many_arguments)]
/// Parse one per-column filter box into `(op, needle)`. A leading operator
/// (`>=`,`<=`,`!=`,`>`,`<`,`=`) picks the comparison; anything else is a
/// case-insensitive `contains`. Blank input means "no filter" (None).
fn parse_col_filter(raw: &str) -> Option<(&'static str, String)> {
    let t = raw.trim();
    if t.is_empty() {
        return None;
    }
    for op in [">=", "<=", "!="] {
        if let Some(rest) = t.strip_prefix(op) {
            return Some((op, rest.trim().to_lowercase()));
        }
    }
    for op in [">", "<", "="] {
        if let Some(rest) = t.strip_prefix(op) {
            return Some((op, rest.trim().to_lowercase()));
        }
    }
    Some(("contains", t.to_lowercase()))
}

/// Drop rows failing any non-empty per-column filter (`col_filters` indexed by
/// ORIGINAL column). Conditions combine with AND.
fn apply_col_filters(g: &model::GridModel, col_filters: &[String]) -> model::GridModel {
    let parsed: Vec<(usize, &'static str, String)> = col_filters
        .iter()
        .enumerate()
        .filter_map(|(ci, raw)| parse_col_filter(raw).map(|(op, n)| (ci, op, n)))
        .collect();
    if parsed.is_empty() {
        return g.clone();
    }
    model::GridModel {
        columns: g.columns.clone(),
        rows: g
            .rows
            .iter()
            .filter(|row| {
                parsed
                    .iter()
                    .all(|(ci, op, n)| row.get(*ci).is_some_and(|cell| cell_matches(cell, op, n)))
            })
            .cloned()
            .collect(),
    }
}

/// Per-column filter box values in DISPLAY order (visible columns only), for
/// feeding the grid's filter-row inputs.
fn display_col_filters(
    col_filters: &[String],
    order: &[usize],
    hidden: &HashSet<usize>,
) -> Vec<SharedString> {
    order
        .iter()
        .copied()
        .filter(|i| !hidden.contains(i))
        .map(|i| SharedString::from(col_filters.get(i).cloned().unwrap_or_default()))
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn build_grid(
    base: &model::GridModel,
    needle: &str,
    fcol: &str,
    fop: &str,
    col_filters: &[String],
    hidden: &HashSet<usize>,
    order: &[usize],
    sort_col: i32,
    sort_asc: bool,
) -> model::GridModel {
    let col = if fcol == "any column" {
        None
    } else {
        base.columns.iter().position(|c| c.name == fcol)
    };
    let filtered = filter_grid_cond(base, needle, col, fop);
    let per_col = apply_col_filters(&filtered, col_filters);
    let sorted = sort_grid(&per_col, sort_col, sort_asc);
    project_cols(&sorted, order, hidden)
}

/// Derive the displayed `ResultView` from a cached base view by applying the
/// active filter, sort, hidden-column set, and column order. Single source of
/// truth for the filter / sort / hide / reorder handlers. `Affected` has no
/// grid, so it passes through unchanged.
#[allow(clippy::too_many_arguments)]
fn compute_view(
    base: &model::ResultView,
    needle: &str,
    fcol: &str,
    fop: &str,
    col_filters: &[String],
    hidden: &HashSet<usize>,
    order: &[usize],
    sort_col: i32,
    sort_asc: bool,
) -> model::ResultView {
    let g = |grid| {
        build_grid(
            grid,
            needle,
            fcol,
            fop,
            col_filters,
            hidden,
            order,
            sort_col,
            sort_asc,
        )
    };
    match base {
        model::ResultView::Table(grid) => model::ResultView::Table(g(grid)),
        model::ResultView::Documents(d) => model::ResultView::Documents(model::DocModel {
            json: d.json.clone(),
            grid: g(&d.grid),
            tree: d.tree.clone(),
        }),
        model::ResultView::Affected(a) => model::ResultView::Affected(a.clone()),
    }
}

thread_local! {
    /// Full JSON-tree nodes + collapsed paths for the currently displayed Mongo
    /// document result. The Slint event loop is single-threaded, so a
    /// thread_local suffices; no cross-tab persistence is needed.
    static DOC_TREES: std::cell::RefCell<[(Vec<model::DocNode>, HashSet<String>); 2]> =
        std::cell::RefCell::new(Default::default());
}

/// Compute the visible JSON-tree rows for the current collapse state and push
/// them to the window.
fn push_doc_tree(
    w: &MainWindow,
    pane: usize,
    full: &[model::DocNode],
    collapsed: &HashSet<String>,
) {
    let rows: Vec<DocRow> = model::visible_doc_rows(full, collapsed)
        .into_iter()
        .map(|(n, expanded)| DocRow {
            depth: n.depth as i32,
            key: SharedString::from(n.key.clone()),
            preview: SharedString::from(n.preview.clone()),
            expandable: n.expandable,
            expanded,
            path: SharedString::from(n.path.clone()),
        })
        .collect();
    set_p_doc_tree(w, pane, ModelRc::from(Rc::new(VecModel::from(rows))));
}

/// Push a `ResultView` into the window, selecting the per-kind result region
/// via `result-kind` (0 Table, 1 Documents, 3 Affected).
fn apply_result(w: &MainWindow, pane: usize, view: model::ResultView) {
    match view {
        model::ResultView::Table(g) => {
            set_p_result_kind(w, pane, 0);
            w.set_doc_json(SharedString::default());
            push_grid(w, pane, &g);
            set_p_result_status(
                w,
                pane,
                SharedString::from(format!("{} rows", g.rows.len())),
            );
        }
        model::ResultView::Documents(d) => {
            set_p_result_kind(w, pane, 1);
            w.set_doc_json(SharedString::from(d.json));
            push_grid(w, pane, &d.grid);
            set_p_result_status(
                w,
                pane,
                SharedString::from(format!("{} documents", d.grid.rows.len())),
            );
            let collapsed = model::default_doc_collapsed(&d.tree);
            push_doc_tree(w, pane, &d.tree, &collapsed);
            DOC_TREES.with(|s| s.borrow_mut()[pane] = (d.tree, collapsed));
        }
        model::ResultView::Affected(status) => {
            set_p_result_kind(w, pane, 3);
            w.set_doc_json(SharedString::default());
            clear_grid(w, pane);
            set_p_result_status(w, pane, SharedString::from(status));
        }
    }
}

/// Present a fresh result view: reset all client-side view state (filter, sort,
/// hidden, order, per-column filters, pending edits), rebuild the column
/// metadata + widths + chart, and push it to the grid. Shared by a new query
/// run and by switching between result tabs.
#[allow(clippy::too_many_arguments)]
fn present_view(
    w: &MainWindow,
    pane: usize,
    v: model::ResultView,
    meta: &str,
    latency: &str,
    last_view: &Arc<std::sync::Mutex<Option<model::ResultView>>>,
    displayed_grid: &Arc<std::sync::Mutex<Option<model::GridModel>>>,
    hidden_cols: &Arc<std::sync::Mutex<HashSet<usize>>>,
    sort_state: &Arc<std::sync::Mutex<(i32, bool)>>,
    col_order: &Arc<std::sync::Mutex<Vec<usize>>>,
    col_filters: &Arc<std::sync::Mutex<Vec<String>>>,
    edit_buf: &Arc<std::sync::Mutex<model::EditBuffer>>,
    browse: &Arc<std::sync::Mutex<BrowseState>>,
) {
    let ncols = match &v {
        model::ResultView::Table(g) => g.columns.len(),
        model::ResultView::Documents(d) => d.grid.columns.len(),
        _ => 0,
    };
    let shown = match &v {
        model::ResultView::Table(g) => g.rows.len(),
        model::ResultView::Documents(d) => d.grid.rows.len(),
        model::ResultView::Affected(_) => 0,
    } as u64;
    *last_view.lock().unwrap() = Some(v.clone());
    w.set_grid_filter(SharedString::default());
    hidden_cols.lock().unwrap().clear();
    *col_order.lock().unwrap() = (0..ncols).collect();
    *sort_state.lock().unwrap() = (-1, true);
    *col_filters.lock().unwrap() = vec![String::new(); ncols];
    set_p_grid_sort(w, pane, -1, true);
    set_p_grid_col_filters(
        w,
        pane,
        ModelRc::from(Rc::new(VecModel::from(vec![
            SharedString::default();
            ncols
        ]))),
    );
    let colnames: Vec<SharedString> = match &v {
        model::ResultView::Table(g) => g
            .columns
            .iter()
            .map(|c| SharedString::from(c.name.clone()))
            .collect(),
        model::ResultView::Documents(d) => d
            .grid
            .columns
            .iter()
            .map(|c| SharedString::from(c.name.clone()))
            .collect(),
        _ => Vec::new(),
    };
    w.set_col_hidden(ModelRc::from(Rc::new(VecModel::from(vec![
        false;
        colnames.len()
    ]))));
    let mut fcols: Vec<SharedString> = vec![SharedString::from("any column")];
    fcols.extend(colnames.iter().cloned());
    w.set_filter_columns(ModelRc::from(Rc::new(VecModel::from(fcols))));
    w.set_filter_col(
        colnames
            .first()
            .cloned()
            .unwrap_or_else(|| SharedString::from("any column")),
    );
    w.set_all_columns(ModelRc::from(Rc::new(VecModel::from(colnames))));
    *displayed_grid.lock().unwrap() = view_grid(&v);
    {
        let st = browse.lock().unwrap();
        let mut b = edit_buf.lock().unwrap();
        b.clear();
        b.table = st.table.clone();
        b.pk_cols = st.pk_cols.clone();
    }
    w.set_pending_count(0);
    set_p_editing(w, pane, -1, -1);
    set_p_status_error(w, pane, false);
    w.set_status_latency(SharedString::from(latency));
    set_p_results_meta(w, pane, SharedString::from(meta));
    let widths: Vec<f32> = match &v {
        model::ResultView::Table(g) => g
            .columns
            .iter()
            .map(|c| default_col_width(&c.name, &c.type_name))
            .collect(),
        _ => vec![140.0; ncols],
    };
    set_p_col_widths(w, pane, ModelRc::from(Rc::new(VecModel::from(widths))));
    let bars: Vec<ChartBar> = match &v {
        model::ResultView::Table(g) => model::chart_data(g)
            .into_iter()
            .map(|(label, value, frac)| ChartBar {
                label: label.into(),
                value: value.into(),
                frac,
            })
            .collect(),
        _ => Vec::new(),
    };
    set_p_chart_bars(w, pane, ModelRc::from(Rc::new(VecModel::from(bars))));
    apply_result(w, pane, v);
    let st = browse.lock().unwrap().clone();
    if st.table.is_some() {
        let (start, end, prev, next) = page_bounds(st.page, st.limit, st.total, shown);
        w.set_page_start(start as i32);
        w.set_page_end(end as i32);
        w.set_total_rows(st.total.map(|t| t as i32).unwrap_or(-1));
        w.set_can_prev(prev);
        w.set_can_next(next);
    }
}

/// Set the result-tab strip labels ("Result 1", …) and active index.
fn set_result_tabs(w: &MainWindow, pane: usize, count: usize, active: usize) {
    let labels: Vec<SharedString> = (1..=count)
        .map(|n| SharedString::from(format!("Result {n}")))
        .collect();
    let labels = ModelRc::from(Rc::new(VecModel::from(labels)));
    if pane == 0 {
        w.set_result_tabs(labels);
        w.set_active_result(active as i32);
    } else {
        w.set_p1_result_tabs(labels);
        w.set_p1_active_result(active as i32);
    }
}

#[cfg(test)]
mod fmt_tests {
    use super::{
        cell_matches, filter_operators, parse_col_filter, replaceable_table_tab_index,
        table_tab_id, workspace_tab_index, WorkspaceTab,
    };
    #[test]
    fn col_filter_prefix_ops() {
        assert_eq!(parse_col_filter(""), None);
        assert_eq!(parse_col_filter("   "), None);
        assert_eq!(parse_col_filter(">100"), Some((">", "100".to_string())));
        assert_eq!(parse_col_filter(">= 5"), Some((">=", "5".to_string())));
        assert_eq!(parse_col_filter("!=X"), Some(("!=", "x".to_string())));
        assert_eq!(
            parse_col_filter("abc"),
            Some(("contains", "abc".to_string()))
        );
    }
    #[test]
    fn ilike_is_postgres_only() {
        use rdb_connstore::Engine;
        assert!(filter_operators(Engine::Postgres)
            .iter()
            .any(|op| op == "ILIKE"));
        assert!(!filter_operators(Engine::MySql)
            .iter()
            .any(|op| op == "ILIKE"));
    }

    #[test]
    fn table_filter_operators_match_values() {
        let cell = crate::model::VmCell {
            text: "Mitra Investindo".into(),
            is_null: false,
        };
        assert!(cell_matches(&cell, "ILIKE", "mitra"));
        assert!(cell_matches(&cell, "IN", "bank, mitra investindo"));
        assert!(cell_matches(&cell, "<>", "other"));
        assert!(!cell_matches(&cell, "=", "other"));
    }

    #[test]
    fn table_tab_identity_includes_connection_database_schema_and_table() {
        assert_eq!(
            table_tab_id("conn-1", "app", "public", "users"),
            "table:conn-1:app:public:users"
        );
        assert_ne!(
            table_tab_id("conn-1", "app", "public", "users"),
            table_tab_id("conn-2", "app", "public", "users")
        );
    }

    #[test]
    fn empty_workspace_has_no_active_tab() {
        assert_eq!(workspace_tab_index(&[], None), None);
        let tabs = vec![WorkspaceTab::sql("query:c:1".into(), 1)];
        assert_eq!(workspace_tab_index(&tabs, None), None);
        assert_eq!(workspace_tab_index(&tabs, Some("query:c:1")), Some(0));
        assert_eq!(workspace_tab_index(&tabs, Some("missing")), None);
    }

    #[test]
    fn only_the_active_unpinned_table_tab_is_replaceable() {
        let mut tab = WorkspaceTab::sql("table:c:db:public:users".into(), 1);
        tab.kind = "table".into();
        tab.pinned = false;
        let mut tabs = vec![tab];

        assert_eq!(
            replaceable_table_tab_index(&tabs, Some(&tabs[0].id)),
            Some(0)
        );
        tabs[0].pinned = true;
        assert_eq!(replaceable_table_tab_index(&tabs, Some(&tabs[0].id)), None);
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
    // RDB_MOCK=1 swaps in a seeded temp store (never the user's real one).
    let store: Rc<RefCell<rdb_connstore::ConnStore>> = Rc::new(RefCell::new(
        if let Ok(dir) = std::env::var("RDB_STORE_DIR") {
            // Explicit store dir (e2e harness): file-backed, real drivers.
            let dir = std::path::PathBuf::from(dir);
            let backend = Box::new(
                rdb_connstore::EncryptedFileBackend::new(&dir).expect("file secret backend"),
            );
            rdb_connstore::ConnStore::load(dir.join("connections.json"), backend)
                .expect("load RDB_STORE_DIR store")
        } else if mock::mock_mode() {
            mock::mock_store(std::env::temp_dir().join(format!("rdb-mock-{}", std::process::id())))
        } else {
            rdb_connstore::ConnStore::open_default().unwrap_or_else(|_| {
                let dir = std::env::temp_dir().join("dbm");
                let _ = std::fs::create_dir_all(&dir);
                let backend = rdb_connstore::secret::select_backend(&dir).expect("secret backend");
                rdb_connstore::ConnStore::new(dir.join("connections.json"), backend)
            })
        },
    ));

    // App preferences (theme, update-check, UI state), persisted alongside the
    // connection store. Follows the same RDB_STORE_DIR / mock overrides so
    // tests and the reference screenshots never touch the user's real file.
    let settings: Rc<RefCell<rdb_connstore::SettingsStore>> = Rc::new(RefCell::new(
        if let Ok(dir) = std::env::var("RDB_STORE_DIR") {
            let dir = std::path::PathBuf::from(dir);
            rdb_connstore::SettingsStore::load(dir.join("settings.json"))
                .expect("load RDB_STORE_DIR settings")
        } else if mock::mock_mode() {
            let dir = std::env::temp_dir().join(format!("rdb-mock-{}", std::process::id()));
            let _ = std::fs::create_dir_all(&dir);
            rdb_connstore::SettingsStore::load(dir.join("settings.json")).expect("mock settings")
        } else {
            rdb_connstore::SettingsStore::open_default().unwrap_or_else(|_| {
                let dir = std::env::temp_dir().join("dbm");
                let _ = std::fs::create_dir_all(&dir);
                rdb_connstore::SettingsStore::load(dir.join("settings.json"))
                    .expect("settings fallback")
            })
        },
    ));

    // Apply the saved theme before the first paint, and seed the settings modal
    // toggles from the persisted values.
    window
        .global::<Theme>()
        .set_dark(settings.borrow().get().theme.is_dark());
    window.set_update_check_enabled(settings.borrow().get().update_check);
    window.set_sidebar_right(settings.borrow().get().ui_state.sidebar_right);
    window.set_app_version(env!("CARGO_PKG_VERSION").into());

    // Fixed window size for the screenshot loop: RDB_WIN=WxH (logical px).
    if let Ok(spec) = std::env::var("RDB_WIN") {
        if let Some((w, h)) = spec.split_once('x') {
            if let (Ok(w), Ok(h)) = (w.parse::<f32>(), h.parse::<f32>()) {
                window.window().set_size(slint::LogicalSize::new(w, h));
            }
        }
    }

    // (engine, driver) so run-query can parse text for the right paradigm.
    let current: Arc<tokio::sync::Mutex<Option<(rdb_connstore::Engine, AnyDriver)>>> =
        Arc::new(tokio::sync::Mutex::new(None));

    // Set of group labels the user has collapsed in the sidebar.
    let collapsed: Rc<RefCell<HashSet<String>>> = Rc::new(RefCell::new(HashSet::new()));
    if mock::mock_mode() {
        // Reference boots with only PROFIN expanded.
        let mut c = collapsed.borrow_mut();
        for g in ["OSS", "LOCAL", "SPMB", UNGROUPED] {
            c.insert(g.to_string());
        }
    } else {
        // Restore the groups the user had collapsed last session.
        let mut c = collapsed.borrow_mut();
        for g in &settings.borrow().get().ui_state.collapsed_groups {
            c.insert(g.clone());
        }
    }
    // Current connection-picker search text.
    // ponytail: filter text is session-only; restoring it would need the picker
    // FilterField to expose a settable `text` (it is write-only today), and a
    // pre-filled box on launch is dubious UX. AppSettings keeps the field for
    // when that plumbing is worth adding.
    let conn_filter: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));

    // Schema sidebar state: raw flat nodes from the last connect (Send, so the
    // connect task can fill it) + the set of expanded table labels.
    let raw_nodes: Arc<std::sync::Mutex<Vec<model::VmTreeNode>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    // Cross-schema completion tree: every schema (labelled by schema name) with
    // its tables/columns, so `schema.table` autocompletes across namespaces.
    // Separate from `raw_nodes` (which stays scoped to the sidebar's one schema).
    let completion_nodes: Arc<std::sync::Mutex<Vec<model::VmTreeNode>>> =
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
    let collapsed_categories: Rc<RefCell<HashSet<String>>> =
        Rc::new(RefCell::new(default_collapsed_cats()));
    // Sidebar tree filter text. Arc<Mutex> so lazy-load tasks can read it.
    let sidebar_filter: Arc<std::sync::Mutex<String>> =
        Arc::new(std::sync::Mutex::new(String::new()));
    // Up to two independent editor+result panes per SQL tab (dual-pane split).
    // Only pane 0 is wired for now; the alias bindings below bind the existing
    // single-pane code to pane 0 so behavior is unchanged.
    let panes: Rc<[PaneRuntime; 2]> = Rc::new([PaneRuntime::new(), PaneRuntime::new()]);
    // Browse-mode pagination state (open container + page window + pk).
    let browse = panes[0].browse.clone();
    // Buffered, uncommitted grid edits (⌘S commits, Esc/Discard drops).
    let edit_buf = panes[0].edit_buf.clone();
    // The grid currently on screen (post client-side filter): edit-buffer row
    // indices refer to THIS grid, and its cells carry the pre-edit pk values.
    let displayed_grid = panes[0].displayed_grid.clone();
    // Column indices (into the last result) hidden via the Columns popup.
    let hidden_cols = panes[0].hidden_cols.clone();
    // Client-side sort: ORIGINAL column index (-1 = none) + ascending flag.
    let sort_state = panes[0].sort_state.clone();
    // Display order of columns as ORIGINAL indices (drag-to-reorder). Reset to
    // 0..ncols on every fresh result.
    let col_order = panes[0].col_order.clone();
    // Per-column filter boxes, raw text indexed by ORIGINAL column. Empty =
    // no filter. Reset to blanks on every fresh result.
    let col_filters = panes[0].col_filters.clone();
    // Result tabs for the SQL editor: cached results + the active index. ⌘\
    // sets `result_new_tab` so the next run appends instead of replacing.
    let results = panes[0].results.clone();
    let active_result = panes[0].active_result.clone();
    // One Rust source of truth for document identity and state. The UI index is
    // derived from `active_tab_id`; it is -1 while this Option is None.
    let workspace_tabs: Arc<std::sync::Mutex<Vec<WorkspaceTab>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    let active_tab_id: Arc<std::sync::Mutex<Option<String>>> =
        Arc::new(std::sync::Mutex::new(None));
    let active_p1_tab_id: Arc<std::sync::Mutex<Option<String>>> =
        Arc::new(std::sync::Mutex::new(None));
    let current_connection_id: Arc<std::sync::Mutex<Option<String>>> =
        Arc::new(std::sync::Mutex::new(None));
    // One-shot database override for the next connect: the database switcher sets
    // it, then re-invokes the connect path, which consumes it (take) so a plain
    // reconnect from the picker still uses the connection's saved database.
    let db_override: Arc<std::sync::Mutex<Option<String>>> = Arc::new(std::sync::Mutex::new(None));
    // Restore persisted SQL tabs on the first connect of the session; later
    // connects to a different connection start fresh.
    let tabs_restored: Rc<std::cell::Cell<bool>> = Rc::new(std::cell::Cell::new(false));
    let query_number = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let query_console: Arc<std::sync::Mutex<Vec<String>>> =
        Arc::new(std::sync::Mutex::new(Vec::new()));
    set_workspace_tabs(&window, &[], None);
    sync_query_console(&window, &query_console);
    {
        let weak = window.as_weak();
        let query_console = query_console.clone();
        window.on_clear_console(move || {
            query_console.lock().unwrap().clear();
            if let Some(w) = weak.upgrade() {
                sync_query_console(&w, &query_console);
            }
        });
    }
    // Engine of the live connection, kept on the UI thread so table clicks can
    // build an engine-appropriate "browse this table" query synchronously.
    let cur_engine: Rc<RefCell<Option<rdb_connstore::Engine>>> = Rc::new(RefCell::new(None));

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

    // ----- SQL editor: Rust-owned buffer + lexer feeding the span model -----
    let ed_state = panes[0].ed_state.clone();
    // Statement head lines the user has folded closed live per-pane in
    // `panes[*].folded_heads`; sync_editor + the fold toggle read them by pane.
    let sync_editor = {
        let weak = window.as_weak();
        let panes = panes.clone();
        move |pane: usize| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let ed_state = panes[pane].ed_state.clone();
            let folded_heads = panes[pane].folded_heads.clone();
            let ed = ed_state.borrow();
            let sel = ed.selection();
            let lines: Vec<ModelRc<Span>> = ed
                .lines
                .iter()
                .enumerate()
                .map(|(li, l)| {
                    let mut spans = editor::lex_line(l);
                    // selection highlight: char-col range covered on this line
                    if let Some(((sl, sc), (el, ec))) = sel {
                        if li >= sl && li <= el {
                            let a = if li == sl { sc } else { 0 };
                            let b = if li == el { ec } else { l.chars().count() };
                            spans = editor::overlay_selection(spans, a, b);
                        }
                    }
                    let spans: Vec<Span> = spans
                        .into_iter()
                        .map(|sp| Span {
                            text: sp.text.into(),
                            kind: sp.kind,
                            sel: sp.sel,
                        })
                        .collect();
                    ModelRc::from(Rc::new(VecModel::from(spans)))
                })
                .collect();
            let lines = ModelRc::from(Rc::new(VecModel::from(lines)));
            // Fold arrows: 1 = open head, 2 = closed head, 0 = plain line.
            // `hidden` blanks out the body lines of a closed statement.
            let n = ed.lines.len();
            let mut hidden = vec![false; n];
            let mut fold_state = vec![0i32; n];
            let folded = folded_heads.borrow();
            for (h, e) in editor::statement_line_spans(&ed.lines) {
                if e > h {
                    let closed = folded.contains(&h);
                    fold_state[h] = if closed { 2 } else { 1 };
                    if closed {
                        for hl in hidden.iter_mut().take(e + 1).skip(h + 1) {
                            *hl = true;
                        }
                    }
                }
            }
            let hidden = ModelRc::from(Rc::new(VecModel::from(hidden)));
            let fold_state = ModelRc::from(Rc::new(VecModel::from(fold_state)));
            if pane == 0 {
                w.set_editor_lines(lines);
                w.set_editor_line_hidden(hidden);
                w.set_editor_fold_state(fold_state);
                w.set_cursor_line(ed.line as i32);
                w.set_cursor_col(ed.col as i32);
                // query-text mirrors the focused editor for tab persistence; the
                // right pane's text lives in panes[1].ed_state (persisted via
                // p1-query in a later step).
                w.set_query_text(SharedString::from(ed.text()));
            } else {
                w.set_p1_editor_lines(lines);
                w.set_p1_editor_line_hidden(hidden);
                w.set_p1_editor_fold_state(fold_state);
                w.set_p1_cursor_line(ed.line as i32);
                w.set_p1_cursor_col(ed.col as i32);
            }
        }
    };
    #[allow(clippy::type_complexity)]
    let load_editor_text: Rc<dyn Fn(usize, &str)> = {
        let panes = panes.clone();
        let sync_editor = sync_editor.clone();
        Rc::new(move |pane: usize, text: &str| {
            *panes[pane].ed_state.borrow_mut() = editor::EditorState::from_text(text);
            sync_editor(pane);
        })
    };
    load_editor_text(0, "");

    // ----- toggle a statement fold from the gutter arrow -----
    {
        let panes = panes.clone();
        let sync_editor = sync_editor.clone();
        let fold: Rc<dyn Fn(usize, i32)> = Rc::new(move |pane: usize, line: i32| {
            let ed_state = panes[pane].ed_state.clone();
            let folded_heads = panes[pane].folded_heads.clone();
            let head = line.max(0) as usize;
            let now_closed = {
                let mut f = folded_heads.borrow_mut();
                if f.remove(&head) {
                    false
                } else {
                    f.insert(head);
                    true
                }
            };
            // Folding a block that contains the caret would strand it on a
            // hidden line; pull the caret up to the visible head.
            if now_closed {
                let mut ed = ed_state.borrow_mut();
                if let Some((_, e)) = editor::statement_line_spans(&ed.lines)
                    .into_iter()
                    .find(|(h, _)| *h == head)
                {
                    if ed.line > head && ed.line <= e {
                        ed.move_to(head as i32, 0, false);
                    }
                }
            }
            sync_editor(pane);
        });
        window.on_toggle_fold({
            let fold = fold.clone();
            move |line| fold(0, line)
        });
        window.on_p1_toggle_fold({
            let fold = fold.clone();
            move |line| fold(1, line)
        });
    }

    // ----- SQL autocomplete: recompute popup, accept a choice -----
    // completion_ctx = (word char length to replace, candidate labels).
    let refresh_completion: Rc<dyn Fn(usize)> = {
        let weak = window.as_weak();
        let panes = panes.clone();
        let completion_nodes = completion_nodes.clone();
        Rc::new(move |pane| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let before = panes[pane].ed_state.borrow().before_cursor_doc();
            let schema = w.get_schema_name().to_string();
            let (word_len, cands) =
                completion::suggest(&before, &completion_nodes.lock().unwrap(), &schema);
            if cands.is_empty() {
                set_p_completion_visible(&w, pane, false);
                *panes[pane].completion_ctx.borrow_mut() = (0, Vec::new());
                return;
            }
            let items: Vec<PaletteItem> = cands
                .iter()
                .map(|c| PaletteItem {
                    label: c.label.clone().into(),
                    kind: c.kind.clone().into(),
                    sub: c.sub.clone().into(),
                    local: false,
                })
                .collect();
            *panes[pane].completion_ctx.borrow_mut() =
                (word_len, cands.iter().map(|c| c.label.clone()).collect());
            set_p_completion_items(&w, pane, ModelRc::from(Rc::new(VecModel::from(items))));
            set_p_completion_selected(&w, pane, 0);
            set_p_completion_visible(&w, pane, true);
        })
    };
    let accept_completion: Rc<dyn Fn(usize, i32)> = {
        let weak = window.as_weak();
        let panes = panes.clone();
        let sync_editor = sync_editor.clone();
        Rc::new(move |pane, idx| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let (word_len, labels) = panes[pane].completion_ctx.borrow().clone();
            let Some(label) = labels.get(idx.max(0) as usize).cloned() else {
                return;
            };
            {
                let mut ed = panes[pane].ed_state.borrow_mut();
                for _ in 0..word_len {
                    ed.backspace();
                }
                ed.insert(&label);
            }
            set_p_completion_visible(&w, pane, false);
            *panes[pane].completion_ctx.borrow_mut() = (0, Vec::new());
            sync_editor(pane);
        })
    };
    {
        let accept = accept_completion.clone();
        window.on_completion_choose(move |i| accept(0, i));
    }
    {
        let accept = accept_completion.clone();
        window.on_p1_completion_choose(move |i| accept(1, i));
    }
    {
        let panes = panes.clone();
        let sync_editor = sync_editor.clone();
        let weak = window.as_weak();
        let refresh_completion = refresh_completion.clone();
        let accept_completion = accept_completion.clone();
        let cur_engine = cur_engine.clone();
        // Shared editor-key handler for both split panes; `pane` selects which
        // editor buffer and completion popup it drives.
        #[allow(clippy::type_complexity)]
        let edit_key: Rc<dyn Fn(usize, SharedString, bool, bool, bool) -> bool> = Rc::new(
            move |pane: usize, text: SharedString, meta: bool, alt: bool, shift: bool| {
                let ed_state = panes[pane].ed_state.clone();
                // Typing in a pane focuses it, so global shortcuts (⌘F/⌘⏎) land
                // on the pane the caret is really in.
                if let Some(w) = weak.upgrade() {
                    if w.get_active_pane() != pane as i32 {
                        w.set_active_pane(pane as i32);
                    }
                }
                // While the autocomplete popup is open it owns nav / accept / close.
                if let Some(w) = weak.upgrade() {
                    if get_p_completion_visible(&w, pane) {
                        let n = p_completion_count(&w, pane);
                        match text.as_str() {
                            "\u{f700}" if n > 0 => {
                                set_p_completion_selected(
                                    &w,
                                    pane,
                                    (get_p_completion_selected(&w, pane) - 1 + n) % n,
                                );
                                return true;
                            }
                            "\u{f701}" if n > 0 => {
                                set_p_completion_selected(
                                    &w,
                                    pane,
                                    (get_p_completion_selected(&w, pane) + 1) % n,
                                );
                                return true;
                            }
                            "\t" | "\n" | "\r" => {
                                accept_completion(pane, get_p_completion_selected(&w, pane));
                                return true;
                            }
                            "\u{1b}" => {
                                set_p_completion_visible(&w, pane, false);
                                return true;
                            }
                            _ => {}
                        }
                    }
                }
                // Cursor motion first: arrows / home / end, with macOS ⌘ (line &
                // document) and ⌥ (word) semantics. shift extends the selection.
                if matches!(
                    text.as_str(),
                    "\u{f700}" | "\u{f701}" | "\u{f702}" | "\u{f703}" | "\u{f729}" | "\u{f72b}"
                ) {
                    {
                        let mut ed = ed_state.borrow_mut();
                        ed.set_selecting(shift);
                        match text.as_str() {
                            "\u{f702}" => {
                                if alt {
                                    ed.move_word(-1)
                                } else if meta {
                                    ed.home()
                                } else {
                                    ed.move_cursor(0, -1)
                                }
                            }
                            "\u{f703}" => {
                                if alt {
                                    ed.move_word(1)
                                } else if meta {
                                    ed.end()
                                } else {
                                    ed.move_cursor(0, 1)
                                }
                            }
                            "\u{f700}" => {
                                if meta {
                                    ed.move_doc_start()
                                } else {
                                    ed.move_cursor(-1, 0)
                                }
                            }
                            "\u{f701}" => {
                                if meta {
                                    ed.move_doc_end()
                                } else {
                                    ed.move_cursor(1, 0)
                                }
                            }
                            "\u{f729}" => ed.home(),
                            "\u{f72b}" => ed.end(),
                            _ => {}
                        }
                    }
                    sync_editor(pane);
                    return true;
                }
                if meta {
                    // ⌘⏎ runs the pane the caret is in. The global window shortcut
                    // always targets pane 0, so intercept it here (outside the ed
                    // borrow) and dispatch to the firing pane instead.
                    if text.as_str() == "\r" || text.as_str() == "\n" {
                        if let Some(w) = weak.upgrade() {
                            if pane == 0 {
                                w.invoke_run_query();
                            } else {
                                w.invoke_p1_run();
                            }
                        }
                        return true;
                    }
                    // Editor-owned cmd combos; everything else bubbles up to the
                    // window shortcut scope (⌘S commit, ⌘R refresh, …).
                    let handled = {
                        let mut ed = ed_state.borrow_mut();
                        match text.as_str() {
                            "a" => {
                                ed.select_all();
                                true
                            }
                            "c" => {
                                // no selection → copy the statement under the cursor
                                let t =
                                    ed.selected_text().unwrap_or_else(|| ed.current_statement());
                                clip_set(&t);
                                true
                            }
                            "x" => {
                                if let Some(t) = ed.selected_text() {
                                    clip_set(&t);
                                    ed.cut_selection();
                                }
                                true
                            }
                            "v" => {
                                if let Some(t) = clip_get() {
                                    ed.insert(&t.replace("\r\n", "\n"));
                                }
                                true
                            }
                            "z" if shift => {
                                ed.redo();
                                true
                            }
                            "z" => {
                                ed.undo();
                                true
                            }
                            "/" => {
                                // Comment marker follows the connected engine's query
                                // language; default to SQL when not yet connected.
                                let engine = cur_engine
                                    .borrow()
                                    .unwrap_or(rdb_connstore::Engine::Postgres);
                                ed.toggle_comment(crate::query_parse::comment_prefix(engine));
                                true
                            }
                            _ => false,
                        }
                    };
                    if handled {
                        sync_editor(pane);
                    }
                    return handled;
                }
                let handled = {
                    let mut ed = ed_state.borrow_mut();
                    // movement keys: shift extends the selection, plain drops it
                    if matches!(
                        text.as_str(),
                        "\u{f700}" | "\u{f701}" | "\u{f702}" | "\u{f703}" | "\u{f729}" | "\u{f72b}"
                    ) {
                        ed.set_selecting(shift);
                    }
                    let mut it = text.chars();
                    match (it.next(), it.next()) {
                        (Some(c), None) => match c {
                            '\u{8}' => {
                                ed.backspace();
                                true
                            }
                            '\u{7f}' => {
                                ed.delete();
                                true
                            }
                            '\n' | '\r' => {
                                ed.newline();
                                true
                            }
                            '\t' => {
                                ed.insert("  ");
                                true
                            }
                            // Esc clears the selection; without one it bubbles
                            // up (modal close).
                            '\u{1b}' if ed.selection().is_some() => {
                                ed.set_selecting(false);
                                true
                            }
                            '\u{f700}' => {
                                ed.move_cursor(-1, 0);
                                true
                            }
                            '\u{f701}' => {
                                ed.move_cursor(1, 0);
                                true
                            }
                            '\u{f702}' => {
                                ed.move_cursor(0, -1);
                                true
                            }
                            '\u{f703}' => {
                                ed.move_cursor(0, 1);
                                true
                            }
                            '\u{f729}' => {
                                ed.home();
                                true
                            }
                            '\u{f72b}' => {
                                ed.end();
                                true
                            }
                            c if !c.is_control() => {
                                ed.insert(&c.to_string());
                                true
                            }
                            _ => false,
                        },
                        (Some(_), Some(_)) if !text.starts_with('\u{f700}') => {
                            ed.insert(&text);
                            true
                        }
                        _ => false,
                    }
                };
                if handled {
                    sync_editor(pane);
                    // Typing/deleting shifts this pane's completion context.
                    refresh_completion(pane);
                }
                handled
            },
        );
        window.on_editor_key({
            let edit_key = edit_key.clone();
            move |text, meta, alt, shift| edit_key(0, text, meta, alt, shift)
        });
        window.on_p1_editor_key({
            let edit_key = edit_key.clone();
            move |text, meta, alt, shift| edit_key(1, text, meta, alt, shift)
        });
    }
    {
        let panes = panes.clone();
        let sync_editor = sync_editor.clone();
        let weak = window.as_weak();
        let press: Rc<dyn Fn(usize, i32, i32)> = Rc::new(move |pane, line, col| {
            if let Some(w) = weak.upgrade() {
                // Clicking a pane focuses it: drives the accent + which pane
                // global shortcuts fall back to.
                w.set_active_pane(pane as i32);
                set_p_completion_visible(&w, pane, false);
            }
            panes[pane].ed_state.borrow_mut().move_to(line, col, false);
            sync_editor(pane);
        });
        window.on_editor_press({
            let press = press.clone();
            move |line, col| press(0, line, col)
        });
        window.on_p1_editor_press({
            let press = press.clone();
            move |line, col| press(1, line, col)
        });
    }
    {
        let panes = panes.clone();
        let sync_editor = sync_editor.clone();
        let drag: Rc<dyn Fn(usize, i32, i32)> = Rc::new(move |pane, line, col| {
            panes[pane].ed_state.borrow_mut().move_to(line, col, true);
            sync_editor(pane);
        });
        window.on_editor_drag({
            let drag = drag.clone();
            move |line, col| drag(0, line, col)
        });
        window.on_p1_editor_drag({
            let drag = drag.clone();
            move |line, col| drag(1, line, col)
        });
    }
    {
        let panes = panes.clone();
        let sync_editor = sync_editor.clone();
        let select_word: Rc<dyn Fn(usize, i32, i32)> = Rc::new(move |pane, line, col| {
            panes[pane].ed_state.borrow_mut().select_word_at(line, col);
            sync_editor(pane);
        });
        window.on_editor_select_word({
            let select_word = select_word.clone();
            move |line, col| select_word(0, line, col)
        });
        window.on_p1_editor_select_word({
            let select_word = select_word.clone();
            move |line, col| select_word(1, line, col)
        });
    }

    // ----- find-in-editor (⌘F), per pane -----
    // Select the find hit at `idx` in `pane` and refresh the "n / total" readout.
    #[allow(clippy::type_complexity)]
    let show_find: Rc<dyn Fn(&MainWindow, usize, usize)> = {
        let panes = panes.clone();
        let sync_editor = sync_editor.clone();
        Rc::new(move |w: &MainWindow, pane: usize, idx: usize| {
            let hits = panes[pane].find_hits.borrow();
            if let Some(&(l, s, e)) = hits.get(idx) {
                panes[pane]
                    .ed_state
                    .borrow_mut()
                    .set_selection((l, s), (l, e));
                sync_editor(pane);
                set_p_find_status(
                    w,
                    pane,
                    SharedString::from(format!("{} / {}", idx + 1, hits.len())),
                );
            } else if get_p_find_text(w, pane).is_empty() {
                set_p_find_status(w, pane, SharedString::default());
            } else {
                set_p_find_status(w, pane, SharedString::from("no matches"));
            }
        })
    };
    // Recompute matches for `needle`, jump to the first at/after the cursor.
    #[allow(clippy::type_complexity)]
    let recompute_find: Rc<dyn Fn(&MainWindow, usize, &str)> = {
        let panes = panes.clone();
        let show_find = show_find.clone();
        Rc::new(move |w: &MainWindow, pane: usize, needle: &str| {
            let (cur, hits) = {
                let ed = panes[pane].ed_state.borrow();
                ((ed.line, ed.col), editor::find_matches(&ed.lines, needle))
            };
            let idx = hits
                .iter()
                .position(|&(l, s, _)| (l, s) >= cur)
                .unwrap_or(0);
            *panes[pane].find_hits.borrow_mut() = hits;
            show_find(w, pane, idx);
        })
    };
    // ⌘F: open/close the find bar for a pane; seed from the selection.
    #[allow(clippy::type_complexity)]
    let toggle_find: Rc<dyn Fn(&MainWindow, usize)> = {
        let panes = panes.clone();
        let recompute_find = recompute_find.clone();
        Rc::new(move |w: &MainWindow, pane: usize| {
            let opening = !get_p_find_open(w, pane);
            set_p_find_open(w, pane, opening);
            if opening {
                if let Some(sel) = panes[pane].ed_state.borrow().selected_text() {
                    if !sel.contains('\n') && !sel.is_empty() {
                        set_p_find_text(w, pane, SharedString::from(sel));
                    }
                }
                let needle = get_p_find_text(w, pane);
                recompute_find(w, pane, &needle);
            }
        })
    };
    // Step to the next/previous hit in a pane.
    #[allow(clippy::type_complexity)]
    let find_step: Rc<dyn Fn(&MainWindow, usize, i32)> = {
        let panes = panes.clone();
        let show_find = show_find.clone();
        Rc::new(move |w: &MainWindow, pane: usize, dir: i32| {
            let n = panes[pane].find_hits.borrow().len();
            if n == 0 {
                return;
            }
            let cur = get_p_cursor(w, pane);
            let here = panes[pane]
                .find_hits
                .borrow()
                .iter()
                .position(|&(l, _, e)| (l, e) == cur)
                .unwrap_or(0);
            let i = ((here as i32 + dir).rem_euclid(n as i32)) as usize;
            show_find(w, pane, i);
        })
    };
    for pane in [0usize, 1usize] {
        let weak = window.as_weak();
        let toggle_find = toggle_find.clone();
        let recompute_find = recompute_find.clone();
        let find_step = find_step.clone();
        let toggle = move || {
            if let Some(w) = weak.upgrade() {
                toggle_find(&w, pane);
            }
        };
        let weak_c = window.as_weak();
        let changed = move |text: SharedString| {
            if let Some(w) = weak_c.upgrade() {
                recompute_find(&w, pane, &text);
            }
        };
        let weak_n = window.as_weak();
        let fs_n = find_step.clone();
        let next = move || {
            if let Some(w) = weak_n.upgrade() {
                fs_n(&w, pane, 1);
            }
        };
        let weak_p = window.as_weak();
        let prev = move || {
            if let Some(w) = weak_p.upgrade() {
                find_step(&w, pane, -1);
            }
        };
        let weak_x = window.as_weak();
        let close = move || {
            if let Some(w) = weak_x.upgrade() {
                set_p_find_open(&w, pane, false);
            }
        };
        if pane == 0 {
            window.on_toggle_find(toggle);
            window.on_find_changed(changed);
            window.on_find_next(next);
            window.on_find_prev(prev);
            window.on_find_close(close);
        } else {
            window.on_p1_toggle_find(toggle);
            window.on_p1_find_changed(changed);
            window.on_p1_find_next(next);
            window.on_p1_find_prev(prev);
            window.on_p1_find_close(close);
        }
    }

    // ----- saved/recent queries (sidebar Queries tab) -----
    let saved_queries: Rc<Vec<(&str, &str)>> = Rc::new(vec![
        (
            "emiten-per-sektor",
            "-- emiten per sektor\nSELECT s.name AS sector, count(*) AS total\nFROM emiten e\nJOIN sectors s ON s.id = e.id_sector\nWHERE e.country = 'indonesia'\nGROUP BY s.name\nORDER BY total DESC;",
        ),
        (
            "seed-sectors",
            "INSERT INTO sectors (name) VALUES\n  ('Financials'),\n  ('Energy'),\n  ('Healthcare'),\n  ('Industrials'),\n  ('Academic & Educational Services');",
        ),
        (
            "dup-email-check",
            "SELECT email, count(*)\nFROM users\nGROUP BY email\nHAVING count(*) > 1;",
        ),
        (
            "tx-volume-daily",
            "SELECT date_trunc('day', created_at) AS day, sum(amount)\nFROM transactions\nGROUP BY 1\nORDER BY 1 DESC;",
        ),
    ]);
    // Live history: filled as queries run; mock mode seeds a few for the
    // screenshot harness.
    let recent_queries: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(if mock::mock_mode() {
        vec![
            "SELECT * FROM emiten LIMIT 100;".into(),
            "INSERT INTO sectors (name) VALUES ('Technology');".into(),
            "UPDATE emiten SET updated_at = now() WHERE code = '93344';".into(),
        ]
    } else {
        load_recent()
    }));
    let rebuild_query_tree = {
        let weak = window.as_weak();
        let saved = saved_queries.clone();
        let recent = recent_queries.clone();
        move |active: &str| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            // Mode 2 (History) shows only the live history; mode 1 (Queries)
            // shows Saved + Recent.
            let history_only = w.get_sidebar_mode() == 2;
            let mut rows: Vec<TreeNode> = Vec::new();
            if !history_only {
                rows.push(TreeNode {
                    label: "Saved".into(),
                    depth: 0,
                    kind: "qcat".into(),
                    expanded: true,
                    db: SharedString::default(),
                    count: saved.len() as i32,
                });
                for (i, (name, _)) in saved.iter().enumerate() {
                    rows.push(TreeNode {
                        label: (*name).into(),
                        depth: 1,
                        kind: "query".into(),
                        expanded: *name == active,
                        db: SharedString::default(),
                        count: i as i32,
                    });
                }
            }
            let recent = recent.borrow();
            rows.push(TreeNode {
                label: if history_only { "History" } else { "Recent" }.into(),
                depth: 0,
                kind: "qcat".into(),
                expanded: true,
                db: SharedString::default(),
                count: recent.len() as i32,
            });
            for (i, q) in recent.iter().enumerate() {
                let label: String = if q.chars().count() > 24 {
                    format!("{}…", q.chars().take(23).collect::<String>())
                } else {
                    q.clone()
                };
                rows.push(TreeNode {
                    label: label.into(),
                    depth: 1,
                    kind: "recent".into(),
                    expanded: false,
                    db: SharedString::default(),
                    count: i as i32,
                });
            }
            w.set_query_tree(ModelRc::from(Rc::new(VecModel::from(rows))));
        }
    };
    rebuild_query_tree("");
    {
        let weak = window.as_weak();
        let saved = saved_queries.clone();
        let recent = recent_queries.clone();
        let load_editor_text = load_editor_text.clone();
        let rebuild_query_tree = rebuild_query_tree.clone();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        window.on_open_query(move |label, idx| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let (title, text, is_saved) =
                if let Some((name, sql)) = saved.iter().find(|(n, _)| *n == label.as_str()) {
                    ((*name).to_string(), (*sql).to_string(), true)
                } else if let Some(sql) = recent.borrow().get(idx.max(0) as usize) {
                    ("Query".to_string(), sql.clone(), false)
                } else {
                    return;
                };
            if active_tab_id.lock().unwrap().is_none() || active_tab_kind(&w) != "sql" {
                w.invoke_new_tab();
            }
            load_editor_text(0, &text);
            w.set_fn_mode(false);
            w.set_active_table(SharedString::default()); // editor mode
            w.set_query_label(SharedString::from(if is_saved {
                format!("{title}.sql · saved")
            } else {
                "unsaved".to_string()
            }));
            if let Some(id) = active_tab_id.lock().unwrap().clone() {
                let mut tabs = workspace_tabs.lock().unwrap();
                if let Some(tab) = tabs.iter_mut().find(|tab| tab.id == id) {
                    tab.title = title.clone();
                    tab.kind = "sql".into();
                    tab.query_text = text;
                }
                set_workspace_tabs(&w, &tabs, Some(&id));
            }
            rebuild_query_tree(&title);
        });
    }

    // ----- function definitions captured at connect (name → CREATE source) -----
    let fn_defs: Arc<std::sync::Mutex<std::collections::HashMap<String, String>>> =
        Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));
    {
        let weak = window.as_weak();
        let fn_defs = fn_defs.clone();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let current_connection_id = current_connection_id.clone();
        window.on_open_function(move |name| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let def = fn_defs
                .lock()
                .unwrap()
                .get(name.as_str())
                .cloned()
                .unwrap_or_default();
            let lines: Vec<ModelRc<Span>> = def
                .lines()
                .map(|l| {
                    let spans: Vec<Span> = editor::lex_line(l)
                        .into_iter()
                        .map(|sp| Span {
                            text: sp.text.into(),
                            kind: sp.kind,
                            sel: false,
                        })
                        .collect();
                    ModelRc::from(Rc::new(VecModel::from(spans)))
                })
                .collect();
            w.set_fn_lines(ModelRc::from(Rc::new(VecModel::from(lines))));
            w.set_fn_name(name.clone());
            w.set_fn_mode(true);
            w.set_active_table(SharedString::default());
            let id = format!(
                "function:{}:{}:{}",
                current_connection_id
                    .lock()
                    .unwrap()
                    .as_deref()
                    .unwrap_or_default(),
                w.get_schema_name(),
                name
            );
            let mut tabs = workspace_tabs.lock().unwrap();
            if let Some(i) = tabs.iter().position(|tab| tab.id == id) {
                *active_tab_id.lock().unwrap() = Some(id.clone());
                set_workspace_tabs(&w, &tabs, Some(&id));
                w.set_active_tab(i as i32);
            } else {
                tabs.push(WorkspaceTab {
                    id: id.clone(),
                    title: name.to_string(),
                    kind: "function".into(),
                    query_text: String::new(),
                    table: None,
                    browse: BrowseState::default(),
                    results: Vec::new(),
                    active_result: 0,
                    view: None,
                    indexes: Vec::new(),
                    loading: false,
                    pinned: true,
                    group: 0,
                    split: false,
                    split_ratio: 0.5,
                    pane1_query: String::new(),
                });
                *active_tab_id.lock().unwrap() = Some(id.clone());
                set_workspace_tabs(&w, &tabs, Some(&id));
            }
        });
    }

    // ----- Open database (⌘⇧O) / Open Connection (⌘O) modals -----
    let conn_modal_map: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
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
                })
                .collect();
            if items.is_empty() {
                return;
            }
            w.set_db_items(ModelRc::from(Rc::new(VecModel::from(items))));
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
            let Some(it) = w.get_db_items().row_data(idx.max(0) as usize) else {
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
                })
                .collect();
            if items.is_empty() {
                return;
            }
            w.set_schema_items(ModelRc::from(Rc::new(VecModel::from(items))));
            w.set_schema_modal_open(true);
        });
    }
    {
        let weak = window.as_weak();
        let rt = rt.clone();
        let current = current.clone();
        let raw_nodes = raw_nodes.clone();
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
            let Some(it) = w.get_schema_items().row_data(idx.max(0) as usize) else {
                return;
            };
            let schema_name = it.label.to_string();
            w.set_schema_name(it.label.clone());
            w.set_bc_schema(it.label);
            // Anything browsed belonged to the old schema; force a fresh pick and
            // drop expand/collapse state that referenced the old schema's tables.
            w.set_active_table(SharedString::default());
            // Table/browse tabs referenced the old schema and must drop, but SQL
            // scratch tabs are schema-agnostic — keep them so a query in progress
            // survives the switch. Snapshot the live editor into the active tab
            // first (the user may not have run/switched since typing).
            let prev_active = active_tab_id.lock().unwrap().clone();
            let keep_active: Option<String> = {
                let mut tabs = workspace_tabs.lock().unwrap();
                if let Some(id) = prev_active.clone() {
                    if let Some(tab) = tabs.iter_mut().find(|t| t.id == id) {
                        tab.query_text = ed_state.borrow().text();
                    }
                }
                tabs.retain(|t| t.kind == "sql");
                // Prefer keeping the tab the user was on (if it was SQL); else the
                // first surviving SQL tab.
                prev_active
                    .clone()
                    .filter(|id| tabs.iter().any(|t| &t.id == id))
                    .or_else(|| tabs.first().map(|t| t.id.clone()))
            };
            // Standby: the active SQL tab survives, so its result is still valid —
            // leave the grid up instead of blanking it.
            let standby = keep_active.is_some() && keep_active == prev_active;
            *active_tab_id.lock().unwrap() = keep_active.clone();
            let limit = browse.lock().unwrap().limit;
            *browse.lock().unwrap() = BrowseState {
                limit,
                ..Default::default()
            };
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
            let expanded_tables = expanded_tables.clone();
            let loaded_dbs = loaded_dbs.clone();
            let query_console = query_console.clone();
            rt.spawn(async move {
                let guard = current.lock_owned().await;
                let Some((_, driver)) = guard.as_ref() else {
                    return;
                };
                let Ok(schema) = driver.schema_for(&schema_name).await else {
                    return;
                };
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
                drop(guard);
                *raw_nodes.lock().unwrap() = nodes;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak2.upgrade() {
                        sync_query_console(&w, &query_console);
                        w.set_schema_tree(ModelRc::from(Rc::new(VecModel::from(rows))));
                        let empty_cols: Vec<StructField> = Vec::new();
                        w.set_structure_columns(ModelRc::from(Rc::new(VecModel::from(empty_cols))));
                    }
                });
            });
        });
    }
    {
        let weak = window.as_weak();
        let store = store.clone();
        let conn_modal_map = conn_modal_map.clone();
        window.on_open_conn_modal(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            // Flatten groups + connections (all groups expanded in the modal).
            let rows = build_conn_items(&store.borrow(), &HashSet::new(), "");
            let mut items: Vec<PaletteItem> = Vec::new();
            let mut map: Vec<i32> = Vec::new();
            for r in rows {
                if r.is_header {
                    items.push(PaletteItem {
                        label: r.group.to_lowercase().into(),
                        kind: "group".into(),
                        sub: SharedString::default(),
                        local: false,
                    });
                    map.push(-1);
                } else {
                    items.push(PaletteItem {
                        label: r.name,
                        kind: r.engine,
                        sub: r.subline,
                        local: r.local,
                    });
                    map.push(r.index);
                }
            }
            *conn_modal_map.borrow_mut() = map;
            w.set_conn_items(ModelRc::from(Rc::new(VecModel::from(items))));
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

    // ----- connections screen: selection fills the right detail panel -----
    let fill_detail = {
        let weak = window.as_weak();
        let store = store.clone();
        move |idx: i32| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let store = store.borrow();
            let Some(s) = store.list().get(idx as usize) else {
                w.set_selected_conn(-1);
                return;
            };
            w.set_selected_conn(idx);
            w.set_sel_name(s.name.clone().into());
            w.set_sel_engine(AnyDriver::badge(s.engine).into());
            let label = AnyDriver::label(s.engine);
            let sub = match s.group.as_deref().filter(|g| !g.trim().is_empty()) {
                Some(g) => format!("{label} · grup {}", g.to_lowercase()),
                None => label.to_string(),
            };
            w.set_sel_sub(sub.into());
            w.set_sel_local(s.local);
            let ssl = match s.sslmode {
                rdb_core::conn::SslMode::Disable => "disable",
                rdb_core::conn::SslMode::Prefer => "prefer",
                rdb_core::conn::SslMode::Require => "require",
            };
            let mut rows = vec![
                KvRow {
                    k: "Host".into(),
                    v: s.host.clone().into(),
                },
                KvRow {
                    k: "Port".into(),
                    v: s.port.to_string().into(),
                },
            ];
            if let Some(db) = &s.database {
                rows.push(KvRow {
                    k: "Database".into(),
                    v: db.clone().into(),
                });
            }
            rows.push(KvRow {
                k: "User".into(),
                v: s.user.clone().into(),
            });
            rows.push(KvRow {
                k: "SSL".into(),
                v: ssl.into(),
            });
            if mock::mock_mode() && s.engine == rdb_connstore::Engine::Postgres {
                rows.push(KvRow {
                    k: "Server".into(),
                    v: "PostgreSQL 16.14".into(),
                });
            }
            w.set_sel_rows(ModelRc::from(Rc::new(VecModel::from(rows))));
            let tags: Vec<SharedString> = s.tags.iter().map(|t| t.as_str().into()).collect();
            w.set_sel_tags(ModelRc::from(Rc::new(VecModel::from(tags))));
            let footer = if mock::mock_mode() {
                "Terakhir terhubung · 2 menit lalu · RDB 1.2.0 open source".to_string()
            } else {
                format!("RDB {} open source", env!("CARGO_PKG_VERSION"))
            };
            w.set_sel_footer(footer.into());
        }
    };
    {
        let fill_detail = fill_detail.clone();
        window.on_select_conn(fill_detail);
    }
    // Mock mode boots with the reference selection ("bot ai tele").
    if mock::mock_mode() {
        let idx = store
            .borrow()
            .list()
            .iter()
            .position(|s| s.name == "bot ai tele")
            .map(|i| i as i32)
            .unwrap_or(-1);
        fill_detail(idx);
    }

    // RDB_SCREEN drives the app to a reference state for screenshots and the
    // e2e harness: "workspace" connects + opens emiten; "sql" opens + runs the
    // saved query; the rest open a specific modal/view.
    if let Ok(screen) = std::env::var("RDB_SCREEN") {
        let idx = store
            .borrow()
            .list()
            .iter()
            .position(|s| s.name == "bot ai tele")
            .map(|i| i as i32)
            .unwrap_or(0);
        // "connections" IS the pre-connect screen; connecting would swap it
        // for the workspace before the shot fires.
        if screen != "connections" {
            let weak = window.as_weak();
            let t1 = Box::leak(Box::new(slint::Timer::default()));
            t1.start(
                slint::TimerMode::SingleShot,
                std::time::Duration::from_millis(250),
                move || {
                    if let Some(w) = weak.upgrade() {
                        w.invoke_connect_clicked(idx);
                    }
                },
            );
        }
        if screen.starts_with("sql") {
            let weak = window.as_weak();
            let t2 = Box::leak(Box::new(slint::Timer::default()));
            t2.start(
                slint::TimerMode::SingleShot,
                std::time::Duration::from_millis(1800),
                move || {
                    if let Some(w) = weak.upgrade() {
                        w.set_sidebar_mode(1);
                        w.invoke_open_query("emiten-per-sektor".into(), 0);
                    }
                },
            );
            let weak = window.as_weak();
            let t3 = Box::leak(Box::new(slint::Timer::default()));
            t3.start(
                slint::TimerMode::SingleShot,
                std::time::Duration::from_millis(2300),
                move || {
                    if let Some(w) = weak.upgrade() {
                        w.invoke_run_query();
                    }
                },
            );
        }
        if screen == "modal-db"
            || screen == "modal-conn"
            || screen == "modal-add-mongo"
            || screen == "function"
            || screen == "palette"
        {
            let weak = window.as_weak();
            let which = screen.clone();
            let t4 = Box::leak(Box::new(slint::Timer::default()));
            t4.start(
                slint::TimerMode::SingleShot,
                std::time::Duration::from_millis(1800),
                move || {
                    if let Some(w) = weak.upgrade() {
                        match which.as_str() {
                            "modal-db" => w.invoke_open_db_modal(),
                            "modal-conn" => w.invoke_open_conn_modal(),
                            "modal-add-mongo" => {
                                w.invoke_open_add_form();
                                w.set_f_engine("MongoDB".into());
                                w.set_f_port("27017".into());
                                w.set_f_import_url("mongodb://root:secret@10.1.237.31:32343/admin?authMechanism=DEFAULT&replicaSet=production-rs".into());
                            }
                            "palette" => w.invoke_toggle_palette(),
                            _ => w.invoke_open_function("uuid_generate_v3".into()),
                        }
                    }
                },
            );
        }
        if screen.starts_with("workspace") {
            let weak = window.as_weak();
            let table = if screen.starts_with("workspace-users") {
                "users"
            } else {
                "emiten"
            };
            let t2 = Box::leak(Box::new(slint::Timer::default()));
            t2.start(
                slint::TimerMode::SingleShot,
                std::time::Duration::from_millis(1800),
                move || {
                    if let Some(w) = weak.upgrade() {
                        w.invoke_open_table("".into(), table.into());
                    }
                },
            );
        }

        // E2E editing scenarios layered on the base screens above. Fixed
        // delays race the async connect/browse pipeline, so each step fires
        // when its precondition is observable, polling every 100ms.
        let when = |cond: Rc<dyn Fn(&MainWindow) -> bool>, act: Rc<dyn Fn(&MainWindow)>| {
            let weak = window.as_weak();
            let t: &'static slint::Timer = Box::leak(Box::new(slint::Timer::default()));
            t.start(
                slint::TimerMode::Repeated,
                std::time::Duration::from_millis(100),
                move || {
                    let Some(w) = weak.upgrade() else {
                        t.stop();
                        return;
                    };
                    if cond(&w) {
                        t.stop();
                        act(&w);
                    }
                },
            );
        };
        use slint::Model as _;
        // grid loaded with an editable page (pk fetched)
        let grid_ready: Rc<dyn Fn(&MainWindow) -> bool> =
            Rc::new(|w| w.get_grid_cells().row_count() > 0 && !w.get_grid_read_only());
        let has_pending: Rc<dyn Fn(&MainWindow) -> bool> = Rc::new(|w| w.get_pending_count() > 0);
        if matches!(
            screen.as_str(),
            "workspace-dirty" | "workspace-guard" | "workspace-commit" | "workspace-tabnav"
        ) {
            when(
                grid_ready.clone(),
                Rc::new(|w| w.invoke_cell_edited(0, 2, "Bayan EDITED".into())),
            );
        }
        if screen == "workspace-active-commit" {
            when(
                grid_ready.clone(),
                Rc::new(|w| {
                    w.invoke_edit_cell(0, 2);
                    w.set_editing_value("Bayan ACTIVE SAVE".into());
                    w.invoke_commit_edits();
                }),
            );
        }
        if screen == "workspace-detail-commit" {
            when(
                grid_ready.clone(),
                Rc::new(|w| {
                    w.invoke_stage_cell(
                        0,
                        2,
                        "Bayan Resources — long detail value saved from the right panel".into(),
                    );
                    w.invoke_commit_edits();
                }),
            );
        }
        if screen == "workspace-long-edit" {
            when(
                grid_ready.clone(),
                Rc::new(|w| {
                    w.invoke_cell_edited(
                        0,
                        2,
                        "Bayan Resources — a deliberately long inline value remains visible while editing, wraps inside a bounded overlay, and still supports cursor navigation all the way to the final character."
                            .into(),
                    );
                    w.invoke_edit_cell(0, 2);
                }),
            );
        }
        if screen == "workspace-pointer-edit" {
            when(
                grid_ready.clone(),
                Rc::new(|w| {
                    use slint::platform::{PointerEventButton, WindowEvent};
                    let position = slint::LogicalPosition::new(650.0, 164.0);
                    for _ in 0..2 {
                        w.window().dispatch_event(WindowEvent::PointerPressed {
                            position,
                            button: PointerEventButton::Left,
                        });
                        w.window().dispatch_event(WindowEvent::PointerReleased {
                            position,
                            button: PointerEventButton::Left,
                        });
                    }
                    assert_eq!((w.get_editing_row(), w.get_editing_col()), (0, 2));
                }),
            );
        }
        if screen == "workspace-dirty" {
            // second pending change: a delete-marked row
            when(
                has_pending.clone(),
                Rc::new(|w| {
                    w.set_selected_row(2);
                    w.invoke_mark_delete();
                }),
            );
        }
        if screen == "workspace-guard" {
            // navigation with pending edits must be refused with a message
            when(has_pending.clone(), Rc::new(|w| w.invoke_next_page()));
        }
        if screen == "workspace-commit" {
            // full CRUD loop: buffer → WriteOps → mock commit → refetch
            when(has_pending.clone(), Rc::new(|w| w.invoke_commit_edits()));
        }
        if screen == "workspace-tabnav" {
            // Tab stores the edited cell and opens the neighbour's editor
            when(
                has_pending.clone(),
                Rc::new(|w| w.invoke_cell_advance(0, 3, "TABBED".into(), true)),
            );
        }
        if screen == "workspace-sql" {
            // Real UI transition: table browse → global SQL button. The fresh
            // query tab must not inherit the table request's loading state.
            when(
                Rc::new(|w| w.get_active_table() == "emiten"),
                Rc::new(|w| w.invoke_new_tab()),
            );
        }
        if screen == "workspace-tabflow" {
            when(
                Rc::new(|w| w.get_active_table() == "emiten" && w.get_tabs().row_count() == 1),
                Rc::new(|w| w.invoke_open_table("".into(), "referral_sources".into())),
            );
            when(
                Rc::new(|w| {
                    w.get_active_table() == "referral_sources" && w.get_tabs().row_count() == 1
                }),
                Rc::new(|w| {
                    w.invoke_pin_table("".into(), "referral_sources".into());
                    w.invoke_open_table("".into(), "sectors".into());
                }),
            );
            when(
                Rc::new(|w| w.get_active_table() == "sectors" && w.get_tabs().row_count() == 2),
                Rc::new(|w| w.invoke_new_tab()),
            );
        }
        if screen == "workspace-filter" {
            when(
                grid_ready.clone(),
                Rc::new(|w| {
                    w.set_data_filter_open(true);
                    w.set_filter_col("name".into());
                    w.set_filter_op("ILIKE".into());
                    w.set_grid_filter("mitra".into());
                    w.invoke_apply_filter();
                }),
            );
        }
        if screen == "workspace-limit" {
            when(
                grid_ready.clone(),
                Rc::new(|w| w.invoke_set_limit("25".into())),
            );
        }
        if screen == "workspace-insert" {
            when(grid_ready.clone(), Rc::new(|w| w.invoke_add_row()));
        }
        if screen == "workspace-users-bool" || screen == "workspace-users-date" {
            when(grid_ready.clone(), Rc::new(|w| w.invoke_add_row()));
            let date = screen == "workspace-users-date";
            when(
                has_pending.clone(),
                Rc::new(move |w| {
                    let rows = w.get_grid_cells().row_count() / w.get_grid_col_count() as usize;
                    w.invoke_edit_cell(rows.saturating_sub(1) as i32, if date { 4 } else { 3 });
                }),
            );
        }
        if screen == "sql-select" {
            // ⌘A select-all: the whole query gets the selection tint
            when(
                Rc::new(|w| !w.get_query_text().trim().is_empty()),
                Rc::new(|w| {
                    w.invoke_editor_key("a".into(), true, false, false);
                }),
            );
        }
        if screen == "sql-empty" {
            // a query with zero rows shows the empty state, not a blank pane
            let load = load_editor_text.clone();
            when(
                Rc::new(|w| !w.get_results_meta().is_empty()),
                Rc::new(move |w| {
                    load(0, "SELECT * FROM emiten OFFSET 99999");
                    w.invoke_run_query();
                }),
            );
        }
        if screen == "sql-find" {
            // ⌘F find bar: highlights the first match of a term
            when(
                Rc::new(|w| !w.get_query_text().trim().is_empty()),
                Rc::new(|w| {
                    w.invoke_toggle_find();
                    w.set_find_text("sector".into());
                    w.invoke_find_changed("sector".into());
                }),
            );
        }
        if screen == "sql-multi" {
            // multi-statement run: status reads "N statements · …"
            let load = load_editor_text.clone();
            when(
                Rc::new(|w| !w.get_results_meta().is_empty()),
                Rc::new(move |w| {
                    load(0, "SELECT 1;\nSELECT * FROM emiten LIMIT 5;");
                    w.invoke_run_query();
                }),
            );
        }
    }

    // Last result view kept in memory so the client-side filter (Feature C)
    // can re-derive the visible rows without re-querying. Arc<Mutex<>> (not Rc)
    // so it can cross into the Send event-loop closure from the query task.
    let last_view: Arc<std::sync::Mutex<Option<model::ResultView>>> =
        Arc::new(std::sync::Mutex::new(None));

    let save_active_tab: Rc<dyn Fn(&MainWindow)> = {
        let tabs = workspace_tabs.clone();
        let active_id = active_tab_id.clone();
        let ed_state = ed_state.clone();
        let browse = browse.clone();
        let results = results.clone();
        let active_result = active_result.clone();
        let last_view = last_view.clone();
        Rc::new(move |w| {
            let Some(id) = active_id.lock().unwrap().clone() else {
                return;
            };
            let mut tabs = tabs.lock().unwrap();
            let Some(tab) = tabs.iter_mut().find(|tab| tab.id == id) else {
                return;
            };
            tab.query_text = ed_state.borrow().text();
            tab.loading = w.get_query_running();
            if tab.kind == "table" {
                tab.browse = browse.lock().unwrap().clone();
            } else if tab.kind == "sql" {
                tab.results = results.lock().unwrap().clone();
                tab.active_result = *active_result.lock().unwrap();
            }
            if tab.kind != "function" {
                tab.view = last_view.lock().unwrap().clone().map(|view| StoredResult {
                    view,
                    meta: w.get_results_meta().to_string(),
                    latency: w.get_status_latency().to_string(),
                });
            }
            // Persist SQL scratch tabs so they survive a restart. Cheap JSON
            // write; fires on switch/new/close/run, which is when text changes.
            save_query_tabs(&tabs, Some(&id));
        })
    };

    // The second workspace group has its own live editor/result runtime. Save
    // it before selecting, closing, or moving one of its tabs just like the
    // left group; otherwise a drag could move an older snapshot.
    let save_p1_tab: Rc<dyn Fn(&MainWindow)> = {
        let tabs = workspace_tabs.clone();
        let active_id = active_p1_tab_id.clone();
        let panes = panes.clone();
        Rc::new(move |w| {
            let Some(id) = active_id.lock().unwrap().clone() else {
                return;
            };
            let mut tabs = tabs.lock().unwrap();
            let Some(tab) = tabs.iter_mut().find(|tab| tab.id == id) else {
                return;
            };
            tab.query_text = panes[1].ed_state.borrow().text();
            tab.loading = w.get_p1_query_running();
            if tab.kind == "sql" {
                tab.results = panes[1].results.lock().unwrap().clone();
                tab.active_result = *panes[1].active_result.lock().unwrap();
            }
            save_query_tabs(&tabs, None);
        })
    };

    #[allow(clippy::type_complexity)]
    let restore_tab: Rc<dyn Fn(&MainWindow, usize)> = {
        let tabs = workspace_tabs.clone();
        let active_id = active_tab_id.clone();
        let load_editor_text = load_editor_text.clone();
        let browse = browse.clone();
        let results = results.clone();
        let active_result = active_result.clone();
        let last_view = last_view.clone();
        let displayed_grid = displayed_grid.clone();
        let hidden_cols = hidden_cols.clone();
        let sort_state = sort_state.clone();
        let col_order = col_order.clone();
        let col_filters = col_filters.clone();
        let edit_buf = edit_buf.clone();
        Rc::new(move |w, index| {
            let tab = {
                let tabs = tabs.lock().unwrap();
                let Some(tab) = tabs.get(index).cloned() else {
                    return;
                };
                tab
            };
            *active_id.lock().unwrap() = Some(tab.id.clone());
            let tabs_guard = tabs.lock().unwrap();
            set_workspace_tabs(w, &tabs_guard, Some(&tab.id));
            drop(tabs_guard);

            load_editor_text(0, &tab.query_text);
            w.set_active_pane(0);
            w.set_fn_mode(tab.kind == "function");
            w.set_query_running(tab.loading);
            w.set_active_table(
                tab.table
                    .as_ref()
                    .map(|table| SharedString::from(table.name.clone()))
                    .unwrap_or_default(),
            );
            if tab.kind == "table" {
                *browse.lock().unwrap() = tab.browse.clone();
            } else {
                let limit = browse.lock().unwrap().limit;
                *browse.lock().unwrap() = BrowseState {
                    limit,
                    ..Default::default()
                };
            }
            w.set_total_rows(tab.browse.total.map(|n| n as i32).unwrap_or(-1));
            w.set_grid_read_only(tab.browse.pk_cols.is_empty());
            let index_rows: Vec<IndexRow> = tab
                .indexes
                .iter()
                .cloned()
                .map(|(name, definition)| IndexRow {
                    name: name.into(),
                    definition: definition.into(),
                })
                .collect();
            w.set_index_rows(ModelRc::from(Rc::new(VecModel::from(index_rows))));

            *results.lock().unwrap() = tab.results.clone();
            *active_result.lock().unwrap() = tab.active_result;
            set_result_tabs(w, 0, tab.results.len(), tab.active_result);
            let selected = if tab.kind == "sql" {
                tab.results.get(tab.active_result).cloned().or(tab.view)
            } else {
                tab.view
            };
            if let Some(stored) = selected {
                present_view(
                    w,
                    0,
                    stored.view,
                    &stored.meta,
                    &stored.latency,
                    &last_view,
                    &displayed_grid,
                    &hidden_cols,
                    &sort_state,
                    &col_order,
                    &col_filters,
                    &edit_buf,
                    &browse,
                );
            } else {
                *last_view.lock().unwrap() = None;
                *displayed_grid.lock().unwrap() = None;
                clear_grid(w, 0);
                w.set_results_meta(SharedString::default());
                w.set_result_status(SharedString::default());
            }
        })
    };

    let restore_p1_tab: Rc<dyn Fn(&MainWindow, usize)> = {
        let tabs = workspace_tabs.clone();
        let active_id = active_p1_tab_id.clone();
        let load_editor_text = load_editor_text.clone();
        let panes = panes.clone();
        let last_view = last_view.clone();
        Rc::new(move |w, group_index| {
            let tab = {
                let tabs = tabs.lock().unwrap();
                tabs.iter()
                    .filter(|tab| tab.group == 1)
                    .nth(group_index)
                    .cloned()
            };
            let Some(tab) = tab else {
                return;
            };
            *active_id.lock().unwrap() = Some(tab.id.clone());
            load_editor_text(1, &tab.query_text);
            *panes[1].results.lock().unwrap() = tab.results.clone();
            *panes[1].active_result.lock().unwrap() = tab.active_result;
            set_result_tabs(w, 1, tab.results.len(), tab.active_result);
            w.set_p1_active_tab(group_index as i32);
            w.set_active_pane(1);
            let selected = if tab.kind == "sql" {
                tab.results.get(tab.active_result).cloned().or(tab.view)
            } else {
                tab.view
            };
            if let Some(stored) = selected {
                present_view(
                    w,
                    1,
                    stored.view,
                    &stored.meta,
                    &stored.latency,
                    &last_view,
                    &panes[1].displayed_grid,
                    &panes[1].hidden_cols,
                    &panes[1].sort_state,
                    &panes[1].col_order,
                    &panes[1].col_filters,
                    &panes[1].edit_buf,
                    &panes[1].browse,
                );
            } else {
                clear_grid(w, 1);
                set_p_results_meta(w, 1, SharedString::default());
                set_p_result_status(w, 1, SharedString::default());
            }
        })
    };

    {
        let restore_p1_tab = restore_p1_tab.clone();
        let save_p1_tab = save_p1_tab.clone();
        let weak = window.as_weak();
        window.on_select_p1_tab(move |index| {
            if let Some(w) = weak.upgrade() {
                save_p1_tab(&w);
                restore_p1_tab(&w, index.max(0) as usize);
            }
        });
    }
    {
        let weak = window.as_weak();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let active_p1_tab_id = active_p1_tab_id.clone();
        let current_connection_id = current_connection_id.clone();
        let query_number = query_number.clone();
        let save_p1_tab = save_p1_tab.clone();
        let restore_p1_tab = restore_p1_tab.clone();
        window.on_new_tab_in_group(move |group| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if group == 0 {
                w.invoke_new_tab();
                return;
            }
            save_p1_tab(&w);
            let number = query_number.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            let connection = current_connection_id
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_default();
            let id = format!("query:{connection}:{number}");
            let right_index = {
                let mut tabs = workspace_tabs.lock().unwrap();
                let mut tab = WorkspaceTab::sql(id.clone(), number);
                tab.group = 1;
                tabs.push(tab);
                let left = active_tab_id.lock().unwrap().clone();
                set_workspace_tabs(&w, &tabs, left.as_deref());
                save_query_tabs(&tabs, left.as_deref());
                tabs.iter().filter(|tab| tab.group == 1).count() - 1
            };
            *active_p1_tab_id.lock().unwrap() = Some(id);
            restore_p1_tab(&w, right_index);
        });
    }
    {
        let weak = window.as_weak();
        let tabs = workspace_tabs.clone();
        let save_active_tab = save_active_tab.clone();
        let save_p1_tab = save_p1_tab.clone();
        let restore_tab = restore_tab.clone();
        let restore_p1_tab = restore_p1_tab.clone();
        let active_p1_tab_id = active_p1_tab_id.clone();
        let active_tab_id = active_tab_id.clone();
        window.on_move_tab_group(move |index, target| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let target = target.clamp(0, 1) as usize;
            save_active_tab(&w);
            save_p1_tab(&w);
            let source = if target == 1 { 0 } else { 1 };
            let moved_id = {
                let mut tabs = tabs.lock().unwrap();
                if source == 0 && tabs.iter().filter(|tab| tab.group == 0).count() == 1 {
                    return;
                }
                let Some(tab) = tabs
                    .iter_mut()
                    .filter(|tab| tab.group == source)
                    .nth(index.max(0) as usize)
                else {
                    return;
                };
                tab.group = target;
                tab.id.clone()
            };
            let (left_index, right_index, left_id) = {
                let tabs = tabs.lock().unwrap();
                let left_index = tabs.iter().position(|tab| tab.group == 0);
                let left_id = left_index.map(|index| tabs[index].id.clone());
                let right_index = tabs
                    .iter()
                    .filter(|tab| tab.group == 1)
                    .position(|tab| tab.id == moved_id)
                    .or_else(|| tabs.iter().filter(|tab| tab.group == 1).position(|_| true));
                set_workspace_tabs(&w, &tabs, left_id.as_deref());
                (left_index, right_index, left_id)
            };
            *active_tab_id.lock().unwrap() = left_id;
            if let Some(index) = left_index {
                restore_tab(&w, index);
            }
            if let Some(index) = right_index {
                restore_p1_tab(&w, index);
            } else {
                *active_p1_tab_id.lock().unwrap() = None;
                w.set_p1_active_tab(-1);
            }
        });
    }

    // ----- toggle a sidebar group's collapsed state (Feature A) -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        let settings = settings.clone();
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
            // Persist the new collapsed set (best-effort; a write failure must
            // not break the UI).
            let groups: Vec<String> = collapsed.borrow().iter().cloned().collect();
            let _ = settings
                .borrow_mut()
                .update(|s| s.ui_state.collapsed_groups = groups);
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

    // ----- star/unstar a saved connection -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        window.on_toggle_favorite(move |idx| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let id = {
                let s = store.borrow();
                match s.list().get(idx as usize) {
                    Some(c) => (c.id.clone(), c.favorite),
                    None => return,
                }
            };
            let (id, was_fav) = id;
            let _ = store.borrow_mut().set_favorite(&id, !was_fav);
            let items =
                build_conn_items(&store.borrow(), &collapsed.borrow(), &conn_filter.borrow());
            w.set_connections(ModelRc::from(Rc::new(VecModel::from(items))));
        });
    }

    // ----- drag-reorder a connection within its group -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        window.on_reorder_conn(move |from_idx, delta| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if delta == 0 {
                return;
            }
            // Resolve the row-step delta against the dragged connection's group,
            // using the same (favorite desc, order asc) display order as the
            // builder, then map back to a store index for `reorder`.
            let (from_id, target_vec_idx) = {
                let s = store.borrow();
                let list = s.list();
                let Some(from) = list.get(from_idx as usize) else {
                    return;
                };
                let group_key = |c: &rdb_connstore::SavedConnection| {
                    c.group
                        .as_deref()
                        .filter(|g| !g.trim().is_empty())
                        .unwrap_or(UNGROUPED)
                        .to_string()
                };
                let g = group_key(from);
                let mut members: Vec<usize> = list
                    .iter()
                    .enumerate()
                    .filter(|(_, c)| group_key(c) == g)
                    .map(|(i, _)| i)
                    .collect();
                members.sort_by_key(|&i| (!list[i].favorite, list[i].order));
                let Some(pos) = members.iter().position(|&i| i == from_idx as usize) else {
                    return;
                };
                let target_pos =
                    (pos as i64 + delta as i64).clamp(0, members.len() as i64 - 1) as usize;
                if target_pos == pos {
                    return;
                }
                (from.id.clone(), members[target_pos])
            };
            let _ = store.borrow_mut().reorder(&from_id, target_vec_idx);
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
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let current_connection_id = current_connection_id.clone();
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
            workspace_tabs.lock().unwrap().clear();
            *active_tab_id.lock().unwrap() = None;
            *current_connection_id.lock().unwrap() = None;
            set_workspace_tabs(&w, &[], None);
            clear_grid(&w, 0);
            *cur_engine.borrow_mut() = None;
            expanded_tables.lock().unwrap().clear();
            loaded_dbs.lock().unwrap().clear();
            *collapsed_categories.borrow_mut() = default_collapsed_cats();
            raw_nodes.lock().unwrap().clear();
            let current = current.clone();
            rt.spawn(async move {
                *current.lock().await = None;
            });
        });
    }

    // Deferred handle to `run_browse` (defined later): per-column filters in
    // browse mode re-query the DB, but this handler is wired before run_browse
    // exists. Filled in once run_browse is built, below.
    #[allow(clippy::type_complexity)]
    let browse_trigger: Rc<RefCell<Option<Rc<dyn Fn()>>>> = Rc::new(RefCell::new(None));

    // ----- apply client-side row filter to the last result (Feature C) -----
    {
        let weak = window.as_weak();
        let last_view = last_view.clone();
        let edit_buf = edit_buf.clone();
        let displayed_grid = displayed_grid.clone();
        let hidden_cols = hidden_cols.clone();
        let sort_state = sort_state.clone();
        let col_order = col_order.clone();
        let col_filters = col_filters.clone();
        window.on_apply_filter(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if guard_pending_edits(&w, &edit_buf) {
                return;
            }
            let needle = w.get_grid_filter().to_string().to_lowercase();
            let fcol = w.get_filter_col().to_string();
            let fop = w.get_filter_op().to_string();
            let hidden = hidden_cols.lock().unwrap().clone();
            let order = col_order.lock().unwrap().clone();
            let cfilters = col_filters.lock().unwrap().clone();
            let (scol, sasc) = *sort_state.lock().unwrap();
            let guard = last_view.lock().unwrap();
            let Some(v) = guard.as_ref() else {
                return;
            };
            if matches!(v, model::ResultView::Affected(_)) {
                return;
            }
            // Filtering renumbers the visible rows, so pending edits keyed by
            // row index would land on the wrong rows — drop them.
            {
                let mut b = edit_buf.lock().unwrap();
                b.clear();
            }
            w.set_pending_count(0);
            w.set_editing_row(-1);
            w.set_editing_col(-1);
            let filtered = compute_view(
                v, &needle, &fcol, &fop, &cfilters, &hidden, &order, scol, sasc,
            );
            *displayed_grid.lock().unwrap() = view_grid(&filtered);
            apply_result(&w, 0, filtered);
        });
    }

    // ----- per-column filter box edited -----
    {
        let weak = window.as_weak();
        let last_view = last_view.clone();
        let edit_buf = edit_buf.clone();
        let displayed_grid = displayed_grid.clone();
        let hidden_cols = hidden_cols.clone();
        let sort_state = sort_state.clone();
        let col_order = col_order.clone();
        let col_filters = col_filters.clone();
        let browse = browse.clone();
        let browse_trigger = browse_trigger.clone();
        window.on_set_col_filter(move |display_c, text| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if guard_pending_edits(&w, &edit_buf) {
                return;
            }
            let hidden = hidden_cols.lock().unwrap().clone();
            let order = col_order.lock().unwrap().clone();
            let (scol, sasc) = *sort_state.lock().unwrap();
            // display position → ORIGINAL column index
            let visible: Vec<usize> = order
                .iter()
                .copied()
                .filter(|i| !hidden.contains(i))
                .collect();
            let Some(&orig) = visible.get(display_c.max(0) as usize) else {
                return;
            };
            // Browse mode (a table is open): push the filter into the query's
            // WHERE clause and re-fetch from the DB, so filtering isn't limited
            // to the current page. SQL result mode stays client-side (below).
            if browse.lock().unwrap().table.is_some() {
                let name = {
                    let guard = last_view.lock().unwrap();
                    match guard.as_ref() {
                        Some(model::ResultView::Table(g)) => {
                            g.columns.get(orig).map(|c| c.name.clone())
                        }
                        Some(model::ResultView::Documents(d)) => {
                            d.grid.columns.get(orig).map(|c| c.name.clone())
                        }
                        _ => None,
                    }
                };
                if let Some(name) = name {
                    {
                        let mut b = browse.lock().unwrap();
                        b.col_filters.retain(|(n, _)| n != &name);
                        if !text.trim().is_empty() {
                            b.col_filters.push((name, text.to_string()));
                        }
                        b.page = 0; // filter changes the row set → back to page 1
                    }
                    if let Some(run) = browse_trigger.borrow().clone() {
                        run();
                    }
                }
                return;
            }
            let cfilters = {
                let mut cf = col_filters.lock().unwrap();
                if orig < cf.len() {
                    cf[orig] = text.to_string();
                }
                cf.clone()
            };
            let needle = w.get_grid_filter().to_string().to_lowercase();
            let fcol = w.get_filter_col().to_string();
            let fop = w.get_filter_op().to_string();
            let guard = last_view.lock().unwrap();
            let Some(v) = guard.as_ref() else {
                return;
            };
            if matches!(v, model::ResultView::Affected(_)) {
                return;
            }
            // Filtering renumbers rows → row-keyed pending edits would misland.
            edit_buf.lock().unwrap().clear();
            w.set_pending_count(0);
            w.set_editing_row(-1);
            w.set_editing_col(-1);
            let filtered = compute_view(
                v, &needle, &fcol, &fop, &cfilters, &hidden, &order, scol, sasc,
            );
            *displayed_grid.lock().unwrap() = view_grid(&filtered);
            // Columns are unchanged, so update only the cells — replacing the
            // columns model would rebuild (and unfocus) the filter inputs.
            if let model::ResultView::Table(g) = &filtered {
                set_grid_cells_only(&w, 0, g);
            } else {
                apply_result(&w, 0, filtered);
            }
        });
    }

    // ----- header click: client-side sort by a column -----
    {
        let weak = window.as_weak();
        let last_view = last_view.clone();
        let edit_buf = edit_buf.clone();
        let displayed_grid = displayed_grid.clone();
        let hidden_cols = hidden_cols.clone();
        let sort_state = sort_state.clone();
        let col_order = col_order.clone();
        let col_filters = col_filters.clone();
        window.on_sort_col(move |display_c| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if guard_pending_edits(&w, &edit_buf) {
                return;
            }
            let hidden = hidden_cols.lock().unwrap().clone();
            let order = col_order.lock().unwrap().clone();
            let cfilters = col_filters.lock().unwrap().clone();
            // display position → ORIGINAL column index
            let visible: Vec<usize> = order
                .iter()
                .copied()
                .filter(|i| !hidden.contains(i))
                .collect();
            let Some(&orig) = visible.get(display_c.max(0) as usize) else {
                return;
            };
            // toggle direction on the same column, else ascending on a new one
            {
                let mut s = sort_state.lock().unwrap();
                if s.0 == orig as i32 {
                    s.1 = !s.1;
                } else {
                    *s = (orig as i32, true);
                }
            }
            let (scol, sasc) = *sort_state.lock().unwrap();
            w.set_grid_sort_col(display_c);
            w.set_grid_sort_asc(sasc);
            let needle = w.get_grid_filter().to_string().to_lowercase();
            let fcol = w.get_filter_col().to_string();
            let fop = w.get_filter_op().to_string();
            let guard = last_view.lock().unwrap();
            let Some(v) = guard.as_ref() else {
                return;
            };
            if matches!(v, model::ResultView::Affected(_)) {
                return;
            }
            // Sorting renumbers rows → row-keyed pending edits would misland.
            edit_buf.lock().unwrap().clear();
            w.set_pending_count(0);
            w.set_editing_row(-1);
            w.set_editing_col(-1);
            let sorted = compute_view(
                v, &needle, &fcol, &fop, &cfilters, &hidden, &order, scol, sasc,
            );
            *displayed_grid.lock().unwrap() = view_grid(&sorted);
            apply_result(&w, 0, sorted);
        });
    }

    // ----- header drag: reorder result columns -----
    {
        let weak = window.as_weak();
        let last_view = last_view.clone();
        let edit_buf = edit_buf.clone();
        let displayed_grid = displayed_grid.clone();
        let hidden_cols = hidden_cols.clone();
        let sort_state = sort_state.clone();
        let col_order = col_order.clone();
        let col_filters = col_filters.clone();
        window.on_reorder_col(move |from, local_x| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if guard_pending_edits(&w, &edit_buf) {
                return;
            }
            let from = from.max(0) as usize;
            let widths: Vec<f32> = w.get_grid_col_widths().iter().collect();
            if from >= widths.len() {
                return;
            }
            // absolute drop position within the columns strip → target display col
            let start: f32 = widths.iter().take(from).sum();
            let drop = start + local_x;
            let mut acc = 0.0f32;
            let mut target = widths.len() - 1;
            for (i, wd) in widths.iter().enumerate() {
                if drop < acc + wd {
                    target = i;
                    break;
                }
                acc += wd;
            }
            if target == from {
                return;
            }
            let hidden = hidden_cols.lock().unwrap().clone();
            // move the dragged column among the visible entries of col_order
            {
                let mut order = col_order.lock().unwrap();
                let visible: Vec<usize> = order
                    .iter()
                    .copied()
                    .enumerate()
                    .filter(|(_, i)| !hidden.contains(i))
                    .map(|(pos, _)| pos)
                    .collect();
                if from >= visible.len() || target >= visible.len() {
                    return;
                }
                let (fp, tp) = (visible[from], visible[target]);
                let val = order.remove(fp);
                let ins = if tp > fp { tp - 1 } else { tp };
                order.insert(ins, val);
            }
            // move the width the same way so a manual resize follows the column
            let mut widths = widths;
            let wv = widths.remove(from);
            let ins = if target > from { target - 1 } else { target };
            widths.insert(ins, wv);
            w.set_grid_col_widths(ModelRc::from(Rc::new(VecModel::from(widths))));
            let order = col_order.lock().unwrap().clone();
            let (scol, sasc) = *sort_state.lock().unwrap();
            // keep the sort arrow on the same column after the shuffle
            if scol >= 0 {
                let vis: Vec<usize> = order
                    .iter()
                    .copied()
                    .filter(|i| !hidden.contains(i))
                    .collect();
                if let Some(p) = vis.iter().position(|&i| i as i32 == scol) {
                    w.set_grid_sort_col(p as i32);
                }
            }
            let needle = w.get_grid_filter().to_string().to_lowercase();
            let fcol = w.get_filter_col().to_string();
            let fop = w.get_filter_op().to_string();
            let guard = last_view.lock().unwrap();
            let Some(v) = guard.as_ref() else {
                return;
            };
            if matches!(v, model::ResultView::Affected(_)) {
                return;
            }
            // Reorder renumbers columns → col-keyed pending edits would misland.
            edit_buf.lock().unwrap().clear();
            w.set_pending_count(0);
            w.set_editing_row(-1);
            w.set_editing_col(-1);
            let cfilters = col_filters.lock().unwrap().clone();
            // filter boxes follow their columns into the new order
            w.set_grid_col_filters(ModelRc::from(Rc::new(VecModel::from(display_col_filters(
                &cfilters, &order, &hidden,
            )))));
            let transformed = compute_view(
                v, &needle, &fcol, &fop, &cfilters, &hidden, &order, scol, sasc,
            );
            *displayed_grid.lock().unwrap() = view_grid(&transformed);
            apply_result(&w, 0, transformed);
        });
    }

    // ----- Columns popup: toggle a column's visibility -----
    {
        let weak = window.as_weak();
        let last_view = last_view.clone();
        let edit_buf = edit_buf.clone();
        let displayed_grid = displayed_grid.clone();
        let hidden_cols = hidden_cols.clone();
        let sort_state = sort_state.clone();
        let col_order = col_order.clone();
        let col_filters = col_filters.clone();
        window.on_toggle_column(move |i| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if guard_pending_edits(&w, &edit_buf) {
                return;
            }
            let idx = i.max(0) as usize;
            let hidden = {
                let mut h = hidden_cols.lock().unwrap();
                if !h.insert(idx) {
                    h.remove(&idx);
                }
                h.clone()
            };
            let n = w.get_all_columns().row_count();
            let flags: Vec<bool> = (0..n).map(|c| hidden.contains(&c)).collect();
            w.set_col_hidden(ModelRc::from(Rc::new(VecModel::from(flags))));
            let needle = w.get_grid_filter().to_string().to_lowercase();
            let fcol = w.get_filter_col().to_string();
            let fop = w.get_filter_op().to_string();
            let order = col_order.lock().unwrap().clone();
            let (scol, sasc) = *sort_state.lock().unwrap();
            let guard = last_view.lock().unwrap();
            let Some(v) = guard.as_ref() else {
                return;
            };
            if matches!(v, model::ResultView::Affected(_)) {
                return;
            }
            // Hiding renumbers columns: cell edits would write through the
            // wrong column index, so drop pending edits and lock editing
            // until the next refresh restores the full column set.
            // ponytail: remap edit indices instead if editing-with-hidden-
            // columns ever matters.
            {
                edit_buf.lock().unwrap().clear();
            }
            w.set_pending_count(0);
            w.set_editing_row(-1);
            w.set_editing_col(-1);
            w.set_grid_read_only(!hidden.is_empty() || edit_buf.lock().unwrap().pk_cols.is_empty());
            let cfilters = col_filters.lock().unwrap().clone();
            w.set_grid_col_filters(ModelRc::from(Rc::new(VecModel::from(display_col_filters(
                &cfilters, &order, &hidden,
            )))));
            let transformed = compute_view(
                v, &needle, &fcol, &fop, &cfilters, &hidden, &order, scol, sasc,
            );
            // ponytail: hiding is structural, so widths reset to type defaults
            // (manual resize is dropped) — same trade-off reorder makes.
            if let Some(g) = view_grid(&transformed) {
                let widths: Vec<f32> = g
                    .columns
                    .iter()
                    .map(|c| default_col_width(&c.name, &c.type_name))
                    .collect();
                w.set_grid_col_widths(ModelRc::from(Rc::new(VecModel::from(widths))));
            }
            *displayed_grid.lock().unwrap() = view_grid(&transformed);
            apply_result(&w, 0, transformed);
        });
    }

    // Handle of the in-flight connect task, so the Cancel button can abort it.
    let connect_handle: Rc<RefCell<Option<tokio::task::JoinHandle<()>>>> =
        Rc::new(RefCell::new(None));

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
        let current_connection_id = current_connection_id.clone();
        let query_console = query_console.clone();
        let results = results.clone();
        let last_view = last_view.clone();
        let edit_buf = edit_buf.clone();
        let db_override = db_override.clone();
        let tabs_restored = tabs_restored.clone();
        let restore_tab = restore_tab.clone();
        window.on_connect_clicked(move |idx| {
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
            let restore = !tabs_restored.get() || db_ovr.is_some();
            tabs_restored.set(true);
            let (init_tabs, init_active) = if restore {
                load_query_tabs()
            } else {
                // Retain the SQL scratch tabs (with their results) so switching
                // connection leaves the last result on screen — "standby". Drop
                // the connection-scoped table/function tabs (their data would be
                // stale) and only clear the in-flight loading flag so no tab is
                // left showing a stuck spinner.
                let mut kept: Vec<WorkspaceTab> =
                    std::mem::take(&mut *workspace_tabs.lock().unwrap())
                        .into_iter()
                        .filter(|t| t.kind == "sql")
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
                (kept, active)
            };
            *workspace_tabs.lock().unwrap() = init_tabs;
            *active_tab_id.lock().unwrap() = init_active.clone();
            results.lock().unwrap().clear();
            *last_view.lock().unwrap() = None;
            edit_buf.lock().unwrap().clear();
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
                clear_grid(&w, 0);
                sync_query_console(&w, &query_console);
                w.set_selected_conn(idx);
                // Show progress + clear any prior failure immediately.
                w.set_connecting(true);
                w.set_picker_error(SharedString::default());
                w.global::<Theme>()
                    .set_accent(theme::accent_or_default(sc.color.as_deref().unwrap_or("")));
                w.set_status_conn(SharedString::from(sc.name.clone()));
                w.set_bc_conn(SharedString::from(sc.name.clone()));
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
                    let schema = driver.schema().await?;
                    Ok::<_, rdb_core::error::RdbError>((driver, schema))
                };
                let result =
                    match tokio::time::timeout(std::time::Duration::from_secs(15), attempt).await {
                        Ok(r) => r,
                        Err(_) => Err(rdb_core::error::RdbError::Connection(
                            "connection timed out".into(),
                        )),
                    };

                match result {
                    Ok((driver, schema)) => {
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
                        *slot = Some((engine, driver));
                        drop(slot);
                        let nodes = model::to_tree_model(&schema);
                        let fields = model::to_structure_model(&schema);
                        // Stash raw nodes for later expand/collapse rebuilds, and
                        // render the initial view (Functions collapsed, Tables open,
                        // fields hidden). Matches the reseed done on connect above;
                        // collapsed_categories itself is !Send so can't cross here.
                        let rows = schema_display_rows(
                            &nodes,
                            &HashSet::new(),
                            &default_collapsed_cats(),
                            &HashSet::new(),
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
                        // Default to "public" when present, else the first name.
                        let schema_current = schema_names
                            .iter()
                            .find(|s| s.as_str() == "public")
                            .unwrap_or(&schema_names[0])
                            .clone();
                        // SQL editor only makes sense for the SQL engines.
                        let sql_capable = matches!(
                            engine,
                            rdb_connstore::Engine::Postgres
                                | rdb_connstore::Engine::MySql
                                | rdb_connstore::Engine::Sqlite
                                | rdb_connstore::Engine::Cassandra
                        );
                        *raw_nodes.lock().unwrap() = nodes;
                        // Seed autocomplete with the active schema's tables plus a
                        // bare node for every other schema name, so `schema.`
                        // autocompletes immediately. The remaining schemas' tables
                        // and columns fill in from the background load below.
                        let all_schema_names: Vec<String> =
                            schema_names.iter().map(|s| s.to_string()).collect();
                        {
                            let mut seed =
                                model::to_completion_nodes(schema_current.as_str(), &schema);
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
                                w.set_picker_error(SharedString::default());
                                w.set_connecting(false);
                                // Swap the picker for the workspace.
                                w.set_connected(true);
                            }
                        });
                        // Load every other schema's tables so cross-schema
                        // `schema.table` autocompletes. Runs after the sidebar
                        // (active schema) already rendered; the popup just gains
                        // more names as this fills.
                        // ponytail: sequential N+1 fetch, fine for typical schema
                        // counts; make it concurrent if a wide DB feels laggy.
                        if matches!(engine, rdb_connstore::Engine::Postgres)
                            && all_schema_names.len() > 1
                        {
                            let guard = store_driver.lock_owned().await;
                            if let Some((_, driver)) = guard.as_ref() {
                                let mut all = Vec::new();
                                for name in &all_schema_names {
                                    if let Ok(s) = driver.schema_for(name).await {
                                        all.extend(model::to_completion_nodes(name, &s));
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
                                w.set_picker_error(SharedString::from(format!(
                                    "connection failed: {e}"
                                )));
                            }
                        });
                    }
                }
            });
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

    // ----- shared query runner: parse text for the live engine, run it, push
    // the result (grid / documents / error) back to the UI. Used by both the
    // Run button and table clicks. -----
    #[allow(clippy::type_complexity)]
    let run_sql: Rc<dyn Fn(usize, String)> = {
        let weak = window.as_weak();
        let rt = rt.clone();
        let current = current.clone();
        let last_view = last_view.clone();
        let panes = panes.clone();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let active_p1_tab_id = active_p1_tab_id.clone();
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
            let active_id = if pane == 1 {
                &active_p1_tab_id
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
            let active_p1_tab_id = active_p1_tab_id.clone();
            let query_console = query_console.clone();
            // ⌘\ set this; consume it so the next plain run replaces again.
            let new_tab = result_new_tab.swap(false, std::sync::atomic::Ordering::SeqCst);
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
            rt.spawn(async move {
                let guard = current.lock().await;
                let t0 = std::time::Instant::now();
                // Multi-statement: SQL engines split on top-level `;` and run
                // each in order, stopping at the first error. The last result
                // is what the grid shows (TablePlus semantics). Redis/Mongo
                // take the whole text as a single command.
                let (outcome, n_stmts) = match guard.as_ref() {
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
                        // Row-limit control (default 300): cap manual SELECTs so a
                        // huge `SELECT * FROM table` can't stream millions of rows
                        // in and freeze the grid. Browse text already carries its
                        // own LIMIT, so cap_select leaves it untouched.
                        let row_limit = browse.lock().unwrap().limit;
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
                        }
                        (out, n)
                    }
                    None => (
                        Err(rdb_core::error::RdbError::Connection(
                            "not connected".into(),
                        )),
                        1,
                    ),
                };
                let elapsed_ms = t0.elapsed().as_millis().max(1) as u64;
                let view = outcome.as_ref().ok().map(model::to_result_view);
                let err = outcome.err();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak2.upgrade() {
                        sync_query_console(&w, &query_console);
                        let active_id = if pane == 1 {
                            &active_p1_tab_id
                        } else {
                            &active_tab_id
                        };
                        let is_active = active_id.lock().unwrap().as_deref() == Some(&target_id);
                        if is_active {
                            set_p_query_running(&w, pane, false);
                        }
                        match (view, err) {
                            (Some(v), _) => {
                                let shown = match &v {
                                    model::ResultView::Table(g) => g.rows.len(),
                                    model::ResultView::Documents(d) => d.grid.rows.len(),
                                    model::ResultView::Affected(_) => 0,
                                } as u64;
                                let meta = if n_stmts > 1 {
                                    format!("{n_stmts} statements · {shown} rows · {elapsed_ms} ms")
                                } else {
                                    format!("{shown} rows · {elapsed_ms} ms")
                                };
                                let latency = format!("{elapsed_ms} ms");
                                let sr = StoredResult {
                                    view: v.clone(),
                                    meta: meta.clone(),
                                    latency: latency.clone(),
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
                                } else if new_tab || tab.results.is_empty() {
                                    tab.results.push(sr);
                                    tab.active_result = tab.results.len() - 1;
                                } else {
                                    if tab.active_result >= tab.results.len() {
                                        tab.active_result = 0;
                                    }
                                    tab.results[tab.active_result] = sr;
                                }
                                let tab_results = tab.results.clone();
                                let tab_active = tab.active_result;
                                if !is_active {
                                    return;
                                }
                                *results.lock().unwrap() = tab_results;
                                *active_result.lock().unwrap() = tab_active;
                                if is_browse {
                                    set_result_tabs(&w, pane, 0, 0);
                                } else {
                                    set_result_tabs(
                                        &w,
                                        pane,
                                        results.lock().unwrap().len(),
                                        tab_active,
                                    );
                                }
                                present_view(
                                    &w,
                                    pane,
                                    v,
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
                                apply_result(
                                    &w,
                                    pane,
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

    // ----- streaming run ("No limit"): pull rows from the driver in batches and
    // append them to the grid live, so a `SELECT * FROM huge_table` shows up
    // progressively and stays cancelable instead of freezing. The query text
    // reaches the driver verbatim — clean log, no injected LIMIT. A UI-thread
    // timer drains batches into the grid; an off-thread task does the fetch. -----
    let run_stream: Rc<dyn Fn(usize, String)> = {
        let weak = window.as_weak();
        let rt = rt.clone();
        let current = current.clone();
        let query_console = query_console.clone();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let active_p1_tab_id = active_p1_tab_id.clone();
        let panes = panes.clone();
        let last_view = last_view.clone();
        Rc::new(move |pane, sql: String| {
            let Some(w) = weak.upgrade() else {
                return;
            };
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
            let active_id = if pane == 1 {
                &active_p1_tab_id
            } else {
                &active_tab_id
            };
            let Some(target_id) = active_id.lock().unwrap().clone() else {
                return;
            };
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
            w.set_grid_read_only(true);
            clear_grid(&w, pane);
            set_p_result_status(&w, pane, SharedString::from("streaming…"));
            set_p_results_meta(&w, pane, SharedString::default());

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
                let active_p1_tab_id = active_p1_tab_id.clone();
                let stream_timer_stop = stream_timer.clone();
                let target_id = target_id.clone();
                let pane = pane;
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
                                Ok(StreamMsg::Done { capped, elapsed_ms }) => {
                                    let g = accum.borrow().clone();
                                    let n = g.rows.len();
                                    let meta = format!(
                                        "{n} rows{} · {elapsed_ms} ms",
                                        if capped { " (capped)" } else { "" }
                                    );
                                    let latency = format!("{elapsed_ms} ms");
                                    let view = model::ResultView::Table(g);
                                    let sr = StoredResult {
                                        view: view.clone(),
                                        meta: meta.clone(),
                                        latency: latency.clone(),
                                    };
                                    if let Some(tab) = workspace_tabs
                                        .lock()
                                        .unwrap()
                                        .iter_mut()
                                        .find(|t| t.id == target_id)
                                    {
                                        tab.loading = false;
                                        tab.view = Some(sr.clone());
                                        tab.results = vec![sr.clone()];
                                        tab.active_result = 0;
                                    }
                                    let active_id = if pane == 1 {
                                        &active_p1_tab_id
                                    } else {
                                        &active_tab_id
                                    };
                                    if active_id.lock().unwrap().as_deref() == Some(&target_id) {
                                        *results.lock().unwrap() = vec![sr];
                                        *active_result.lock().unwrap() = 0;
                                        set_result_tabs(&w, pane, 1, 0);
                                        present_view(
                                            &w,
                                            pane,
                                            view,
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
                                        &active_p1_tab_id
                                    } else {
                                        &active_tab_id
                                    };
                                    if active_id.lock().unwrap().as_deref() == Some(&target_id) {
                                        apply_result(
                                            &w,
                                            pane,
                                            model::ResultView::Affected(format!("error: {e}")),
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
            // UI channel. Holds the driver lock for the whole stream (like a DB
            // client cursor); Cancel or a new run stops it at the next batch.
            let q = rdb_core::query::Query::Sql(sql);
            let current = current.clone();
            rt.spawn(async move {
                let t0 = std::time::Instant::now();
                let guard = current.lock().await;
                let (ctx, mut crx) = tokio::sync::mpsc::channel::<rdb_core::result::StreamItem>(4);
                let cancel_prod = cancel.clone();
                let producer = async move {
                    match guard.as_ref() {
                        Some((_engine, driver)) => {
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
                    capped
                };
                let (pres, capped) = tokio::join!(producer, consumer);
                let elapsed_ms = t0.elapsed().as_millis().max(1) as u64;
                match pres {
                    Ok(()) => {
                        let _ = ui_tx.send(StreamMsg::Done { capped, elapsed_ms });
                    }
                    Err(e) => {
                        let _ = ui_tx.send(StreamMsg::Err(e.to_string()));
                    }
                }
            });
        })
    };

    // ----- run query (editor) -----
    {
        let weak = window.as_weak();
        let run_sql = run_sql.clone();
        let run_stream = run_stream.clone();
        let cur_engine = cur_engine.clone();
        let browse = browse.clone();
        let ed_state = ed_state.clone();
        let recent_queries = recent_queries.clone();
        let rebuild_query_tree = rebuild_query_tree.clone();
        window.on_run_query(move || {
            if let Some(w) = weak.upgrade() {
                // ⌘⏎ / Run: the highlighted selection, else just the statement
                // under the cursor — so a buffer with several statements runs
                // only the one the user is editing (⌘A then Run for all).
                let text = {
                    let ed = ed_state.borrow();
                    ed.selected_text()
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| ed.current_statement())
                };
                if text.is_empty() {
                    return;
                }
                record_recent(&recent_queries, &text);
                // A manual query result has no row identity — never editable.
                // (The browse path re-enables editing after its PK fetch.)
                if active_tab_kind(&w) != "table" {
                    w.set_grid_read_only(true);
                }
                // "No limit" (browse.limit == 0) on a bare SELECT streams the
                // rows in progressively; everything else runs buffered (capped).
                let stream = active_tab_kind(&w) != "table"
                    && browse.lock().unwrap().limit == 0
                    && cur_engine
                        .borrow()
                        .map(|e| is_bare_select(e, &text))
                        .unwrap_or(false);
                if stream {
                    run_stream(0, text);
                } else {
                    run_sql(0, text);
                }
                if w.get_sidebar_mode() != 0 {
                    rebuild_query_tree("");
                }
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
        let run_p1 = move || {
            let text = {
                let ed = panes[1].ed_state.borrow();
                ed.selected_text()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| ed.current_statement())
            };
            if text.is_empty() {
                return;
            }
            record_recent(&recent_queries, &text);
            let stream = weak
                .upgrade()
                .map(|w| {
                    panes[1].browse.lock().unwrap().limit == 0
                        && cur_engine
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
        window.on_p1_run(run_p1.clone());
        window.on_p1_run_selection(run_p1);
    }

    // ----- Copy results (TSV → clipboard) / Export CSV (~/Downloads) -----
    {
        let weak = window.as_weak();
        let displayed_grid = displayed_grid.clone();
        window.on_copy_results(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let Some(grid) = displayed_grid.lock().unwrap().clone() else {
                return;
            };
            use copypasta::ClipboardProvider;
            let msg = match copypasta::ClipboardContext::new()
                .and_then(|mut cb| cb.set_contents(export::to_tsv(&grid)))
            {
                Ok(()) => format!("copied {} rows", grid.rows.len()),
                Err(e) => format!("copy failed: {e}"),
            };
            w.set_results_meta(SharedString::from(msg));
        });
    }
    {
        let weak = window.as_weak();
        let displayed_grid = displayed_grid.clone();
        window.on_export_results(move |fmt| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let Some(grid) = displayed_grid.lock().unwrap().clone() else {
                w.set_results_meta(SharedString::from("nothing to export"));
                return;
            };
            // format: 0 CSV, 1 JSON, 2 TSV, 3 SQL INSERT, 4 Markdown
            let (ext, filter, contents) = match fmt {
                1 => ("json", "JSON", export::to_json(&grid)),
                2 => ("tsv", "TSV", export::to_tsv(&grid)),
                3 => {
                    let table = w.get_active_table();
                    let table = if table.is_empty() {
                        "results"
                    } else {
                        table.as_str()
                    };
                    ("sql", "SQL", export::to_sql_insert(&grid, table))
                }
                4 => ("md", "Markdown", export::to_markdown(&grid)),
                _ => ("csv", "CSV", export::to_csv(&grid)),
            };
            // Native save dialog via rfd's async API + Slint's local executor:
            // the sync dialog blocks the winit event loop and never surfaces.
            // Parent it to our window so macOS presents the panel as a sheet
            // (an unparented NSSavePanel silently no-ops for a non-foreground
            // process, so nothing appears and nothing is written).
            let parent = w.window().window_handle();
            let weak = w.as_weak();
            let _ = slint::spawn_local(async move {
                let Some(file) = rfd::AsyncFileDialog::new()
                    .set_parent(&parent)
                    .set_file_name(format!("rdb-export.{ext}"))
                    .add_filter(filter, &[ext])
                    .save_file()
                    .await
                else {
                    return; // user cancelled
                };
                let msg = match std::fs::write(file.path(), contents) {
                    Ok(()) => format!("exported → {}", file.path().display()),
                    Err(e) => format!("export failed: {e}"),
                };
                if let Some(w) = weak.upgrade() {
                    w.set_results_meta(SharedString::from(msg));
                }
            });
        });
    }

    // ----- Run Selection: selected text, else statement under the cursor -----
    {
        let weak = window.as_weak();
        let run_sql = run_sql.clone();
        let run_stream = run_stream.clone();
        let cur_engine = cur_engine.clone();
        let browse = browse.clone();
        let ed_state = ed_state.clone();
        let recent_queries = recent_queries.clone();
        window.on_run_selection(move || {
            let stmt = {
                let ed = ed_state.borrow();
                ed.selected_text()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| ed.current_statement())
            };
            if !stmt.is_empty() {
                let mut stream = false;
                if let Some(w) = weak.upgrade() {
                    if active_tab_kind(&w) != "table" {
                        w.set_grid_read_only(true);
                    }
                    stream = active_tab_kind(&w) != "table"
                        && browse.lock().unwrap().limit == 0
                        && cur_engine
                            .borrow()
                            .map(|e| is_bare_select(e, &stmt))
                            .unwrap_or(false);
                }
                record_recent(&recent_queries, &stmt);
                if stream {
                    run_stream(0, stmt);
                } else {
                    run_sql(0, stmt);
                }
            }
        });
    }

    // ----- Cancel a running stream ("No limit" fetch) -----
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        window.on_cancel_stream(move || {
            if let Some(c) = panes[0].stream_cancel.borrow().as_ref() {
                c.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            if let Some(w) = weak.upgrade() {
                set_p_streaming(&w, 0, false);
            }
        });
    }
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        window.on_p1_reorder_col(move |from, local_x| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let from = from.max(0) as usize;
            let mut widths: Vec<f32> = w.get_p1_col_widths().iter().collect();
            if from >= widths.len() {
                return;
            }
            let drop = widths.iter().take(from).sum::<f32>() + local_x;
            let mut acc = 0.0;
            let mut target = widths.len() - 1;
            for (index, width) in widths.iter().enumerate() {
                if drop < acc + width {
                    target = index;
                    break;
                }
                acc += width;
            }
            if target == from {
                return;
            }
            let hidden = panes[1].hidden_cols.lock().unwrap().clone();
            {
                let mut order = panes[1].col_order.lock().unwrap();
                let visible: Vec<usize> = order
                    .iter()
                    .copied()
                    .enumerate()
                    .filter(|(_, index)| !hidden.contains(index))
                    .map(|(position, _)| position)
                    .collect();
                let (Some(&from_position), Some(&to_position)) =
                    (visible.get(from), visible.get(target))
                else {
                    return;
                };
                let value = order.remove(from_position);
                order.insert(
                    if to_position > from_position {
                        to_position - 1
                    } else {
                        to_position
                    },
                    value,
                );
            }
            let width = widths.remove(from);
            widths.insert(if target > from { target - 1 } else { target }, width);
            set_p_col_widths(&w, 1, ModelRc::from(Rc::new(VecModel::from(widths))));
            let view = {
                let results = panes[1].results.lock().unwrap();
                let active = *panes[1].active_result.lock().unwrap();
                results.get(active).map(|result| result.view.clone())
            };
            let Some(view) = view else {
                return;
            };
            let order = panes[1].col_order.lock().unwrap().clone();
            let filters = panes[1].col_filters.lock().unwrap().clone();
            let (sort_col, ascending) = *panes[1].sort_state.lock().unwrap();
            let reordered = compute_view(
                &view, "", "", "", &filters, &hidden, &order, sort_col, ascending,
            );
            *panes[1].displayed_grid.lock().unwrap() = view_grid(&reordered);
            apply_result(&w, 1, reordered);
        });
    }

    // ----- right-pane grid interactions -----
    {
        let weak = window.as_weak();
        window.on_p1_select_cell(move |row, col| {
            if let Some(w) = weak.upgrade() {
                w.set_p1_selected_row(row);
                w.set_p1_selected_col(col);
            }
        });
    }
    {
        let weak = window.as_weak();
        window.on_p1_edit_cell(move |row, col| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if w.get_grid_read_only() || row < 0 || col < 0 {
                set_p_result_status(&w, 1, SharedString::from("read-only result"));
                return;
            }
            let count = w.get_p1_col_count();
            let value = if count > 0 {
                w.get_p1_cells()
                    .row_data((row * count + col) as usize)
                    .filter(|cell| !cell.is_null)
                    .map(|cell| cell.text)
                    .unwrap_or_default()
            } else {
                SharedString::default()
            };
            w.set_p1_editing_value(value);
            set_p_editing(&w, 1, row, col);
        });
    }
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        window.on_p1_stage_cell(move |row, col, text| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if w.get_grid_read_only() || row < 0 || col < 0 {
                return;
            }
            let base = panes[1]
                .displayed_grid
                .lock()
                .unwrap()
                .as_ref()
                .map(|grid| grid.rows.len())
                .unwrap_or(0);
            panes[1].edit_buf.lock().unwrap().set_cell(
                base,
                row as usize,
                col as usize,
                text.to_string(),
            );
            let count = w.get_p1_col_count();
            if count > 0 {
                let index = (row * count + col) as usize;
                let cells = w.get_p1_cells();
                if let Some(mut cell) = cells.row_data(index) {
                    cell.text = text;
                    cell.is_null = false;
                    cell.state = 1;
                    cells.set_row_data(index, cell);
                }
            }
        });
    }
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        window.on_p1_cell_edited(move |row, col, text| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let base = panes[1]
                .displayed_grid
                .lock()
                .unwrap()
                .as_ref()
                .map(|grid| grid.rows.len())
                .unwrap_or(0);
            panes[1].edit_buf.lock().unwrap().set_cell(
                base,
                row.max(0) as usize,
                col.max(0) as usize,
                text.to_string(),
            );
            set_p_editing(&w, 1, -1, -1);
            if let Some(grid) = panes[1].displayed_grid.lock().unwrap().as_ref() {
                let edits = panes[1].edit_buf.lock().unwrap();
                paint_grid_with_edits(&w, 1, grid, &edits);
            }
        });
    }
    {
        let weak = window.as_weak();
        window.on_p1_edit_cancelled(move || {
            if let Some(w) = weak.upgrade() {
                set_p_editing(&w, 1, -1, -1);
            }
        });
    }
    {
        let weak = window.as_weak();
        window.on_p1_cell_advance(move |row, col, text, forward| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let count = w.get_p1_col_count();
            if count <= 0 {
                set_p_editing(&w, 1, -1, -1);
                return;
            }
            w.invoke_p1_cell_edited(row, col, text);
            let next = if forward { col + 1 } else { col - 1 };
            let (next_row, next_col) = if next >= count {
                (row + 1, 0)
            } else if next < 0 {
                (row - 1, count - 1)
            } else {
                (row, next)
            };
            w.set_p1_selected_row(next_row);
            w.set_p1_selected_col(next_col);
            set_p_editing(&w, 1, next_row, next_col);
        });
    }
    {
        let weak = window.as_weak();
        window.on_p1_resize_col(move |i, delta| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let mut widths: Vec<f32> = w.get_p1_col_widths().iter().collect();
            if let Some(width) = widths.get_mut(i.max(0) as usize) {
                *width = (*width + delta).clamp(60.0, 1000.0);
                w.set_p1_col_widths(ModelRc::from(Rc::new(VecModel::from(widths))));
            }
        });
    }
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        window.on_p1_sort_col(move |display_col| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let view = {
                let results = panes[1].results.lock().unwrap();
                let active = *panes[1].active_result.lock().unwrap();
                results.get(active).map(|result| result.view.clone())
            };
            let Some(view) = view else {
                return;
            };
            let hidden = panes[1].hidden_cols.lock().unwrap().clone();
            let order = panes[1].col_order.lock().unwrap().clone();
            let visible: Vec<usize> = order
                .iter()
                .copied()
                .filter(|index| !hidden.contains(index))
                .collect();
            let Some(&original) = visible.get(display_col.max(0) as usize) else {
                return;
            };
            let (sort_col, ascending) = {
                let mut sort = panes[1].sort_state.lock().unwrap();
                if sort.0 == original as i32 {
                    sort.1 = !sort.1;
                } else {
                    *sort = (original as i32, true);
                }
                *sort
            };
            let filters = panes[1].col_filters.lock().unwrap().clone();
            let sorted = compute_view(
                &view, "", "", "", &filters, &hidden, &order, sort_col, ascending,
            );
            *panes[1].displayed_grid.lock().unwrap() = view_grid(&sorted);
            set_p_grid_sort(&w, 1, display_col, ascending);
            apply_result(&w, 1, sorted);
        });
    }
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        window.on_p1_set_col_filter(move |display_col, text| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let view = {
                let results = panes[1].results.lock().unwrap();
                let active = *panes[1].active_result.lock().unwrap();
                results.get(active).map(|result| result.view.clone())
            };
            let Some(view) = view else {
                return;
            };
            let hidden = panes[1].hidden_cols.lock().unwrap().clone();
            let order = panes[1].col_order.lock().unwrap().clone();
            let visible: Vec<usize> = order
                .iter()
                .copied()
                .filter(|index| !hidden.contains(index))
                .collect();
            let Some(&original) = visible.get(display_col.max(0) as usize) else {
                return;
            };
            let filters = {
                let mut filters = panes[1].col_filters.lock().unwrap();
                if let Some(filter) = filters.get_mut(original) {
                    *filter = text.to_string();
                }
                filters.clone()
            };
            let (sort_col, ascending) = *panes[1].sort_state.lock().unwrap();
            let filtered = compute_view(
                &view, "", "", "", &filters, &hidden, &order, sort_col, ascending,
            );
            *panes[1].displayed_grid.lock().unwrap() = view_grid(&filtered);
            apply_result(&w, 1, filtered);
        });
    }
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        window.on_p1_cancel_stream(move || {
            if let Some(c) = panes[1].stream_cancel.borrow().as_ref() {
                c.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            if let Some(w) = weak.upgrade() {
                set_p_streaming(&w, 1, false);
            }
        });
    }

    // ----- Explain button: run EXPLAIN for the editor SQL -----
    {
        let weak = window.as_weak();
        let run_sql = run_sql.clone();
        window.on_explain_query(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if !w.get_sql_capable() {
                return;
            }
            let text = w.get_query_text().to_string();
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
            let text = panes[1].ed_state.borrow().text();
            let trimmed = text.trim();
            if trimmed.is_empty() {
                return;
            }
            w.set_grid_read_only(true);
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
        let weak = window.as_weak();
        let load_editor_text = load_editor_text.clone();
        window.on_format_sql(move || {
            if let Some(w) = weak.upgrade() {
                let text = w.get_query_text().to_string();
                if !text.trim().is_empty() {
                    load_editor_text(0, &sql_format::format(&text));
                }
            }
        });
    }
    {
        let load_editor_text = load_editor_text.clone();
        let panes = panes.clone();
        window.on_p1_format_sql(move || {
            let text = panes[1].ed_state.borrow().text();
            if !text.trim().is_empty() {
                load_editor_text(1, &sql_format::format(&text));
            }
        });
    }

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
    let guard_pending: Rc<dyn Fn(&MainWindow) -> bool> = {
        let edit_buf = edit_buf.clone();
        Rc::new(move |w: &MainWindow| guard_pending_edits(w, &edit_buf))
    };

    let run_browse: Rc<dyn Fn()> = {
        let cur_engine = cur_engine.clone();
        let browse = browse.clone();
        let run_sql = run_sql.clone();
        let load_editor_text = load_editor_text.clone();
        Rc::new(move || {
            let Some(engine) = *cur_engine.borrow() else {
                return;
            };
            let st = browse.lock().unwrap().clone();
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
            load_editor_text(0, &text);
            run_sql(0, text);
        })
    };
    // Wire the deferred handle so per-column browse filters can re-query.
    *browse_trigger.borrow_mut() = Some(run_browse.clone());

    {
        let weak = window.as_weak();
        let cur_engine = cur_engine.clone();
        let browse = browse.clone();
        let current = current.clone();
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
            w.set_active_table(SharedString::from(label));
            w.set_show_structure(false);
            w.set_total_rows(-1);
            w.set_grid_read_only(true);
            w.set_index_rows(ModelRc::from(Rc::new(VecModel::<IndexRow>::default())));
            *last_view.lock().unwrap() = None;
            *displayed_grid.lock().unwrap() = None;
            results.lock().unwrap().clear();
            set_result_tabs(&w, 0, 0, 0);
            clear_grid(&w, 0);
            run_browse();

            // Fetch total + primary key off-thread; footer updates when done.
            let weak2 = weak.clone();
            let current = current.clone();
            let browse = browse.clone();
            let edit_buf = edit_buf.clone();
            let workspace_tabs = workspace_tabs.clone();
            let active_tab_id = active_tab_id.clone();
            let query_console = query_console.clone();
            rt.spawn(async move {
                let guard = current.lock().await;
                let Some((engine, driver)) = guard.as_ref() else {
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

    {
        let weak = window.as_weak();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        window.on_pin_tab(move |index| {
            let active = active_tab_id.lock().unwrap().clone();
            let mut tabs = workspace_tabs.lock().unwrap();
            let index = tabs
                .iter()
                .enumerate()
                .filter(|(_, tab)| tab.group == 0)
                .nth(index.max(0) as usize)
                .map(|(index, _)| index);
            if let Some(tab) = index.and_then(|index| tabs.get_mut(index)) {
                tab.pinned = true;
                if let Some(w) = weak.upgrade() {
                    set_workspace_tabs(&w, &tabs, active.as_deref());
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
            run_browse();
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
            run_browse();
        });
    }
    {
        let weak = window.as_weak();
        let browse = browse.clone();
        let current = current.clone();
        let rt = rt.clone();
        let run_browse = run_browse.clone();
        let guard_pending = guard_pending.clone();
        window.on_refresh_page(move || {
            if weak.upgrade().is_some_and(|w| guard_pending(&w)) {
                return;
            }
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
                run_browse();
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
            run_browse();
        });
    }

    {
        let panes = panes.clone();
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
            panes[1].browse.lock().unwrap().limit = limit;
        });
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
            let raw = w.get_mongo_filter().to_string();
            let trimmed = raw.trim();
            // Empty clears the filter; otherwise it must be a JSON document.
            if !trimmed.is_empty() {
                if let Err(e) = serde_json::from_str::<serde_json::Value>(trimmed) {
                    w.set_status_error(true);
                    w.set_result_status(SharedString::from(format!("invalid filter JSON: {e}")));
                    return;
                }
            }
            {
                let mut st = browse.lock().unwrap();
                st.mongo_filter = trimmed.to_string();
                st.page = 0;
            }
            run_browse();
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
        window.on_toggle_schema_node(move |label| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let engine = *cur_engine.borrow();
            let label = label.to_string();

            // Mongo/Redis/Cassandra: database (or keyspace) headers open an
            // opt-in set (default closed) and load their leaves (collections /
            // keys / tables) lazily on first expand.
            if matches!(
                engine,
                Some(rdb_connstore::Engine::Mongo)
                    | Some(rdb_connstore::Engine::Redis)
                    | Some(rdb_connstore::Engine::Cassandra)
            ) {
                let leaf_kind = match engine {
                    Some(rdb_connstore::Engine::Redis) => "key",
                    Some(rdb_connstore::Engine::Cassandra) => "table",
                    _ => "collection",
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
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let current_connection_id = current_connection_id.clone();
        let query_number = query_number.clone();
        let save_active_tab = save_active_tab.clone();
        let results = results.clone();
        let active_result = active_result.clone();
        let browse = browse.clone();
        let last_view = last_view.clone();
        let load_editor_text = load_editor_text.clone();
        window.on_new_tab(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            save_active_tab(&w);
            let number = query_number.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            let connection = current_connection_id
                .lock()
                .unwrap()
                .clone()
                .unwrap_or_default();
            let id = format!("query:{connection}:{number}");
            let mut tabs = workspace_tabs.lock().unwrap();
            tabs.push(WorkspaceTab::sql(id.clone(), number));
            *active_tab_id.lock().unwrap() = Some(id.clone());
            set_workspace_tabs(&w, &tabs, Some(&id));
            save_query_tabs(&tabs, Some(&id));
            drop(tabs);
            load_editor_text(0, "");
            // Fresh query tab starts with no result tabs.
            results.lock().unwrap().clear();
            *active_result.lock().unwrap() = 0;
            let limit = browse.lock().unwrap().limit;
            *browse.lock().unwrap() = BrowseState {
                limit,
                ..Default::default()
            };
            *last_view.lock().unwrap() = None;
            set_result_tabs(&w, 0, 0, 0);
            clear_grid(&w, 0);
            w.set_active_pane(0);
            w.set_active_table(SharedString::default());
            w.set_fn_mode(false);
            w.set_query_running(false);
            w.set_results_meta(SharedString::default());
        });
    }

    // ----- ⌘\: run the current statement into a NEW result tab -----
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        let run_sql = run_sql.clone();
        let recent_queries = recent_queries.clone();
        window.on_run_new_tab(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let pane = w.get_active_pane().clamp(0, 1) as usize;
            let stmt = {
                let ed = panes[pane].ed_state.borrow();
                ed.selected_text()
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .unwrap_or_else(|| ed.current_statement())
            };
            if stmt.is_empty() {
                return;
            }
            // Same editor; the run lands in an appended result tab.
            panes[pane]
                .result_new_tab
                .store(true, std::sync::atomic::Ordering::SeqCst);
            if pane == 0 {
                w.set_grid_read_only(true);
            }
            record_recent(&recent_queries, &stmt);
            run_sql(pane, stmt);
        });
    }

    // ----- switch / close result tabs -----
    {
        let weak = window.as_weak();
        let results = results.clone();
        let active_result = active_result.clone();
        let last_view = last_view.clone();
        let displayed_grid = displayed_grid.clone();
        let hidden_cols = hidden_cols.clone();
        let sort_state = sort_state.clone();
        let col_order = col_order.clone();
        let col_filters = col_filters.clone();
        let edit_buf = edit_buf.clone();
        let browse = browse.clone();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        window.on_select_result_tab(move |i| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let i = i.max(0) as usize;
            let sr = results.lock().unwrap().get(i).cloned();
            let Some(sr) = sr else {
                return;
            };
            *active_result.lock().unwrap() = i;
            if let Some(id) = active_tab_id.lock().unwrap().clone() {
                if let Some(tab) = workspace_tabs
                    .lock()
                    .unwrap()
                    .iter_mut()
                    .find(|tab| tab.id == id)
                {
                    tab.active_result = i;
                }
            }
            w.set_active_result(i as i32);
            present_view(
                &w,
                0,
                sr.view,
                &sr.meta,
                &sr.latency,
                &last_view,
                &displayed_grid,
                &hidden_cols,
                &sort_state,
                &col_order,
                &col_filters,
                &edit_buf,
                &browse,
            );
        });
    }
    {
        let weak = window.as_weak();
        let results = results.clone();
        let active_result = active_result.clone();
        let last_view = last_view.clone();
        let displayed_grid = displayed_grid.clone();
        let hidden_cols = hidden_cols.clone();
        let sort_state = sort_state.clone();
        let col_order = col_order.clone();
        let col_filters = col_filters.clone();
        let edit_buf = edit_buf.clone();
        let browse = browse.clone();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        window.on_close_result_tab(move |i| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let i = i.max(0) as usize;
            let (count, active, sr) = {
                let mut rv = results.lock().unwrap();
                if i >= rv.len() {
                    return;
                }
                rv.remove(i);
                let mut ar = active_result.lock().unwrap();
                if *ar >= rv.len() {
                    *ar = rv.len().saturating_sub(1);
                }
                (rv.len(), *ar, rv.get(*ar).cloned())
            };
            set_result_tabs(&w, 0, count, active);
            if let Some(id) = active_tab_id.lock().unwrap().clone() {
                if let Some(tab) = workspace_tabs
                    .lock()
                    .unwrap()
                    .iter_mut()
                    .find(|tab| tab.id == id)
                {
                    tab.results = results.lock().unwrap().clone();
                    tab.active_result = active;
                }
            }
            match sr {
                Some(sr) => present_view(
                    &w,
                    0,
                    sr.view,
                    &sr.meta,
                    &sr.latency,
                    &last_view,
                    &displayed_grid,
                    &hidden_cols,
                    &sort_state,
                    &col_order,
                    &col_filters,
                    &edit_buf,
                    &browse,
                ),
                None => {
                    clear_grid(&w, 0);
                    *last_view.lock().unwrap() = None;
                }
            }
        });
    }

    {
        let weak = window.as_weak();
        let panes = panes.clone();
        let last_view = last_view.clone();
        let workspace_tabs = workspace_tabs.clone();
        let active_p1_tab_id = active_p1_tab_id.clone();
        window.on_p1_select_result_tab(move |i| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let i = i.max(0) as usize;
            let sr = panes[1].results.lock().unwrap().get(i).cloned();
            let Some(sr) = sr else {
                return;
            };
            *panes[1].active_result.lock().unwrap() = i;
            if let Some(id) = active_p1_tab_id.lock().unwrap().clone() {
                if let Some(tab) = workspace_tabs
                    .lock()
                    .unwrap()
                    .iter_mut()
                    .find(|tab| tab.id == id)
                {
                    tab.active_result = i;
                }
            }
            w.set_p1_active_result(i as i32);
            present_view(
                &w,
                1,
                sr.view,
                &sr.meta,
                &sr.latency,
                &last_view,
                &panes[1].displayed_grid,
                &panes[1].hidden_cols,
                &panes[1].sort_state,
                &panes[1].col_order,
                &panes[1].col_filters,
                &panes[1].edit_buf,
                &panes[1].browse,
            );
        });
    }
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        let last_view = last_view.clone();
        let workspace_tabs = workspace_tabs.clone();
        let active_p1_tab_id = active_p1_tab_id.clone();
        window.on_p1_close_result_tab(move |i| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let i = i.max(0) as usize;
            let (count, active, sr) = {
                let mut rv = panes[1].results.lock().unwrap();
                if i >= rv.len() {
                    return;
                }
                rv.remove(i);
                let mut ar = panes[1].active_result.lock().unwrap();
                if *ar >= rv.len() {
                    *ar = rv.len().saturating_sub(1);
                }
                (rv.len(), *ar, rv.get(*ar).cloned())
            };
            set_result_tabs(&w, 1, count, active);
            if let Some(id) = active_p1_tab_id.lock().unwrap().clone() {
                if let Some(tab) = workspace_tabs
                    .lock()
                    .unwrap()
                    .iter_mut()
                    .find(|tab| tab.id == id)
                {
                    tab.results = panes[1].results.lock().unwrap().clone();
                    tab.active_result = active;
                }
            }
            match sr {
                Some(sr) => present_view(
                    &w,
                    1,
                    sr.view,
                    &sr.meta,
                    &sr.latency,
                    &last_view,
                    &panes[1].displayed_grid,
                    &panes[1].hidden_cols,
                    &panes[1].sort_state,
                    &panes[1].col_order,
                    &panes[1].col_filters,
                    &panes[1].edit_buf,
                    &panes[1].browse,
                ),
                None => {
                    clear_grid(&w, 1);
                    set_p_results_meta(&w, 1, SharedString::default());
                }
            }
        });
    }

    // ----- close tab -----
    {
        let weak = window.as_weak();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let save_active_tab = save_active_tab.clone();
        let restore_tab = restore_tab.clone();
        let load_editor_text = load_editor_text.clone();
        let browse = browse.clone();
        let results = results.clone();
        let last_view = last_view.clone();
        let displayed_grid = displayed_grid.clone();
        let guard_pending = guard_pending.clone();
        window.on_close_tab(move |requested| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if guard_pending(&w) {
                return;
            }
            save_active_tab(&w);
            let mut tabs = workspace_tabs.lock().unwrap();
            if tabs.is_empty() {
                return;
            }
            let remove_at = if requested >= 0 {
                tabs.iter()
                    .enumerate()
                    .filter(|(_, tab)| tab.group == 0)
                    .nth(requested as usize)
                    .map(|(index, _)| index)
                    .unwrap_or(tabs.len())
            } else {
                workspace_tab_index(&tabs, active_tab_id.lock().unwrap().as_deref()).unwrap_or(0)
            };
            if remove_at >= tabs.len() {
                return;
            }
            if tabs.iter().filter(|tab| tab.group == 0).count() == 1
                && tabs.iter().any(|tab| tab.group == 1)
            {
                return;
            }
            let removed_active =
                active_tab_id.lock().unwrap().as_deref() == Some(&tabs[remove_at].id);
            tabs.remove(remove_at);
            if tabs.is_empty() {
                *active_tab_id.lock().unwrap() = None;
                set_workspace_tabs(&w, &tabs, None);
                save_query_tabs(&tabs, None);
                drop(tabs);
                load_editor_text(0, "");
                let limit = browse.lock().unwrap().limit;
                *browse.lock().unwrap() = BrowseState {
                    limit,
                    ..Default::default()
                };
                results.lock().unwrap().clear();
                *last_view.lock().unwrap() = None;
                *displayed_grid.lock().unwrap() = None;
                clear_grid(&w, 0);
                w.set_active_table(SharedString::default());
                w.set_fn_mode(false);
                w.set_query_running(false);
                w.set_results_meta(SharedString::default());
                return;
            }
            if removed_active {
                let next = tabs
                    .iter()
                    .enumerate()
                    .filter(|(_, tab)| tab.group == 0)
                    .nth(requested.max(0) as usize)
                    .map(|(index, _)| index)
                    .or_else(|| {
                        tabs.iter()
                            .enumerate()
                            .find(|(_, tab)| tab.group == 0)
                            .map(|(index, _)| index)
                    });
                drop(tabs);
                if let Some(next) = next {
                    restore_tab(&w, next);
                }
            } else {
                let active = active_tab_id.lock().unwrap().clone();
                set_workspace_tabs(&w, &tabs, active.as_deref());
            }
            save_query_tabs(
                &workspace_tabs.lock().unwrap(),
                active_tab_id.lock().unwrap().as_deref(),
            );
        });
    }

    // Closing a right-group tab uses the right group's filtered index. When its
    // last tab closes, `set_workspace_tabs` removes the whole second group.
    {
        let weak = window.as_weak();
        let workspace_tabs = workspace_tabs.clone();
        let active_p1_tab_id = active_p1_tab_id.clone();
        let save_p1_tab = save_p1_tab.clone();
        let restore_p1_tab = restore_p1_tab.clone();
        let active_tab_id = active_tab_id.clone();
        window.on_close_p1_tab(move |requested| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if requested < 0 {
                return;
            }
            save_p1_tab(&w);
            let remove_at = {
                let tabs = workspace_tabs.lock().unwrap();
                tabs.iter()
                    .enumerate()
                    .filter(|(_, tab)| tab.group == 1)
                    .nth(requested as usize)
                    .map(|(index, _)| index)
            };
            let Some(remove_at) = remove_at else {
                return;
            };
            let mut tabs = workspace_tabs.lock().unwrap();
            tabs.remove(remove_at);
            let remaining = tabs.iter().filter(|tab| tab.group == 1).count();
            let active = active_tab_id.lock().unwrap().clone();
            set_workspace_tabs(&w, &tabs, active.as_deref());
            save_query_tabs(&tabs, active.as_deref());
            drop(tabs);
            if remaining == 0 {
                *active_p1_tab_id.lock().unwrap() = None;
                load_editor_text(1, "");
                clear_grid(&w, 1);
            } else {
                restore_p1_tab(&w, requested.min((remaining - 1) as i32) as usize);
            }
        });
    }

    // ----- rename a query tab (double-click opens a modal) -----
    {
        let weak = window.as_weak();
        window.on_open_rename(move |idx, title| {
            if let Some(w) = weak.upgrade() {
                w.set_rename_target(idx);
                w.set_rename_group(0);
                w.set_rename_text(title);
                w.set_rename_modal_open(true);
            }
        });
    }
    {
        let weak = window.as_weak();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        window.on_rename_commit(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let i = w.get_rename_target().max(0) as usize;
            let group = w.get_rename_group().clamp(0, 1) as usize;
            let name = w.get_rename_text().trim().to_string();
            let mut tabs = workspace_tabs.lock().unwrap();
            let index = tabs
                .iter()
                .enumerate()
                .filter(|(_, tab)| tab.group == group)
                .nth(i)
                .map(|(index, _)| index);
            if let Some(tab) = index.and_then(|index| tabs.get_mut(index)) {
                if !name.is_empty() {
                    tab.title = name;
                }
            }
            let active = active_tab_id.lock().unwrap().clone();
            set_workspace_tabs(&w, &tabs, active.as_deref());
            save_query_tabs(&tabs, active.as_deref());
        });
    }

    // ----- select tab -----
    {
        let weak = window.as_weak();
        let workspace_tabs = workspace_tabs.clone();
        let save_active_tab = save_active_tab.clone();
        let restore_tab = restore_tab.clone();
        let guard_pending = guard_pending.clone();
        window.on_select_tab(move |idx| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if idx < 0 {
                return;
            }
            if guard_pending(&w) {
                return;
            }
            save_active_tab(&w);
            let index = workspace_tabs
                .lock()
                .unwrap()
                .iter()
                .enumerate()
                .filter(|(_, tab)| tab.group == 0)
                .nth(idx as usize)
                .map(|(index, _)| index);
            if let Some(index) = index {
                restore_tab(&w, index);
            }
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

    // Repaint the grid with the buffer's pending edits overlaid, and refresh
    // the footer badge. UI-thread helper shared by every edit handler.
    let repaint_edits: Rc<dyn Fn(&MainWindow)> = {
        let edit_buf = edit_buf.clone();
        let displayed_grid = displayed_grid.clone();
        Rc::new(move |w: &MainWindow| {
            let buf = edit_buf.lock().unwrap();
            if let Some(g) = displayed_grid.lock().unwrap().as_ref() {
                paint_grid_with_edits(w, 0, g, &buf);
            }
            w.set_pending_count(buf.pending_count() as i32);
        })
    };

    // Detail-panel edits join the same buffer as inline grid edits. Avoid a
    // full-grid repaint per keystroke; selection and commit repaint naturally.
    {
        let weak = window.as_weak();
        let edit_buf = edit_buf.clone();
        let displayed_grid = displayed_grid.clone();
        window.on_stage_cell(move |r, c, text| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if w.get_grid_read_only() || r < 0 || c < 0 {
                return;
            }
            let base = displayed_grid
                .lock()
                .unwrap()
                .as_ref()
                .map(|g| g.rows.len())
                .unwrap_or(0);
            let pending = {
                let mut buf = edit_buf.lock().unwrap();
                buf.set_cell(base, r as usize, c as usize, text.to_string());
                buf.pending_count()
            };
            let cols = w.get_grid_col_count();
            if cols > 0 {
                let idx = (r * cols + c) as usize;
                let cells = w.get_grid_cells();
                if let Some(mut cell) = cells.row_data(idx) {
                    cell.text = text;
                    cell.is_null = false;
                    if cell.state != 3 {
                        cell.state = 1;
                    }
                    cells.set_row_data(idx, cell);
                }
            }
            w.set_pending_count(pending as i32);
        });
    }

    // ----- inline editing: open editor on double-click -----
    {
        let weak = window.as_weak();
        let displayed_grid = displayed_grid.clone();
        window.on_edit_cell(move |r, c| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            // Editable only in a tabular browse view with a known identity.
            if w.get_grid_read_only() || w.get_show_structure() || w.get_result_kind() == 3 {
                // Tell the user why the double-click did nothing.
                if w.get_grid_read_only() && !w.get_show_structure() && w.get_result_kind() == 0 {
                    let msg = if active_tab_kind(&w) == "table" {
                        "read-only — table has no primary key"
                    } else {
                        "read-only result — open the table from the sidebar to edit"
                    };
                    w.set_status_error(false);
                    w.set_result_status(SharedString::from(msg));
                }
                return;
            }
            let cols = w.get_grid_col_count();
            let value = if r >= 0 && c >= 0 && cols > 0 {
                w.get_grid_cells()
                    .row_data((r * cols + c) as usize)
                    .filter(|cell| !cell.is_null)
                    .map(|cell| cell.text)
                    .unwrap_or_default()
            } else {
                SharedString::default()
            };
            w.set_editing_value(value);
            w.set_editing_row(r);
            w.set_editing_col(c);
            let base = displayed_grid
                .lock()
                .unwrap()
                .as_ref()
                .map(|g| g.rows.len())
                .unwrap_or(0);
            if r >= base as i32 {
                w.set_scroll_grid_tick(w.get_scroll_grid_tick() + 1);
            }
        });
    }

    // ----- inline editing: Enter confirms the cell into the buffer -----
    {
        let weak = window.as_weak();
        let edit_buf = edit_buf.clone();
        let displayed_grid = displayed_grid.clone();
        let repaint_edits = repaint_edits.clone();
        window.on_cell_edited(move |r, c, text| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            w.set_editing_row(-1);
            w.set_editing_col(-1);
            let base = displayed_grid
                .lock()
                .unwrap()
                .as_ref()
                .map(|g| g.rows.len())
                .unwrap_or(0);
            {
                let mut buf = edit_buf.lock().unwrap();
                buf.set_cell(base, r as usize, c as usize, text.to_string());
            }
            repaint_edits(&w);
            if r as usize >= base {
                w.set_scroll_grid_tick(w.get_scroll_grid_tick() + 1);
            }
        });
    }

    {
        let weak = window.as_weak();
        window.on_edit_cancelled(move || {
            if let Some(w) = weak.upgrade() {
                w.set_editing_row(-1);
                w.set_editing_col(-1);
            }
        });
    }

    // ----- inline editing: Tab stores the cell and edits the neighbour -----
    {
        let weak = window.as_weak();
        let edit_buf = edit_buf.clone();
        let displayed_grid = displayed_grid.clone();
        let repaint_edits = repaint_edits.clone();
        window.on_cell_advance(move |r, c, text, forward| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let (base, ncols) = {
                let dg = displayed_grid.lock().unwrap();
                let Some(g) = dg.as_ref() else {
                    return;
                };
                (g.rows.len(), g.columns.len())
            };
            if ncols == 0 {
                return;
            }
            let nrows = base + edit_buf.lock().unwrap().inserts.len();
            {
                edit_buf
                    .lock()
                    .unwrap()
                    .set_cell(base, r as usize, c as usize, text.to_string());
            }
            // neighbour cell, wrapping across row ends (TablePlus-style)
            let (mut nr, mut nc) = (r, c);
            if forward {
                nc += 1;
                if nc >= ncols as i32 {
                    nc = 0;
                    nr += 1;
                }
            } else {
                nc -= 1;
                if nc < 0 {
                    nc = ncols as i32 - 1;
                    nr -= 1;
                }
            }
            repaint_edits(&w);
            if nr < 0 || nr >= nrows as i32 {
                // ran off the grid: close the editor, keep the stored value
                w.set_editing_row(-1);
                w.set_editing_col(-1);
                return;
            }
            w.set_selected_row(nr);
            w.set_selected_col(nc);
            w.set_editing_row(nr);
            w.set_editing_col(nc);
            if nr as usize >= base {
                w.set_scroll_grid_tick(w.get_scroll_grid_tick() + 1);
            }
        });
    }

    // ----- footer: + appends a pending insert row and starts editing it -----
    {
        let weak = window.as_weak();
        let edit_buf = edit_buf.clone();
        let displayed_grid = displayed_grid.clone();
        let repaint_edits = repaint_edits.clone();
        window.on_add_row(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if w.get_grid_read_only() {
                return;
            }
            let dg = displayed_grid.lock().unwrap();
            let Some(g) = dg.as_ref() else {
                return;
            };
            let (base, ncols) = (g.rows.len(), g.columns.len());
            drop(dg);
            let row_idx = {
                let mut buf = edit_buf.lock().unwrap();
                buf.inserts.push(vec![String::new(); ncols]);
                base + buf.inserts.len() - 1
            };
            repaint_edits(&w);
            w.set_selected_row(row_idx as i32);
            w.set_editing_row(row_idx as i32);
            w.set_editing_col(0);
            w.set_scroll_grid_tick(w.get_scroll_grid_tick() + 1);
        });
    }

    // ----- footer: − toggles delete on the selected row -----
    {
        let weak = window.as_weak();
        let edit_buf = edit_buf.clone();
        let displayed_grid = displayed_grid.clone();
        let repaint_edits = repaint_edits.clone();
        window.on_mark_delete(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if w.get_grid_read_only() {
                return;
            }
            let r = w.get_selected_row();
            if r < 0 {
                return;
            }
            let r = r as usize;
            let base = displayed_grid
                .lock()
                .unwrap()
                .as_ref()
                .map(|g| g.rows.len())
                .unwrap_or(0);
            {
                let mut buf = edit_buf.lock().unwrap();
                if r >= base {
                    // deleting a pending insert just removes it
                    let i = r - base;
                    if i < buf.inserts.len() {
                        buf.inserts.remove(i);
                    }
                } else if !buf.deletes.remove(&r) {
                    buf.deletes.insert(r);
                }
            }
            repaint_edits(&w);
        });
    }

    // ----- discard: drop the buffer, restore the fetched page -----
    {
        let weak = window.as_weak();
        let edit_buf = edit_buf.clone();
        let repaint_edits = repaint_edits.clone();
        window.on_discard_edits(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            edit_buf.lock().unwrap().clear();
            w.set_editing_row(-1);
            w.set_editing_col(-1);
            w.set_status_error(false);
            repaint_edits(&w);
        });
    }

    // ----- ⌘S: turn the buffer into WriteOps and commit them -----
    {
        let weak = window.as_weak();
        let edit_buf = edit_buf.clone();
        let displayed_grid = displayed_grid.clone();
        let current = current.clone();
        let rt = rt.clone();
        let commit_buf = edit_buf.clone();
        let cur_engine = cur_engine.clone();
        let query_console = query_console.clone();
        let repaint_edits = repaint_edits.clone();
        window.on_commit_edits(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if w.get_editing_row() >= 0 && w.get_editing_col() >= 0 {
                let base = displayed_grid
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map(|g| g.rows.len())
                    .unwrap_or(0);
                edit_buf.lock().unwrap().set_cell(
                    base,
                    w.get_editing_row() as usize,
                    w.get_editing_col() as usize,
                    w.get_editing_value().to_string(),
                );
                w.set_editing_row(-1);
                w.set_editing_col(-1);
                repaint_edits(&w);
            }
            let ops = {
                let buf = edit_buf.lock().unwrap();
                if buf.is_empty() {
                    return;
                }
                let dg = displayed_grid.lock().unwrap();
                let Some(g) = dg.as_ref() else {
                    return;
                };
                buf.to_ops(g)
            };
            let ops = match ops {
                Ok(ops) if !ops.is_empty() => ops,
                Ok(_) => return,
                Err(msg) => {
                    w.set_status_error(true);
                    w.set_result_status(SharedString::from(format!("error: {msg}")));
                    return;
                }
            };
            let weak2 = weak.clone();
            let current = current.clone();
            let commit_buf = commit_buf.clone();
            if let Some(engine) = *cur_engine.borrow() {
                for statement in dispatch::write_statements(engine, &ops) {
                    append_query_console(&query_console, statement);
                }
                sync_query_console(&w, &query_console);
            }
            let query_console = query_console.clone();
            if let Some(w) = weak.upgrade() {
                w.set_status_error(false);
                w.set_result_status(SharedString::from("saving…"));
            }
            rt.spawn(async move {
                let outcome = {
                    let guard = current.lock().await;
                    match guard.as_ref() {
                        Some((_, driver)) => driver.commit(&ops).await,
                        None => Err(rdb_core::error::RdbError::Connection(
                            "not connected".into(),
                        )),
                    }
                };
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(w) = weak2.upgrade() else {
                        return;
                    };
                    sync_query_console(&w, &query_console);
                    match outcome {
                        Ok(n) => {
                            // Written: drop the buffer BEFORE the refresh so
                            // the pending-edits guard lets the refetch through.
                            commit_buf.lock().unwrap().clear();
                            w.set_pending_count(0);
                            w.set_status_error(false);
                            w.set_result_status(SharedString::from(format!("{n} rows written")));
                            w.invoke_refresh_page();
                        }
                        Err(e) => {
                            // Keep the buffer so nothing typed is lost.
                            w.set_status_error(true);
                            w.set_result_status(SharedString::from(format!("error: {e}")));
                        }
                    }
                });
            });
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
                    let names: Vec<(String, &'static str)> = store
                        .borrow()
                        .list()
                        .iter()
                        .map(|s| (s.name.clone(), AnyDriver::badge(s.engine)))
                        .collect();
                    let mut items: Vec<PaletteItem> = names
                        .iter()
                        .map(|(n, badge)| PaletteItem {
                            label: n.clone().into(),
                            kind: (*badge).into(),
                            sub: SharedString::default(),
                            local: false,
                        })
                        .collect();
                    for n in w.get_schema_tree().iter().filter(|n| n.kind == "table") {
                        items.push(PaletteItem {
                            label: n.label.clone(),
                            kind: "table".into(),
                            sub: SharedString::default(),
                            local: false,
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
                let names: Vec<(String, &'static str)> = store
                    .borrow()
                    .list()
                    .iter()
                    .map(|s| (s.name.clone(), AnyDriver::badge(s.engine)))
                    .collect();
                let mut items: Vec<PaletteItem> = names
                    .iter()
                    .filter(|(n, _)| n.to_lowercase().contains(&needle))
                    .map(|(n, badge)| PaletteItem {
                        label: n.clone().into(),
                        kind: (*badge).into(),
                        sub: SharedString::default(),
                        local: false,
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
                        sub: SharedString::default(),
                        local: false,
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
        let settings = settings.clone();
        window.on_toggle_theme(move || {
            if let Some(w) = weak.upgrade() {
                let t = w.global::<Theme>();
                let now = t.get_dark();
                t.set_dark(!now);
                let _ = settings
                    .borrow_mut()
                    .update(|s| s.theme = rdb_connstore::ThemeMode::from_dark(!now));
            }
        });
    }

    // ----- settings: check-for-updates toggle -----
    {
        let weak = window.as_weak();
        let settings = settings.clone();
        window.on_set_update_check(move |v| {
            let _ = settings.borrow_mut().update(|s| s.update_check = v);
            if let Some(w) = weak.upgrade() {
                w.set_update_check_enabled(v);
            }
        });
    }

    // ----- settings: sidebar-on-the-right toggle -----
    {
        let settings = settings.clone();
        window.on_set_sidebar_side(move |v| {
            let _ = settings
                .borrow_mut()
                .update(|s| s.ui_state.sidebar_right = v);
        });
    }

    // ----- connection form (add / edit / delete) -----
    fn default_port(engine_label: &str) -> &'static str {
        match engine_label {
            "MySQL" => "3306",
            "Redis" => "6379",
            "MongoDB" => "27017",
            "SQLite" => "0", // file-based: port unused
            "Cassandra" => "9042",
            _ => "5432",
        }
    }
    fn label_to_engine(label: &str) -> rdb_connstore::Engine {
        match label {
            "MySQL" => rdb_connstore::Engine::MySql,
            "Redis" => rdb_connstore::Engine::Redis,
            "MongoDB" => rdb_connstore::Engine::Mongo,
            "SQLite" => rdb_connstore::Engine::Sqlite,
            "Cassandra" => rdb_connstore::Engine::Cassandra,
            _ => rdb_connstore::Engine::Postgres,
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
            w.set_f_params(SharedString::default());
            w.set_f_color(SharedString::from("#2c5fd8"));
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
                rdb_core::conn::SslMode::Disable => "Disable",
                rdb_core::conn::SslMode::Prefer => "Prefer",
                rdb_core::conn::SslMode::Require => "Require",
            }));
            w.set_f_params(SharedString::from(sc.params.unwrap_or_default()));
            w.set_f_color(SharedString::from(
                sc.color.unwrap_or_else(|| "#2c5fd8".into()),
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
            match rdb_connstore::parse_conn_url(&raw) {
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
                            rdb_core::conn::SslMode::Disable => "Disable",
                            rdb_core::conn::SslMode::Prefer => "Prefer",
                            rdb_core::conn::SslMode::Require => "Require",
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
    // export saved connections. Passwords are not in the list — they live in
    // the keychain — so the dump is safe to write. Arg: 0 = JSON, 1 = CSV.
    {
        let weak = window.as_weak();
        let store = store.clone();
        window.on_export_conns(move |fmt| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let st = store.borrow();
            let (ext, body) = if fmt == 1 {
                ("csv", Ok(export::conns_to_csv(st.list())))
            } else {
                ("json", serde_json::to_string_pretty(st.list()))
            };
            let contents = match body {
                Ok(j) => j,
                Err(e) => {
                    w.set_sel_footer(SharedString::from(format!("export failed: {e}")));
                    return;
                }
            };
            // Native save dialog via rfd's async API + Slint's local executor:
            // the sync dialog blocks the winit event loop and never surfaces.
            let weak = w.as_weak();
            let _ = slint::spawn_local(async move {
                let Some(file) = rfd::AsyncFileDialog::new()
                    .set_file_name(format!("rdb-connections.{ext}"))
                    .add_filter(ext.to_uppercase(), &[ext])
                    .save_file()
                    .await
                else {
                    return; // user cancelled
                };
                let msg = match std::fs::write(file.path(), contents) {
                    Ok(()) => format!("export → {}", file.path().display()),
                    Err(e) => format!("export failed: {e}"),
                };
                if let Some(w) = weak.upgrade() {
                    w.set_sel_footer(SharedString::from(msg));
                }
            });
        });
    }
    // quick test from the picker detail pane: saved config, result in the
    // detail footer line.
    {
        let weak = window.as_weak();
        let rt = rt.clone();
        let store = store.clone();
        window.on_test_conn_quick(move |idx| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let (engine, cfg) = {
                let st = store.borrow();
                let Some(sc) = st.list().get(idx.max(0) as usize) else {
                    return;
                };
                match st.conn_config_for(&sc.id) {
                    Ok(c) => (sc.engine, c),
                    Err(e) => {
                        w.set_sel_footer(SharedString::from(format!("connection failed: {e}")));
                        return;
                    }
                }
            };
            w.set_sel_footer(SharedString::from("Testing connection…"));
            let weak2 = weak.clone();
            rt.spawn(async move {
                let result = try_connect(engine, cfg).await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak2.upgrade() {
                        w.set_sel_footer(SharedString::from(match result {
                            Ok(ms) => format!("connection ok · {ms}ms"),
                            Err(e) => format!("connection failed: {e}"),
                        }));
                    }
                });
            });
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
                "Disable" => rdb_core::conn::SslMode::Disable,
                "Require" => rdb_core::conn::SslMode::Require,
                _ => rdb_core::conn::SslMode::Prefer,
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
            let params = {
                let p = w.get_f_params().to_string();
                if p.trim().is_empty() {
                    None
                } else {
                    Some(p)
                }
            };
            let cfg = rdb_core::conn::ConnConfig {
                host,
                port,
                user: w.get_f_user().to_string(),
                database,
                password,
                sslmode,
                params,
            };

            w.set_test_busy(true);
            w.set_test_ok(false);
            w.set_test_result(SharedString::default());
            w.set_form_error(SharedString::default());

            let weak2 = weak.clone();
            rt.spawn(async move {
                let result = try_connect(engine, cfg).await;
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak2.upgrade() {
                        w.set_test_busy(false);
                        match result {
                            Ok(_) => {
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
                "Disable" => rdb_core::conn::SslMode::Disable,
                "Require" => rdb_core::conn::SslMode::Require,
                _ => rdb_core::conn::SslMode::Prefer,
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
            let params = {
                let p = w.get_f_params().to_string();
                if p.trim().is_empty() {
                    None
                } else {
                    Some(p)
                }
            };
            let password = w.get_f_password().to_string();
            let id = editing_id.borrow().clone();

            let result: rdb_connstore::Result<()> = (|| {
                let mut st = store.borrow_mut();
                let conn_id = if id.is_empty() {
                    let mut sc = rdb_connstore::SavedConnection::new(
                        name,
                        engine,
                        host,
                        port,
                        w.get_f_user().to_string(),
                    );
                    sc.database = database;
                    sc.sslmode = sslmode;
                    sc.color = color;
                    sc.params = params;
                    let cid = sc.id.clone();
                    st.add(sc)?;
                    cid
                } else {
                    let mut sc = st
                        .get(&id)
                        .cloned()
                        .ok_or_else(|| rdb_connstore::ConnStoreError::NotFound(id.clone()))?;
                    sc.name = name;
                    sc.engine = engine;
                    sc.host = host;
                    sc.port = port;
                    sc.user = w.get_f_user().to_string();
                    sc.database = database;
                    sc.sslmode = sslmode;
                    sc.color = color;
                    sc.params = params;
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

    // ----- update reminder: open the release page / dismiss -----
    {
        window.on_update_open(move || {
            let _ = open::that(update::release_page());
        });
    }
    {
        let weak = window.as_weak();
        window.on_update_dismiss(move || {
            if let Some(w) = weak.upgrade() {
                w.set_update_available(false);
            }
        });
    }

    // ----- update check: once/day, gated by the setting, off the UI thread -----
    // Skip in mock mode so the reference screenshots stay deterministic.
    if !mock::mock_mode() {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let (enabled, last) = {
            let s = settings.borrow();
            (s.get().update_check, s.get().last_update_check)
        };
        if enabled && update::due_for_check(last, now) {
            // Persist "checked now" up front on the UI thread — the Rc settings
            // store cannot cross into the worker thread. Worst case a failed
            // check simply waits a day, which is the intended throttle.
            let _ = settings
                .borrow_mut()
                .update(|s| s.last_update_check = Some(now));
            let weak = window.as_weak();
            std::thread::spawn(move || {
                let Some(tag) = update::fetch_latest_tag() else {
                    return;
                };
                if !update::is_newer(&tag, env!("CARGO_PKG_VERSION")) {
                    return;
                }
                let version = tag.trim_start_matches('v').to_string();
                let hint = update::InstallMethod::detect().upgrade_hint().to_string();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(w) = weak.upgrade() {
                        w.set_update_version(version.into());
                        w.set_update_hint(hint.into());
                        w.set_update_available(true);
                    }
                });
            });
        }
    }

    // AppKit replaces the process icon while Slint initializes its native
    // window. Apply ours once the event loop has started so raw `cargo run`
    // binaries get the same Dock icon as packaged builds.
    let app_icon_timer = slint::Timer::default();
    app_icon_timer.start(
        slint::TimerMode::SingleShot,
        std::time::Duration::from_millis(100),
        install_macos_app_icon,
    );

    #[cfg(feature = "mock")]
    shot::install(&window);
    let run_result = window.run();
    // On exit, capture the active tab's latest text (edits made without a tab
    // switch) and persist, so a plain type-then-quit is not lost.
    save_active_tab(&window);
    run_result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persisted_tabs_round_trip() {
        let payload = PersistedTabs {
            tabs: vec![
                PersistedTab {
                    id: "query:c:1".into(),
                    title: "Query 1".into(),
                    query_text: "select * from users;".into(),
                    group: 1,
                    split: true,
                    pane1_query: "select * from orders;".into(),
                    split_ratio: 0.35,
                },
                PersistedTab {
                    id: "query:c:2".into(),
                    title: "scratch".into(),
                    query_text: String::new(),
                    group: 0,
                    split: false,
                    pane1_query: String::new(),
                    split_ratio: 0.5,
                },
            ],
            active: Some("query:c:2".into()),
        };
        let json = serde_json::to_string(&payload).unwrap();
        let back: PersistedTabs = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tabs.len(), 2);
        assert_eq!(back.tabs[0].query_text, "select * from users;");
        assert_eq!(back.tabs[0].group, 1);
        assert!(back.tabs[0].split);
        assert_eq!(back.tabs[0].pane1_query, "select * from orders;");
        assert_eq!(back.tabs[0].split_ratio, 0.35);
        assert_eq!(back.active.as_deref(), Some("query:c:2"));
    }

    #[test]
    fn persisted_tabs_accept_pre_split_files() {
        let payload: PersistedTabs = serde_json::from_str(
            r#"{"tabs":[{"id":"query:c:1","title":"Query 1","query_text":"select 1"}],"active":null}"#,
        )
        .unwrap();
        assert!(!payload.tabs[0].split);
        assert_eq!(payload.tabs[0].group, 0);
        assert!(payload.tabs[0].pane1_query.is_empty());
        assert_eq!(payload.tabs[0].split_ratio, 0.5);
    }

    #[test]
    fn mongo_browse_default_is_twenty() {
        use rdb_connstore::Engine;
        assert_eq!(default_browse_limit(Engine::Mongo), 20);
        assert_eq!(default_browse_limit(Engine::Postgres), 300);
        assert_eq!(default_browse_limit(Engine::MySql), 300);
        assert_eq!(default_browse_limit(Engine::Redis), 300);
    }

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
            Some(rdb_connstore::Engine::Postgres),
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
            Some(rdb_connstore::Engine::Postgres),
            "",
        );
        assert_eq!(rows.iter().filter(|r| r.kind == "table").count(), 3);
        let header = rows.iter().find(|r| r.label.as_str() == "Tables").unwrap();
        assert_eq!(header.count, 3);
    }

    #[test]
    fn page_bounds_first_page_full() {
        // 300 shown of 1000 total: 1–300, no prev, next available
        assert_eq!(page_bounds(0, 300, Some(1000), 300), (1, 300, false, true));
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
        let t = rdb_core::write::TableRef {
            database: None,
            schema: Some("public".into()),
            name: "users".into(),
        };
        assert_eq!(
            browse_text(rdb_connstore::Engine::Postgres, &t, 1, 300, "", &[]),
            "SELECT * FROM \"public\".\"users\" LIMIT 300 OFFSET 300"
        );
        assert_eq!(
            browse_text(rdb_connstore::Engine::MySql, &t, 0, 50, "", &[]),
            "SELECT * FROM `users` LIMIT 50 OFFSET 0"
        );
        assert_eq!(
            browse_text(rdb_connstore::Engine::Redis, &t, 2, 100, "", &[]),
            "BROWSE users 200 100"
        );
        let m = rdb_core::write::TableRef {
            database: Some("shop".into()),
            schema: None,
            name: "orders".into(),
        };
        assert_eq!(
            browse_text(rdb_connstore::Engine::Mongo, &m, 1, 50, "", &[]),
            "{\"collection\":\"orders\",\"database\":\"shop\",\"op\":\"find\",\"body\":{},\"limit\":50,\"skip\":50}"
        );
        // A filter document lands in the find body.
        assert_eq!(
            browse_text(rdb_connstore::Engine::Mongo, &m, 0, 20, r#"{"status":"A"}"#, &[]),
            "{\"collection\":\"orders\",\"database\":\"shop\",\"op\":\"find\",\"body\":{\"status\":\"A\"},\"limit\":20,\"skip\":0}"
        );
    }

    #[test]
    fn browse_text_column_filters_build_where() {
        let t = rdb_core::write::TableRef {
            database: None,
            schema: Some("public".into()),
            name: "users".into(),
        };
        // Bare text → contains match; Postgres uses ILIKE, other SQL LIKE.
        // A leading operator is honoured verbatim; empty exprs are skipped.
        let filters = [
            ("name".to_string(), "ali".to_string()),
            ("age".to_string(), ">= 18".to_string()),
            ("city".to_string(), "".to_string()),
        ];
        assert_eq!(
            browse_text(rdb_connstore::Engine::Postgres, &t, 0, 50, "", &filters),
            "SELECT * FROM \"public\".\"users\" WHERE \"name\" ILIKE '%ali%' AND \"age\" >= '18' LIMIT 50 OFFSET 0"
        );
        assert_eq!(
            browse_text(rdb_connstore::Engine::MySql, &t, 0, 50, "", &filters),
            "SELECT * FROM `users` WHERE `name` LIKE '%ali%' AND `age` >= '18' LIMIT 50 OFFSET 0"
        );
        // Single quotes in the value are doubled (no injection).
        let q = [("name".to_string(), "=o'brien".to_string())];
        assert_eq!(
            browse_text(rdb_connstore::Engine::Sqlite, &t, 0, 10, "", &q),
            "SELECT * FROM \"users\" WHERE \"name\" = 'o''brien' LIMIT 10 OFFSET 0"
        );
    }

    #[test]
    fn sidebar_filter_is_case_insensitive() {
        let rows = schema_display_rows(
            &nodes(),
            &HashSet::new(),
            &HashSet::new(),
            &HashSet::new(),
            Some(rdb_connstore::Engine::Postgres),
            "USERS",
        );
        assert_eq!(rows.iter().filter(|r| r.kind == "table").count(), 1);
    }
}
