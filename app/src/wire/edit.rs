//! Grid row/cell selection and inline editing: row nav, cell click, opening
//! the editor on double-click, Enter/Tab commit, the footer's append-row and
//! delete-row toggles, and discard/commit of the pending write buffer for both
//! panes.
//!
//! Split out of `main`; the handler bodies are unchanged.

use std::rc::Rc;

use slint::{ComponentHandle, Model, SharedString};

use crate::*;

pub(crate) fn wire(window: &MainWindow, state: &AppState) {
    let AppState {
        rt,
        panes,
        current,
        cur_engine,
        query_console,
        ..
    } = state.clone();
    let edit_buf = panes[0].edit_buf.clone();
    let displayed_grid = panes[0].displayed_grid.clone();

    // ----- row nav (j/k) -----
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        window.on_move_row(move |delta| {
            if let Some(w) = weak.upgrade() {
                let pane = w.get_active_pane().clamp(0, 1) as usize;
                let n = if pane == 0 {
                    w.get_grid_col_count()
                } else {
                    w.get_p1_col_count()
                };
                let total = if n > 0 {
                    let cells = if pane == 0 {
                        w.get_grid_cells()
                    } else {
                        w.get_p1_cells()
                    };
                    (cells.row_count() as i32) / n
                } else {
                    0
                };
                if total > 0 {
                    let selected = if pane == 0 {
                        w.get_selected_row()
                    } else {
                        w.get_p1_selected_row()
                    };
                    let next = (selected + delta).clamp(0, total - 1);
                    set_p_selected_row(&w, pane, next);
                    if let Some(g) = panes[pane].displayed_grid.lock().unwrap().as_ref() {
                        refresh_detail_pretty(&w, pane, g, next);
                    }
                }
            }
        });
    }

    // ----- cell selection (click in the grid) -----
    {
        let weak = window.as_weak();
        let displayed_grid = displayed_grid.clone();
        window.on_select_cell(move |r, c| {
            if let Some(w) = weak.upgrade() {
                w.set_selected_row(r);
                w.set_selected_col(c);
                if let Some(g) = displayed_grid.lock().unwrap().as_ref() {
                    refresh_detail_pretty(&w, 0, g, r);
                }
            }
        });
    }
    {
        let weak = window.as_weak();
        window.on_row_press(move |row, col, shift| {
            if let Some(w) = weak.upgrade() {
                if !shift || w.get_range_anchor_row() < 0 {
                    set_p_range_anchor(&w, 0, row);
                    set_p_range_anchor_col(&w, 0, col);
                }
                set_p_selected_row(&w, 0, row);
                w.set_selected_col(col);
                SELECTED_RANGE.with(|s| s.borrow_mut()[0] = None);
            }
        });
    }
    {
        let weak = window.as_weak();
        window.on_row_drag(move |row| {
            if let Some(w) = weak.upgrade() {
                set_p_selected_row(&w, 0, row);
                let anchor = w.get_range_anchor_row();
                let anchor_col = w.get_range_anchor_col();
                SELECTED_RANGE.with(|s| {
                    s.borrow_mut()[0] =
                        (anchor >= 0 && anchor != row && anchor_col >= 0).then(|| {
                            (
                                anchor.min(row) as usize,
                                anchor.max(row) as usize,
                                anchor_col as usize,
                            )
                        });
                });
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
            // Structure view and chart results have no cell to open. Everything
            // else opens the cell overlay: an editor when the grid is writable,
            // otherwise a read-only, selectable viewer so the value can be copied
            // out (the read-only branch is driven by grid-read-only in the UI).
            if w.get_show_structure() || w.get_result_kind() == 3 {
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
            let (value, is_large) = format_cell_edit_value(value);
            w.set_editing_row(r);
            w.set_editing_col(c);
            w.set_editing_large(is_large);
            w.set_editing_value(value);
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
            // The right split pane is read-only, so a delete key there must
            // not mutate the editable left-pane buffer.
            if w.get_active_pane() == 1 {
                return;
            }
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
                let driver = {
                    let guard = current.lock().await;
                    guard.as_ref().map(|(_, d)| d.clone())
                };
                let outcome = match driver {
                    Some(driver) => driver.commit(&ops).await,
                    None => Err(rdb_core::error::RdbError::Connection(
                        "not connected".into(),
                    )),
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
                            w.set_result_status(SharedString::from(format!(
                                "error: {}",
                                editor::strip_error_marker(&e.to_string())
                            )));
                        }
                    }
                });
            });
        });
    }

    // ----- "Copy SQL": the statements a commit would run, without running them -----
    // Serves both panes; the pending-edits strip already reports whichever pane
    // is active, so the button follows the same rule.
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        let cur_engine = cur_engine.clone();
        let query_console = query_console.clone();
        window.on_copy_edit_sql(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let pane = if w.get_split() {
                w.get_active_pane().clamp(0, 1) as usize
            } else {
                0
            };
            let ops = {
                let buf = panes[pane].edit_buf.lock().unwrap();
                if buf.is_empty() {
                    return;
                }
                let dg = panes[pane].displayed_grid.lock().unwrap();
                let Some(g) = dg.as_ref() else {
                    return;
                };
                buf.to_ops(g)
            };
            let ops = match ops {
                Ok(ops) if !ops.is_empty() => ops,
                Ok(_) => return,
                Err(msg) => {
                    set_p_status_error(&w, pane, true);
                    set_p_result_status(&w, pane, SharedString::from(format!("error: {msg}")));
                    return;
                }
            };
            let Some(engine) = *cur_engine.borrow() else {
                return;
            };
            let statements = dispatch::write_statements(engine, &ops);
            if statements.is_empty() {
                return;
            }
            for statement in &statements {
                append_query_console(&query_console, statement.clone());
            }
            sync_query_console(&w, &query_console);
            let sql = statements.join(";\n") + ";";
            let n = statements.len();
            let copied = clip_set(&sql);
            set_p_status_error(&w, pane, !copied);
            set_p_result_status(
                &w,
                pane,
                SharedString::from(if copied {
                    format!("{n} statement{} copied", if n == 1 { "" } else { "s" })
                } else {
                    "error: no clipboard available".to_string()
                }),
            );
        });
    }

    // ----- right (split) pane: discard/commit, mirrors the left pane above -----
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        window.on_p1_discard_edits(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            panes[1].edit_buf.lock().unwrap().clear();
            set_p_pending_count(&w, 1, 0);
            set_p_editing(&w, 1, -1, -1);
            set_p_status_error(&w, 1, false);
            if let Some(grid) = panes[1].displayed_grid.lock().unwrap().as_ref() {
                let edits = panes[1].edit_buf.lock().unwrap();
                paint_grid_with_edits(&w, 1, grid, &edits);
            }
        });
    }
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        let current = current.clone();
        let rt = rt.clone();
        let cur_engine = cur_engine.clone();
        let query_console = query_console.clone();
        window.on_p1_commit_edits(move || {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if w.get_p1_editing_row() >= 0 && w.get_p1_editing_col() >= 0 {
                let base = panes[1]
                    .displayed_grid
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map(|g| g.rows.len())
                    .unwrap_or(0);
                panes[1].edit_buf.lock().unwrap().set_cell(
                    base,
                    w.get_p1_editing_row() as usize,
                    w.get_p1_editing_col() as usize,
                    w.get_p1_editing_value().to_string(),
                );
                set_p_editing(&w, 1, -1, -1);
                if let Some(grid) = panes[1].displayed_grid.lock().unwrap().as_ref() {
                    let edits = panes[1].edit_buf.lock().unwrap();
                    paint_grid_with_edits(&w, 1, grid, &edits);
                }
            }
            let ops = {
                let buf = panes[1].edit_buf.lock().unwrap();
                if buf.is_empty() {
                    return;
                }
                let dg = panes[1].displayed_grid.lock().unwrap();
                let Some(g) = dg.as_ref() else {
                    return;
                };
                buf.to_ops(g)
            };
            let ops = match ops {
                Ok(ops) if !ops.is_empty() => ops,
                Ok(_) => return,
                Err(msg) => {
                    set_p_status_error(&w, 1, true);
                    set_p_result_status(&w, 1, SharedString::from(format!("error: {msg}")));
                    return;
                }
            };
            let weak2 = weak.clone();
            let current = current.clone();
            let commit_buf = panes[1].edit_buf.clone();
            if let Some(engine) = *cur_engine.borrow() {
                for statement in dispatch::write_statements(engine, &ops) {
                    append_query_console(&query_console, statement);
                }
                sync_query_console(&w, &query_console);
            }
            let query_console = query_console.clone();
            set_p_status_error(&w, 1, false);
            set_p_result_status(&w, 1, SharedString::from("saving…"));
            rt.spawn(async move {
                let driver = {
                    let guard = current.lock().await;
                    guard.as_ref().map(|(_, d)| d.clone())
                };
                let outcome = match driver {
                    Some(driver) => driver.commit(&ops).await,
                    None => Err(rdb_core::error::RdbError::Connection(
                        "not connected".into(),
                    )),
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
                            // A no-op for a raw-query edit (browse.table is
                            // None there) — same as the left pane.
                            commit_buf.lock().unwrap().clear();
                            set_p_pending_count(&w, 1, 0);
                            set_p_status_error(&w, 1, false);
                            set_p_result_status(
                                &w,
                                1,
                                SharedString::from(format!("{n} rows written")),
                            );
                            w.invoke_p1_refresh_page();
                        }
                        Err(e) => {
                            // Keep the buffer so nothing typed is lost.
                            set_p_status_error(&w, 1, true);
                            set_p_result_status(
                                &w,
                                1,
                                SharedString::from(format!(
                                    "error: {}",
                                    editor::strip_error_marker(&e.to_string())
                                )),
                            );
                        }
                    }
                });
            });
        });
    }
}
