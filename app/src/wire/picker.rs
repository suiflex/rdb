//! The connection picker and sidebar groups: selecting a connection into the
//! detail panel, the split-pane tab handlers, collapsing/deleting/renaming
//! groups, the search box, favourites, drag-reorder between groups,
//! disconnect, reconnect, and opening an external link.
//!
//! Split out of `main`; the handler bodies are unchanged.

use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::*;

pub(crate) fn wire(window: &MainWindow, state: &AppState, fns: &AppFns) {
    let AppState {
        rt,
        store,
        settings,
        panes,
        current,
        driver_pool,
        cur_engine,
        collapsed,
        conn_filter,
        raw_nodes,
        expanded_tables,
        loaded_dbs,
        collapsed_categories,
        workspace_tabs,
        active_tab_id,
        active_group1_tab_id,
        current_connection_id,
        connected_ids,
        query_number,
        conn_modal_map,
        ..
    } = state.clone();
    let AppFns {
        load_editor_text,
        save_active_tab,
        restore_tab,
        save_p1_tab,
        restore_p1_tab,
        ..
    } = fns.clone();

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
            w.set_sel_color(theme::accent_or_default(s.color.as_deref().unwrap_or("")));
            w.set_sel_has_custom_color(s.color.is_some());
            let label = AnyDriver::label(s.engine);
            let gsub = group_sub_label(s.group.as_deref());
            let sub = if gsub.is_empty() {
                label.to_string()
            } else {
                format!("{label} · {gsub}")
            };
            w.set_sel_sub(sub.into());
            w.set_sel_local(s.local);
            w.set_sel_ssh_enabled(s.ssh_enabled);
            w.set_sel_env_tag_label(theme::env_tag_label(s.env_tag).into());
            w.set_sel_env_tag_color(
                theme::env_tag_color(s.env_tag).unwrap_or_else(|| theme::accent_or_default("")),
            );
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
            if s.ssh_enabled {
                if let Some(host) = &s.ssh_host {
                    let port = s.ssh_port.unwrap_or(22);
                    let user = s.ssh_user.as_deref().unwrap_or("");
                    rows.push(KvRow {
                        k: "SSH".into(),
                        v: format!("{user}@{host}:{port} ({})", s.ssh_auth_mode.as_str()).into(),
                    });
                }
            }
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
    // Mock mode boots with the reference selection ("chat bot").
    if mock::mock_mode() {
        let idx = store
            .borrow()
            .list()
            .iter()
            .position(|s| s.name == "chat bot")
            .map(|i| i as i32)
            .unwrap_or(-1);
        fill_detail(idx);
    }

    // RDB_SCREEN e2e harness: see `wire_screen_harness`.
    wire_screen_harness(window, &store, &load_editor_text);

    // Last result view kept in memory so the client-side filter (Feature C)
    // can re-derive the visible rows without re-querying. Arc<Mutex<>> (not Rc)
    // so it can cross into the Send event-loop closure from the query task.

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
        let active_group1_tab_id = active_group1_tab_id.clone();
        let current_connection_id = current_connection_id.clone();
        let query_number = query_number.clone();
        let save_p1_tab = save_p1_tab.clone();
        let restore_p1_tab = restore_p1_tab.clone();
        let store = store.clone();
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
            let badge = connection_badge_info(&store.borrow(), &connection);
            let right_index = {
                let mut tabs = workspace_tabs.lock().unwrap();
                let mut tab = WorkspaceTab::sql(id.clone(), number);
                tab.group = 1;
                tab.connection_id = (!connection.is_empty()).then(|| connection.clone());
                tab.engine = badge.engine;
                tab.connection_name = badge.name;
                tab.color = badge.color;
                tab.has_custom_color = badge.has_custom_color;
                tabs.push(tab);
                let left = active_tab_id.lock().unwrap().clone();
                set_workspace_tabs(&w, &tabs, left.as_deref());
                save_query_tabs(&w, &tabs, left.as_deref());
                tabs.iter().filter(|tab| tab.group == 1).count() - 1
            };
            *active_group1_tab_id.lock().unwrap() = Some(id);
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
        let active_group1_tab_id = active_group1_tab_id.clone();
        let active_tab_id = active_tab_id.clone();
        window.on_move_tab_group(move |index, target| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let target = target.clamp(0, 1) as usize;
            save_active_tab(&w);
            save_p1_tab(&w);
            let source = if target == 1 { 0 } else { 1 };
            // Whichever tab was active in each pane before this move, so a
            // split (or moving one back) doesn't jump focus to the first tab
            // in that pane — it only needs to change when the tab that just
            // moved WAS the active one.
            let prev_left = active_tab_id.lock().unwrap().clone();
            let prev_right = active_group1_tab_id.lock().unwrap().clone();
            let moved_id = {
                let mut tabs = tabs.lock().unwrap();
                if source == 0
                    && tabs
                        .iter()
                        .filter(|tab| tab.group == 0 && workspace_tab_visible(&w, tab))
                        .count()
                        == 1
                {
                    return;
                }
                let Some(index) =
                    visible_abs_index_for_group(&w, &tabs, source, index.max(0) as usize)
                else {
                    return;
                };
                let Some(tab) = tabs.get_mut(index) else {
                    return;
                };
                tab.group = target;
                tab.id.clone()
            };
            let (left_index, right_index, left_id, right_id) = {
                let tabs = tabs.lock().unwrap();
                // The pane the tab just landed in follows it; the other pane
                // keeps its previously-active tab if it's still there.
                let left_id = if target == 0 {
                    Some(moved_id.clone())
                } else {
                    prev_left.filter(|id| {
                        tabs.iter()
                            .any(|t| t.group == 0 && workspace_tab_visible(&w, t) && &t.id == id)
                    })
                }
                .or_else(|| {
                    tabs.iter()
                        .find(|t| t.group == 0 && workspace_tab_visible(&w, t))
                        .map(|t| t.id.clone())
                });
                let right_id = if target == 1 {
                    Some(moved_id.clone())
                } else {
                    prev_right.filter(|id| {
                        tabs.iter()
                            .any(|t| t.group == 1 && workspace_tab_visible(&w, t) && &t.id == id)
                    })
                }
                .or_else(|| {
                    tabs.iter()
                        .find(|t| t.group == 1 && workspace_tab_visible(&w, t))
                        .map(|t| t.id.clone())
                });
                let left_index = left_id
                    .as_ref()
                    .and_then(|id| tabs.iter().position(|t| &t.id == id));
                let right_index = right_id.as_ref().and_then(|id| {
                    tabs.iter()
                        .filter(|t| t.group == 1 && workspace_tab_visible(&w, t))
                        .position(|t| &t.id == id)
                });
                set_workspace_tabs(&w, &tabs, left_id.as_deref());
                (left_index, right_index, left_id, right_id)
            };
            *active_tab_id.lock().unwrap() = left_id;
            if let Some(index) = left_index {
                restore_tab(&w, index);
            }
            if let (Some(id), Some(index)) = (right_id, right_index) {
                *active_group1_tab_id.lock().unwrap() = Some(id);
                restore_p1_tab(&w, index);
            } else {
                *active_group1_tab_id.lock().unwrap() = None;
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
        let conn_modal_map = conn_modal_map.clone();
        let connected_ids = connected_ids.clone();
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
            // In place, so the picker animates the group open/shut.
            update_sidebar_model(
                &w,
                group_conn_items(build_conn_items(
                    &store.borrow(),
                    &collapsed.borrow(),
                    &conn_filter.borrow(),
                    &connected_ids.lock().unwrap(),
                )),
            );
            // Keep the ⌘O modal in sync too, whichever surface triggered this.
            if w.get_conn_modal_open() {
                fill_conn_modal(
                    &w,
                    &store.borrow(),
                    &collapsed.borrow(),
                    &connected_ids,
                    &conn_modal_map,
                );
            }
        });
    }

    // ----- delete a group: its connections + descendant subfolders promote
    // one level up (a top-level folder's members fall back to Ungrouped,
    // same as before nesting existed) -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        let settings = settings.clone();
        let connected_ids = connected_ids.clone();
        window.on_group_delete(move |g| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let g = g.to_string();
            {
                let mut st = store.borrow_mut();
                let members: Vec<(String, String)> = st
                    .list()
                    .iter()
                    .filter_map(|s| {
                        let grp = s.group.as_deref()?;
                        rdb_connstore::is_descendant(grp, &g)
                            .then(|| (s.id.clone(), grp.to_string()))
                    })
                    .collect();
                for (id, grp) in members {
                    if let Some(mut sc) = st.get(&id).cloned() {
                        sc.group = cascade_delete_group(&g, &grp);
                        let _ = st.update(sc);
                    }
                }
            }
            collapsed
                .borrow_mut()
                .retain(|p| !rdb_connstore::is_descendant(p, &g));
            let groups: Vec<String> = collapsed.borrow().iter().cloned().collect();
            let _ = settings
                .borrow_mut()
                .update(|s| s.ui_state.collapsed_groups = groups);
            w.set_connections(build_sidebar_model(
                &store.borrow(),
                &collapsed.borrow(),
                &conn_filter.borrow(),
                &connected_ids.lock().unwrap(),
            ));
        });
    }

    // ----- rename a group: every member connection and descendant subfolder
    // follows, prefix-replaced -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        let settings = settings.clone();
        let connected_ids = connected_ids.clone();
        window.on_group_rename(move |old, new| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let old = old.to_string();
            let Some(new) = rdb_connstore::normalize_group_path(&new) else {
                return;
            };
            // Also rejects a no-op rename (new == old counts as its own
            // descendant) and renaming a folder into its own subtree, which
            // would otherwise create a cycle.
            if rdb_connstore::is_descendant(&new, &old) {
                return;
            }
            {
                let mut st = store.borrow_mut();
                let members: Vec<(String, String)> = st
                    .list()
                    .iter()
                    .filter_map(|s| {
                        let grp = s.group.as_deref()?;
                        rdb_connstore::is_descendant(grp, &old)
                            .then(|| (s.id.clone(), grp.to_string()))
                    })
                    .collect();
                for (id, grp) in members {
                    if let Some(mut sc) = st.get(&id).cloned() {
                        sc.group = Some(cascade_rename_group(&old, &new, &grp));
                        let _ = st.update(sc);
                    }
                }
            }
            // Carry the collapsed state over so renaming doesn't silently
            // re-expand a group (or one of its subfolders) the user had
            // folded shut.
            {
                let mut c = collapsed.borrow_mut();
                let renamed: Vec<String> = c
                    .iter()
                    .filter(|p| rdb_connstore::is_descendant(p, &old))
                    .cloned()
                    .collect();
                for p in renamed {
                    c.remove(&p);
                    c.insert(cascade_rename_group(&old, &new, &p));
                }
            }
            let groups: Vec<String> = collapsed.borrow().iter().cloned().collect();
            let _ = settings
                .borrow_mut()
                .update(|s| s.ui_state.collapsed_groups = groups);
            w.set_connections(build_sidebar_model(
                &store.borrow(),
                &collapsed.borrow(),
                &conn_filter.borrow(),
                &connected_ids.lock().unwrap(),
            ));
        });
    }

    // ----- connection-picker search (filter the connection list) -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        let connected_ids = connected_ids.clone();
        window.on_conn_filter(move |t| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            *conn_filter.borrow_mut() = t.to_string();
            w.set_connections(build_sidebar_model(
                &store.borrow(),
                &collapsed.borrow(),
                &conn_filter.borrow(),
                &connected_ids.lock().unwrap(),
            ));
        });
    }

    // ----- star/unstar a saved connection -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        let connected_ids = connected_ids.clone();
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
            w.set_connections(build_sidebar_model(
                &store.borrow(),
                &collapsed.borrow(),
                &conn_filter.borrow(),
                &connected_ids.lock().unwrap(),
            ));
        });
    }

    // ----- drag-reorder within a group, or drop onto a different group -----
    {
        let weak = window.as_weak();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        let connected_ids = connected_ids.clone();
        window.on_reorder_conn(move |from_idx, delta, drop_y| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let group_key = |c: &rdb_connstore::SavedConnection| {
                c.group
                    .as_deref()
                    .filter(|g| !g.trim().is_empty())
                    .unwrap_or(UNGROUPED)
                    .to_string()
            };

            // Cross-group drop: the release point landed on a different
            // group's header or row than the dragged connection's own group.
            let rendered = build_conn_items(
                &store.borrow(),
                &collapsed.borrow(),
                &conn_filter.borrow(),
                &connected_ids.lock().unwrap(),
            );
            if let Some(target_group) = row_group_at_y(&rendered, drop_y) {
                let from_id = {
                    let s = store.borrow();
                    let Some(from) = s.list().get(from_idx as usize) else {
                        return;
                    };
                    if group_key(from) == target_group {
                        None
                    } else {
                        Some(from.id.clone())
                    }
                };
                if let Some(id) = from_id {
                    let sc = store.borrow().get(&id).cloned();
                    if let Some(mut sc) = sc {
                        sc.group = if target_group == UNGROUPED {
                            None
                        } else {
                            Some(target_group)
                        };
                        let _ = store.borrow_mut().update(sc);
                    }
                    w.set_connections(build_sidebar_model(
                        &store.borrow(),
                        &collapsed.borrow(),
                        &conn_filter.borrow(),
                        &connected_ids.lock().unwrap(),
                    ));
                    return;
                }
            }

            // Same-group reorder: resolve the row-step delta against the
            // dragged connection's own group, using the same (favorite desc,
            // order asc) display order as the builder.
            if delta == 0 {
                return;
            }
            let (from_id, target_vec_idx) = {
                let s = store.borrow();
                let list = s.list();
                let Some(from) = list.get(from_idx as usize) else {
                    return;
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
            w.set_connections(build_sidebar_model(
                &store.borrow(),
                &collapsed.borrow(),
                &conn_filter.borrow(),
                &connected_ids.lock().unwrap(),
            ));
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
        let panes = panes.clone();
        let driver_pool = driver_pool.clone();
        let store = store.clone();
        let collapsed = collapsed.clone();
        let conn_filter = conn_filter.clone();
        let connected_ids = connected_ids.clone();
        let activate_connection = crate::wire::connect::build_activate_connection(state);
        window.on_disconnect(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            // Which connection this actually drops — the one currently
            // browsed. Two connections can be live at once; this must never
            // be confused with "disconnect everything".
            let disconnecting_id = current_connection_id.lock().unwrap().clone();
            if let Some(id) = &disconnecting_id {
                connected_ids.lock().unwrap().remove(id);
            }
            // Cancel the in-flight query before tearing the connection down,
            // so the server drops it instead of running it to completion.
            // Resolved by `disconnecting_id`, not the legacy `current` slot —
            // with two connections open they can point at different ones.
            {
                let current = current.clone();
                let driver_pool = driver_pool.clone();
                let disconnecting_id = disconnecting_id.clone();
                rt.spawn(async move {
                    let driver =
                        resolve_driver(&driver_pool, &current, disconnecting_id.as_deref())
                            .await
                            .map(|(_, d)| d);
                    let Some(driver) = driver else { return };
                    let _ = driver.cancel_running().await;
                    // Drop the pool entry too, not just `current` — but only
                    // if a fast reconnect hasn't already replaced it.
                    if let Some(id) = disconnecting_id {
                        let mut pool = driver_pool.write().await;
                        if still_the_disconnected_driver(&pool, &id, &driver) {
                            pool.remove(&id);
                        }
                    }
                });
            }
            for p in [0usize, 1] {
                if let Some(c) = panes[p].stream_cancel.borrow().as_ref() {
                    c.store(true, std::sync::atomic::Ordering::SeqCst);
                }
                if let Some(h) = panes[p].query_abort.borrow_mut().take() {
                    h.abort();
                }
                set_p_query_running(&w, p, false);
                set_p_streaming(&w, p, false);
            }
            w.set_selected_conn(-1);
            w.set_active_table(SharedString::default());
            w.set_status_conn(SharedString::from("no connection"));
            w.set_status_latency(SharedString::default());
            w.set_schema_tree(ModelRc::from(Rc::new(VecModel::<TreeNode>::default())));
            w.set_structure_columns(ModelRc::from(Rc::new(VecModel::<StructField>::default())));
            // Keep every tab. Table and collection tabs are connection-scoped,
            // but each records the `connection_id` it was opened against, so
            // reconnecting to the same connection can pick them straight back
            // up. Dropping them here is what made a collection tab vanish when
            // the user switched connections — a switch is a disconnect followed
            // by a connect, so this path ran and took the tab with it. The rows
            // already fetched stay readable while disconnected; only the
            // spinner is cleared, since nothing is loading any more.
            {
                let mut tabs = workspace_tabs.lock().unwrap();
                for t in tabs.iter_mut() {
                    t.loading = false;
                }
                let keep_active = active_tab_id
                    .lock()
                    .unwrap()
                    .clone()
                    .filter(|id| tabs.iter().any(|t| t.id == *id))
                    .or_else(|| tabs.first().map(|t| t.id.clone()));
                *active_tab_id.lock().unwrap() = keep_active.clone();
                set_workspace_tabs(&w, &tabs, keep_active.as_deref());
            }
            *current_connection_id.lock().unwrap() = None;
            clear_grid(&w, 0);
            *cur_engine.borrow_mut() = None;
            expanded_tables.lock().unwrap().clear();
            loaded_dbs.lock().unwrap().clear();
            *collapsed_categories.borrow_mut() = default_collapsed_cats();
            raw_nodes.lock().unwrap().clear();
            // Only fall back to the landing picker when no other connection
            // is actually still connected — with two connections open,
            // disconnecting one must not blow away the other's workspace.
            // `connected_ids` (not tab scoping — a tab keeps its
            // `connection_id` after its connection drops) already had
            // `disconnecting_id` removed above, so any entry left here is a
            // genuinely different, still-live connection.
            let other_live = !connected_ids.lock().unwrap().is_empty();
            // The connection the workspace carries on with: whatever the
            // focused tab belongs to, as long as that one is itself still
            // live. A tab of a connection disconnected earlier leaves
            // nothing to carry on with, live sibling or not.
            let surviving = focused_tab_connection_id(&active_tab_id, &workspace_tabs)
                .filter(|cid| connected_ids.lock().unwrap().contains(cid));
            // Reactivating below refills the slot from the pool. With
            // nothing to reactivate it has to be emptied here instead, or it
            // keeps handing out the driver that was just dropped.
            if surviving.is_none() {
                let current = current.clone();
                rt.spawn(async move {
                    *current.lock().await = None;
                });
            }
            w.set_connections(build_sidebar_model(
                &store.borrow(),
                &collapsed.borrow(),
                &conn_filter.borrow(),
                &connected_ids.lock().unwrap(),
            ));
            if other_live {
                // Resync chrome to whatever tab stayed focused, so the
                // topbar reflects a connection that's actually still there
                // instead of the one just dropped.
                sync_conn_chrome(&w, &store.borrow(), surviving.as_deref());
                if let Some(cid) = surviving {
                    *current_connection_id.lock().unwrap() = Some(cid.clone());
                    // The teardown above cleared state the survivor still
                    // needs: its engine (every `cur_engine` guard — open
                    // table, browse, add column — silently no-ops without
                    // it), its sidebar tree, and the `current` slot the
                    // health poll pings. Reactivating it from the pool puts
                    // all three back without a handshake.
                    activate_connection(&w, &cid);
                }
            } else {
                w.set_connected(false);
            }
        });
    }

    // ----- reconnect: retry the current connection after a health drop -----
    {
        let weak = window.as_weak();
        window.on_reconnect(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            // The health poll leaves selected_conn pointing at the live
            // connection, so replay the connect path against it.
            let idx = w.get_selected_conn();
            if idx >= 0 {
                w.invoke_connect_clicked(idx);
            }
        });
    }

    // ----- open an external link (Product Hunt / GitHub) in the browser -----
    {
        window.on_open_url(move |u| {
            let _ = open::that(u.as_str());
        });
    }
}

