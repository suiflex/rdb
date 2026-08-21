#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

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
mod format;
mod mock;
mod model;
mod pane;
mod query_parse;
mod self_update;
#[cfg(feature = "mock")]
mod shot;
mod theme;
mod update;
mod wire;

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use slint::{Model, ModelRc, SharedString, VecModel};

use dispatch::AnyDriver;
use pane::*;

/// Label used for connections with no explicit group.
const UNGROUPED: &str = "Ungrouped";
const MIN_FONT_SIZE: i32 = 10;
const MAX_FONT_SIZE: i32 = 18;
/// Documents sampled per Mongo collection to seed field-name completion.
const MONGO_FIELD_SAMPLE_SIZE: u32 = 20;

fn clamp_font_size(size: i32) -> i32 {
    size.clamp(MIN_FONT_SIZE, MAX_FONT_SIZE)
}

fn shortcut_labels(
    os: &str,
) -> (
    &'static str,
    &'static str,
    &'static str,
    &'static str,
    &'static str,
) {
    if os == "macos" {
        ("⌘", "⇧", "", "↩", "⌫")
    } else {
        ("Ctrl", "Shift", "+", "Enter", "Backspace")
    }
}

#[test]
fn shortcut_labels_follow_desktop_conventions() {
    assert_eq!(shortcut_labels("macos"), ("⌘", "⇧", "", "↩", "⌫"));
    assert_eq!(
        shortcut_labels("windows"),
        ("Ctrl", "Shift", "+", "Enter", "Backspace")
    );
    assert_eq!(shortcut_labels("linux").0, "Ctrl");
}

/// Open a native "save file" dialog and write `contents` to the chosen path.
///
/// macOS: rfd's NSSavePanel would not present from this non-bundled + winit
/// setup no matter how we coaxed activation policy / parenting, so we shell out
/// to `osascript`'s `choose file name` — a separate process that always shows
/// the native panel, immune to our event-loop quirks. Other platforms keep rfd.
/// ponytail: eprintln diagnostics, swap for `tracing` if the app grows structured logs.
fn save_via_dialog(
    w: &MainWindow,
    file_name: String,
    filter_label: String,
    ext: String,
    contents: String,
    report: impl FnOnce(&MainWindow, String) + Send + 'static,
) {
    eprintln!("[export] request: {file_name} ({} bytes)", contents.len());
    let weak = w.as_weak();
    #[cfg(target_os = "macos")]
    {
        let _ = (&filter_label, &ext); // osascript panel filters by name only
        std::thread::spawn(move || {
            // `choose file name` is the save-style panel; default name prefills
            // the extension. Quotes stripped so they can't break the AppleScript.
            let default_name = file_name.replace(['"', '\\'], "");
            let script = format!(
                "POSIX path of (choose file name with prompt \"Export\" default name \"{default_name}\")"
            );
            eprintln!("[export] opening save dialog for {file_name}");
            let out = std::process::Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .output();
            let path = match out {
                Ok(o) if o.status.success() => {
                    String::from_utf8_lossy(&o.stdout).trim().to_string()
                }
                _ => {
                    eprintln!("[export] dialog cancelled or failed");
                    return;
                }
            };
            if path.is_empty() {
                return;
            }
            let msg = match std::fs::write(&path, contents) {
                Ok(()) => format!("exported → {path}"),
                Err(e) => format!("export failed: {e}"),
            };
            eprintln!("[export] {msg}");
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(w) = weak.upgrade() {
                    report(&w, msg);
                }
            });
        });
    }
    #[cfg(not(target_os = "macos"))]
    {
        let parent = w.window().window_handle();
        if slint::spawn_local(async move {
            eprintln!("[export] opening save dialog for {file_name}");
            let picked = rfd::AsyncFileDialog::new()
                .set_parent(&parent)
                .set_file_name(&file_name)
                .add_filter(&filter_label, &[ext.as_str()])
                .save_file()
                .await;
            let Some(file) = picked else {
                eprintln!("[export] dialog closed with no file (cancelled or failed to open)");
                return;
            };
            let msg = match std::fs::write(file.path(), contents) {
                Ok(()) => format!("exported → {}", file.path().display()),
                Err(e) => format!("export failed: {e}"),
            };
            eprintln!("[export] {msg}");
            if let Some(w) = weak.upgrade() {
                report(&w, msg);
            }
        })
        .is_err()
        {
            eprintln!("[export] no UI event loop; dialog not opened");
        }
    }
}

/// Next cursor line when moving vertically past folded (hidden) lines. A closed
/// fold at head `h` hides its body `h+1..=e`; stepping into that body jumps to
/// the first visible line beyond it (down) or back to the fold head (up), so the
/// caret never lands on a line the editor isn't drawing.
fn fold_skip_line(lines: &[String], folded: &HashSet<usize>, line: usize, down: bool) -> usize {
    let regions = editor::fold_regions(lines);
    let hidden = |l: usize| {
        regions
            .iter()
            .any(|&(h, e)| folded.contains(&h) && l > h && l <= e)
    };
    let mut l = line;
    if down {
        while hidden(l) && l + 1 < lines.len() {
            l += 1;
        }
    }
    // Up, or a down move that hit a fold running to EOF: retreat to a visible line.
    while hidden(l) && l > 0 {
        l -= 1;
    }
    l
}

/// Distinct, non-empty group names already in use, sorted.
fn existing_groups(store: &rdb_connstore::ConnStore) -> Vec<String> {
    let mut groups: Vec<String> = store
        .list()
        .iter()
        .filter_map(|s| s.group.clone())
        .filter(|g| !g.trim().is_empty())
        .collect();
    groups.sort();
    groups.dedup();
    groups
}

/// Distinct top-level group names (the first path segment of every entry
/// `existing_groups` returns), sorted.
fn top_level_groups(store: &rdb_connstore::ConnStore) -> Vec<String> {
    let mut tops: Vec<String> = existing_groups(store)
        .iter()
        .map(|g| g.split('/').next().unwrap_or(g).to_string())
        .collect();
    tops.sort();
    tops.dedup();
    tops
}

/// Options for the New/Edit dialog's top-level Group dropdown: "None" +
/// every distinct top-level group already in use + a trailing "create new"
/// entry. Nesting under a chosen group is a separate Subgroup dropdown, see
/// `subgroup_picker_options`.
fn group_picker_options(store: &rdb_connstore::ConnStore) -> Vec<String> {
    let mut opts = vec!["None".to_string()];
    opts.extend(top_level_groups(store));
    opts.push("+ New group…".to_string());
    opts
}

/// Options for the Subgroup dropdown once `parent` is picked as the top
/// group: "None" + every direct child of `parent` already in use (by leaf
/// name) + a trailing "create new" entry. Grandchildren (anything nested
/// two or more levels under `parent`) are not listed here — the guided
/// picker is two levels deep; deeper nesting still works by typing a
/// `/`-containing name into the "create new" free-text field.
fn subgroup_picker_options(store: &rdb_connstore::ConnStore, parent: &str) -> Vec<String> {
    let mut leaves: Vec<String> = existing_groups(store)
        .iter()
        .filter(|g| rdb_connstore::group_parent(g) == Some(parent))
        .map(|g| rdb_connstore::group_leaf(g).to_string())
        .collect();
    leaves.sort();
    leaves.dedup();
    let mut opts = vec!["None".to_string()];
    opts.extend(leaves);
    opts.push("+ New subgroup…".to_string());
    opts
}

/// Where a connection/subfolder currently at `sc_group` (a descendant of
/// `deleted`, itself included) lands once `deleted` is removed: a direct
/// member promotes to `deleted`'s parent (`None` if `deleted` was
/// top-level); a deeper descendant has `deleted`'s own segment spliced out
/// and is reparented one level up, same as a file manager deleting a folder.
fn cascade_delete_group(deleted: &str, sc_group: &str) -> Option<String> {
    if sc_group == deleted {
        return rdb_connstore::group_parent(deleted).map(str::to_string);
    }
    let suffix = &sc_group[deleted.len() + 1..]; // past "deleted/"
    Some(match rdb_connstore::group_parent(deleted) {
        Some(parent) => format!("{parent}/{suffix}"),
        None => suffix.to_string(),
    })
}

/// Where a connection/subfolder currently at `sc_group` (a descendant of
/// `old`, itself included) lands once `old` is renamed to `new`: the `old`
/// prefix is swapped for `new`, keeping whatever nested suffix followed it.
fn cascade_rename_group(old: &str, new: &str, sc_group: &str) -> String {
    if sc_group == old {
        new.to_string()
    } else {
        format!("{new}{}", &sc_group[old.len()..]) // keeps the "/suffix" tail
    }
}

#[cfg(test)]
mod group_cascade_tests {
    use super::*;

    #[test]
    fn delete_direct_member_promotes_to_parent() {
        assert_eq!(cascade_delete_group("Work", "Work"), None);
        assert_eq!(
            cascade_delete_group("Work/Production", "Work/Production"),
            Some("Work".to_string())
        );
    }

    #[test]
    fn delete_descendant_splices_out_the_deleted_segment() {
        assert_eq!(
            cascade_delete_group("Work/Production", "Work/Production/DB"),
            Some("Work/DB".to_string())
        );
        assert_eq!(
            cascade_delete_group("Work", "Work/Production/DB"),
            Some("Production/DB".to_string())
        );
    }

    #[test]
    fn rename_direct_member_and_descendant_suffix() {
        assert_eq!(
            cascade_rename_group("Work", "Work-Legacy", "Work"),
            "Work-Legacy"
        );
        assert_eq!(
            cascade_rename_group("Work", "Work-Legacy", "Work/Production"),
            "Work-Legacy/Production"
        );
        assert_eq!(
            cascade_rename_group("Work/Production", "Work/Prod", "Work/Production/DB"),
            "Work/Prod/DB"
        );
    }
}

#[cfg(test)]
mod build_conn_items_tests {
    use super::*;
    use rdb_connstore::{ConnStore, EncryptedFileBackend, Engine, SavedConnection};

    /// A throwaway `ConnStore` backed by a temp dir, seeded with connections
    /// at the given group paths (`None` = ungrouped). Never touches the real
    /// platform config dir.
    fn store_with_groups(groups: &[Option<&str>]) -> (tempfile::TempDir, ConnStore) {
        let dir = tempfile::tempdir().expect("tempdir");
        let backend = EncryptedFileBackend::new(dir.path()).expect("file secret backend");
        let mut store = ConnStore::new(dir.path().join("connections.json"), Box::new(backend));
        for (i, g) in groups.iter().enumerate() {
            let mut sc = SavedConnection::new(format!("conn{i}"), Engine::Postgres, "h", 5432, "u");
            sc.group = g.map(str::to_string);
            store.add(sc).expect("add");
        }
        (dir, store)
    }

    #[test]
    fn implied_ancestor_header_renders_with_zero_direct_members() {
        let (_dir, store) = store_with_groups(&[Some("Work/Production")]);
        let rows = build_conn_items(&store, &HashSet::new(), "");
        // Work (0 direct, 1 nested) -> Work/Production (1 direct) -> the connection.
        assert_eq!(rows.len(), 3);
        assert!(rows[0].is_header);
        assert_eq!(rows[0].group.as_str(), "Work");
        assert_eq!(rows[0].depth, 0);
        assert_eq!(rows[0].count, 1);
        assert!(rows[1].is_header);
        assert_eq!(rows[1].group.as_str(), "Work/Production");
        assert_eq!(rows[1].depth, 1);
        assert_eq!(rows[1].count, 1);
        assert!(!rows[2].is_header);
        assert_eq!(rows[2].group.as_str(), "Work/Production");
        assert_eq!(rows[2].depth, 1);
    }

    #[test]
    fn collapsing_a_parent_hides_the_whole_subtree() {
        let (_dir, store) = store_with_groups(&[Some("Work/Production"), Some("Work")]);
        let collapsed = HashSet::from(["Work".to_string()]);
        let rows = build_conn_items(&store, &collapsed, "");
        // Only the Work header itself remains visible.
        assert_eq!(rows.len(), 1);
        assert!(rows[0].is_header);
        assert_eq!(rows[0].group.as_str(), "Work");
        assert!(!rows[0].expanded);
        assert!(rows[0].is_group_end);
    }

    #[test]
    fn is_group_end_lands_on_the_deepest_last_row_of_each_top_level_group() {
        let (_dir, store) = store_with_groups(&[Some("Work/Production"), Some("LOCAL")]);
        let rows = build_conn_items(&store, &HashSet::new(), "");
        // Work, Work/Production, conn0 (deepest last row of the Work card),
        // LOCAL, conn1 (last row of the LOCAL card).
        assert_eq!(rows.len(), 5);
        assert!(rows[2].is_group_end, "conn0 should close the Work card");
        assert!(
            !rows[1].is_group_end,
            "the Work/Production header is not the card's true end"
        );
        assert!(rows[4].is_group_end, "conn1 should close the LOCAL card");
    }
}

/// Total connections directly or transitively under `path` (used for a
/// folder header's count badge).
fn subtree_conn_count(
    path: &str,
    child_folders: &std::collections::HashMap<Option<String>, Vec<String>>,
    direct_conns: &std::collections::HashMap<String, Vec<usize>>,
) -> i32 {
    let mut total = direct_conns.get(path).map_or(0, |v| v.len() as i32);
    if let Some(children) = child_folders.get(&Some(path.to_string())) {
        for child in children {
            total += subtree_conn_count(child, child_folders, direct_conns);
        }
    }
    total
}

/// Depth-first: push `path`'s header row, then (unless collapsed) its child
/// folders followed by its own direct connection rows. Never touches
/// `is_group_end` — that is only meaningful at the top-level group's true
/// last visible row, which the caller marks after the whole subtree returns.
fn emit_group_subtree(
    path: &str,
    depth: i32,
    store: &rdb_connstore::ConnStore,
    child_folders: &std::collections::HashMap<Option<String>, Vec<String>>,
    direct_conns: &std::collections::HashMap<String, Vec<usize>>,
    collapsed: &HashSet<String>,
    rows: &mut Vec<ConnItem>,
) {
    let expanded = !collapsed.contains(path);
    rows.push(ConnItem {
        id: SharedString::default(),
        name: rdb_connstore::group_leaf(path).to_uppercase().into(),
        engine: SharedString::default(),
        color: theme::accent_or_default(""),
        has_custom_color: false,
        is_header: true,
        expanded,
        index: -1,
        group: path.into(),
        subline: SharedString::default(),
        local: false,
        ssh_enabled: false,
        count: subtree_conn_count(path, child_folders, direct_conns),
        favorite: false,
        env_tag_label: SharedString::default(),
        env_tag_color: theme::accent_or_default(""),
        is_group_end: false,
        depth,
    });
    if !expanded {
        // Collapsing a folder hides its whole subtree, not just its direct rows.
        return;
    }
    if let Some(children) = child_folders.get(&Some(path.to_string())) {
        for child in children {
            emit_group_subtree(
                child,
                depth + 1,
                store,
                child_folders,
                direct_conns,
                collapsed,
                rows,
            );
        }
    }
    if let Some(idxs) = direct_conns.get(path) {
        // Favorites float to the top, then explicit sort order. Stable sort
        // keeps store insertion order for equal keys (order == 0).
        let mut idxs = idxs.clone();
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
                has_custom_color: s.color.is_some(),
                is_header: false,
                expanded: true,
                index: i as i32,
                group: path.into(),
                subline: subline.into(),
                local: s.local,
                ssh_enabled: s.ssh_enabled,
                count: 0,
                favorite: s.favorite,
                env_tag_label: theme::env_tag_label(s.env_tag).into(),
                env_tag_color: theme::env_tag_color(s.env_tag)
                    .unwrap_or_else(|| theme::accent_or_default("")),
                is_group_end: false,
                depth,
            });
        }
    }
}

