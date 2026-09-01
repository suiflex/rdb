//! Result export and the right (split) pane: copying a detail field or the
//! whole grid, CSV/JSON/SQL export, cancelling a running stream or query, and
//! the right pane's own grid interactions, which mirror the left pane's.
//!
//! Split out of `main`; the handler bodies are unchanged.

use std::rc::Rc;

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};

use crate::*;

/// Footer note for a partial copy: how much of the grid actually went to the
/// clipboard.
fn copied_msg(grid: &model::GridModel, r: CellRange) -> String {
    if r.rows() == 1 && r.cols() == 1 {
        format!("copied 1 cell from {}", grid.columns[r.c0].name)
    } else if r.cols() == 1 {
        format!(
            "copied {} of {} rows, column {}",
            r.rows(),
            grid.rows.len(),
            grid.columns[r.c0].name
        )
    } else {
        format!(
            "copied {} of {} rows x {} columns",
            r.rows(),
            grid.rows.len(),
            r.cols()
        )
    }
}

pub(crate) fn wire(window: &MainWindow, state: &AppState) {
    let AppState {
        panes, rt, current, ..
    } = state.clone();
    let displayed_grid = panes[0].displayed_grid.clone();

    // ----- Details panel: copy one field value to the clipboard -----
    {
        let weak = window.as_weak();
        window.on_copy_text(move |s| {
            let copied = clip_set(&s);
            if let Some(w) = weak.upgrade() {
                let msg = if copied {
                    "copied field"
                } else {
                    "copy failed"
                };
                if w.get_active_pane() == 1 {
                    w.set_p1_result_status(msg.into());
                } else {
                    w.set_result_status(msg.into());
                }
            }
        });
    }

    // ----- Copy results (TSV → clipboard) / Export CSV (~/Downloads) -----
    {
        let weak = window.as_weak();
        let displayed_grid = displayed_grid.clone();
        window.on_copy_results(move |all| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let Some(grid) = displayed_grid.lock().unwrap().clone() else {
                return;
            };
            // A selection copies its own cells, values only — the header row
            // belongs to a whole-grid copy, not to a couple of cells pasted
            // into a spreadsheet.
            let block = selected_block(&w, 0, &grid, all);
            let text = export::for_clipboard(match block {
                Some(r) => export::to_tsv_body(&sliced_grid(&grid, r)),
                None => export::to_tsv(&grid),
            });
            use copypasta::ClipboardProvider;
            let msg =
                match copypasta::ClipboardContext::new().and_then(|mut cb| cb.set_contents(text)) {
                    Ok(()) => match block {
                        Some(r) => copied_msg(&grid, r),
                        None => format!("copied {} rows", grid.rows.len()),
                    },
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
                eprintln!("[export] nothing to export: no result grid loaded");
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
            save_via_dialog(
                &w,
                format!("rdb-export.{ext}"),
                filter.to_string(),
                ext.to_string(),
                contents,
                |w, msg| w.set_results_meta(SharedString::from(msg)),
            );
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

    // ----- Cancel a running buffered query -----
    //
    // Two halves, and both are needed. Aborting the tokio task frees the pane
    // and the connection guard immediately, which is what the user sees. But
    // the abort only drops the client end of the socket: the statement keeps
    // running on the server, still holding locks and burning CPU, until the
    // server notices. `cancel_running` is the half that actually stops it.
    //
    // The server-side request has to be fired before the abort, and off the UI
    // thread, since it does its own I/O (Postgres dials a second connection,
    // MySQL and ClickHouse issue a KILL, Mongo walks currentOp). It is
    // best-effort by contract, so its outcome is not reported: the pane is
    // already free and a failure here means the statement was gone anyway, or
    // the user lacks the privilege — neither is worth a modal over.
    for pane in 0..2 {
        let weak = window.as_weak();
        let panes = panes.clone();
        let rt = rt.clone();
        let current = current.clone();
        let cancel = move || {
            let current = current.clone();
            rt.spawn(async move {
                let driver = { current.lock().await.as_ref().map(|(_, d)| d.clone()) };
                if let Some(d) = driver {
                    let _ = d.cancel_running().await;
                }
            });
            if let Some(h) = panes[pane].query_abort.borrow_mut().take() {
                h.abort();
            }
            if let Some(w) = weak.upgrade() {
                set_p_query_running(&w, pane, false);
            }
        };
        if pane == 0 {
            window.on_cancel_query(cancel);
        } else {
            window.on_p1_cancel_query(cancel);
        }
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
            apply_result(&w, 1, &reordered);
        });
    }

    // ----- right-pane grid interactions -----
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        window.on_p1_select_cell(move |row, col| {
            if let Some(w) = weak.upgrade() {
                w.set_p1_selected_row(row);
                w.set_p1_selected_col(col);
                if let Some(g) = panes[1].displayed_grid.lock().unwrap().as_ref() {
                    refresh_detail_pretty(&w, 1, g, row);
                }
            }
        });
    }
    {
        let weak = window.as_weak();
        window.on_p1_row_press(move |row, col, shift| {
            if let Some(w) = weak.upgrade() {
                if !shift || w.get_p1_range_anchor_row() < 0 {
                    set_p_range_anchor(&w, 1, row);
                    set_p_range_anchor_col(&w, 1, col);
                }
                set_p_selected_row(&w, 1, row);
                w.set_p1_selected_col(col);
                // A shift-click finalizes the block on its own: it never fires
                // a drag, so without this the tint showed a block that Copy
                // then ignored.
                let anchor = (w.get_p1_range_anchor_row(), w.get_p1_range_anchor_col());
                SELECTED_RANGE.with(|s| {
                    s.borrow_mut()[1] = CellRange::between(anchor, (row, col));
                });
            }
        });
    }
    {
        let weak = window.as_weak();
        window.on_p1_cell_drag(move |row, origin_col, dx| {
            if let Some(w) = weak.upgrade() {
                let col = drag_column(&w, 1, origin_col, dx);
                set_p_selected_row(&w, 1, row);
                w.set_p1_selected_col(col);
                let anchor = (w.get_p1_range_anchor_row(), w.get_p1_range_anchor_col());
                SELECTED_RANGE.with(|s| {
                    s.borrow_mut()[1] = CellRange::between(anchor, (row, col));
                });
            }
        });
    }
    {
        let weak = window.as_weak();
        window.on_p1_edit_cell(move |row, col| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            // Open the cell overlay even when read-only — as a selectable viewer
            // (the grid's read-only branch), so the value can be copied out.
            if row < 0 || col < 0 {
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
            let (value, is_large) = format_cell_edit_value(value);
            set_p_editing(&w, 1, row, col);
            set_p_editing_large(&w, 1, is_large);
            w.set_p1_editing_value(value);
        });
    }
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        window.on_p1_stage_cell(move |row, col, text| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            if w.get_p1_grid_read_only() || row < 0 || col < 0 {
                return;
            }
            let base = panes[1]
                .displayed_grid
                .lock()
                .unwrap()
                .as_ref()
                .map(|grid| grid.rows.len())
                .unwrap_or(0);
            let pending = {
                let mut buf = panes[1].edit_buf.lock().unwrap();
                buf.set_cell(base, row as usize, col as usize, text.to_string());
                buf.pending_count() as i32
            };
            set_p_pending_count(&w, 1, pending);
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
            let pending = {
                let mut buf = panes[1].edit_buf.lock().unwrap();
                buf.set_cell(
                    base,
                    row.max(0) as usize,
                    col.max(0) as usize,
                    text.to_string(),
                );
                buf.pending_count() as i32
            };
            set_p_editing(&w, 1, -1, -1);
            if let Some(grid) = panes[1].displayed_grid.lock().unwrap().as_ref() {
                let edits = panes[1].edit_buf.lock().unwrap();
                paint_grid_with_edits(&w, 1, grid, &edits);
            }
            set_p_pending_count(&w, 1, pending);
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
            apply_result(&w, 1, &sorted);
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
            apply_result(&w, 1, &filtered);
        });
    }
    // ----- right pane: filter row Apply (client-side, mirrors group 0) -----
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        window.on_p1_apply_filter(move || {
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
            if matches!(*view, model::ResultView::Affected(_)) {
                return;
            }
            let needle = w.get_p1_grid_filter().to_string().to_lowercase();
            let fcol = w.get_p1_filter_col().to_string();
            let fop = w.get_p1_filter_op().to_string();
            let hidden = panes[1].hidden_cols.lock().unwrap().clone();
            let order = panes[1].col_order.lock().unwrap().clone();
            let cfilters = panes[1].col_filters.lock().unwrap().clone();
            let (scol, sasc) = *panes[1].sort_state.lock().unwrap();
            // Filtering renumbers rows, so pending edits keyed by row index would
            // land on the wrong rows — drop them.
            panes[1].edit_buf.lock().unwrap().clear();
            set_p_pending_count(&w, 1, 0);
            set_p_editing(&w, 1, -1, -1);
            let filtered = compute_view(
                &view, &needle, &fcol, &fop, &cfilters, &hidden, &order, scol, sasc,
            );
            *panes[1].displayed_grid.lock().unwrap() = view_grid(&filtered);
            apply_result(&w, 1, &filtered);
        });
    }
    // ----- right pane: Copy result grid as TSV -----
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        window.on_p1_copy_results(move |all| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let Some(grid) = panes[1].displayed_grid.lock().unwrap().clone() else {
                return;
            };
            let block = selected_block(&w, 1, &grid, all);
            let text = export::for_clipboard(match block {
                Some(r) => export::to_tsv_body(&sliced_grid(&grid, r)),
                None => export::to_tsv(&grid),
            });
            use copypasta::ClipboardProvider;
            let msg =
                match copypasta::ClipboardContext::new().and_then(|mut cb| cb.set_contents(text)) {
                    Ok(()) => match block {
                        Some(r) => copied_msg(&grid, r),
                        None => format!("copied {} rows", grid.rows.len()),
                    },
                    Err(e) => format!("copy failed: {e}"),
                };
            set_p_results_meta(&w, 1, SharedString::from(msg));
        });
    }
    // ----- right pane: Export result grid -----
    {
        let weak = window.as_weak();
        let panes = panes.clone();
        window.on_p1_export_results(move |fmt| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let Some(grid) = panes[1].displayed_grid.lock().unwrap().clone() else {
                set_p_results_meta(&w, 1, SharedString::from("nothing to export"));
                return;
            };
            let (ext, filter, contents) = match fmt {
                1 => ("json", "JSON", export::to_json(&grid)),
                2 => ("tsv", "TSV", export::to_tsv(&grid)),
                3 => {
                    let table = w.get_p1_active_table();
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
            save_via_dialog(
                &w,
                format!("rdb-export.{ext}"),
                filter.to_string(),
                ext.to_string(),
                contents,
                |w, msg| set_p_results_meta(w, 1, SharedString::from(msg)),
            );
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
}