/// Drives the app to a reference state for screenshots and e2e tests,
/// gated behind `RDB_SCREEN` (mock mode only). Kept out of `wire`: this is
/// test harness plumbing, not a real callback.
type ScreenCond = Rc<dyn Fn(&MainWindow) -> bool>;
type ScreenAct = Rc<dyn Fn(&MainWindow)>;

/// RDB_SCREEN drives the app to a reference state for screenshots and the
/// e2e harness: "workspace" connects + opens emiten; "sql" opens + runs the
/// saved query; the rest open a specific modal/view. Each `schedule_*`
/// helper below owns one screen family and no-ops for any other screen, so
/// this function is just the list of families, not their branching.
fn wire_screen_harness(
    window: &MainWindow,
    store: &Rc<RefCell<rdb_connstore::ConnStore>>,
    load_editor_text: &PaneTextFn,
) {
    let Ok(screen) = std::env::var("RDB_SCREEN") else {
        return;
    };
    pin_theme(window);
    schedule_connect_timer(window, store, &screen);
    schedule_multi_connection_scenario(window, store, &screen);
    schedule_sql_open_timer(window, &screen);
    schedule_modal_timer(window, &screen);
    schedule_workspace_open_timer(window, &screen);
    schedule_grid_edit_scenarios(window, &screen);
    schedule_tab_flow_scenarios(window, &screen);
    schedule_sql_editor_scenarios(window, &screen, load_editor_text);
}