/// Build the grouped sidebar row model: a header row per group (and, once
/// nested, per subgroup) followed by its connection rows, depth-first,
/// unless the group is collapsed. `index` on each connection row is its
/// position in the store list, so connect/edit callbacks stay correct
/// regardless of grouping or ordering. `collapsed` holds the set of full
/// group paths currently folded shut.
///
/// `SavedConnection.group` is treated as a `/`-delimited path: `"Work"` is a
/// top-level group same as before, `"Work/Production"` nests under it. A
/// group's ancestors are registered as folder headers even if they hold no
/// connections directly, so `"Work"` still renders when every connection
/// under it lives at `"Work/Production"`.
fn build_conn_items(
    store: &rdb_connstore::ConnStore,
    collapsed: &HashSet<String>,
    filter: &str,
) -> Vec<ConnItem> {
    let needle = filter.trim().to_lowercase();
    // child_folders[None] is the top-level order; child_folders[Some(p)] is
    // p's direct subfolders, both in first-seen order across the store scan.
    let mut child_folders: std::collections::HashMap<Option<String>, Vec<String>> =
        std::collections::HashMap::new();
    let mut registered: HashSet<String> = HashSet::new();
    let mut direct_conns: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();
    for (i, sc) in store.list().iter().enumerate() {
        let g = sc
            .group
            .as_deref()
            .and_then(rdb_connstore::normalize_group_path)
            .unwrap_or_else(|| UNGROUPED.to_string());
        if !needle.is_empty()
            && !sc.name.to_lowercase().contains(&needle)
            && !g.to_lowercase().contains(&needle)
            && !sc.env_tag.as_str().to_lowercase().contains(&needle)
        {
            continue;
        }
        if g == UNGROUPED {
            if registered.insert(UNGROUPED.to_string()) {
                child_folders
                    .entry(None)
                    .or_default()
                    .push(UNGROUPED.to_string());
            }
        } else {
            // Register g and every ancestor, shallowest first, so a parent
            // is always in child_folders before its child is appended to it.
            for anc in rdb_connstore::group_ancestors(&g)
                .map(str::to_string)
                .chain(std::iter::once(g.clone()))
            {
                if registered.insert(anc.clone()) {
                    let parent = rdb_connstore::group_parent(&anc).map(str::to_string);
                    child_folders.entry(parent).or_default().push(anc);
                }
            }
        }
        direct_conns.entry(g).or_default().push(i);
    }

    let root_order = child_folders.get(&None).cloned().unwrap_or_default();
    let mut rows: Vec<ConnItem> = Vec::new();
    for path in &root_order {
        // Ungrouped connections list flat (no header) when they are the only
        // top-level bucket; once real groups exist they get their own
        // UNGROUPED header like any other top-level folder.
        if path == UNGROUPED && root_order.len() == 1 {
            if let Some(idxs) = direct_conns.get(UNGROUPED) {
                let mut idxs = idxs.clone();
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
                        has_custom_color: s.color.is_some(),
                        is_header: false,
                        expanded: true,
                        index: i as i32,
                        group: UNGROUPED.into(),
                        subline: subline.into(),
                        local: s.local,
                        ssh_enabled: s.ssh_enabled,
                        count: 0,
                        favorite: s.favorite,
                        env_tag_label: theme::env_tag_label(s.env_tag).into(),
                        env_tag_color: theme::env_tag_color(s.env_tag)
                            .unwrap_or_else(|| theme::accent_or_default("")),
                        is_group_end: false,
                        depth: 0,
                    });
                }
            }
        } else {
            emit_group_subtree(
                path,
                0,
                store,
                &child_folders,
                &direct_conns,
                collapsed,
                &mut rows,
            );
        }
        // Whatever this top-level group's true last visible row ended up
        // being (its header if collapsed, its deepest descendant otherwise)
        // is the one that rounds the card's bottom corners.
        if let Some(last) = rows.last_mut() {
            last.is_group_end = true;
        }
    }
    rows
}

/// Bucket `build_conn_items`'s flat rows into one `TopLevelGroup` per
/// top-level header, so the sidebar can draw a single continuous outline
/// around each top-level card (header + every nested subgroup/connection
/// under it) instead of guessing card boundaries from a flat list. The
/// no-groups case (every connection ungrouped) never emits a header row at
/// all — that stays a single header-less group, same as before.
fn group_conn_items(flat: Vec<ConnItem>) -> Vec<TopLevelGroup> {
    if flat.first().is_some_and(|first| !first.is_header) {
        return vec![TopLevelGroup {
            has_header: false,
            rows: ModelRc::from(Rc::new(VecModel::from(flat))),
        }];
    }
    let mut groups = Vec::new();
    let mut cur: Vec<ConnItem> = Vec::new();
    for item in flat {
        if item.is_header && item.depth == 0 && !cur.is_empty() {
            groups.push(TopLevelGroup {
                has_header: true,
                rows: ModelRc::from(Rc::new(VecModel::from(std::mem::take(&mut cur)))),
            });
        }
        cur.push(item);
    }
    if !cur.is_empty() {
        groups.push(TopLevelGroup {
            has_header: true,
            rows: ModelRc::from(Rc::new(VecModel::from(cur))),
        });
    }
    groups
}

/// `build_conn_items` + `group_conn_items`, wrapped straight into the
/// `ModelRc` the sidebar's `set_connections` expects — the one-liner every
/// call site should use instead of repeating the two-step build+wrap.
fn build_sidebar_model(
    store: &rdb_connstore::ConnStore,
    collapsed: &HashSet<String>,
    filter: &str,
) -> ModelRc<TopLevelGroup> {
    ModelRc::from(Rc::new(VecModel::from(group_conn_items(build_conn_items(
        store, collapsed, filter,
    )))))
}

/// Flatten `build_conn_items`'s output into the ⌘O "Open Connection" modal's
/// `PaletteItem` list plus a parallel index map (`-1` for a header row, the
/// real store index for a connection row) — shared by the modal's open
/// handler and the group-toggle handler so both stay in sync however the
/// toggle was triggered.
fn build_conn_palette_items(
    store: &rdb_connstore::ConnStore,
    collapsed: &HashSet<String>,
    filter: &str,
) -> (Vec<PaletteItem>, Vec<i32>) {
    let rows = build_conn_items(store, collapsed, filter);
    let mut items: Vec<PaletteItem> = Vec::new();
    let mut map: Vec<i32> = Vec::new();
    for r in rows {
        if r.is_header {
            items.push(PaletteItem {
                label: rdb_connstore::group_leaf(&r.group).to_lowercase().into(),
                kind: "group".into(),
                sub: SharedString::default(),
                local: false,
                color: theme::accent_or_default(""),
                has_custom_color: false,
                env_tag_label: SharedString::default(),
                env_tag_color: theme::accent_or_default(""),
                group: r.group,
                expanded: r.expanded,
                is_group_end: r.is_group_end,
                depth: r.depth,
            });
            map.push(-1);
        } else {
            items.push(PaletteItem {
                label: r.name,
                kind: r.engine,
                sub: r.subline,
                local: r.local,
                color: r.color,
                has_custom_color: r.has_custom_color,
                env_tag_label: r.env_tag_label,
                env_tag_color: r.env_tag_color,
                group: r.group,
                expanded: r.expanded,
                is_group_end: r.is_group_end,
                depth: r.depth,
            });
            map.push(r.index);
        }
    }
    (items, map)
}

/// Bucket a flat `PaletteItem` list into `PaletteGroup`s for `ListModal`,
/// same rule as `group_conn_items`: a `"group"`-kind row at depth 0 starts a
/// new top-level card. Every `ListModal` caller other than the ⌘O connection
/// picker never emits a `"group"`-kind row at all, so their list's first
/// item is never `"group"` and this always falls through to the single
/// `has_header: false` bucket — unaffected, same as before.
fn group_palette_items(flat: Vec<PaletteItem>) -> Vec<PaletteGroup> {
    if flat.first().is_some_and(|first| first.kind != "group") {
        let start_index = 0;
        return vec![PaletteGroup {
            has_header: false,
            start_index,
            rows: ModelRc::from(Rc::new(VecModel::from(flat))),
        }];
    }
    let mut groups = Vec::new();
    let mut cur: Vec<PaletteItem> = Vec::new();
    let mut cur_start = 0i32;
    for (i, item) in flat.into_iter().enumerate() {
        if item.kind == "group" && item.depth == 0 && !cur.is_empty() {
            groups.push(PaletteGroup {
                has_header: true,
                start_index: cur_start,
                rows: ModelRc::from(Rc::new(VecModel::from(std::mem::take(&mut cur)))),
            });
            cur_start = i as i32;
        }
        cur.push(item);
    }
    if !cur.is_empty() {
        groups.push(PaletteGroup {
            has_header: true,
            start_index: cur_start,
            rows: ModelRc::from(Rc::new(VecModel::from(cur))),
        });
    }
    groups
}

/// Walk `rendered` (the same list `build_conn_items` produced) accumulating
/// each row's on-screen height to find which one a drag's release point
/// (`y`, pixels from the top of the list content) landed on, returning its
/// group. Heights mirror `picker.slint`'s row layout: 28px header (+8px
/// `Tokens.sp2` extra on every header but the first, for the gap between
/// cards) / 40px row, no spacing between rows otherwise — if that layout
/// changes, update both.
fn row_group_at_y(rendered: &[ConnItem], y: i32) -> Option<String> {
    const HEADER_H: i32 = 28;
    const HEADER_GAP: i32 = 8;
    const ROW_H: i32 = 40;
    let mut top = 0;
    for (i, item) in rendered.iter().enumerate() {
        let h = if item.is_header {
            if i > 0 {
                HEADER_H + HEADER_GAP
            } else {
                HEADER_H
            }
        } else {
            ROW_H
        };
        if y < top + h {
            return Some(item.group.to_string());
        }
        top += h;
    }
    rendered.last().map(|item| item.group.to_string())
}

#[cfg(test)]
mod row_group_at_y_tests {
    use super::*;

    fn header(group: &str) -> ConnItem {
        ConnItem {
            id: SharedString::default(),
            name: group.into(),
            engine: SharedString::default(),
            color: Default::default(),
            has_custom_color: false,
            is_header: true,
            expanded: true,
            index: -1,
            group: group.into(),
            subline: SharedString::default(),
            local: false,
            ssh_enabled: false,
            count: 1,
            favorite: false,
            env_tag_label: SharedString::default(),
            env_tag_color: Default::default(),
            is_group_end: false,
            depth: 0,
        }
    }

    fn row(group: &str) -> ConnItem {
        ConnItem {
            is_header: false,
            index: 0,
            ..header(group)
        }
    }

    #[test]
    fn lands_on_the_row_under_the_release_point() {
        // header 0 (28, no gap) [0,28) | row (40) [28,68)
        // header 1 (28+8 gap)   [68,104) | row (40) [104,144)
        let rendered = vec![header("A"), row("A"), header("B"), row("B")];
        assert_eq!(row_group_at_y(&rendered, 10), Some("A".into())); // on A's header
        assert_eq!(row_group_at_y(&rendered, 50), Some("A".into())); // on A's row
        assert_eq!(row_group_at_y(&rendered, 90), Some("B".into())); // on B's header
        assert_eq!(row_group_at_y(&rendered, 130), Some("B".into())); // on B's row
    }

    #[test]
    fn clamps_out_of_range_y_to_the_nearest_end() {
        let rendered = vec![header("A"), row("A")];
        assert_eq!(row_group_at_y(&rendered, -50), Some("A".into()));
        assert_eq!(row_group_at_y(&rendered, 9999), Some("A".into()));
    }

