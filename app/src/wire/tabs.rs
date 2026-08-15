//! Query-tab management: new tab, run-into-a-new-result-tab, switching and
//! closing result tabs, closing a query tab, renaming one, and tab selection.
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
        active_group1_tab_id,
        current_connection_id,
        query_number,
        last_view,
        history_cap,
        cur_engine,
        recent_queries,
        ..
    } = state.clone();
    let AppFns {
        rebuild_query_tree,
        load_editor_text,
        save_active_tab,
        restore_tab,
        save_p1_tab,
        restore_p1_tab,
        guard_pending,
        run_sql,
    } = fns.clone();
    let browse = panes[0].browse.clone();
    let edit_buf = panes[0].edit_buf.clone();
    let displayed_grid = panes[0].displayed_grid.clone();
    let results = panes[0].results.clone();
    let active_result = panes[0].active_result.clone();
    let hidden_cols = panes[0].hidden_cols.clone();
    let sort_state = panes[0].sort_state.clone();
    let col_order = panes[0].col_order.clone();
    let col_filters = panes[0].col_filters.clone();

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
        let store = store.clone();
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
            let badge = connection_badge_info(&store.borrow(), &connection);
            let mut tabs = workspace_tabs.lock().unwrap();
            let mut tab = WorkspaceTab::sql(id.clone(), number);
            tab.connection_id = (!connection.is_empty()).then(|| connection.clone());
            tab.engine = badge.engine;
            tab.connection_name = badge.name;
            tab.color = badge.color;
            tab.has_custom_color = badge.has_custom_color;
            tabs.push(tab);
            *active_tab_id.lock().unwrap() = Some(id.clone());
            set_workspace_tabs(&w, &tabs, Some(&id));
            save_query_tabs(&w, &tabs, Some(&id));
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
            set_result_tabs(&w, 0, &[], 0);
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
        let history_cap = history_cap.clone();
        let cur_engine = cur_engine.clone();
        let rebuild_query_tree = rebuild_query_tree.clone();
        let store = store.clone();
        let current_connection_id = current_connection_id.clone();
        let run_new_tab: Rc<dyn Fn(usize)> = Rc::new(move |pane| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let pane = pane.min(1);
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
            set_p_read_only(&w, pane, true);
            record_recent(
                &recent_queries,
                &stmt,
                history_cap.get(),
                cur_engine.borrow().map(AnyDriver::badge).map(String::from),
                resolve_conn_color(&current_connection_id, &store),
            );
            if w.get_sidebar_mode() == 2 {
                rebuild_query_tree("");
            }
            run_sql(pane, stmt);
        });
        let weak = window.as_weak();
        let run_new_tab_active = run_new_tab.clone();
        window.on_run_new_tab(move || {
            if let Some(w) = weak.upgrade() {
                run_new_tab_active(w.get_active_pane().clamp(0, 1) as usize);
            }
        });
        let run_new_tab_p1 = run_new_tab;
        window.on_p1_run_new_tab(move || run_new_tab_p1(1));
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
        let panes = panes.clone();
        window.on_select_result_tab(move |i| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let i = i.max(0) as usize;
            // Stash the current result's live grid view before leaving it, so a
            // round-trip back restores its filters/sort/hidden/order/widths.
            let old = *active_result.lock().unwrap();
            {
                let state = capture_grid_state(&w, 0, &panes[0]);
                if let Some(cur) = results.lock().unwrap().get_mut(old) {
                    cur.grid = state;
                }
            }
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
                &sr.view,
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
            restore_grid_state(&w, 0, &panes[0], &sr);
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
            let (results_vec, active, sr) = {
                let mut rv = results.lock().unwrap();
                if i >= rv.len() {
                    return;
                }
                rv.remove(i);
                let mut ar = active_result.lock().unwrap();
                if *ar >= rv.len() {
                    *ar = rv.len().saturating_sub(1);
                }
                (rv.clone(), *ar, rv.get(*ar).cloned())
            };
            set_result_tabs(&w, 0, &results_vec, active);
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
                    &sr.view,
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
        let active_group1_tab_id = active_group1_tab_id.clone();
        window.on_p1_select_result_tab(move |i| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let i = i.max(0) as usize;
            let old = *panes[1].active_result.lock().unwrap();
            {
                let state = capture_grid_state(&w, 1, &panes[1]);
                if let Some(cur) = panes[1].results.lock().unwrap().get_mut(old) {
                    cur.grid = state;
                }
            }
            let sr = panes[1].results.lock().unwrap().get(i).cloned();
            let Some(sr) = sr else {
                return;
            };
            *panes[1].active_result.lock().unwrap() = i;
            if let Some(id) = active_group1_tab_id.lock().unwrap().clone() {
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
                &sr.view,
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
            restore_grid_state(&w, 1, &panes[1], &sr);
        });
    }
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        let last_view = last_view.clone();
        let workspace_tabs = workspace_tabs.clone();
        let active_group1_tab_id = active_group1_tab_id.clone();
        window.on_p1_close_result_tab(move |i| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let i = i.max(0) as usize;
            let (results_vec, active, sr) = {
                let mut rv = panes[1].results.lock().unwrap();
                if i >= rv.len() {
                    return;
                }
                rv.remove(i);
                let mut ar = panes[1].active_result.lock().unwrap();
                if *ar >= rv.len() {
                    *ar = rv.len().saturating_sub(1);
                }
                (rv.clone(), *ar, rv.get(*ar).cloned())
            };
            set_result_tabs(&w, 1, &results_vec, active);
            if let Some(id) = active_group1_tab_id.lock().unwrap().clone() {
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
                    &sr.view,
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
                save_query_tabs(&w, &tabs, None);
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
                &w,
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
        let active_group1_tab_id = active_group1_tab_id.clone();
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
            save_query_tabs(&w, &tabs, active.as_deref());
            drop(tabs);
            if remaining == 0 {
                *active_group1_tab_id.lock().unwrap() = None;
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
            save_query_tabs(&w, &tabs, active.as_deref());
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
}