/// "connections" IS the pre-connect screen; connecting would swap it for
/// the workspace before the shot fires.
fn schedule_connect_timer(
    window: &MainWindow,
    store: &Rc<RefCell<rdb_connstore::ConnStore>>,
    screen: &str,
) {
    // "tooltip" hovers a control on this same pre-connect screen.
    if matches!(
        screen,
        "connections" | "tooltip" | "export-menu" | "menu-hover" | "multi-connection"
    ) {
        return;
    }
    let idx = store
        .borrow()
        .list()
        .iter()
        .position(|s| s.name == "chat bot")
        .map(|i| i as i32)
        .unwrap_or(0);
    connect_after(window, idx, 250);
    // "rail": open a second connection, then switch back to the first
    // through the pool, so the rail shows two live tiles.
    if screen == "rail" {
        connect_after(window, if idx == 0 { 1 } else { 0 }, 1500);
        connect_after(window, idx, 2800);
    }
}

/// Aborts the run with a readable reason. A screen that quietly fails to
/// reach its end state still paints a frame and still exits 0, so anything
/// this harness actually *checks* has to end the process itself.
fn screen_fail(msg: &str) -> ! {
    eprintln!("SCREEN ASSERT FAILED: {msg}");
    std::process::exit(2);
}

/// Runs `steps` in order: each waits for its condition, then fires its action
/// once. A step whose condition never holds inside `TICK_LIMIT` fails the run
/// rather than letting the screenshot land on a half-driven UI, which would
/// read as a pass.
fn sequence(window: &MainWindow, steps: Vec<(&'static str, ScreenCond, ScreenAct)>) {
    const TICK_LIMIT: u32 = 30; // 3s per step
    let weak = window.as_weak();
    let t: &'static slint::Timer = Box::leak(Box::new(slint::Timer::default()));
    let state = Rc::new(RefCell::new((0usize, 0u32)));
    t.start(
        slint::TimerMode::Repeated,
        std::time::Duration::from_millis(100),
        move || {
            let Some(w) = weak.upgrade() else {
                t.stop();
                return;
            };
            let (step, ticks) = *state.borrow();
            let Some((name, cond, act)) = steps.get(step) else {
                t.stop();
                return;
            };
            if cond(&w) {
                act(&w);
                *state.borrow_mut() = (step + 1, 0);
            } else if ticks + 1 > TICK_LIMIT {
                screen_fail(&format!(
                    "step '{name}' never became ready; tabs={} active={} engine={:?} tree_loading={}",
                    tab_strip(&w),
                    w.get_active_tab(),
                    active_tab_engine(&w),
                    w.get_tree_loading(),
                ));
            } else {
                *state.borrow_mut() = (step, ticks + 1);
            }
        },
    );
}

/// Whether the connection at store index `idx` has an open driver right now,
/// read off the sidebar model's own `live` flag.
fn connection_is_live(w: &MainWindow, idx: i32) -> bool {
    use slint::Model as _;
    w.get_connections().iter().any(|group| {
        group
            .rows
            .iter()
            .any(|row| row.index == idx && !row.is_header && row.live)
    })
}

/// The left group's tab strip as `engine:title` pairs, for failure messages.
fn tab_strip(w: &MainWindow) -> String {
    use slint::Model as _;
    w.get_tabs()
        .iter()
        .map(|t| format!("{}:{}", t.engine, t.title))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Badge key of the tab the left group is showing, empty when there is none.
fn active_tab_engine(w: &MainWindow) -> String {
    use slint::Model as _;
    let idx = w.get_active_tab();
    if idx < 0 {
        return String::new();
    }
    w.get_tabs()
        .row_data(idx as usize)
        .map(|t| t.engine.to_string())
        .unwrap_or_default()
}

/// The multi-connection regression, driven end to end: two live connections
/// on different engines, and every "new action" — New Query, opening a table
/// from the sidebar — has to target the connection whose tab is focused.
///
/// It asserts rather than posing for a screenshot: binding a tab to the wrong
/// database looks completely normal in a frame, which is exactly how this
/// shipped. Mock mode gives two real pooled drivers (`AnyDriver::connect`
/// builds one per connect) carrying their own engines, so the switch under
/// test is the real one and no server is needed.
fn schedule_multi_connection_scenario(
    window: &MainWindow,
    store: &Rc<RefCell<rdb_connstore::ConnStore>>,
    screen: &str,
) {
    if screen != "multi-connection" {
        return;
    }
    // The screen seeds its own pair rather than leaning on the mock list:
    // RDB_STORE_DIR (which every screen needs, or the run reads the
    // developer's real connections and query tabs) replaces the seeded mock
    // store with an empty file-backed one. Two rows in the isolated store
    // cost nothing and make the scenario independent of either seed.
    for (name, engine, port) in [
        ("screen postgres", rdb_connstore::Engine::Postgres, 5432u16),
        ("screen mongo", rdb_connstore::Engine::Mongo, 27017u16),
    ] {
        if store.borrow().list().iter().any(|c| c.name == name) {
            continue;
        }
        let conn =
            rdb_connstore::SavedConnection::new(name, engine, "203.0.113.10", port, "screen_user");
        if store.borrow_mut().add(conn).is_err() {
            screen_fail(&format!("could not seed the '{name}' connection"));
        }
    }
    let idx_of = |name: &str| {
        store
            .borrow()
            .list()
            .iter()
            .position(|s| s.name == name)
            .map(|i| i as i32)
    };
    let (Some(pg), Some(mongo)) = (idx_of("screen postgres"), idx_of("screen mongo")) else {
        let names: Vec<String> = store
            .borrow()
            .list()
            .iter()
            .map(|s| s.name.clone())
            .collect();
        screen_fail(&format!(
            "the seeded connection pair is missing; saw {names:?}"
        ));
    };
    sequence(
        window,
        vec![
            (
                "connect to postgres",
                Rc::new(|w: &MainWindow| !w.get_connecting()),
                Rc::new(move |w: &MainWindow| w.invoke_connect_clicked(pg)),
            ),
            (
                "open a table on postgres",
                Rc::new(|w: &MainWindow| w.get_connected() && !w.get_tree_loading()),
                Rc::new(|w: &MainWindow| w.invoke_open_table("".into(), "emiten".into())),
            ),
            (
                "connect to mongo with the postgres tab open",
                Rc::new(|w: &MainWindow| w.get_active_table() == "emiten"),
                Rc::new(move |w: &MainWindow| w.invoke_connect_clicked(mongo)),
            ),
            (
                "the connect lands on a tab of its own connection",
                Rc::new(|w: &MainWindow| w.get_connected() && !w.get_tree_loading()),
                Rc::new(|w: &MainWindow| {
                    let engine = active_tab_engine(w);
                    if engine != "mongo" {
                        screen_fail(&format!(
                            "connecting to mongo left the focus on a '{engine}' tab"
                        ));
                    }
                    // Back to the postgres tab: it is first in the strip, and
                    // scoped tab visibility is off by default, so nothing is
                    // hidden.
                    w.invoke_select_tab(0);
                }),
            ),
            (
                "the focused tab pulls the context back to postgres",
                Rc::new(|w: &MainWindow| {
                    active_tab_engine(w) == "postgres" && !w.get_tree_loading()
                }),
                Rc::new(|w: &MainWindow| w.invoke_new_tab()),
            ),
            (
                "a new tab targets the focused tab's connection",
                Rc::new(|w: &MainWindow| {
                    use slint::Model as _;
                    w.get_tabs().row_count() >= 3
                }),
                Rc::new(|w: &MainWindow| {
                    let engine = active_tab_engine(w);
                    if engine != "postgres" {
                        screen_fail(&format!(
                            "New Query off a postgres tab opened a '{engine}' tab"
                        ));
                    }
                    // Drop the connection those postgres tabs belong to,
                    // leaving mongo as the only live one.
                    w.invoke_disconnect();
                }),
            ),
            (
                "the disconnect empties the tree it belonged to",
                Rc::new(|w: &MainWindow| {
                    use slint::Model as _;
                    w.get_schema_tree().row_count() == 0
                }),
                // Over to the mongo tab, which is still live.
                Rc::new(|w: &MainWindow| w.invoke_select_tab(2)),
            ),
            (
                "the live connection is still there to switch to",
                Rc::new(|w: &MainWindow| {
                    use slint::Model as _;
                    active_tab_engine(w) == "mongo"
                        && !w.get_tree_loading()
                        && w.get_schema_tree().row_count() > 0
                }),
                // Back to a tab whose connection is now gone. Half-switching
                // to it is what left the workspace contradicting itself, so
                // it has to come back live instead.
                Rc::new(|w: &MainWindow| w.invoke_select_tab(0)),
            ),
            (
                "focusing a disconnected tab brings its connection back",
                // Its `live` flag, not the tree on screen: the tree left
                // behind by the other connection is not empty, so anything
                // that only counts rows passes on the broken state too.
                Rc::new(move |w: &MainWindow| {
                    connection_is_live(w, pg) && active_tab_engine(w) == "postgres"
                }),
                Rc::new(|_w: &MainWindow| {}),
            ),
        ],
    );
}

fn connect_after(window: &MainWindow, idx: i32, ms: u64) {
    let weak = window.as_weak();
    let t = Box::leak(Box::new(slint::Timer::default()));
    t.start(
        slint::TimerMode::SingleShot,
        std::time::Duration::from_millis(ms),
        move || {
            if let Some(w) = weak.upgrade() {
                w.invoke_connect_clicked(idx);
            }
        },
    );
}

fn schedule_sql_open_timer(window: &MainWindow, screen: &str) {
    if !screen.starts_with("sql") {
        return;
    }
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

/// `RDB_THEME`: pin the theme for a reference shot, whatever the OS is set to.
/// Deferred past startup, which applies the persisted mode and would otherwise
/// overwrite this (the settings callback is also installed after this module).
fn pin_theme(window: &MainWindow) {
    let Ok(theme) = std::env::var("RDB_THEME") else {
        return;
    };
    let light = theme == "light";
    let weak = window.as_weak();
    let t = Box::leak(Box::new(slint::Timer::default()));
    t.start(
        slint::TimerMode::SingleShot,
        std::time::Duration::from_millis(1500),
        move || {
            if let Some(w) = weak.upgrade() {
                w.invoke_set_theme_mode(if light { 1 } else { 2 });
            }
        },
    );
}

/// Settings tab a `settings*` screen opens on: Appearance, Updates, About.
fn settings_screen_tab(which: &str) -> i32 {
    match which {
        "settings" => 0,
        "settings-updates" => 1,
        _ => 2,
    }
}

/// Chrome review screens: the picker's Export menu (opened, or opened and
/// hovered), the rail notch in the light theme (in dark the card is nearly
/// the canvas colour), and the collapsed sidebar with the rail kept.
fn show_chrome_screen(w: &MainWindow, which: &str) {
    use slint::platform::{PointerEventButton, WindowEvent};
    match which {
        "notch-light" => w.invoke_set_theme_mode(1),
        "sidebar-collapsed" => w.set_sidebar_rail(true),
        // The rail's "+": the modal lists only connections not open yet.
        "conn-add" => {
            w.set_conn_modal_adding(true);
            w.invoke_open_conn_modal();
        }
        // Right-click a document tab: its rename/split/close menu.
        "tab-menu" => {
            let position = slint::LogicalPosition::new(590.0, 75.0);
            w.window().dispatch_event(WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Right,
            });
            w.window().dispatch_event(WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Right,
            });
        }
        _ => {
            // The picker footer's "Export ▾".
            let position = slint::LogicalPosition::new(466.0, 722.0);
            w.window().dispatch_event(WindowEvent::PointerPressed {
                position,
                button: PointerEventButton::Left,
            });
            w.window().dispatch_event(WindowEvent::PointerReleased {
                position,
                button: PointerEventButton::Left,
            });
            if which == "menu-hover" {
                // A row of the menu, which opens upward from the button.
                w.window().dispatch_event(WindowEvent::PointerMoved {
                    position: slint::LogicalPosition::new(466.0, 680.0),
                });
            }
        }
    }
}

/// Update screens: neither the download, the swap nor the version check runs
/// in mock mode, so fake the state each one reports.
/// Synthetic left click at a logical window position, for the screens that
/// have to go through real event delivery rather than setting properties.
fn click_at(w: &MainWindow, x: f32, y: f32) {
    use slint::platform::{PointerEventButton, WindowEvent};
    let position = slint::LogicalPosition::new(x, y);
    w.window()
        .dispatch_event(WindowEvent::PointerMoved { position });
    w.window().dispatch_event(WindowEvent::PointerPressed {
        position,
        button: PointerEventButton::Left,
    });
    w.window().dispatch_event(WindowEvent::PointerReleased {
        position,
        button: PointerEventButton::Left,
    });
}

fn show_update_screen(w: &MainWindow, which: &str) {
    if which == "whats-new" {
        // A release-please shaped sample body.
        let body = "## [9.9.9](https://example.com) (2026-01-01)\n\
            ### App Features\n\
            * **app:** show open connections as a rail ([abc1234](https://example.com))\n\
            * **app:** ask before installing a downloaded update\n\
            ### Bug Fixes\n\
            * **app:** stop tab titles truncating in the document tab strip\n";
        super::update::open_whats_new(w, "9.9.9", crate::release_notes::parse(body));
        return;
    }
    w.set_update_version("9.9.9".into());
    w.set_update_self_update_supported(true);
    w.set_update_available(true);
    if which == "update-ready" || which == "update-install" {
        // Download finished: the install-now dialog.
        w.set_update_stage("ready".into());
        w.set_update_ready_open(which != "update-install");
        if which == "update-install" {
            w.set_settings_tab(1);
            w.set_settings_open(true);
        }
        // "update-install" carries on and clicks "Install and Relaunch" for
        // real. Setting the properties by hand is not the same test: the
        // click closes the dialog from inside the button's own TouchArea,
        // which is what used to abort the app, so the event has to come
        // through the window. Tuned for the harness default RDB_WIN=1280x800;
        // at another size the click lands on the veil and the shot still
        // shows the dialog, which is the failure showing itself.
        if which == "update-install" {
            let weak = w.as_weak();
            let t = Box::leak(Box::new(slint::Timer::default()));
            t.start(
                slint::TimerMode::SingleShot,
                std::time::Duration::from_millis(500),
                move || {
                    if let Some(w) = weak.upgrade() {
                        click_at(&w, 520.0, 355.0);
                    }
                },
            );
        }
    } else if which == "update-restarting" {
        // The install as it really unfolds, with Settings → Updates open
        // behind the dialog the whole time (where 0.47.1 aborted in the
        // field): the download lands and opens the dialog, "Install and
        // Relaunch" is clicked through the window, then each macOS install
        // step arrives frames apart, the way `perform_swap`'s `on_step` posts
        // them. The other update screens set their state before the first
        // frame, so none of them re-lays-out the Updates tab mid-install.
        w.set_settings_tab(1);
        w.set_settings_open(true);
        w.set_update_stage("downloading".into());
        w.set_update_progress(0.4);
        let at = |ms: u64, f: Box<dyn Fn(&MainWindow)>| {
            let weak = w.as_weak();
            let t = Box::leak(Box::new(slint::Timer::default()));
            t.start(
                slint::TimerMode::SingleShot,
                std::time::Duration::from_millis(ms),
                move || {
                    if let Some(w) = weak.upgrade() {
                        f(&w);
                    }
                },
            );
        };
        at(
            500,
            Box::new(|w| {
                w.set_update_progress(1.0);
                w.set_update_stage("ready".into());
                w.set_update_ready_open(true);
            }),
        );
        // "Install and Relaunch" at the harness default 1280x800.
        at(1000, Box::new(|w| click_at(w, 802.0, 377.0)));
        for (i, step) in ["Mounting", "Copying", "Replacing", "Relaunching"]
            .into_iter()
            .enumerate()
        {
            at(
                1200 + 200 * i as u64,
                Box::new(move |w| {
                    w.set_update_step(step.into());
                    w.set_update_stage("restarting".into());
                }),
            );
        }
    } else {
        // Banner mid-install.
        w.set_update_stage("restarting".into());
        w.set_update_step("Copying".into());
    }
}

fn schedule_modal_timer(window: &MainWindow, screen: &str) {
    if !matches!(
        screen,
        "modal-db"
            | "modal-conn"
            | "modal-add-mongo"
            | "function"
            | "palette"
            | "update-installing"
            | "update-restarting"
            | "update-ready"
            | "update-install"
            | "whats-new"
            | "settings"
            | "settings-updates"
            | "settings-about"
            | "tooltip"
            | "zoom"
            | "shortcuts"
            | "export-menu"
            | "menu-hover"
            | "notch-light"
            | "sidebar-collapsed"
            | "conn-add"
            | "tab-menu"
    ) {
        return;
    }
    let weak = window.as_weak();
    let which = screen.to_string();
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
                        w.set_f_import_url("mongodb://root:secret@203.0.113.31:32343/admin?authMechanism=DEFAULT&replicaSet=rs0".into());
                    }
                    "palette" => w.invoke_toggle_palette(),
                    "update-installing" | "update-restarting" | "update-ready" | "update-install"
                    | "whats-new" => {
                        show_update_screen(&w, &which)
                    }
                    // Settings modal: Appearance (0) or Updates (1) tab.
                    // Hover the picker's "+": a Material tooltip inside the
                    // clipped connection card (it used to be cut off there).
                    "tooltip" => {
                        w.window()
                            .dispatch_event(slint::platform::WindowEvent::PointerMoved {
                                position: slint::LogicalPosition::new(516.0, 100.0),
                            });
                    }
                    // Three ⌘+ steps (level 16 ≈ 123%): icons and Material
                    // controls must scale with the text.
                    "zoom" => w.invoke_zoom_step(3),
                    // Keyboard Shortcuts modal (long list; scrolls in a short window).
                    "shortcuts" => w.set_shortcuts_open(true),
                    // Click the picker footer's "Export ▾" so the popup menu
                    // (not reachable any other way) shows in the screenshot.
                    "export-menu" | "menu-hover" | "notch-light" | "sidebar-collapsed"
                    | "conn-add" | "tab-menu" => {
                        show_chrome_screen(&w, &which)
                    }
                    "settings" | "settings-updates" | "settings-about" => {
                        w.set_settings_tab(settings_screen_tab(&which));
                        w.set_settings_open(true);
                    }
                    _ => w.invoke_open_function("uuid_generate_v3".into()),
                }
            }
        },
    );
}