    #[test]
    fn empty_list_has_no_target() {
        assert_eq!(row_group_at_y(&[], 0), None);
    }
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
            sub: SharedString::default(),
            sub_color: Default::default(),
            sub_has_custom_color: false,
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
                    sub: SharedString::default(),
                    sub_color: Default::default(),
                    sub_has_custom_color: false,
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
                    sub: SharedString::default(),
                    sub_color: Default::default(),
                    sub_has_custom_color: false,
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
                    sub: SharedString::default(),
                    sub_color: Default::default(),
                    sub_has_custom_color: false,
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
            sub: SharedString::default(),
            sub_color: Default::default(),
            sub_has_custom_color: false,
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
                depth: 2,
                kind: "hint".into(),
                expanded: false,
                db: db.clone().into(),
                count: 0,
                sub: SharedString::default(),
                sub_color: Default::default(),
                sub_has_custom_color: false,
            });
            continue;
        }
        for n in leaves {
            rows.push(TreeNode {
                label: n.label.clone().into(),
                depth: 2,
                kind: leaf_kind.into(),
                expanded: false,
                db: db.clone().into(),
                count: 0,
                sub: SharedString::default(),
                sub_color: Default::default(),
                sub_has_custom_color: false,
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
        Engine::MySql | Engine::MariaDb | Engine::Sqlite | Engine::Mssql | Engine::Clickhouse => &[
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
        Engine::Redis | Engine::Valkey | Engine::Mongo => &[
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

/// Make the process a Regular macOS app. A bare `cargo run` binary (no .app
/// bundle) starts as Prohibited; rfd downgrades that to Accessory, under which
/// NSSavePanel silently never presents — so Export looked dead. Idempotent and a
/// no-op on the bundled build (already Regular) and on non-macOS.
#[cfg(target_os = "macos")]
fn ensure_regular_activation_policy() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    let Some(main_thread) = MainThreadMarker::new() else {
        eprintln!("[export] activation policy: not on main thread, skipped");
        return;
    };
    let app = NSApplication::sharedApplication(main_thread);
    let before = app.activationPolicy();
    let changed = app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    eprintln!("[export] activation policy before={before:?} set_regular_ok={changed}");
}

#[cfg(target_os = "macos")]
fn install_macos_app_icon() {
    use objc2::{AllocAnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    ensure_regular_activation_policy();
    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(main_thread);
    let data = NSData::with_bytes(include_bytes!("../assets/icon@512.png"));
    let Some(icon) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    // SAFETY: `icon` is a live NSImage and AppKit is called on the main thread.
    unsafe { app.setApplicationIconImage(Some(&icon)) };
}

#[cfg(not(target_os = "macos"))]
fn install_macos_app_icon() {}

/// One cached SQL result, shown as a result tab. ⌘⏎ replaces the active one;
/// ⌘\ appends a new one.
/// Client-side grid view state (filters/sort/hidden/order/widths) for one result,
/// so switching Result tabs restores each result exactly as it was left. An empty
/// `col_order` is the "never touched" sentinel — restore is skipped, leaving the
/// freshly-presented defaults.
#[derive(Clone, Default)]
struct GridState {
    col_filters: Vec<String>,
    sort: (i32, bool),
    hidden: Vec<usize>,
    col_order: Vec<usize>,
    col_widths: Vec<f32>,
    // Client-side search bar state (the "Filter" box above the grid). Kept per
    // result so switching result/query tabs doesn't drop the active filter.
    grid_filter: String,
    filter_col: String,
    filter_op: String,
}

#[derive(Clone)]
struct StoredResult {
    /// Shared, never mutated in place: a tab keeps every result it has run and
    /// the active pane mirrors that list, so cloning a `StoredResult` used to
    /// deep-copy a whole grid per stored result on every query completion.
    view: Arc<model::ResultView>,
    meta: String,
    latency: String,
    grid: GridState,
    // Connection that produced THIS run — captured when the run started, not
    // read from the tab, since a long-lived SQL tab can be re-run after the
    // user switches the active connection (each Result N chip needs its own).
    connection_id: Option<String>,
    engine: String,
    connection_name: String,
    color: slint::Color,
    has_custom_color: bool,
}

fn store_result(
    results: &mut Vec<StoredResult>,
    active: &mut usize,
    result: StoredResult,
    new_tab: bool,
) {
    if new_tab || results.is_empty() {
        results.push(result);
        *active = results.len() - 1;
    } else {
        *active = (*active).min(results.len() - 1);
        results[*active] = result;
    }
}

#[cfg(test)]
mod result_tab_tests {
    use super::*;

    fn result(name: &str) -> StoredResult {
        StoredResult {
            view: Arc::new(model::ResultView::Affected(name.into())),
            meta: String::new(),
            latency: String::new(),
            grid: GridState::default(),
            connection_id: None,
            engine: String::new(),
            connection_name: String::new(),
            color: theme::accent_or_default(""),
            has_custom_color: false,
        }
    }

    #[test]
    fn replaces_active_or_appends_as_requested() {
        let mut results = vec![result("first"), result("second")];
        let mut active = 1;
        store_result(&mut results, &mut active, result("replacement"), false);
        assert_eq!(results.len(), 2);
        assert!(matches!(&*results[1].view, model::ResultView::Affected(v) if v == "replacement"));

        store_result(&mut results, &mut active, result("third"), true);
        assert_eq!(results.len(), 3);
        assert_eq!(active, 2);
    }
}

/// The live editor + result state a single **workspace tab group** owns
/// independently. The workspace has two groups (left = 0, right = 1); each holds
/// its own editor buffer, folds, completion, find hits, result set, grid
/// view/sort/filter, and streaming. Held as `groups[0/1]` (the local binding is
/// still named `panes` in some places for historical reasons).
///
/// Naming note: the Slint side still uses `p1-*` property/callback names for
/// group 1 (e.g. `p1-cells`, `on_p1_run`). Those are compatibility plumbing —
/// "p1" means "group 1" — kept to avoid renaming ~170 UI bindings in one pass.
struct GroupRuntime {
    ed_state: Rc<RefCell<editor::EditorState>>,
    folded_heads: Rc<RefCell<HashSet<usize>>>,
    #[allow(clippy::type_complexity)]
    completion_ctx: Rc<RefCell<(usize, Vec<(String, String)>)>>,
    find_hits: Rc<RefCell<Vec<(usize, usize, usize)>>>,
    edit_buf: Arc<std::sync::Mutex<model::EditBuffer>>,
    results: Arc<std::sync::Mutex<Vec<StoredResult>>>,
    active_result: Arc<std::sync::Mutex<usize>>,
    result_new_tab: Arc<std::sync::atomic::AtomicBool>,
    // Set when a run contains 2+ statements: keep one result tab per statement.
    split_results: Arc<std::sync::atomic::AtomicBool>,
    displayed_grid: Arc<std::sync::Mutex<Option<model::GridModel>>>,
    browse: Arc<std::sync::Mutex<BrowseState>>,
    hidden_cols: Arc<std::sync::Mutex<HashSet<usize>>>,
    sort_state: Arc<std::sync::Mutex<(i32, bool)>>,
    col_order: Arc<std::sync::Mutex<Vec<usize>>>,
    col_filters: Arc<std::sync::Mutex<Vec<String>>>,
    stream_cancel: Rc<RefCell<Option<Arc<std::sync::atomic::AtomicBool>>>>,
    stream_timer: Rc<RefCell<Option<slint::Timer>>>,
    // Abort handle for the in-flight buffered query task, so a slow query can be
    // hard-cancelled. Overwritten on each run; aborting a finished task is a
    // no-op, so no clearing on completion is needed.
    query_abort: Rc<RefCell<Option<tokio::task::AbortHandle>>>,
    // Editor error highlight: (line the engine pointed at, first line of the
    // failing statement, last line of it), 0-based in the buffer. Kept here so
    // a re-lex (any edit or cursor move) can re-apply it instead of dropping it.
    error_mark: Arc<Mutex<Option<ErrorMark>>>,
    // 0-based line where the next run's text starts in the editor buffer — Run
    // sends only the statement under the cursor, so an error line reported
    // against it has to be shifted back. Taken (and reset to the top of the
    // buffer) by run_sql/run_stream, so a run that isn't editor text (saved
    // query, table browse) is unaffected.
    pending_run_origin: Rc<Cell<(i32, i32)>>,
}

impl GroupRuntime {
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
            split_results: Arc::new(std::sync::atomic::AtomicBool::new(false)),
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
            query_abort: Rc::new(RefCell::new(None)),
            error_mark: Arc::new(Mutex::new(None)),
            pending_run_origin: Rc::new(Cell::new((0, 0))),
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
    // Line indices of the statement heads the user has folded closed, kept
    // sorted. They index into `query_text` and mean nothing against any other
    // buffer — which is exactly why they live on the tab rather than on the
    // pane, where every tab would share one set and fold each other's lines.
    //
    // One set, not one per pane: a tab is edited by whichever group has it
    // active, and both groups write that group's active tab's `query_text`.
    folded_heads: Vec<usize>,
    // Connection the tab was created against (snapshot, not live-tracked).
    connection_id: Option<String>,
    // DbBadge key, e.g. "postgres"; empty when no connection was active yet.
    engine: String,
    connection_name: String,
    // Connection's custom accent (falls back to a generic color when unset —
    // has_custom_color says whether to prefer it over the per-engine color).
    color: slint::Color,
    has_custom_color: bool,
}

/// A pane's live fold set as the sorted vec a tab stores.
///
/// Sorted so a save does not rewrite the file just because the `HashSet`
/// happened to iterate in a different order.
fn snapshot_folds(folded: &RefCell<HashSet<usize>>) -> Vec<usize> {
    let mut v: Vec<usize> = folded.borrow().iter().copied().collect();
    v.sort_unstable();
    v
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
            folded_heads: Vec::new(),
            connection_id: None,
            engine: String::new(),
            connection_name: String::new(),
            color: theme::accent_or_default(""),
            has_custom_color: false,
        }
    }

    fn is_preview(&self) -> bool {
        self.kind == "table" && !self.pinned
    }
}

fn table_tab_id(connection_id: &str, database: &str, schema: &str, table: &str) -> String {
    format!("table:{connection_id}:{database}:{schema}:{table}")
}

// Badge key + display name + accent color for a saved connection id; default
// (empty strings, generic accent, has_custom_color false) when unknown — not
// yet connected, or the connection was deleted since the tab opened.
#[derive(Clone)]
struct ConnBadgeInfo {
    engine: String,
    name: String,
    color: slint::Color,
    has_custom_color: bool,
}

impl Default for ConnBadgeInfo {
    fn default() -> Self {
        Self {
            engine: String::new(),
            name: String::new(),
            color: theme::accent_or_default(""),
            has_custom_color: false,
        }
    }
}

fn connection_badge_info(store: &rdb_connstore::ConnStore, connection_id: &str) -> ConnBadgeInfo {
    store
        .list()
        .iter()
        .find(|c| c.id == connection_id)
        .map(|c| ConnBadgeInfo {
            engine: AnyDriver::badge(c.engine).to_string(),
            name: c.name.clone(),
            color: theme::accent_or_default(c.color.as_deref().unwrap_or("")),
            has_custom_color: c.color.is_some(),
        })
        .unwrap_or_default()
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

/// Absolute `tabs` index of the `group_index`-th tab in `group` (i.e. that
/// group's tab-strip position → the underlying vector index). None if out of
/// range. Inverse of [`group_relative_index`].
fn abs_index_for_group(tabs: &[WorkspaceTab], group: usize, group_index: usize) -> Option<usize> {
    tabs.iter()
        .enumerate()
        .filter(|(_, t)| t.group == group)
        .nth(group_index)
        .map(|(abs, _)| abs)
}

/// Position of the tab at absolute index `abs` within its own group's tab strip.
fn group_relative_index(tabs: &[WorkspaceTab], abs: usize) -> usize {
    let group = tabs[abs].group;
    tabs[..abs].iter().filter(|t| t.group == group).count()
}

#[cfg(test)]
mod group_tests {
    use super::{
        abs_index_for_group, group_relative_index, replaceable_table_tab_index,
        workspace_tab_index, WorkspaceTab,
    };

    fn tab(id: &str, group: usize) -> WorkspaceTab {
        let mut t = WorkspaceTab::sql(id.into(), 0);
        t.group = group;
        t
    }

    #[test]
    fn abs_and_relative_index_are_inverse() {
        // Interleaved groups: L R L R L.
        let tabs = vec![
            tab("a", 0),
            tab("b", 1),
            tab("c", 0),
            tab("d", 1),
            tab("e", 0),
        ];
        // Left strip = a(0), c(2), e(4); right strip = b(1), d(3).
        assert_eq!(abs_index_for_group(&tabs, 0, 0), Some(0));
        assert_eq!(abs_index_for_group(&tabs, 0, 1), Some(2));
        assert_eq!(abs_index_for_group(&tabs, 0, 2), Some(4));
        assert_eq!(abs_index_for_group(&tabs, 0, 3), None);
        assert_eq!(abs_index_for_group(&tabs, 1, 0), Some(1));
        assert_eq!(abs_index_for_group(&tabs, 1, 1), Some(3));
        assert_eq!(abs_index_for_group(&tabs, 1, 2), None);
        assert_eq!(group_relative_index(&tabs, 4), 2);
        assert_eq!(group_relative_index(&tabs, 3), 1);
        // Round-trip: every absolute index maps back to itself.
        for (abs, t) in tabs.iter().enumerate() {
            let gi = group_relative_index(&tabs, abs);
            assert_eq!(abs_index_for_group(&tabs, t.group, gi), Some(abs));
        }
    }

    #[test]
    fn workspace_tab_index_finds_by_id() {
        let tabs = vec![tab("a", 0), tab("b", 1)];
        assert_eq!(workspace_tab_index(&tabs, Some("b")), Some(1));
        assert_eq!(workspace_tab_index(&tabs, Some("x")), None);
        assert_eq!(workspace_tab_index(&tabs, None), None);
    }

    #[test]
    fn replaceable_table_tab_only_for_unpinned_table() {
        let mut t = WorkspaceTab::sql("t".into(), 0);
        t.kind = "table".into();
        t.pinned = false; // an unpinned table tab is a "preview"
        assert_eq!(replaceable_table_tab_index(&[t], Some("t")), Some(0));
        // A SQL tab is never replaceable.
        assert_eq!(replaceable_table_tab_index(&[tab("s", 0)], Some("s")), None);
    }
}

fn set_workspace_tabs(w: &MainWindow, tabs: &[WorkspaceTab], active_id: Option<&str>) {
    let left: Vec<&WorkspaceTab> = tabs.iter().filter(|tab| tab.group == 0).collect();
    let items: Vec<TabItem> = left
        .iter()
        .map(|tab| TabItem {
            kind: tab.kind.clone().into(),
            title: tab.title.clone().into(),
            preview: tab.is_preview(),
            engine: tab.engine.clone().into(),
            connection_name: tab.connection_name.clone().into(),
            color: tab.color,
            has_custom_color: tab.has_custom_color,
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
            engine: tab.engine.clone().into(),
            connection_name: tab.connection_name.clone().into(),
            color: tab.color,
            has_custom_color: tab.has_custom_color,
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
/// Quote a database/schema/table identifier for `engine`, doubling the quote
/// char so a name with an embedded quote can't break out. MySQL uses backticks,
/// everything else standard double-quotes.
fn quote_ident(name: &str, engine: rdb_connstore::Engine) -> String {
    match engine {
        rdb_connstore::Engine::MySql => format!("`{}`", name.replace('`', "``")),
        _ => format!("\"{}\"", name.replace('"', "\"\"")),
    }
}

/// One column as entered in the new-table designer.
struct ColSpec {
    name: String,
    ty: String,
    nullable: bool,
    pk: bool,
}

/// Build a `CREATE TABLE` statement from the designer rows, quoting every
/// identifier for `engine`. `schema` qualifies the table when the engine uses
/// namespaces (Postgres). Returns a user-facing message on invalid input.
fn build_create_table(
    schema: Option<&str>,
    table: &str,
    cols: &[ColSpec],
    engine: rdb_connstore::Engine,
) -> std::result::Result<String, String> {
    let table = table.trim();
    if table.is_empty() {
        return Err("Table name can't be empty".into());
    }
    if cols.is_empty() {
        return Err("Add at least one column".into());
    }
    let mut defs = Vec::new();
    let mut pks = Vec::new();
    for c in cols {
        let n = c.name.trim();
        let ty = c.ty.trim();
        if n.is_empty() {
            return Err("Column name can't be empty".into());
        }
        if ty.is_empty() {
            return Err(format!("Type for \"{n}\" can't be empty"));
        }
        let mut d = format!("{} {ty}", quote_ident(n, engine));
        if !c.nullable {
            d.push_str(" NOT NULL");
        }
        defs.push(d);
        if c.pk {
            pks.push(quote_ident(n, engine));
        }
    }
    if !pks.is_empty() {
        defs.push(format!("PRIMARY KEY ({})", pks.join(", ")));
    }
    let target = match schema {
        Some(s) => format!("{}.{}", quote_ident(s, engine), quote_ident(table, engine)),
        None => quote_ident(table, engine),
    };
    Ok(format!(
        "CREATE TABLE {target} (\n  {}\n)",
        defs.join(",\n  ")
    ))
}

/// Default type for the seeded primary-key column of a new table, per engine.
fn default_pk_type(engine: rdb_connstore::Engine) -> &'static str {
    match engine {
        rdb_connstore::Engine::Postgres => "serial",
        rdb_connstore::Engine::MySql => "INT",
        rdb_connstore::Engine::Sqlite => "INTEGER",
        rdb_connstore::Engine::Mssql => "INT IDENTITY(1,1)",
        // ClickHouse has no auto-increment/serial equivalent — ORDER BY is
        // its closest concept, not a per-column identity default.
        rdb_connstore::Engine::Clickhouse => "UInt64",
        _ => "int",
    }
}

/// Default type for an added column, per engine.
fn default_col_type(engine: rdb_connstore::Engine) -> &'static str {
    match engine {
        rdb_connstore::Engine::MySql => "VARCHAR(255)",
        rdb_connstore::Engine::Sqlite => "TEXT",
        rdb_connstore::Engine::Mssql => "NVARCHAR(255)",
        rdb_connstore::Engine::Clickhouse => "String",
        _ => "text",
    }
}

/// Connect + ping, bounded to 8s so "Testing connection…" always resolves.
/// Returns the elapsed milliseconds on success.
// ponytail: timeout-bounded, no hard abort; add CancellationToken if a true
// cancel button is ever needed.
async fn try_connect(
    engine: rdb_connstore::Engine,
    cfg: rdb_core::conn::ConnConfig,
) -> Result<u64, rdb_core::error::RdbError> {
    let t0 = std::time::Instant::now();
    let timeout_secs = if cfg.ssh.is_some() { 25 } else { 10 };
    let attempt = async {
        let driver = AnyDriver::connect(engine, &cfg).await?;
        driver.ping().await?;
        Ok::<_, rdb_core::error::RdbError>(())
    };
    match tokio::time::timeout(std::time::Duration::from_secs(timeout_secs), attempt).await {
        Ok(Ok(())) => Ok(t0.elapsed().as_millis().max(1) as u64),
        Ok(Err(e)) => Err(e),
        Err(_) => Err(rdb_core::error::RdbError::Connection(
            "connection timed out".into(),
        )),
    }
}

/// Build the query editor's completion seed for one schema/database. Mongo
/// has no real schema, so this samples each collection's fields first (see
/// `Driver::sample_fields`) and completes only the seed with them — the
/// sidebar tree keeps the real, unsampled schema so it doesn't start
/// rendering field rows under every collection. Every other engine just
/// converts the schema tree directly. Shared by the connect flow and the
/// schema/database switcher, which both need to (re)populate this seed.
async fn build_completion_seed(
    driver: &Arc<AnyDriver>,
    engine: rdb_connstore::Engine,
    schema_current: &str,
    schema: &rdb_core::schema::Schema,
) -> Vec<model::VmTreeNode> {
    if !matches!(engine, rdb_connstore::Engine::Mongo) {
        return model::to_completion_nodes(schema_current, schema);
    }
    let Some(dbase) = schema.databases.first() else {
        return model::to_completion_nodes(schema_current, schema);
    };
    let mut set = tokio::task::JoinSet::new();
    for cont in &dbase.containers {
        let driver = driver.clone();
        let db = dbase.name.clone();
        let name = cont.name.clone();
        set.spawn(async move {
            let fields = driver
                .sample_fields(&db, &name, MONGO_FIELD_SAMPLE_SIZE)
                .await
                .unwrap_or_default();
            (name, fields)
        });
    }
    let mut sampled: HashMap<String, Vec<rdb_core::schema::Field>> = HashMap::new();
    while let Some(res) = set.join_next().await {
        if let Ok((name, fields)) = res {
            sampled.insert(name, fields);
        }
    }
    let mut cloned = schema.clone();
    if let Some(d) = cloned.databases.first_mut() {
        for cont in &mut d.containers {
            if let Some(fields) = sampled.remove(&cont.name) {
                cont.fields = fields;
            }
        }
    }
    model::to_completion_nodes(schema_current, &cloned)
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

/// Clipboard write; returns false when no clipboard is available.
fn clip_set(s: &str) -> bool {
    use copypasta::ClipboardProvider;
    if let Ok(mut c) = copypasta::ClipboardContext::new() {
        return c.set_contents(s.to_string()).is_ok();
    }
    false
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

/// One run of a query, kept for the History sidebar tab.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct HistoryEntry {
    sql: String,
    /// Unix seconds; 0 means unknown (migrated from the pre-timestamp format).
    ran_at: u64,
    /// Driver badge key at run time (`AnyDriver::badge`, e.g. "postgres") —
    /// tagging by engine reads better than by connection name, since many
    /// connections share the same driver and the tag stays meaningful even
    /// if the connection is later renamed or deleted.
    #[serde(default)]
    engine: Option<String>,
    /// Connection's accent hex at run time (`#rrggbb`), for the History
    /// badge — `None` means no custom color, fall back to the plain engine
    /// color the same way connection rows elsewhere in the app do.
    #[serde(default)]
    color: Option<String>,
}

/// Parse persisted history JSON, falling back to the pre-timestamp
/// plain-string-array format so existing `recent_queries.json` files keep
/// loading (migrated entries get `ran_at: 0`).
fn parse_recent(raw: &str) -> Vec<HistoryEntry> {
    if let Ok(entries) = serde_json::from_str::<Vec<HistoryEntry>>(raw) {
        return entries;
    }
    serde_json::from_str::<Vec<String>>(raw)
        .unwrap_or_default()
        .into_iter()
        .map(|sql| HistoryEntry {
            sql,
            ran_at: 0,
            engine: None,
            color: None,
        })
        .collect()
}

/// Load persisted recent-query history; empty on a missing/unreadable file.
fn load_recent() -> Vec<HistoryEntry> {
    let Ok(path) = rdb_connstore::ConnStore::recent_queries_path() else {
        return Vec::new();
    };
    let Ok(raw) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    parse_recent(&raw)
}

/// Persist the recent-query history (best-effort; I/O errors are ignored).
fn save_recent(list: &[HistoryEntry]) {
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

/// "Today" / "Yesterday" / "Mon D, YYYY" bucket label for a history entry,
/// in local time. `ran_at == 0` (pre-migration entries) always bucket last.
fn history_date_label(ran_at: u64, now: u64) -> String {
    use chrono::TimeZone;
    if ran_at == 0 {
        return "Earlier".into();
    }
    let day = chrono::Local
        .timestamp_opt(ran_at as i64, 0)
        .single()
        .map(|dt| dt.date_naive());
    let today = chrono::Local
        .timestamp_opt(now as i64, 0)
        .single()
        .map(|dt| dt.date_naive());
    match (day, today) {
        (Some(d), Some(t)) if d == t => "Today".into(),
        (Some(d), Some(t)) if Some(d) == t.pred_opt() => "Yesterday".into(),
        (Some(d), _) => d.format("%b %-d, %Y").to_string(),
        _ => "Earlier".into(),
    }
}

/// Seed shown the first time, before the user has a `saved_queries.json`.
fn default_saved() -> Vec<(String, String)> {
    [
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
    ]
    .into_iter()
    .map(|(n, s)| (n.to_string(), s.to_string()))
    .collect()
}

/// Load persisted saved queries. A missing/unreadable file falls back to the
/// seed; a present-but-empty file stays empty so "delete all" sticks.
fn load_saved() -> Vec<(String, String)> {
    let Ok(path) = rdb_connstore::ConnStore::saved_queries_path() else {
        return default_saved();
    };
    match std::fs::read_to_string(path) {
        Ok(s) => serde_json::from_str(&s).unwrap_or_else(|_| default_saved()),
        Err(_) => default_saved(),
    }
}

/// Persist saved queries (best-effort; I/O errors are ignored).
fn save_saved(list: &[(String, String)]) {
    let Ok(path) = rdb_connstore::ConnStore::saved_queries_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(list) {
        let _ = std::fs::write(path, json);
    }
}

/// Build a short, unique slug name when saving a history query into the Saved
/// list — first non-comment line, alphanumerics only, deduped against existing.
fn derive_query_name(sql: &str, existing: &[(String, String)]) -> String {
    let first = sql
        .lines()
        .map(|l| l.split("--").next().unwrap_or("").trim())
        .find(|l| !l.is_empty())
        .unwrap_or("")
        .to_lowercase();
    let mut slug: String = first
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    while slug.contains("--") {
        slug = slug.replace("--", "-");
    }
    let slug: String = slug.trim_matches('-').chars().take(32).collect();
    let base = if slug.is_empty() {
        "query".to_string()
    } else {
        slug
    };
    if !existing.iter().any(|(n, _)| n == &base) {
        return base;
    }
    (2..)
        .map(|i| format!("{base}-{i}"))
        .find(|c| !existing.iter().any(|(n, _)| n == c))
        .unwrap_or(base)
}

/// Pretty-print (indent) a JSON string for display. Returns `None` when the
/// text isn't a JSON object/array, so plain cells are shown as-is.
fn pretty_json(s: &str) -> Option<String> {
    let t = s.trim();
    if !(t.starts_with('{') || t.starts_with('[')) {
        return None;
    }
    serde_json::from_str::<serde_json::Value>(t)
        .ok()
        .and_then(|v| serde_json::to_string_pretty(&v).ok())
}

/// Prepares a cell's raw text for the edit overlay: pretty-prints JSON (so it
/// has real line breaks to wrap on), and flags values that should open the
/// roomy centered inspector modal instead of the row-height inline overlay
/// (which is too short to host a scrollable multi-line editor — see
/// tabular-grid.slint's modal comment). JSON always qualifies as "large"
/// since it's always going to want the formatting room regardless of length.
fn format_cell_edit_value(value: SharedString) -> (SharedString, bool) {
    match pretty_json(value.as_str()) {
        Some(pretty) => (SharedString::from(pretty), true),
        None => {
            let is_large = value.chars().count() > 80;
            (value, is_large)
        }
    }
}

/// Strip SQL line comments (`--` to end of line) and blank lines, so a
/// commented-out scratch statement leaves only its runnable SQL. Word-scan, not
/// a parser: a `--` inside a string literal is a rare edge we accept trimming.
fn strip_sql_comments(text: &str) -> String {
    text.lines()
        .map(|l| l.split("--").next().unwrap_or("").trim_end())
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Record an executed query at the head of the history (dedupe, cap
/// `cap`) and persist it, except in mock mode. Comment-only text is
/// dropped and comments are stripped so history keeps only runnable SQL.
/// Re-running the same SQL updates its recorded time/engine and floats
/// it back to the top rather than adding a second entry.
fn record_recent(
    list: &RefCell<Vec<HistoryEntry>>,
    text: &str,
    cap: usize,
    engine: Option<String>,
    color: Option<String>,
) {
    let t = strip_sql_comments(text);
    let t = t.trim();
    if t.is_empty() {
        return;
    }
    let ran_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut v = list.borrow_mut();
    v.retain(|e| e.sql != t);
    v.insert(
        0,
        HistoryEntry {
            sql: t.to_string(),
            ran_at,
            engine,
            color,
        },
    );
    v.truncate(cap.max(1));
    if !mock::mock_mode() {
        save_recent(&v);
    }
}

/// Snapshot the currently-connected connection's accent hex, for stamping
/// onto a new history entry. `None` if nothing is connected, it was
/// deleted, or it never had a custom color.
fn resolve_conn_color(
    current_connection_id: &std::sync::Mutex<Option<String>>,
    store: &RefCell<rdb_connstore::ConnStore>,
) -> Option<String> {
    let id = current_connection_id.lock().unwrap().clone()?;
    store.borrow().get(&id).and_then(|sc| sc.color.clone())
}

fn recent_preview(text: &str) -> String {
    let preview = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut out: String = preview.chars().take(600).collect();
    if preview.chars().count() > out.chars().count() {
        out.push_str("...");
    }
    out
}

fn remove_recent(list: &RefCell<Vec<HistoryEntry>>, index: usize) -> bool {
    let mut list = list.borrow_mut();
    if index >= list.len() {
        return false;
    }
    list.remove(index);
    true
}

fn query_timing_meta(
    rows: u64,
    statements: usize,
    total_ms: u64,
    queue_ms: u64,
    driver_ms: u64,
    model_ms: u64,
) -> String {
    let prefix = if statements > 1 {
        format!("{statements} statements · {rows} rows")
    } else {
        format!("{rows} rows")
    };
    let total = model::format_latency(total_ms);
    let overhead_ms = queue_ms + model_ms;
    if overhead_ms >= 25 {
        // Breakdown stays bare so the parenthesis reads as a split of the
        // total; it carries the total's unit.
        let (db, wait, process) = if total_ms >= 1_000 {
            (
                driver_ms as f64 / 1e3,
                queue_ms as f64 / 1e3,
                model_ms as f64 / 1e3,
            )
        } else {
            (driver_ms as f64, queue_ms as f64, model_ms as f64)
        };
        let d = if total_ms >= 1_000 { 1 } else { 0 };
        format!("{prefix} · {total} (db {db:.d$} · wait {wait:.d$} · process {process:.d$})")
    } else {
        format!("{prefix} · {total}")
    }
}

#[cfg(test)]
mod record_recent_tests {
    use super::*;

    #[test]
    fn comment_only_is_not_recorded() {
        let list = RefCell::new(Vec::new());
        record_recent(
            &list,
            "-- \\ Check Perizinan\n-- mp.username ilike '%x%'",
            RECENT_CAP,
            None,
            None,
        );
        assert!(list.borrow().is_empty());
    }

    #[test]
    fn leading_comment_is_stripped_but_sql_kept() {
        let list = RefCell::new(Vec::new());
        record_recent(
            &list,
            "-- pick emiten\nselect * from emiten; -- trailing",
            RECENT_CAP,
            Some("postgres".into()),
            Some("#e05a4e".into()),
        );
        let entries = list.borrow();
        assert_eq!(entries[0].sql, "select * from emiten;");
        assert_eq!(entries[0].engine.as_deref(), Some("postgres"));
        assert_eq!(entries[0].color.as_deref(), Some("#e05a4e"));
    }

    #[test]
    fn history_date_label_buckets() {
        let now = 1_800_000_000u64; // arbitrary fixed reference instant
        assert_eq!(history_date_label(0, now), "Earlier");
        assert_eq!(history_date_label(now, now), "Today");
        assert_eq!(history_date_label(now - 86_400, now), "Yesterday");
        assert!(!["Today", "Yesterday", "Earlier"]
            .contains(&history_date_label(now - 20 * 86_400, now).as_str()));
    }

    #[test]
    fn parse_recent_migrates_plain_string_array() {
        let entries = parse_recent(r#"["select 1"]"#);
        assert_eq!(entries[0].sql, "select 1");
        assert_eq!(entries[0].ran_at, 0);
        assert!(entries[0].engine.is_none());
    }

    #[test]
    fn parse_recent_reads_current_format() {
        let entries = parse_recent(r#"[{"sql":"select 2","ran_at":100,"engine":"mysql"}]"#);
        assert_eq!(entries[0].sql, "select 2");
        assert_eq!(entries[0].ran_at, 100);
        assert_eq!(entries[0].engine.as_deref(), Some("mysql"));
    }

    #[test]
    fn query_timing_only_expands_when_client_overhead_is_visible() {
        assert_eq!(query_timing_meta(1, 1, 12, 1, 10, 1), "1 rows · 12 ms");
        assert_eq!(
            query_timing_meta(2, 2, 97, 30, 60, 7),
            "2 statements · 2 rows · 97 ms (db 60 · wait 30 · process 7)"
        );
    }

    #[test]
    fn query_timing_switches_to_seconds_past_a_thousand_ms() {
        assert_eq!(
            query_timing_meta(1, 1, 27519, 1, 27510, 1),
            "1 rows · 27.5 s"
        );
        assert_eq!(
            query_timing_meta(1, 1, 27519, 300, 27200, 19),
            "1 rows · 27.5 s (db 27.2 · wait 0.3 · process 0.0)"
        );
    }

    #[test]
    fn remove_recent_only_removes_a_valid_entry() {
        let entry = |sql: &str| HistoryEntry {
            sql: sql.into(),
            ran_at: 0,
            engine: None,
            color: None,
        };
        let list = RefCell::new(vec![entry("first"), entry("second")]);
        assert!(remove_recent(&list, 0));
        assert_eq!(list.borrow()[0].sql, "second");
        assert!(!remove_recent(&list, 1));
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
    /// Folded statement heads. `default` so a tabs file written before folding
    /// was persisted still loads — it simply comes back fully expanded.
    #[serde(default)]
    folded_heads: Vec<usize>,
    #[serde(default = "default_split_ratio")]
    split_ratio: f32,
    #[serde(default)]
    connection_id: Option<String>,
    #[serde(default)]
    engine: String,
    #[serde(default)]
    connection_name: String,
}

fn default_split_ratio() -> f32 {
    0.5
}

/// Whether a connect should read the tabs back off disk. False once anything
/// has already restored them — normally `main` at startup, so the very first
/// connect of a session takes the keep-what-is-in-memory path like every
/// later one. Reconnecting must never re-read the file: the in-memory
/// workspace holds tabs and results created since the last persistence write.
fn should_restore_query_tabs(tabs_restored: bool) -> bool {
    !tabs_restored
}

/// Where the open query tabs are persisted.
///
/// `RDB_STORE_DIR` wins when set, the same way it overrides the connection and
/// settings stores. Without this the e2e harness would isolate connections and
/// settings but still read and overwrite the developer's real tabs, since
/// `ConnStore::query_tabs_path` resolves the platform config dir directly.
fn query_tabs_path() -> Option<std::path::PathBuf> {
    if let Ok(dir) = std::env::var("RDB_STORE_DIR") {
        return Some(std::path::PathBuf::from(dir).join("query_tabs.json"));
    }
    rdb_connstore::ConnStore::query_tabs_path().ok()
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct PersistedTabs {
    tabs: Vec<PersistedTab>,
    active: Option<String>,
    // Active tab in the right group and which group was focused last session.
    #[serde(default)]
    active_p1: Option<String>,
    #[serde(default)]
    active_group: usize,
}

/// Persist the SQL tabs (kind == "sql") plus each group's active tab and the
/// focused group (read from the window). Best-effort; I/O errors are ignored.
/// Skipped in mock mode.
fn save_query_tabs(w: &MainWindow, tabs: &[WorkspaceTab], active: Option<&str>) {
    if mock::mock_mode() {
        return;
    }
    let Some(path) = query_tabs_path() else {
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
            folded_heads: t.folded_heads.clone(),
            split_ratio: t.split_ratio,
            connection_id: t.connection_id.clone(),
            engine: t.engine.clone(),
            connection_name: t.connection_name.clone(),
        })
        .collect();
    let active = active
        .filter(|id| sql.iter().any(|t| t.id == *id))
        .map(|s| s.to_string());
    // Right-group active tab: map the p1 strip index back to a tab id.
    let active_p1 = tabs
        .iter()
        .filter(|t| t.group == 1)
        .nth(w.get_p1_active_tab().max(0) as usize)
        .filter(|t| t.kind == "sql")
        .map(|t| t.id.clone());
    let payload = PersistedTabs {
        tabs: sql,
        active,
        active_p1,
        active_group: (w.get_active_pane().max(0) as usize).min(1),
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(&payload) {
        let _ = std::fs::write(path, json);
    }
}

/// Tab ids are minted as `query:<connection>:<N>`; pull `N` back out so a
/// restored tab's number can be folded into the in-memory counter and never
/// re-handed-out to a freshly created tab.
fn tab_id_number(id: &str) -> Option<usize> {
    id.rsplit(':').next()?.parse().ok()
}

/// Load persisted SQL tabs into fresh `WorkspaceTab`s (empty results/browse).
/// Returns the tabs, each group's active tab id (if it still exists), the
/// focused group, and the highest tab number embedded in any restored id (0
/// if none) — callers fold this into their `query_number` counter so a
/// newly-created tab can never collide with a restored one.
fn load_query_tabs() -> (
    Vec<WorkspaceTab>,
    Option<String>,
    Option<String>,
    usize,
    usize,
) {
    let Some(path) = query_tabs_path() else {
        return (Vec::new(), None, None, 0, 0);
    };
    let Some(payload): Option<PersistedTabs> = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
    else {
        return (Vec::new(), None, None, 0, 0);
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
            t.folded_heads = p.folded_heads;
            t.split_ratio = p.split_ratio.clamp(0.2, 0.8);
            t.connection_id = p.connection_id;
            t.engine = p.engine;
            t.connection_name = p.connection_name;
            t
        })
        .collect();
    let max_number = tabs
        .iter()
        .filter_map(|t| tab_id_number(&t.id))
        .max()
        .unwrap_or(0);
    let exists = |id: &String| tabs.iter().any(|t| t.id == *id);
    let active = payload.active.filter(|id| exists(id));
    let active_p1 = payload.active_p1.filter(|id| exists(id));
    (
        tabs,
        active,
        active_p1,
        payload.active_group.min(1),
        max_number,
    )
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
    // Size the column to fit its header — bold name + dim type label in the mono
    // font — so real schema names (often long, `_`-joined) read without dragging.
    // Coefficients are deliberately a touch above the mono advance at font-sm/xs so
    // a name never lands right on the elision edge; the arrow + cell padding is the
    // constant. Clamped so nothing collapses or runs off-screen.
    let name_px = name.chars().count() as f32 * 8.4;
    let type_px = type_name.chars().count() as f32 * 6.8;
    (name_px + type_px + 56.0).clamp(100.0, 460.0)
}

#[cfg(test)]
mod col_width_tests {
    use super::default_col_width;

    #[test]
    fn fits_name_within_clamp() {
        // A longer header is wider, and both stay inside the clamp.
        let short = default_col_width("id", "int4");
        let long = default_col_width("alasan_penolakan", "varchar");
        assert!(long > short);
        assert!((100.0..=460.0).contains(&short));
        assert!((100.0..=460.0).contains(&long));
        // The bar sentinel keeps its fixed width.
        assert_eq!(default_col_width("share", "bar"), 520.0);
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
        rdb_connstore::Engine::MySql | rdb_connstore::Engine::MariaDb => {
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
        rdb_connstore::Engine::Redis | rdb_connstore::Engine::Valkey => {
            format!("BROWSE {} {offset} {limit}", table.name)
        }
        rdb_connstore::Engine::Mssql => {
            // ponytail: TOP-only — T-SQL's OFFSET/FETCH requires an ORDER BY
            // column, which there's no reliable default for here; paging
            // beyond the first page is a follow-up (same class of caveat as
            // Cassandra's LIMIT-only browse above).
            let where_sql = sql_where(
                col_filters,
                |c| format!("\"{}\"", c.replace('"', "\"\"")),
                "LIKE",
            );
            format!(
                "SELECT TOP ({limit}) * FROM \"{}\"{where_sql}",
                table.name.replace('"', "\"\"")
            )
        }
        rdb_connstore::Engine::Clickhouse => {
            let q = |s: &str| s.replace('"', "\"\"");
            let where_sql = sql_where(col_filters, |c| format!("\"{}\"", q(c)), "LIKE");
            let target = match table.database.as_deref() {
                Some(db) if !db.is_empty() => format!("\"{}\".\"{}\"", q(db), q(&table.name)),
                _ => format!("\"{}\"", q(&table.name)),
            };
            format!("SELECT * FROM {target}{where_sql} LIMIT {limit} OFFSET {offset}")
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
    // Multiple statements can't stream as one cursor (Postgres rejects multiple
    // commands in a prepared statement); they take the split-and-run path.
    if editor::split_statements(sql).len() > 1 {
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
    Done {
        capped: bool,
        elapsed_ms: u64,
        // Computed inside the same producer/consumer task, before Done is
        // sent, so it lands atomically with the rest of the result — no
        // window where the grid looks interactive but table/pk_cols hasn't
        // caught up yet (see the run_stream race this replaced).
        pk_hint: Option<(rdb_core::write::TableRef, Vec<String>)>,
    },
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

/// Update a `VecModel` to hold `rows` by mutating it in place — set existing
/// indices, push/remove the tail — instead of replacing the whole model. The
/// editor's line model is bound to a `for` that renders a per-line `TouchArea`;
/// swapping in a fresh model recreates every row and drops the `TouchArea`
/// currently grabbing a mouse drag, which kills drag-select. In-place updates
/// keep the row items (and the grab) alive.
fn sync_vec_model<T: Clone + 'static>(m: &VecModel<T>, rows: Vec<T>) {
    let cur = m.row_count();
    for (i, r) in rows.iter().enumerate() {
        if i < cur {
            m.set_row_data(i, r.clone());
        } else {
            m.push(r.clone());
        }
    }
    while m.row_count() > rows.len() {
        m.remove(m.row_count() - 1);
    }
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

/// The editor's error highlight, all 0-based in the full buffer: the line the
/// engine pointed at, the span of the statement it belongs to, and the token to
/// underline (`len == 0` when the engine reported no position, only a line).
#[derive(Clone, Copy)]
pub(crate) struct ErrorMark {
    pub(crate) line: i32,
    pub(crate) from: i32,
    pub(crate) to: i32,
    pub(crate) col: i32,
    pub(crate) len: i32,
}

/// Shift a driver's error spot into buffer coordinates. `origin` is where the
/// executed fragment starts (Run sends only one statement); its column applies
/// only on the fragment's own first line, past that the columns already line up.
fn mark_from(spot: &editor::ErrorSpot, origin: (i32, i32)) -> ErrorMark {
    ErrorMark {
        line: spot.line + origin.0,
        from: spot.stmt_lines.0 + origin.0,
        to: spot.stmt_lines.1 + origin.0,
        col: spot.col + if spot.line == 0 { origin.1 } else { 0 },
        len: spot.len,
    }
}

/// Slice `grid` down to the pane's finalized drag/shift-click range, if any
/// is active and still in bounds; otherwise return the grid unchanged. A
/// range is always exactly one column wide (the column the drag started
/// in) — see the `range-anchor-col` doc comment in tabular-grid.slint.
fn range_sliced_grid(
    grid: &model::GridModel,
    pane: usize,
) -> (model::GridModel, Option<(usize, usize, usize)>) {
    let range = SELECTED_RANGE.with(|s| s.borrow()[pane]);
    match range {
        Some((start, end, col)) if end < grid.rows.len() && col < grid.columns.len() => (
            model::GridModel {
                columns: vec![grid.columns[col].clone()],
                rows: grid.rows[start..=end]
                    .iter()
                    .map(|row| vec![row[col].clone()])
                    .collect(),
            },
            Some((start, end, col)),
        ),
        _ => (grid.clone(), None),
    }
}
/// Per-column indented JSON for one grid row, empty where the value isn't JSON.
/// Feeds the Details panel so a JSON cell reads as formatted text there too.
fn detail_pretty_row(g: &model::GridModel, row: usize) -> Vec<SharedString> {
    g.rows
        .get(row)
        .map(|r| {
            r.iter()
                .map(|cell| {
                    pretty_json(&cell.text)
                        .map(SharedString::from)
                        .unwrap_or_default()
                })
                .collect()
        })
        .unwrap_or_default()
}
fn refresh_detail_pretty(w: &MainWindow, pane: usize, g: &model::GridModel, row: i32) {
    let rows = detail_pretty_row(g, row.max(0) as usize);
    set_p_detail_pretty(w, pane, ModelRc::from(Rc::new(VecModel::from(rows))));
}
/// Snapshot the live client-side grid view of a group, to stash on the result the
/// user is leaving so switching back restores filters/sort/hidden/order/widths.
fn capture_grid_state(w: &MainWindow, pane: usize, g: &GroupRuntime) -> GridState {
    let mut hidden: Vec<usize> = g.hidden_cols.lock().unwrap().iter().copied().collect();
    hidden.sort_unstable();
    GridState {
        col_filters: g.col_filters.lock().unwrap().clone(),
        sort: *g.sort_state.lock().unwrap(),
        hidden,
        col_order: g.col_order.lock().unwrap().clone(),
        col_widths: get_p_col_widths(w, pane),
        grid_filter: w.get_grid_filter().to_string(),
        filter_col: w.get_filter_col().to_string(),
        filter_op: w.get_filter_op().to_string(),
    }
}
/// Re-apply a result's saved grid view after `present_view` reset it to defaults.
/// No-op when nothing was saved (`col_order` empty = never touched).
fn restore_grid_state(w: &MainWindow, pane: usize, g: &GroupRuntime, sr: &StoredResult) {
    let st = &sr.grid;
    if st.col_order.is_empty() {
        return;
    }
    let ncols = match &*sr.view {
        model::ResultView::Table(grid) => grid.columns.len(),
        model::ResultView::Documents(d) => d.grid.columns.len(),
        _ => return,
    };
    let hidden: HashSet<usize> = st.hidden.iter().copied().collect();
    *g.col_filters.lock().unwrap() = st.col_filters.clone();
    *g.sort_state.lock().unwrap() = st.sort;
    *g.hidden_cols.lock().unwrap() = hidden.clone();
    *g.col_order.lock().unwrap() = st.col_order.clone();
    set_p_grid_sort(w, pane, st.sort.0, st.sort.1);
    set_p_grid_col_filters(
        w,
        pane,
        ModelRc::from(Rc::new(VecModel::from(display_col_filters(
            &st.col_filters,
            &st.col_order,
            &hidden,
        )))),
    );
    // Column hiding is a left-group feature (no p1 setter); flags apply there.
    if pane == 0 {
        let flags: Vec<bool> = (0..ncols).map(|c| hidden.contains(&c)).collect();
        w.set_col_hidden(ModelRc::from(Rc::new(VecModel::from(flags))));
    }
    if !st.col_widths.is_empty() {
        set_p_col_widths(
            w,
            pane,
            ModelRc::from(Rc::new(VecModel::from(st.col_widths.clone()))),
        );
    }
    // Re-apply the search-bar filter that `present_view` reset to empty.
    let fcol = if st.filter_col.is_empty() {
        "any column".to_string()
    } else {
        st.filter_col.clone()
    };
    let fop = if st.filter_op.is_empty() {
        "=".to_string()
    } else {
        st.filter_op.clone()
    };
    w.set_grid_filter(st.grid_filter.clone().into());
    w.set_filter_col(fcol.clone().into());
    w.set_filter_op(fop.clone().into());
    let filtered = compute_view(
        &sr.view,
        &st.grid_filter.to_lowercase(),
        &fcol,
        &fop,
        &st.col_filters,
        &hidden,
        &st.col_order,
        st.sort.0,
        st.sort.1,
    );
    *g.displayed_grid.lock().unwrap() = view_grid(&filtered);
    apply_result(w, pane, &filtered);
}
/// What activating (choose/double-click) a non-section palette row does.
/// Index-aligned with the `Vec<PaletteItem>` `build_palette_items` returns —
/// section header rows get `None`.
#[derive(Debug, Clone)]
enum PaletteAction {
    None,
    Connect(usize),
    OpenTable(SharedString, SharedString),
    OpenFunction(SharedString),
    OpenSavedQuery(String, usize),
    OpenRecent(usize),
}

/// Formats a `/`-delimited group path (e.g. `"Work/Postgre"`) as `"Group: Work
/// · Subgroup: Postgre"`, or just `"Group: Work"` with no subgroup. Empty
/// string (no dangling separator to strip) when `group` is `None`/blank, so
/// callers can drop the whole line rather than showing an empty group.
fn group_sub_label(group: Option<&str>) -> String {
    let Some(g) = group.map(str::trim).filter(|g| !g.is_empty()) else {
        return String::new();
    };
    let mut parts = g.split('/').filter(|p| !p.trim().is_empty());
    match (parts.next(), parts.collect::<Vec<_>>().join(" / ")) {
        (Some(group), sub) if !sub.is_empty() => format!("Group: {group} · Subgroup: {sub}"),
        (Some(group), _) => format!("Group: {group}"),
        (None, _) => String::new(),
    }
}

type PaletteConnName = (
    String,
    &'static str,
    slint::Color,
    bool,
    SharedString,
    slint::Color,
    String,
);

/// Connection fields the ⌘K palette needs, including the normalized group
/// path so its search can match group/env like the sidebar filter does
/// (`build_conn_items`). Shared by both `on_toggle_palette` and
/// `on_palette_filter` so their tuple shape can't drift apart.
fn build_palette_conn_names(store: &rdb_connstore::ConnStore) -> Vec<PaletteConnName> {
    store
        .list()
        .iter()
        .map(|s| {
            (
                s.name.clone(),
                AnyDriver::badge(s.engine),
                theme::accent_or_default(s.color.as_deref().unwrap_or("")),
                s.color.is_some(),
                theme::env_tag_label(s.env_tag).into(),
                theme::env_tag_color(s.env_tag).unwrap_or_else(|| theme::accent_or_default("")),
                s.group
                    .as_deref()
                    .and_then(rdb_connstore::normalize_group_path)
                    .unwrap_or_default(),
            )
        })
        .collect()
}

/// Build the ⌘K palette model, grouped GitHub-style under non-selectable
/// "section" header rows. `needle` (lowercase) filters both groups; empty
/// shows everything. Empty groups drop their header. Returns the flat item
/// list alongside an index-aligned action list `on_palette_choose` dispatches
/// on.
fn build_palette_items(
    names: &[PaletteConnName],
    w: &MainWindow,
    saved_queries: &[(String, String)],
    recent_queries: &[HistoryEntry],
    needle: &str,
) -> (Vec<PaletteItem>, Vec<PaletteAction>) {
    let section = |t: &str| PaletteItem {
        label: t.into(),
        kind: "section".into(),
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
    };
    let mut items: Vec<PaletteItem> = Vec::new();
    let mut actions: Vec<PaletteAction> = Vec::new();
    let conns: Vec<_> = names
        .iter()
        .enumerate()
        .filter(|(_, (n, _, _, _, env_tag_label, _, group))| {
            needle.is_empty()
                || n.to_lowercase().contains(needle)
                || group.to_lowercase().contains(needle)
                || env_tag_label.to_lowercase().contains(needle)
        })
        .collect();
    if !conns.is_empty() {
        items.push(section("Connections"));
        actions.push(PaletteAction::None);
        for (idx, (n, badge, color, has_custom_color, env_tag_label, env_tag_color, group)) in conns
        {
            items.push(PaletteItem {
                label: n.clone().into(),
                kind: (*badge).into(),
                sub: group_sub_label(Some(group.as_str())).into(),
                local: false,
                color: *color,
                has_custom_color: *has_custom_color,
                env_tag_label: env_tag_label.clone(),
                env_tag_color: *env_tag_color,
                group: SharedString::default(),
                expanded: false,
                is_group_end: false,
                depth: 0,
            });
            actions.push(PaletteAction::Connect(idx));
        }
    }
    let schema_tree = w.get_schema_tree();
    let by_kind = |kind: &'static str| -> Vec<(SharedString, SharedString)> {
        schema_tree
            .iter()
            .filter(|n| {
                n.kind == kind && (needle.is_empty() || n.label.to_lowercase().contains(needle))
            })
            .map(|n| (n.db.clone(), n.label.clone()))
            .collect()
    };
    let tables = by_kind("table");
    if !tables.is_empty() {
        items.push(section("Tables"));
        actions.push(PaletteAction::None);
        for (db, label) in tables {
            items.push(PaletteItem {
                label: label.clone(),
                kind: "table".into(),
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
            });
            actions.push(PaletteAction::OpenTable(db, label));
        }
    }
    let views = by_kind("view");
    if !views.is_empty() {
        items.push(section("Views"));
        actions.push(PaletteAction::None);
        for (db, label) in views {
            items.push(PaletteItem {
                label: label.clone(),
                kind: "view".into(),
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
            });
            actions.push(PaletteAction::OpenTable(db, label));
        }
    }
    let functions = by_kind("function");
    if !functions.is_empty() {
        items.push(section("Functions"));
        actions.push(PaletteAction::None);
        for (_db, label) in functions {
            items.push(PaletteItem {
                label: label.clone(),
                kind: "function".into(),
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
            });
            actions.push(PaletteAction::OpenFunction(label));
        }
    }
    let saved: Vec<_> = saved_queries
        .iter()
        .enumerate()
        .filter(|(_, (n, _))| needle.is_empty() || n.to_lowercase().contains(needle))
        .collect();
    if !saved.is_empty() {
        items.push(section("Saved Queries"));
        actions.push(PaletteAction::None);
        for (idx, (name, sql)) in saved {
            items.push(PaletteItem {
                label: name.clone().into(),
                kind: "command".into(),
                sub: recent_preview(sql).into(),
                local: false,
                color: theme::accent_or_default(""),
                has_custom_color: false,
                env_tag_label: SharedString::default(),
                env_tag_color: theme::accent_or_default(""),
                group: SharedString::default(),
                expanded: false,
                is_group_end: false,
                depth: 0,
            });
            actions.push(PaletteAction::OpenSavedQuery(name.clone(), idx));
        }
    }
    let recent: Vec<_> = recent_queries
        .iter()
        .enumerate()
        .filter(|(_, e)| needle.is_empty() || e.sql.to_lowercase().contains(needle))
        .collect();
    if !recent.is_empty() {
        items.push(section("Recent History"));
        actions.push(PaletteAction::None);
        for (idx, entry) in recent {
            items.push(PaletteItem {
                label: recent_preview(&entry.sql).into(),
                kind: "command".into(),
                sub: entry.engine.clone().unwrap_or_default().into(),
                local: false,
                color: theme::accent_or_default(""),
                has_custom_color: false,
                env_tag_label: SharedString::default(),
                env_tag_color: theme::accent_or_default(""),
                group: SharedString::default(),
                expanded: false,
                is_group_end: false,
                depth: 0,
            });
            actions.push(PaletteAction::OpenRecent(idx));
        }
    }
    (items, actions)
}
/// Does any cell of `row` contain `needle` (already lowercased)? An empty
/// needle matches every row.
fn row_contains(row: &[model::VmCell], needle: &str) -> bool {
    needle.is_empty() || row.iter().any(|c| contains_ci(&c.text, needle))
}

/// Case-insensitive `contains`; `needle` must already be lowercase. The
/// all-ASCII case — which is nearly every cell — compares bytes in place
/// instead of allocating a lowercased copy of the haystack, and this runs once
/// per cell per keystroke of the filter box.
fn contains_ci(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.is_ascii() && needle.is_ascii() {
        let (h, n) = (haystack.as_bytes(), needle.as_bytes());
        n.len() <= h.len() && h.windows(n.len()).any(|w| w.eq_ignore_ascii_case(n))
    } else {
        haystack.to_lowercase().contains(needle)
    }
}

/// Case-insensitive equality against an already-lowercased `needle`, ASCII
/// fast path as in [`contains_ci`].
fn eq_ci(haystack: &str, needle: &str) -> bool {
    if haystack.is_ascii() && needle.is_ascii() {
        haystack.len() == needle.len() && haystack.eq_ignore_ascii_case(needle)
    } else {
        haystack.to_lowercase() == needle
    }
}

/// Case-insensitive ordering against an already-lowercased `needle`.
fn cmp_ci(haystack: &str, needle: &str) -> std::cmp::Ordering {
    if haystack.is_ascii() && needle.is_ascii() {
        haystack
            .bytes()
            .map(|b| b.to_ascii_lowercase())
            .cmp(needle.bytes())
    } else {
        haystack.to_lowercase().as_str().cmp(needle)
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
        "=" => eq_ci(&cell.text, needle),
        "≠" | "!=" | "<>" => !eq_ci(&cell.text, needle),
        ">" | "<" | ">=" | "<=" => {
            let ord = match (cell.text.parse::<f64>(), needle.parse::<f64>()) {
                (Ok(a), Ok(b)) => a.partial_cmp(&b),
                _ => Some(cmp_ci(&cell.text, needle)),
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
            .any(|v| eq_ci(&cell.text, v.trim().trim_matches(['\'', '"']))),
        "LIKE" | "ILIKE" => contains_ci(
            &cell.text,
            needle.trim().trim_matches(['\'', '"']).trim_matches('%'),
        ),
        _ => contains_ci(&cell.text, needle),
    }
}

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

/// Build the on-screen grid from a base result grid: keep the rows passing the
/// global filter and every per-column filter, order them by `sort_col`, then
/// project columns into `order` minus `hidden`. Column index arguments are
/// ORIGINAL indices into `base`.
///
/// One pass over row indices, materializing cells exactly once at the end.
/// This used to be four chained helpers, each deep-copying the entire grid —
/// four full copies of the result on every keystroke in a filter box, every
/// header click, and every column hide or reorder.
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
    let gcol = if fcol == "any column" {
        None
    } else {
        base.columns.iter().position(|c| c.name == fcol)
    };
    // null checks ignore the value box; other operators with an empty value
    // keep every row (matches the plain filter's behavior)
    let global_off = gcol.is_some()
        && needle.is_empty()
        && !matches!(fop, "is null" | "not null" | "IS NULL" | "IS NOT NULL");

    let per_col: Vec<(usize, &'static str, String)> = col_filters
        .iter()
        .enumerate()
        .filter_map(|(ci, raw)| parse_col_filter(raw).map(|(op, n)| (ci, op, n)))
        .collect();

    let mut keep: Vec<usize> = (0..base.rows.len())
        .filter(|&r| {
            let row = &base.rows[r];
            let global_ok = global_off
                || match gcol {
                    Some(c) => row
                        .get(c)
                        .is_some_and(|cell| cell_matches(cell, fop, needle)),
                    None => row_contains(row, needle),
                };
            global_ok
                && per_col
                    .iter()
                    .all(|(ci, op, n)| row.get(*ci).is_some_and(|cell| cell_matches(cell, op, n)))
        })
        .collect();

    if sort_col >= 0 && (sort_col as usize) < base.columns.len() {
        let c = sort_col as usize;
        // Decorate-sort-undecorate: the numeric parse and the lowercased text
        // are computed once per row instead of once per comparison.
        let mut keyed: Vec<(SortKey, usize)> = keep
            .into_iter()
            .map(|r| (SortKey::of(&base.rows[r][c]), r))
            .collect();
        keyed.sort_by(|a, b| {
            let ord = a.0.cmp(&b.0);
            // Nulls always sort last, regardless of direction, so they are
            // kept out of the reversal.
            match (a.0.is_null(), b.0.is_null()) {
                (true, true) => std::cmp::Ordering::Equal,
                (true, false) => std::cmp::Ordering::Greater,
                (false, true) => std::cmp::Ordering::Less,
                _ if sort_asc => ord,
                _ => ord.reverse(),
            }
        });
        keep = keyed.into_iter().map(|(_, r)| r).collect();
    }

    let idx: Vec<usize> = order
        .iter()
        .copied()
        .filter(|i| *i < base.columns.len() && !hidden.contains(i))
        .collect();

    model::GridModel {
        columns: idx.iter().map(|&i| base.columns[i].clone()).collect(),
        rows: keep
            .into_iter()
            .map(|r| idx.iter().map(|&c| base.rows[r][c].clone()).collect())
            .collect(),
    }
}

/// Sort key for one cell, computed once per row. Numeric cells order before
/// text cells so a mixed column still gets a total order; nulls carry their own
/// variant because they always sort last.
#[derive(PartialEq, PartialOrd)]
enum SortKey {
    Num(f64),
    Text(String),
    Null,
}

impl SortKey {
    fn of(cell: &model::VmCell) -> Self {
        if cell.is_null {
            SortKey::Null
        } else if let Ok(n) = cell.text.parse::<f64>() {
            SortKey::Num(n)
        } else {
            SortKey::Text(cell.text.to_lowercase())
        }
    }

    fn is_null(&self) -> bool {
        matches!(self, SortKey::Null)
    }

    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.partial_cmp(other).unwrap_or(std::cmp::Ordering::Equal)
    }
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
    /// Finalized (start, end) row range from the last drag/shift-click in the
    /// results grid, per pane. `None` once a plain click lands with no range,
    /// or once a fresh result is presented — Copy falls back to the full grid
    /// then.
    static SELECTED_RANGE: std::cell::RefCell<[Option<(usize, usize, usize)>; 2]> =
        std::cell::RefCell::new(Default::default());
    /// What each row of the currently displayed ⌘K palette list does when
    /// chosen, rebuilt alongside `palette_items` every open/filter — see
    /// `build_palette_items`.
    static PALETTE_ACTIONS: std::cell::RefCell<Vec<PaletteAction>> =
        const { std::cell::RefCell::new(Vec::new()) };
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
            full: SharedString::from(n.full.clone()),
            expandable: n.expandable,
            expanded,
            path: SharedString::from(n.path.clone()),
        })
        .collect();
    set_p_doc_tree(w, pane, ModelRc::from(Rc::new(VecModel::from(rows))));
}

/// Push a `ResultView` into the window, selecting the per-kind result region
/// via `result-kind` (0 Table, 1 Documents, 3 Affected).
fn apply_result(w: &MainWindow, pane: usize, view: &model::ResultView) {
    match view {
        model::ResultView::Table(g) => {
            set_p_result_kind(w, pane, 0);
            w.set_doc_json(SharedString::default());
            push_grid(w, pane, g);
            set_p_result_status(
                w,
                pane,
                SharedString::from(format!("{} rows", g.rows.len())),
            );
        }
        model::ResultView::Documents(d) => {
            set_p_result_kind(w, pane, 1);
            w.set_doc_json(SharedString::from(d.json.clone()));
            push_grid(w, pane, &d.grid);
            set_p_result_status(
                w,
                pane,
                SharedString::from(format!("{} documents", d.grid.rows.len())),
            );
            let collapsed = model::default_doc_collapsed(&d.tree);
            push_doc_tree(w, pane, &d.tree, &collapsed);
            DOC_TREES.with(|s| s.borrow_mut()[pane] = (d.tree.clone(), collapsed));
        }
        model::ResultView::Affected(status) => {
            set_p_result_kind(w, pane, 3);
            w.set_doc_json(SharedString::default());
            clear_grid(w, pane);
            set_p_result_status(w, pane, SharedString::from(status.clone()));
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
    v: &model::ResultView,
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
    let ncols = match v {
        model::ResultView::Table(g) => g.columns.len(),
        model::ResultView::Documents(d) => d.grid.columns.len(),
        _ => 0,
    };
    let shown = match v {
        model::ResultView::Table(g) => g.rows.len(),
        model::ResultView::Documents(d) => d.grid.rows.len(),
        model::ResultView::Affected(_) => 0,
    } as u64;
    *last_view.lock().unwrap() = Some(v.clone());
    set_p_grid_filter(w, pane, SharedString::default());
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
    let colnames: Vec<SharedString> = match v {
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
    set_p_filter_columns(w, pane, ModelRc::from(Rc::new(VecModel::from(fcols))));
    set_p_filter_col(
        w,
        pane,
        colnames
            .first()
            .cloned()
            .unwrap_or_else(|| SharedString::from("any column")),
    );
    if pane == 0 {
        w.set_all_columns(ModelRc::from(Rc::new(VecModel::from(colnames))));
    }
    *displayed_grid.lock().unwrap() = view_grid(v);
    {
        let st = browse.lock().unwrap();
        let mut b = edit_buf.lock().unwrap();
        b.clear();
        b.table = st.table.clone();
        b.pk_cols = st.pk_cols.clone();
    }
    set_p_pending_count(w, pane, 0);
    set_p_editing(w, pane, -1, -1);
    set_p_status_error(w, pane, false);
    w.set_status_latency(SharedString::from(latency));
    set_p_results_meta(w, pane, SharedString::from(meta));
    let widths: Vec<f32> = match v {
        model::ResultView::Table(g) => g
            .columns
            .iter()
            .map(|c| default_col_width(&c.name, &c.type_name))
            .collect(),
        _ => vec![140.0; ncols],
    };
    set_p_col_widths(w, pane, ModelRc::from(Rc::new(VecModel::from(widths))));
    let bars: Vec<ChartBar> = match v {
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
    // A fresh result starts on the first row. A stale selection left over from a
    // larger previous result is out of range for the new (smaller) one, and the
    // Details gate `(selected-row + 1) * col-count <= cells.length` then evaluates
    // false — so the panel silently vanishes even while its toggle is on.
    set_p_selected_row(w, pane, 0);
    set_p_range_anchor(w, pane, -1);
    set_p_range_anchor_col(w, pane, -1);
    SELECTED_RANGE.with(|s| s.borrow_mut()[pane] = None);
    // Seed the Details JSON preview for the first row of the new result.
    if let Some(g) = displayed_grid.lock().unwrap().as_ref() {
        refresh_detail_pretty(w, pane, g, 0);
    }
    let st = browse.lock().unwrap().clone();
    if st.table.is_some() {
        let (start, end, prev, next) = page_bounds(st.page, st.limit, st.total, shown);
        set_p_page_bounds(w, pane, start as i32, end as i32, prev, next);
        set_p_total_rows(w, pane, st.total.map(|t| t as i32).unwrap_or(-1));
    }
}

/// Set the result-tab strip ("Result 1", …, each badged with the connection
/// that produced it) and active index.
fn set_result_tabs(w: &MainWindow, pane: usize, results: &[StoredResult], active: usize) {
    let items: Vec<ResultTabItem> = results
        .iter()
        .enumerate()
        .map(|(n, r)| {
            let label = if r.connection_name.is_empty() {
                format!("Result {}", n + 1)
            } else {
                format!("{} {}", r.connection_name, n + 1)
            };
            ResultTabItem {
                label: label.into(),
                engine: r.engine.clone().into(),
                connection_name: r.connection_name.clone().into(),
                color: r.color,
                has_custom_color: r.has_custom_color,
            }
        })
        .collect();
    let items = ModelRc::from(Rc::new(VecModel::from(items)));
    if pane == 0 {
        w.set_result_tabs(items);
        w.set_active_result(active as i32);
    } else {
        w.set_p1_result_tabs(items);
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

/// Reset the connection form to "new connection" defaults and open it.
/// `parent` is the top-level group to pre-nest under ("New Subgroup" on the
/// picker's group context menu): `Some(g)` starts Group on `g` and Subgroup on
/// "+ New subgroup…" so the user only types the leaf name; `None` starts both
/// on "None". Engine defaults come from `rdb_connstore::ENGINES`.
fn open_add_form(w: &MainWindow, store: &rdb_connstore::ConnStore, parent: Option<&str>) {
    let engine = rdb_connstore::Engine::Postgres;
    w.set_form_edit_mode(false);
    w.set_f_name(SharedString::default());
    w.set_f_engine(SharedString::from(engine.display()));
    w.set_f_host(SharedString::from("localhost"));
    w.set_f_port(SharedString::from(engine.default_port()));
    w.set_f_user(SharedString::default());
    w.set_f_database(SharedString::default());
    w.set_f_password(SharedString::default());
    w.set_f_has_password(false);
    // Reset alongside it, or a warning from a previously edited connection
    // would follow the user into a brand-new form.
    w.set_f_password_unreadable(false);
    w.set_f_sslmode(SharedString::from("Disable"));
    w.set_f_params(SharedString::default());
    w.set_f_color(SharedString::from("#2c5fd8"));
    w.set_f_env_tag(SharedString::from("None"));
    w.set_f_ssh_enabled(false);
    w.set_f_ssh_host(SharedString::default());
    w.set_f_ssh_port(SharedString::from("22"));
    w.set_f_ssh_user(SharedString::default());
    w.set_f_ssh_auth_mode(SharedString::from("Agent"));
    w.set_f_ssh_key_path(SharedString::default());
    w.set_f_ssh_password(SharedString::default());
    w.set_f_ssh_passphrase(SharedString::default());
    w.set_f_has_ssh_secret(false);
    w.set_f_new_group_text(SharedString::default());
    w.set_f_new_subgroup_text(SharedString::default());
    w.set_group_options(ModelRc::from(Rc::new(VecModel::from(
        group_picker_options(store)
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<_>>(),
    ))));
    match parent {
        Some(parent) => {
            w.set_f_group_display(SharedString::from(parent));
            w.set_f_subgroup_display(SharedString::from("+ New subgroup…"));
            w.set_subgroup_options(ModelRc::from(Rc::new(VecModel::from(
                subgroup_picker_options(store, parent)
                    .into_iter()
                    .map(SharedString::from)
                    .collect::<Vec<_>>(),
            ))));
        }
        None => {
            w.set_f_group_display(SharedString::from("None"));
            w.set_f_subgroup_display(SharedString::from("None"));
            w.set_subgroup_options(ModelRc::from(Rc::new(VecModel::from(vec![
                SharedString::from("None"),
                SharedString::from("+ New subgroup…"),
            ]))));
        }
    }
    w.set_f_import_url(SharedString::default());
    w.set_form_error(SharedString::default());
    w.set_test_result(SharedString::default());
    w.set_test_ok(false);
    w.set_test_busy(false);
    w.set_form_open(true);
}

// Both of these read the engine picker's display label. They resolve through
// `rdb_connstore::ENGINES` rather than their own string tables, so adding an
// engine is one row there instead of two matches here that nothing
// cross-checks. An unrecognized label still has to answer something; Postgres
// stays the fallback, as before.
fn default_port(engine_label: &str) -> &'static str {
    label_to_engine(engine_label).default_port()
}

fn label_to_engine(label: &str) -> rdb_connstore::Engine {
    rdb_connstore::Engine::from_display(label).unwrap_or(rdb_connstore::Engine::Postgres)
}

/// The connection fields the form holds, already validated and normalized.
/// Shared by Test Connection and Save, which used to carry byte-identical
/// copies of this parsing — including the SSL-mode comment below — and only
/// differed in where the error text lands.
struct FormConn {
    engine: rdb_connstore::Engine,
    host: String,
    port: u16,
    user: String,
    database: Option<String>,
    password: Option<String>,
    params: Option<String>,
    sslmode: rdb_core::conn::SslMode,
    ssh_enabled: bool,
    ssh_host: Option<String>,
    ssh_port: Option<u16>,
    ssh_user: Option<String>,
    ssh_auth_mode: rdb_core::conn::SshAuthMode,
    ssh_key_path: Option<String>,
    ssh_password: Option<String>,
    ssh_passphrase: Option<String>,
}

/// Read the connection form into a validated `FormConn`, or the message to
/// show the user. Empty optional fields normalize to `None`; for the password
/// that means "keep the stored secret", which `ConnStore::save_connection`
/// relies on.
fn read_conn_form(w: &MainWindow) -> Result<FormConn, &'static str> {
    let engine = label_to_engine(w.get_f_engine().as_ref());
    // SQLite is a local file: no host/port, the file path lives in database.
    let (host, port) = if engine == rdb_connstore::Engine::Sqlite {
        if w.get_f_database().to_string().trim().is_empty() {
            return Err("file path is required");
        }
        (String::new(), 0u16)
    } else {
        let host = w.get_f_host().to_string();
        if host.trim().is_empty() {
            return Err("host is required");
        }
        let port: u16 = match w.get_f_port().to_string().parse() {
            Ok(p) if p != 0 => p,
            _ => return Err("port must be a number 1-65535"),
        };
        (host, port)
    };
    let sslmode = match w.get_f_sslmode().to_string().as_str() {
        "Require" => rdb_core::conn::SslMode::Require,
        "Prefer" => rdb_core::conn::SslMode::Prefer,
        // Empty/unset (the SSL mode field is hidden for engines like Redis
        // that never populate `f-sslmode`) must not silently become `Prefer`
        // — that maps to `rediss://` and hangs a TLS handshake against a
        // plain server instead of failing fast or connecting at all.
        _ => rdb_core::conn::SslMode::Disable,
    };
    let non_empty = |s: String| if s.trim().is_empty() { None } else { Some(s) };

    let ssh_enabled = w.get_f_ssh_enabled();
    let ssh_host = non_empty(w.get_f_ssh_host().to_string());
    let ssh_port: Option<u16> = w.get_f_ssh_port().to_string().parse().ok();
    let ssh_user = non_empty(w.get_f_ssh_user().to_string());
    let ssh_auth_mode = rdb_core::conn::SshAuthMode::parse(w.get_f_ssh_auth_mode().as_ref());
    let ssh_key_path = non_empty(w.get_f_ssh_key_path().to_string());
    let ssh_password = non_empty(w.get_f_ssh_password().to_string());
    let ssh_passphrase = non_empty(w.get_f_ssh_passphrase().to_string());

    if ssh_enabled && engine != rdb_connstore::Engine::Sqlite {
        if ssh_host.is_none() {
            return Err("SSH host is required when SSH tunnel is enabled");
        }
        if ssh_user.is_none() {
            return Err("SSH user is required when SSH tunnel is enabled");
        }
    }

    Ok(FormConn {
        engine,
        host,
        port,
        user: w.get_f_user().to_string(),
        database: {
            let d = w.get_f_database().to_string();
            if d.is_empty() {
                None
            } else {
                Some(d)
            }
        },
        password: {
            let p = w.get_f_password().to_string();
            if p.is_empty() {
                None
            } else {
                Some(p)
            }
        },
        params: non_empty(w.get_f_params().to_string()),
        sslmode,
        ssh_enabled,
        ssh_host,
        ssh_port,
        ssh_user,
        ssh_auth_mode,
        ssh_key_path,
        ssh_password,
        ssh_passphrase,
    })
}

/// The live driver behind an `Arc` so callers clone it out of the mutex and run
/// queries/pings lock-free: the mutex only guards the slot swap, never a whole
/// query. Drivers are `&self`, internally pooled and cheap to clone, so this
/// removes query-vs-ping (and query-vs-query) serialization.
type DriverSlot = Arc<tokio::sync::Mutex<Option<(rdb_connstore::Engine, Arc<AnyDriver>)>>>;

/// Slot holding the "re-run the current browse query" closure. Set once the
/// browse view knows what it is browsing; `None` before that.
type BrowseTrigger = Rc<RefCell<Option<Rc<dyn Fn()>>>>;

/// Shared state handed to the wiring modules in `wire`.
///
/// Each field is already an `Rc`/`Arc`, so cloning the struct is a handful of
/// refcount bumps — the point is that a handler no longer opens with a stack of
/// individual `let x = x.clone();` lines naming exactly what it captures, which
/// is what kept every one of them pinned inside `main`. Fields are added as
/// each cluster of handlers moves out.
///
/// Deliberately `!Send` (the `Rc` fields): anything crossing onto a tokio task
/// still has to clone the specific `Arc` handles it needs, which is what the
/// spawning callbacks already did.
#[derive(Clone)]
struct AppState {
    rt: Arc<tokio::runtime::Runtime>,
    store: Rc<RefCell<rdb_connstore::ConnStore>>,
    settings: Rc<RefCell<rdb_connstore::SettingsStore>>,
    history_cap: Rc<Cell<usize>>,
    current: DriverSlot,
    raw_nodes: Arc<Mutex<Vec<model::VmTreeNode>>>,
    expanded_tables: Arc<Mutex<HashSet<String>>>,
    loaded_dbs: Arc<Mutex<HashSet<String>>>,
    last_view: Arc<Mutex<Option<model::ResultView>>>,
    browse_trigger: BrowseTrigger,
    query_console: Arc<Mutex<Vec<String>>>,
    workspace_tabs: Arc<Mutex<Vec<WorkspaceTab>>>,
    active_tab_id: Arc<Mutex<Option<String>>>,
    active_group1_tab_id: Arc<Mutex<Option<String>>>,
    current_connection_id: Arc<Mutex<Option<String>>>,
    query_number: Arc<std::sync::atomic::AtomicUsize>,
    collapsed_categories: Rc<RefCell<HashSet<String>>>,
    sidebar_filter: Arc<Mutex<String>>,
    collapsed_history_groups: Rc<RefCell<HashSet<String>>>,
    conn_modal_map: Rc<RefCell<Vec<i32>>>,
    db_override: Arc<Mutex<Option<String>>>,
    table_cols: Rc<VecModel<TableCol>>,
    fn_defs: Arc<Mutex<HashMap<String, String>>>,
    completion_nodes: Arc<Mutex<Vec<model::VmTreeNode>>>,
    connect_handle: Rc<RefCell<Option<tokio::task::JoinHandle<()>>>>,
    tabs_restored: Rc<Cell<bool>>,
    panes: Rc<[GroupRuntime; 2]>,
    cur_engine: Rc<RefCell<Option<rdb_connstore::Engine>>>,
    saved_queries: Rc<RefCell<Vec<(String, String)>>>,
    recent_queries: Rc<RefCell<Vec<HistoryEntry>>>,
    collapsed: Rc<RefCell<HashSet<String>>>,
    conn_filter: Rc<RefCell<String>>,
    editing_id: Rc<RefCell<String>>,
}

/// Callback shapes used by [`AppFns`]. Named so the struct reads as a list of
/// jobs rather than a wall of `Rc<dyn Fn(...)>`.
type PaneTextFn = Rc<dyn Fn(usize, &str)>;
type PaneFn = Rc<dyn Fn(usize)>;
type WindowFn = Rc<dyn Fn(&MainWindow)>;
type WindowPaneFn = Rc<dyn Fn(&MainWindow, usize)>;
type WindowGuardFn = Rc<dyn Fn(&MainWindow) -> bool>;
type PaneSqlFn = Rc<dyn Fn(usize, String)>;

/// The long-lived closures `main` builds once and every handler leans on:
/// re-render the sidebar, re-run a query, restore a tab, and so on.
///
/// Separate from [`AppState`] because these are built *from* that state, in
/// dependency order — a wiring module gets both.
#[derive(Clone)]
struct AppFns {
    rebuild_query_tree: Rc<dyn Fn(&str)>,
    load_editor_text: PaneTextFn,
    save_active_tab: WindowFn,
    restore_tab: WindowPaneFn,
    save_p1_tab: WindowFn,
    restore_p1_tab: WindowPaneFn,
    guard_pending: WindowGuardFn,
    run_sql: PaneSqlFn,
    run_stream: PaneSqlFn,
    sync_editor: PaneFn,
}

fn main() -> Result<(), slint::PlatformError> {
    // Name this build to every server it connects to, so RDB is attributable in
    // pg_stat_activity / SHOW PROCESSLIST / currentOp rather than showing up as
    // an anonymous client. Set before any connection can be opened. The version
    // has to come from here: rdb-core carries its own crate version, which is
    // not the one release-please bumps.
    rdb_core::conn::set_client_id(format!("RDB {}", env!("CARGO_PKG_VERSION")));

    // tokio multi-thread runtime on background threads; the Slint event loop
    // owns the main thread. Async results return via invoke_from_event_loop.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    // Kept alive for the rest of main(): Slint's winit backend, rfd, and
    // notify-rust all pull in zbus on Linux and need an entered runtime the
    // moment they touch D-Bus during startup, before any of our own async
    // code runs.
    let _rt_guard = rt.enter();
    let rt = Arc::new(rt);

    let window = MainWindow::new()?;
    let (primary, shift, separator, enter, backspace) = shortcut_labels(std::env::consts::OS);
    let shortcuts = Shortcut::get(&window);
    shortcuts.set_primary(primary.into());
    shortcuts.set_shift(shift.into());
    shortcuts.set_separator(separator.into());
    shortcuts.set_enter(enter.into());
    shortcuts.set_backspace(backspace.into());

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
        .set_mode(settings.borrow().get().theme.to_index());
    window
        .global::<Tokens>()
        .set_font_base(clamp_font_size(settings.borrow().get().editor.font_size as i32) as f32);
    window.set_update_check_enabled(settings.borrow().get().update_check);
    window.set_sidebar_right(settings.borrow().get().ui_state.sidebar_right);
    let history_cap = Rc::new(Cell::new(
        settings.borrow().get().editor.history_max_entries.max(1) as usize,
    ));
    window.set_history_max_entries(history_cap.get() as i32);
    window.set_nosql_collection_limit(settings.borrow().get().nosql_collection_limit.max(1) as i32);
    window.set_auto_table_alias(settings.borrow().get().editor.auto_table_alias);
    window.set_error_highlight(settings.borrow().get().editor.error_highlight);
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
    // The live driver behind an Arc so callers clone it out of the mutex and run
    // queries/pings lock-free: the mutex only guards the slot swap, never a whole
    // query. Drivers are `&self`, internally pooled and cheap to clone, so this
    // removes query↔ping (and query↔query) serialization.
    let current: DriverSlot = Arc::new(tokio::sync::Mutex::new(None));

    // Set of group labels the user has collapsed in the sidebar.
    let collapsed: Rc<RefCell<HashSet<String>>> = Rc::new(RefCell::new(HashSet::new()));
    if mock::mock_mode() {
        // Reference boots with only ACME expanded.
        let mut c = collapsed.borrow_mut();
        for g in ["CORE", "LOCAL", "EDGE", UNGROUPED] {
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
    let panes: Rc<[GroupRuntime; 2]> = Rc::new([GroupRuntime::new(), GroupRuntime::new()]);
    // Browse-mode pagination state (open container + page window + pk).
    let browse = panes[0].browse.clone();
    // Buffered, uncommitted grid edits (⌘S commits, Esc/Discard drops).
    let edit_buf = panes[0].edit_buf.clone();
    // The grid currently on screen (post client-side filter): edit-buffer row
    // indices refer to THIS grid, and its cells carry the pre-edit pk values.
    // Column indices (into the last result) hidden via the Columns popup.
    // Client-side sort: ORIGINAL column index (-1 = none) + ascending flag.
    // Display order of columns as ORIGINAL indices (drag-to-reorder). Reset to
    // 0..ncols on every fresh result.
    // Per-column filter boxes, raw text indexed by ORIGINAL column. Empty =
    // no filter. Reset to blanks on every fresh result.
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
    let active_group1_tab_id: Arc<std::sync::Mutex<Option<String>>> =
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
    // Shared UI state that outlives every callback. Declared together so
    // `AppState` below can be built in one place; each of these used to be
    // introduced next to the first handler that happened to need it.
    let conn_modal_map: Rc<RefCell<Vec<i32>>> = Rc::new(RefCell::new(Vec::new()));
    let table_cols: Rc<VecModel<TableCol>> = Rc::new(VecModel::default());
    window.set_table_cols(ModelRc::from(table_cols.clone()));
    let last_view: Arc<std::sync::Mutex<Option<model::ResultView>>> =
        Arc::new(std::sync::Mutex::new(None));
    let browse_trigger: BrowseTrigger = Rc::new(RefCell::new(None));
    let connect_handle: Rc<RefCell<Option<tokio::task::JoinHandle<()>>>> =
        Rc::new(RefCell::new(None));
    let editing_id: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));
    // ----- saved/recent queries (sidebar Queries tab) -----
    // User-curated saved queries: seeded on first run, then editable (delete)
    // and persisted to disk. Mock mode always shows the seed and never writes.
    let saved_queries: Rc<RefCell<Vec<(String, String)>>> =
        Rc::new(RefCell::new(if mock::mock_mode() {
            default_saved()
        } else {
            load_saved()
        }));
    // Live history: filled as queries run; mock mode seeds a few for the
    // screenshot harness.
    let recent_queries: Rc<RefCell<Vec<HistoryEntry>>> =
        Rc::new(RefCell::new(if mock::mock_mode() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            vec![
                HistoryEntry {
                    sql: "SELECT * FROM emiten LIMIT 100;".into(),
                    ran_at: now,
                    engine: Some("postgres".into()),
                    color: None,
                },
                HistoryEntry {
                    sql: "INSERT INTO sectors (name) VALUES ('Technology');".into(),
                    ran_at: now,
                    engine: Some("postgres".into()),
                    color: None,
                },
                HistoryEntry {
                    sql: "UPDATE emiten SET updated_at = now() WHERE code = '93344';".into(),
                    ran_at: now,
                    engine: Some("postgres".into()),
                    color: None,
                },
            ]
        } else {
            let mut recent = load_recent();
            recent.truncate(history_cap.get());
            recent
        }));
    // Date-bucket headers in the History tab (e.g. "Today"/"Yesterday") that
    // the user has folded. Kept separate from `collapsed_categories` (the
    // Items-tree schema headers) so toggling one never touches the other.
    let collapsed_history_groups: Rc<RefCell<HashSet<String>>> =
        Rc::new(RefCell::new(HashSet::new()));
    // ----- function definitions captured at connect (name → CREATE source) -----
    let fn_defs: Arc<std::sync::Mutex<std::collections::HashMap<String, String>>> =
        Arc::new(std::sync::Mutex::new(std::collections::HashMap::new()));

    // One bundle handed to every wiring module below; see `AppState`.
    let state = AppState {
        rt: rt.clone(),
        store: store.clone(),
        settings: settings.clone(),
        history_cap: history_cap.clone(),
        current: current.clone(),
        raw_nodes: raw_nodes.clone(),
        expanded_tables: expanded_tables.clone(),
        loaded_dbs: loaded_dbs.clone(),
        last_view: last_view.clone(),
        browse_trigger: browse_trigger.clone(),
        query_console: query_console.clone(),
        workspace_tabs: workspace_tabs.clone(),
        active_tab_id: active_tab_id.clone(),
        active_group1_tab_id: active_group1_tab_id.clone(),
        current_connection_id: current_connection_id.clone(),
        query_number: query_number.clone(),
        collapsed_categories: collapsed_categories.clone(),
        sidebar_filter: sidebar_filter.clone(),
        collapsed_history_groups: collapsed_history_groups.clone(),
        conn_modal_map: conn_modal_map.clone(),
        db_override: db_override.clone(),
        table_cols: table_cols.clone(),
        fn_defs: fn_defs.clone(),
        completion_nodes: completion_nodes.clone(),
        connect_handle: connect_handle.clone(),
        tabs_restored: tabs_restored.clone(),
        panes: panes.clone(),
        cur_engine: cur_engine.clone(),
        saved_queries: saved_queries.clone(),
        recent_queries: recent_queries.clone(),
        collapsed: collapsed.clone(),
        conn_filter: conn_filter.clone(),
        editing_id: editing_id.clone(),
    };

    let rebuild_sidebar = {
        let weak = window.as_weak();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            w.set_connections(build_sidebar_model(
                &store.borrow(),
                &collapsed.borrow(),
                &conn_filter.borrow(),
            ));
        }
    };
    rebuild_sidebar();
    window.set_schema_tree(ModelRc::from(Rc::new(VecModel::<TreeNode>::default())));

    let (sync_editor, load_editor_text) = wire::editor::wire(&window, &state);
    let ed_state = panes[0].ed_state.clone();

    // Query tabs are documents, not connection state: show them as soon as the
    // window opens rather than making the user connect first. `wire::connect`
    // then sees `tabs_restored` already true and keeps this in-memory set
    // across the first connect instead of re-reading the file.
    //
    // Skipped in mock mode, where the screenshot harness must not pick up
    // whatever the developer left on disk.
    if !mock::mock_mode() {
        let (tabs, active, active_p1, active_group, max_number) = load_query_tabs();
        if !tabs.is_empty() {
            // Never let a freshly-minted tab reuse a number a restored tab
            // already holds — `fetch_max` only ever raises the counter.
            query_number.fetch_max(max_number, std::sync::atomic::Ordering::Relaxed);
            let init_text = active
                .as_deref()
                .and_then(|id| {
                    tabs.iter()
                        .find(|t| t.id == id)
                        .map(|t| t.query_text.clone())
                })
                .unwrap_or_default();
            set_workspace_tabs(&window, &tabs, active.as_deref());
            window.set_active_pane(active_group as i32);
            *workspace_tabs.lock().unwrap() = tabs;
            *active_tab_id.lock().unwrap() = active;
            *active_group1_tab_id.lock().unwrap() = active_p1;
            load_editor_text(0, &init_text);
            tabs_restored.set(true);
        }
    }

    let save_active_tab: Rc<dyn Fn(&MainWindow)> = {
        let tabs = workspace_tabs.clone();
        let active_id = active_tab_id.clone();
        let ed_state = ed_state.clone();
        let browse = browse.clone();
        let results = results.clone();
        let active_result = active_result.clone();
        let last_view = last_view.clone();
        let panes = panes.clone();
        Rc::new(move |w| {
            let Some(id) = active_id.lock().unwrap().clone() else {
                return;
            };
            let mut tabs = tabs.lock().unwrap();
            let Some(tab) = tabs.iter_mut().find(|tab| tab.id == id) else {
                return;
            };
            tab.query_text = ed_state.borrow().text();
            // Folds index into the text above, so they are snapshotted with it.
            tab.folded_heads = snapshot_folds(&panes[0].folded_heads);
            tab.loading = w.get_query_running();
            if tab.kind == "table" {
                tab.browse = browse.lock().unwrap().clone();
            } else if tab.kind == "sql" {
                // Snapshot the live grid (sort/filters/search) into the active
                // result before cloning, so it survives a tab switch.
                let ai = *active_result.lock().unwrap();
                if let Some(r) = results.lock().unwrap().get_mut(ai) {
                    r.grid = capture_grid_state(w, 0, &panes[0]);
                }
                tab.results = results.lock().unwrap().clone();
                tab.active_result = ai;
            }
            if tab.kind != "function" {
                let (view_id, view_engine, view_name, view_color, view_has_custom_color) = tab
                    .results
                    .get(tab.active_result)
                    .map(|r| {
                        (
                            r.connection_id.clone(),
                            r.engine.clone(),
                            r.connection_name.clone(),
                            r.color,
                            r.has_custom_color,
                        )
                    })
                    .unwrap_or_else(|| {
                        (
                            None,
                            String::new(),
                            String::new(),
                            theme::accent_or_default(""),
                            false,
                        )
                    });
                tab.view = last_view.lock().unwrap().clone().map(|view| StoredResult {
                    view: Arc::new(view),
                    meta: w.get_results_meta().to_string(),
                    latency: w.get_status_latency().to_string(),
                    grid: GridState::default(),
                    connection_id: view_id,
                    engine: view_engine,
                    connection_name: view_name,
                    color: view_color,
                    has_custom_color: view_has_custom_color,
                });
            }
            // Persist SQL scratch tabs so they survive a restart. Cheap JSON
            // write; fires on switch/new/close/run, which is when text changes.
            save_query_tabs(w, &tabs, Some(&id));
        })
    };

    // The second workspace group has its own live editor/result runtime. Save
    // it before selecting, closing, or moving one of its tabs just like the
    // left group; otherwise a drag could move an older snapshot.
    let save_p1_tab: Rc<dyn Fn(&MainWindow)> = {
        let tabs = workspace_tabs.clone();
        let active_id = active_group1_tab_id.clone();
        // The left group's active tab is needed only to hand it back to
        // `save_query_tabs`, which rewrites the whole file: passing `None` here
        // erased it, so any right-pane action left the workspace with no active
        // tab on the left and the next launch restored it unfocused.
        let left_active_id = active_tab_id.clone();
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
            tab.folded_heads = snapshot_folds(&panes[1].folded_heads);
            tab.loading = w.get_p1_query_running();
            if tab.kind == "sql" {
                tab.results = panes[1].results.lock().unwrap().clone();
                tab.active_result = *panes[1].active_result.lock().unwrap();
            }
            let left_active = left_active_id.lock().unwrap().clone();
            save_query_tabs(w, &tabs, left_active.as_deref());
        })
    };

    #[allow(clippy::type_complexity)]
    // Restore a tab into a group's runtime (`pane` 0 left / 1 right). `group_index`
    // is the position within that group's tab strip. Both groups share this path;
    // per-group runtime state lives in `panes[pane]`. Chrome that is still a shared
    // MainWindow property (fn-mode/active-table/pagination footer/indexes) is only
    // painted for the left group until it is mirrored per-group.
    // Restore the tab at absolute index `abs_index` into ITS group's runtime
    // (`pane` = tab.group). `group_index` (position within that group's strip) is
    // derived. Per-group runtime state lives in `panes[pane]`; chrome that is still
    // a shared MainWindow property is only painted for the left group for now.
    #[allow(clippy::type_complexity)]
    let restore_tab_for_pane: Rc<dyn Fn(&MainWindow, usize)> = {
        let tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let active_group1_tab_id = active_group1_tab_id.clone();
        let load_editor_text = load_editor_text.clone();
        let panes = panes.clone();
        let last_view = last_view.clone();
        Rc::new(move |w, abs_index| {
            let (tab, pane, group_index) = {
                let tabs = tabs.lock().unwrap();
                let Some(tab) = tabs.get(abs_index).cloned() else {
                    return;
                };
                let pane = tab.group.min(1);
                let group_index = group_relative_index(&tabs, abs_index);
                (tab, pane, group_index)
            };
            if pane == 0 {
                *active_tab_id.lock().unwrap() = Some(tab.id.clone());
            } else {
                *active_group1_tab_id.lock().unwrap() = Some(tab.id.clone());
            }
            {
                // Keep the group-0 selection stable when restoring the right group.
                let tabs_guard = tabs.lock().unwrap();
                let left_active = active_tab_id.lock().unwrap().clone();
                set_workspace_tabs(w, &tabs_guard, left_active.as_deref());
            }
            w.set_active_pane(pane as i32);
            if pane == 1 {
                w.set_p1_active_tab(group_index as i32);
            }

            load_editor_text(pane, &tab.query_text);
            // Replace the pane's fold set with this tab's, rather than leaving
            // the previous tab's line numbers to fold whatever now sits at
            // them. The set is only meaningful against the text just loaded.
            {
                let mut folded = panes[pane].folded_heads.borrow_mut();
                folded.clear();
                folded.extend(tab.folded_heads.iter().copied());
            }

            // Per-group browse state (drives that group's pagination + edits).
            let browse = &panes[pane].browse;
            if tab.kind == "table" {
                *browse.lock().unwrap() = tab.browse.clone();
            } else {
                let limit = browse.lock().unwrap().limit;
                *browse.lock().unwrap() = BrowseState {
                    limit,
                    ..Default::default()
                };
            }

            // Function source is still a left-group view; table chrome is fully
            // group-local so moving a table keeps its toolbar and footer.
            if pane == 0 {
                w.set_fn_mode(tab.kind == "function");
                w.set_query_running(tab.loading);
            }
            set_p_active_table(
                w,
                pane,
                tab.table
                    .as_ref()
                    .map(|table| SharedString::from(table.name.clone()))
                    .unwrap_or_default(),
            );
            set_p_total_rows(w, pane, tab.browse.total.map(|n| n as i32).unwrap_or(-1));
            set_p_read_only(w, pane, tab.browse.pk_cols.is_empty());
            let index_rows: Vec<IndexRow> = tab
                .indexes
                .iter()
                .cloned()
                .map(|(name, definition)| IndexRow {
                    name: name.into(),
                    definition: definition.into(),
                })
                .collect();
            set_p_index_rows(w, pane, ModelRc::from(Rc::new(VecModel::from(index_rows))));

            *panes[pane].results.lock().unwrap() = tab.results.clone();
            *panes[pane].active_result.lock().unwrap() = tab.active_result;
            set_result_tabs(w, pane, &tab.results, tab.active_result);
            let selected = if tab.kind == "sql" {
                tab.results
                    .get(tab.active_result)
                    .cloned()
                    .or(tab.view.clone())
            } else {
                tab.view.clone()
            };
            if let Some(stored) = selected {
                present_view(
                    w,
                    pane,
                    &stored.view,
                    &stored.meta,
                    &stored.latency,
                    &last_view,
                    &panes[pane].displayed_grid,
                    &panes[pane].hidden_cols,
                    &panes[pane].sort_state,
                    &panes[pane].col_order,
                    &panes[pane].col_filters,
                    &panes[pane].edit_buf,
                    &panes[pane].browse,
                );
                // present_view reset the grid to defaults; re-apply the saved
                // sort/filters/search for the active result (sql tabs only).
                if tab.kind == "sql" {
                    if let Some(sr) = tab.results.get(tab.active_result) {
                        restore_grid_state(w, pane, &panes[pane], sr);
                    }
                }
            } else {
                *last_view.lock().unwrap() = None;
                *panes[pane].displayed_grid.lock().unwrap() = None;
                clear_grid(w, pane);
                set_p_results_meta(w, pane, SharedString::default());
                set_p_result_status(w, pane, SharedString::default());
            }
        })
    };

    // `restore_tab` takes an absolute `workspace_tabs` index (what most callers
    // already compute). The UI select handlers pass a group-relative strip index
    // and convert it before calling.
    #[allow(clippy::type_complexity)]
    let restore_tab: Rc<dyn Fn(&MainWindow, usize)> = restore_tab_for_pane.clone();

    // `restore_p1_tab` takes a group-1 strip index and maps it to the absolute
    // index the shared restore expects.
    #[allow(clippy::type_complexity)]
    let restore_p1_tab: Rc<dyn Fn(&MainWindow, usize)> = {
        let f = restore_tab_for_pane.clone();
        let tabs = workspace_tabs.clone();
        Rc::new(move |w, group1_index| {
            let abs = {
                let tabs = tabs.lock().unwrap();
                abs_index_for_group(&tabs, 1, group1_index)
            };
            if let Some(abs) = abs {
                f(w, abs);
            }
        })
    };

    let (run_sql, run_stream) = wire::runner::build(&window, &state);

    let rebuild_query_tree: Rc<dyn Fn(&str)> = Rc::new({
        let weak = window.as_weak();
        let saved = saved_queries.clone();
        let recent = recent_queries.clone();
        let collapsed_history_groups = collapsed_history_groups.clone();
        move |active: &str| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            // Mode 2 (History) shows only the live history; mode 1 (Queries)
            // shows Saved + Recent.
            let history_only = w.get_sidebar_mode() == 2;
            let mut rows: Vec<TreeNode> = Vec::new();
            if !history_only {
                let saved = saved.borrow();
                rows.push(TreeNode {
                    label: "Saved".into(),
                    depth: 0,
                    kind: "qcat".into(),
                    expanded: true,
                    db: SharedString::default(),
                    count: saved.len() as i32,
                    sub: SharedString::default(),
                    sub_color: Default::default(),
                    sub_has_custom_color: false,
                });
                for (i, (name, _)) in saved.iter().enumerate() {
                    rows.push(TreeNode {
                        label: name.as_str().into(),
                        depth: 1,
                        kind: "query".into(),
                        expanded: name == active,
                        db: SharedString::default(),
                        count: i as i32,
                        sub: SharedString::default(),
                        sub_color: Default::default(),
                        sub_has_custom_color: false,
                    });
                }
            }
            // Queries (mode 1) is the curated Saved list only; History (mode 2)
            // is the live run history, grouped by the date each query ran
            // (entries are already newest-first, so same-label runs stay
            // contiguous — no need to bucket out of order).
            if history_only {
                let recent = recent.borrow();
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let collapsed = collapsed_history_groups.borrow();
                let mut order: Vec<String> = Vec::new();
                let mut buckets: std::collections::HashMap<String, Vec<usize>> =
                    std::collections::HashMap::new();
                for (i, entry) in recent.iter().enumerate() {
                    let label = history_date_label(entry.ran_at, now);
                    if !buckets.contains_key(&label) {
                        order.push(label.clone());
                    }
                    buckets.entry(label).or_default().push(i);
                }
                for label in &order {
                    let idxs = &buckets[label];
                    let is_open = !collapsed.contains(label);
                    rows.push(TreeNode {
                        label: label.as_str().into(),
                        depth: 0,
                        kind: "qcat".into(),
                        expanded: is_open,
                        db: SharedString::default(),
                        count: idxs.len() as i32,
                        sub: SharedString::default(),
                        sub_color: Default::default(),
                        sub_has_custom_color: false,
                    });
                    if !is_open {
                        continue;
                    }
                    for &i in idxs {
                        // Collapse whitespace to one line so a multi-line query
                        // does not paint over the rows below; the row elides.
                        let preview = recent_preview(&recent[i].sql);
                        rows.push(TreeNode {
                            label: preview.clone().into(),
                            depth: 1,
                            kind: "recent".into(),
                            expanded: false,
                            db: SharedString::from(preview),
                            count: i as i32,
                            sub: recent[i].engine.clone().unwrap_or_default().into(),
                            sub_color: recent[i]
                                .color
                                .as_deref()
                                .and_then(theme::parse_hex)
                                .unwrap_or_default(),
                            sub_has_custom_color: recent[i].color.is_some(),
                        });
                    }
                }
            }
            w.set_query_tree(ModelRc::from(Rc::new(VecModel::from(rows))));
        }
    });

    let guard_pending: Rc<dyn Fn(&MainWindow) -> bool> = {
        let edit_buf = edit_buf.clone();
        Rc::new(move |w: &MainWindow| guard_pending_edits(w, &edit_buf))
    };

    // Bundled once every closure above exists; see `AppFns`.
    let fns = AppFns {
        rebuild_query_tree: rebuild_query_tree.clone(),
        load_editor_text: load_editor_text.clone(),
        save_active_tab: save_active_tab.clone(),
        restore_tab: restore_tab.clone(),
        save_p1_tab: save_p1_tab.clone(),
        restore_p1_tab: restore_p1_tab.clone(),
        guard_pending: guard_pending.clone(),
        run_sql: run_sql.clone(),
        run_stream: run_stream.clone(),
        sync_editor: sync_editor.clone(),
    };

    // Every wiring module, in one place. These only register callbacks, so the
    // order between them does not matter — `main` just has to have built the
    // state and the shared closures first.
    wire::connect::wire(&window, &state, &fns);
    wire::grid::wire(&window, &state);
    wire::find::wire(&window, &state, &fns);
    wire::schema::wire(&window, &state, &fns);
    wire::picker::wire(&window, &state, &fns);
    wire::query::wire(&window, &state, &fns);
    wire::split_pane::wire(&window, &state);
    wire::browse::wire(&window, &state, &fns);
    wire::tabs::wire(&window, &state, &fns);
    wire::edit::wire(&window, &state);
    wire::settings::wire(&window, &state, &fns);
    wire::conn_form::wire(&window, &state);
    wire::update::wire(&window, &state);

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
    // On exit, capture each pane's active tab (edits made without a tab switch)
    // and persist, so a plain type-then-quit is not lost. Both panes: quitting
    // used to save only the left one, so anything typed into the right pane
    // since its last tab switch was gone on the next launch. The left one goes
    // last because each call rewrites the whole file and only this one carries
    // the left group's active tab id.
    save_p1_tab(&window);
    save_active_tab(&window);
    run_result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid(cols: &[&str], rows: &[&[&str]]) -> model::GridModel {
        model::GridModel {
            columns: cols
                .iter()
                .map(|n| model::VmColumn {
                    name: (*n).to_string(),
                    ..Default::default()
                })
                .collect(),
            rows: rows
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|t| model::VmCell {
                            text: (*t).to_string(),
                            is_null: *t == "NULL",
                        })
                        .collect()
                })
                .collect(),
        }
    }

    fn texts(g: &model::GridModel) -> Vec<Vec<&str>> {
        g.rows
            .iter()
            .map(|r| r.iter().map(|c| c.text.as_str()).collect())
            .collect()
    }

    /// One pass has to reproduce what the old filter -> per-column filter ->
    /// sort -> project chain produced.
    #[test]
    fn build_grid_filters_sorts_and_projects() {
        let base = grid(
            &["id", "name", "note"],
            &[
                &["2", "Bob", "x"],
                &["10", "alice", "y"],
                &["1", "Carol", "x"],
                &["3", "dave", "z"],
            ],
        );
        let no_filters = vec![String::new(); 3];
        let all = HashSet::new();

        // global "contains" filter is case-insensitive across every cell
        let g = build_grid(
            &base,
            "car",
            "any column",
            "contains",
            &no_filters,
            &all,
            &[0, 1, 2],
            -1,
            true,
        );
        assert_eq!(texts(&g), vec![vec!["1", "Carol", "x"]]);

        // numeric sort, not lexicographic: 10 comes after 3
        let g = build_grid(
            &base,
            "",
            "any column",
            "contains",
            &no_filters,
            &all,
            &[0, 1, 2],
            0,
            true,
        );
        assert_eq!(
            g.rows
                .iter()
                .map(|r| r[0].text.as_str())
                .collect::<Vec<_>>(),
            vec!["1", "2", "3", "10"]
        );

        // per-column filter (AND) plus hide + reorder in one go
        let mut col_filters = vec![String::new(); 3];
        col_filters[2] = "x".to_string();
        let mut hidden = HashSet::new();
        hidden.insert(2);
        let g = build_grid(
            &base,
            "",
            "any column",
            "contains",
            &col_filters,
            &hidden,
            &[1, 0, 2],
            0,
            false,
        );
        assert_eq!(
            g.columns
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>(),
            vec!["name", "id"]
        );
        assert_eq!(texts(&g), vec![vec!["Bob", "2"], vec!["Carol", "1"]]);
    }

    /// Nulls sort last in both directions, and the column-scoped operators
    /// still work off the shared `cell_matches`.
    #[test]
    fn build_grid_nulls_sort_last_and_column_ops_apply() {
        let base = grid(&["n"], &[&["5"], &["NULL"], &["1"], &["9"]]);
        let none = vec![String::new()];
        let all = HashSet::new();

        for asc in [true, false] {
            let g = build_grid(
                &base,
                "",
                "any column",
                "contains",
                &none,
                &all,
                &[0],
                0,
                asc,
            );
            assert_eq!(g.rows.last().unwrap()[0].text, "NULL", "asc={asc}");
        }

        // `>` compares text when either side is not numeric, so a NULL cell
        // ("null" > "4") stays in — long-standing behavior of `cell_matches`,
        // pinned here so the single-pass rewrite did not quietly change it.
        let g = build_grid(&base, "4", "n", ">", &none, &all, &[0], 0, true);
        assert_eq!(texts(&g), vec![vec!["5"], vec!["9"], vec!["NULL"]]);

        let g = build_grid(&base, "", "n", "is null", &none, &all, &[0], -1, true);
        assert_eq!(texts(&g), vec![vec!["NULL"]]);
    }

    #[test]
    fn ci_helpers_handle_non_ascii() {
        assert!(contains_ci("Grüße", "grüße"));
        assert!(contains_ci("HELLO world", "lo wor"));
        assert!(!contains_ci("hello", "zz"));
        assert!(eq_ci("ÄPFEL", "äpfel"));
        assert!(!eq_ci("apple", "apples"));
    }

    #[test]
    fn create_table_sql_quotes_and_pk() {
        let cols = vec![
            ColSpec {
                name: "id".into(),
                ty: "serial".into(),
                nullable: false,
                pk: true,
            },
            ColSpec {
                name: "na\"me".into(),
                ty: "text".into(),
                nullable: true,
                pk: false,
            },
        ];
        let sql = build_create_table(
            Some("public"),
            "users",
            &cols,
            rdb_connstore::Engine::Postgres,
        )
        .unwrap();
        assert_eq!(
            sql,
            "CREATE TABLE \"public\".\"users\" (\n  \"id\" serial NOT NULL,\n  \"na\"\"me\" text,\n  PRIMARY KEY (\"id\")\n)"
        );

        // MySQL: backtick quoting, no schema qualifier.
        let my = build_create_table(None, "t", &cols[..1], rdb_connstore::Engine::MySql).unwrap();
        assert_eq!(
            my,
            "CREATE TABLE `t` (\n  `id` serial NOT NULL,\n  PRIMARY KEY (`id`)\n)"
        );
    }

    #[test]
    fn create_table_rejects_bad_input() {
        assert!(build_create_table(None, "  ", &[], rdb_connstore::Engine::Postgres).is_err());
        let no_type = vec![ColSpec {
            name: "x".into(),
            ty: "".into(),
            nullable: true,
            pk: false,
        }];
        assert!(build_create_table(None, "t", &no_type, rdb_connstore::Engine::Postgres).is_err());
    }

    /// Startup restores the tabs and flips the flag, so connect must not read
    /// the file a second time and clobber anything opened in between.
    #[test]
    fn query_tabs_restore_once_per_session() {
        assert!(should_restore_query_tabs(false));
        assert!(!should_restore_query_tabs(true));
    }

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
                    connection_id: Some("c".into()),
                    engine: "postgres".into(),
                    connection_name: "prod-db".into(),
                    folded_heads: Vec::new(),
                },
                PersistedTab {
                    id: "query:c:2".into(),
                    title: "scratch".into(),
                    query_text: String::new(),
                    group: 0,
                    split: false,
                    pane1_query: String::new(),
                    split_ratio: 0.5,
                    connection_id: None,
                    engine: String::new(),
                    connection_name: String::new(),
                    folded_heads: Vec::new(),
                },
            ],
            active: Some("query:c:2".into()),
            active_p1: Some("query:c:1".into()),
            active_group: 1,
        };
        let json = serde_json::to_string(&payload).unwrap();
        let back: PersistedTabs = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tabs.len(), 2);
        assert_eq!(back.tabs[0].query_text, "select * from users;");
        assert_eq!(back.tabs[0].group, 1);
        assert!(back.tabs[0].split);
        assert_eq!(back.tabs[0].pane1_query, "select * from orders;");
        assert_eq!(back.tabs[0].split_ratio, 0.35);
        assert_eq!(back.tabs[0].connection_id.as_deref(), Some("c"));
        assert_eq!(back.tabs[0].engine, "postgres");
        assert_eq!(back.tabs[0].connection_name, "prod-db");
        assert_eq!(back.active.as_deref(), Some("query:c:2"));
    }

    #[test]
    fn folded_heads_survive_a_persistence_round_trip() {
        let payload = PersistedTabs {
            tabs: vec![PersistedTab {
                id: "query:c:1".into(),
                title: "Query 1".into(),
                query_text: "update t set a = 1;\nselect * from t;".into(),
                group: 0,
                split: false,
                pane1_query: String::new(),
                split_ratio: 0.5,
                connection_id: None,
                engine: String::new(),
                connection_name: String::new(),
                folded_heads: vec![0, 5, 9],
            }],
            active: None,
            active_p1: None,
            active_group: 0,
        };
        let json = serde_json::to_string(&payload).unwrap();
        let back: PersistedTabs = serde_json::from_str(&json).unwrap();
        assert_eq!(back.tabs[0].folded_heads, vec![0, 5, 9]);
    }

    #[test]
    fn a_tabs_file_written_before_folding_was_persisted_still_loads() {
        // The regression that would hit every existing user at once: their
        // query_tabs.json has no folded_heads key at all. It must load, with
        // nothing folded — which is exactly today's behaviour.
        let payload: PersistedTabs = serde_json::from_str(
            r#"{"tabs":[{"id":"query:c:1","title":"Query 1","query_text":"select 1"}],"active":null}"#,
        )
        .unwrap();
        assert_eq!(payload.tabs.len(), 1);
        assert!(payload.tabs[0].folded_heads.is_empty());
    }

    #[test]
    fn snapshot_folds_sorts_so_saves_do_not_churn() {
        // A HashSet iterates in an arbitrary order; without sorting, every save
        // could rewrite the file with the same folds in a different order.
        let folded = RefCell::new(HashSet::from([9usize, 0, 5]));
        assert_eq!(snapshot_folds(&folded), vec![0, 5, 9]);
        assert!(snapshot_folds(&RefCell::new(HashSet::new())).is_empty());
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
