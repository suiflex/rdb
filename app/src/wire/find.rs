//! Find-in-editor (⌘F), wired identically for both editor panes.
//!
//! Split out of `main`; the handler bodies are unchanged.

use std::rc::Rc;

use slint::{ComponentHandle, SharedString};

use crate::*;

pub(crate) fn wire(window: &MainWindow, state: &AppState, fns: &AppFns) {
    let AppState {
        store,
        panes,
        workspace_tabs,
        active_tab_id,
        current_connection_id,
        saved_queries,
        recent_queries,
        fn_defs,
        ..
    } = state.clone();
    let AppFns {
        sync_editor,
        load_editor_text,
        rebuild_query_tree,
        ..
    } = fns.clone();

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
                bump_p_scroll_request(w, pane);
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
            let saved_hit = saved
                .borrow()
                .iter()
                .find(|(n, _)| n == label.as_str())
                .map(|(n, s)| (n.clone(), s.clone()));
            let (title, text, is_saved) = if let Some((name, sql)) = saved_hit {
                (name, sql, true)
            } else if let Some(entry) = recent.borrow().get(idx.max(0) as usize) {
                ("Query".to_string(), entry.sql.clone(), false)
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

    {
        let weak = window.as_weak();
        let fn_defs = fn_defs.clone();
        let workspace_tabs = workspace_tabs.clone();
        let active_tab_id = active_tab_id.clone();
        let current_connection_id = current_connection_id.clone();
        let store = store.clone();
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
                    // Function bodies are always SQL (Postgres introspection),
                    // regardless of the tab's connected engine.
                    let spans: Vec<Span> = editor::lex_line(rdb_connstore::QueryLanguage::Sql, l)
                        .into_iter()
                        .map(|sp| Span {
                            cols: sp.text.chars().count() as i32,
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
            let connection_id = current_connection_id.lock().unwrap().clone();
            let id = format!(
                "function:{}:{}:{}",
                connection_id.as_deref().unwrap_or_default(),
                w.get_schema_name(),
                name
            );
            let badge = connection_id
                .as_deref()
                .map(|cid| connection_badge_info(&store.borrow(), cid))
                .unwrap_or_default();
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
                    folded_heads: Vec::new(),
                    connection_id,
                    engine: badge.engine,
                    connection_name: badge.name,
                    color: badge.color,
                    has_custom_color: badge.has_custom_color,
                });
                *active_tab_id.lock().unwrap() = Some(id.clone());
                set_workspace_tabs(&w, &tabs, Some(&id));
            }
        });
    }
}