/// "workspace" opens the mock `emiten` fixture; "workspace-<name>" opens
/// that table instead, so the harness can drive a real connection whose
/// tables it cannot know in advance.
fn schedule_workspace_open_timer(window: &MainWindow, screen: &str) {
    // "chart" opens the default table too (not a `workspace-<table>` name).
    if !screen.starts_with("workspace") && screen != "chart" {
        return;
    }
    let weak = window.as_weak();
    let table = match screen.strip_prefix("workspace-") {
        Some(name) if !name.is_empty() => name.to_string(),
        _ => "emiten".to_string(),
    };
    let t2 = Box::leak(Box::new(slint::Timer::default()));
    t2.start(
        slint::TimerMode::SingleShot,
        std::time::Duration::from_millis(1800),
        move || {
            if let Some(w) = weak.upgrade() {
                w.invoke_open_table("".into(), table.as_str().into());
            }
        },
    );
}

/// Polls every 100ms until `cond` holds, then fires `act` once and stops.
/// e2e editing scenarios are layered on the base screens with this instead
/// of a fixed delay, since a fixed delay would race the async
/// connect/browse pipeline.
fn when(window: &MainWindow, cond: ScreenCond, act: ScreenAct) {
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
}

/// grid loaded with an editable page (pk fetched)
fn grid_ready_cond() -> ScreenCond {
    Rc::new(|w| w.get_grid_cells().row_count() > 0 && !w.get_grid_read_only())
}

