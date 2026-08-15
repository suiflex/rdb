//! Client-side result-grid interactions for the left pane: the global filter,
//! the per-column filter row, sort-by-header, column drag-reorder, and the
//! Columns visibility popup.
//!
//! All of these re-derive the displayed grid from the cached base view via
//! `build_grid`; none of them touch the driver. Split out of `main`; the
//! handler bodies are unchanged.

use std::rc::Rc;

use slint::{ComponentHandle, Model, ModelRc, VecModel};

use crate::*;

pub(crate) fn wire(window: &MainWindow, state: &AppState) {
    let AppState {
        panes,
        last_view,
        browse_trigger,
        ..
    } = state.clone();
    let browse = panes[0].browse.clone();
    let edit_buf = panes[0].edit_buf.clone();
    let displayed_grid = panes[0].displayed_grid.clone();
    let hidden_cols = panes[0].hidden_cols.clone();
    let sort_state = panes[0].sort_state.clone();
    let col_order = panes[0].col_order.clone();
    let col_filters = panes[0].col_filters.clone();

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
            apply_result(&w, 0, &filtered);
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
                apply_result(&w, 0, &filtered);
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
            apply_result(&w, 0, &sorted);
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
            apply_result(&w, 0, &transformed);
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
            apply_result(&w, 0, &transformed);
        });
    }

    // Handle of the in-flight connect task, so the Cancel button can abort it.
}