fn has_pending_cond() -> ScreenCond {
    Rc::new(|w| w.get_pending_count() > 0)
}

/// Cell-edit scenarios against a freshly loaded grid: the initial edit
/// (shared by four screens) plus each screen's own follow-up once that
/// edit is pending.
fn schedule_grid_edit_scenarios(window: &MainWindow, screen: &str) {
    let grid_ready = grid_ready_cond();
    if matches!(
        screen,
        "workspace-dirty" | "workspace-guard" | "workspace-commit" | "workspace-tabnav"
    ) {
        when(
            window,
            grid_ready.clone(),
            Rc::new(|w| w.invoke_cell_edited(0, 2, "Bayan EDITED".into())),
        );
    }
    match screen {
        "workspace-active-commit" => when(
            window,
            grid_ready.clone(),
            Rc::new(|w| {
                w.invoke_edit_cell(0, 2);
                w.set_editing_value("Bayan ACTIVE SAVE".into());
                w.invoke_commit_edits();
            }),
        ),
        "workspace-detail-commit" => when(
            window,
            grid_ready.clone(),
            Rc::new(|w| {
                w.invoke_stage_cell(
                    0,
                    2,
                    "Bayan Resources — long detail value saved from the right panel".into(),
                );
                w.invoke_commit_edits();
            }),
        ),
        "workspace-long-edit" => when(
            window,
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
        ),
        "workspace-pointer-edit" => when(
            window,
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
        ),
        _ => {}
    }
    schedule_pending_edit_followups(window, screen);
    schedule_standalone_grid_actions(window, screen, grid_ready);
}

/// Second step for the four screens above: fires once the initial edit is
/// pending, each with its own distinct follow-up action.
fn schedule_pending_edit_followups(window: &MainWindow, screen: &str) {
    let has_pending = has_pending_cond();
    match screen {
        // second pending change: a delete-marked row
        "workspace-dirty" => when(
            window,
            has_pending,
            Rc::new(|w| {
                w.set_selected_row(2);
                w.invoke_mark_delete();
            }),
        ),
        // navigation with pending edits must be refused with a message
        "workspace-guard" => when(window, has_pending, Rc::new(|w| w.invoke_next_page())),
        // full CRUD loop: buffer → WriteOps → mock commit → refetch
        "workspace-commit" => when(window, has_pending, Rc::new(|w| w.invoke_commit_edits())),
        // Tab stores the edited cell and opens the neighbour's editor
        "workspace-tabnav" => when(
            window,
            has_pending,
            Rc::new(|w| w.invoke_cell_advance(0, 3, "TABBED".into(), true)),
        ),
        _ => {}
    }
}

/// Grid actions that don't chain off the shared initial edit above: each
/// applies straight to the freshly loaded grid.
fn schedule_standalone_grid_actions(window: &MainWindow, screen: &str, grid_ready: ScreenCond) {
    match screen {
        "workspace-filter" => when(
            window,
            grid_ready,
            Rc::new(|w| {
                w.set_data_filter_open(true);
                w.set_filter_col("name".into());
                w.set_filter_op("ILIKE".into());
                w.set_grid_filter("mitra".into());
                w.invoke_apply_filter();
            }),
        ),
        "workspace-limit" => when(
            window,
            grid_ready,
            Rc::new(|w| w.invoke_set_limit("25".into())),
        ),
        // Chart view of the loaded grid (column pickers + bars).
        "chart" => when(window, grid_ready, Rc::new(|w| w.set_sql_view_mode(2))),
        "workspace-insert" => when(window, grid_ready, Rc::new(|w| w.invoke_add_row())),
        "workspace-users-bool" | "workspace-users-date" => {
            when(window, grid_ready, Rc::new(|w| w.invoke_add_row()));
            let date = screen == "workspace-users-date";
            when(
                window,
                has_pending_cond(),
                Rc::new(move |w| {
                    let rows = w.get_grid_cells().row_count() / w.get_grid_col_count() as usize;
                    w.invoke_edit_cell(rows.saturating_sub(1) as i32, if date { 4 } else { 3 });
                }),
            );
        }
        _ => {}
    }
}

/// Real UI transition: table browse → global SQL button, and a multi-tab
/// pin-and-switch flow. The fresh query tab must not inherit the table
/// request's loading state.
fn schedule_tab_flow_scenarios(window: &MainWindow, screen: &str) {
    use slint::Model as _;
    match screen {
        "workspace-sql" => when(
            window,
            Rc::new(|w| w.get_active_table() == "emiten"),
            Rc::new(|w| w.invoke_new_tab()),
        ),
        "workspace-tabflow" => {
            when(
                window,
                Rc::new(|w| w.get_active_table() == "emiten" && w.get_tabs().row_count() == 1),
                Rc::new(|w| w.invoke_open_table("".into(), "referral_sources".into())),
            );
            when(
                window,
                Rc::new(|w| {
                    w.get_active_table() == "referral_sources" && w.get_tabs().row_count() == 1
                }),
                Rc::new(|w| {
                    w.invoke_pin_table("".into(), "referral_sources".into());
                    w.invoke_open_table("".into(), "sectors".into());
                }),
            );
            when(
                window,
                Rc::new(|w| w.get_active_table() == "sectors" && w.get_tabs().row_count() == 2),
                Rc::new(|w| w.invoke_new_tab()),
            );
        }
        _ => {}
    }
}

fn schedule_sql_editor_scenarios(window: &MainWindow, screen: &str, load_editor_text: &PaneTextFn) {
    match screen {
        // ⌘A select-all: the whole query gets the selection tint
        "sql-select" => when(
            window,
            Rc::new(|w| !w.get_query_text().trim().is_empty()),
            Rc::new(|w| {
                w.invoke_editor_key("a".into(), true, false, false);
            }),
        ),
        // a query with zero rows shows the empty state, not a blank pane
        "sql-empty" => {
            let load = load_editor_text.clone();
            when(
                window,
                Rc::new(|w| !w.get_results_meta().is_empty()),
                Rc::new(move |w| {
                    load(0, "SELECT * FROM emiten OFFSET 99999");
                    w.invoke_run_query();
                }),
            );
        }
        // ⌘F find bar: highlights the first match of a term
        "sql-find" => when(
            window,
            Rc::new(|w| !w.get_query_text().trim().is_empty()),
            Rc::new(|w| {
                w.invoke_toggle_find();
                w.set_find_text("sector".into());
                w.invoke_find_changed("sector".into());
            }),
        ),
        // multi-statement run: status reads "N statements · …"
        "sql-multi" => {
            let load = load_editor_text.clone();
            when(
                window,
                Rc::new(|w| !w.get_results_meta().is_empty()),
                Rc::new(move |w| {
                    load(0, "SELECT 1;\nSELECT * FROM emiten LIMIT 5;");
                    w.invoke_run_query();
                }),
            );
        }
        _ => {}
    }
}
