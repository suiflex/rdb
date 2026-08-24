//! Pane-indexed property accessors for the two result panes.
//!
//! The `.slint` window exposes a separate property per pane (`cells` /
//! `p1_cells`, `columns` / `p1_columns`, and so on) rather than an indexed
//! model, so every read and write needs the same `if pane == 0 { … } else { … }`
//! fork. These wrappers are that fork, once each, and let the rest of the app
//! pass a `pane: usize` around instead.

use slint::{Model, ModelRc, SharedString};

use crate::{ChartBar, DocRow, ErrorMark, GridCell, GridColumn, IndexRow, MainWindow, PaletteItem};

// ----- per-pane result setters: pane 0 writes the base properties, pane 1 the
// p1-* mirror. Workspace groups render independently, including table chrome.
pub(crate) fn set_p_cells(w: &MainWindow, pane: usize, m: ModelRc<GridCell>) {
    if pane == 0 {
        w.set_grid_cells(m);
    } else {
        w.set_p1_cells(m);
    }
}

pub(crate) fn set_p_columns(w: &MainWindow, pane: usize, m: ModelRc<GridColumn>) {
    if pane == 0 {
        w.set_grid_columns(m);
    } else {
        w.set_p1_columns(m);
    }
}

pub(crate) fn set_p_col_count(w: &MainWindow, pane: usize, n: i32) {
    if pane == 0 {
        w.set_grid_col_count(n);
    } else {
        w.set_p1_col_count(n);
    }
}

pub(crate) fn set_p_col_widths(w: &MainWindow, pane: usize, m: ModelRc<f32>) {
    if pane == 0 {
        w.set_grid_col_widths(m);
    } else {
        w.set_p1_col_widths(m);
    }
}

pub(crate) fn set_p_result_kind(w: &MainWindow, pane: usize, k: i32) {
    if pane == 0 {
        w.set_result_kind(k);
    } else {
        w.set_p1_result_kind(k);
    }
}

pub(crate) fn set_p_result_status(w: &MainWindow, pane: usize, s: SharedString) {
    if pane == 0 {
        w.set_result_status(s);
    } else {
        w.set_p1_result_status(s);
    }
}

pub(crate) fn set_p_status_error(w: &MainWindow, pane: usize, b: bool) {
    if pane == 0 {
        w.set_status_error(b);
    } else {
        w.set_p1_status_error(b);
    }
}

pub(crate) fn set_p_active_table(w: &MainWindow, pane: usize, table: SharedString) {
    if pane == 0 {
        w.set_active_table(table);
    } else {
        w.set_p1_active_table(table);
    }
}

pub(crate) fn set_p_total_rows(w: &MainWindow, pane: usize, total: i32) {
    if pane == 0 {
        w.set_total_rows(total);
    } else {
        w.set_p1_total_rows(total);
    }
}

pub(crate) fn set_p_page_bounds(
    w: &MainWindow,
    pane: usize,
    start: i32,
    end: i32,
    prev: bool,
    next: bool,
) {
    if pane == 0 {
        w.set_page_start(start);
        w.set_page_end(end);
        w.set_can_prev(prev);
        w.set_can_next(next);
    } else {
        w.set_p1_page_start(start);
        w.set_p1_page_end(end);
        w.set_p1_can_prev(prev);
        w.set_p1_can_next(next);
    }
}

pub(crate) fn set_p_read_only(w: &MainWindow, pane: usize, read_only: bool) {
    if pane == 0 {
        w.set_grid_read_only(read_only);
    } else {
        w.set_p1_grid_read_only(read_only);
    }
}

/// Arm (or clear) the editor's error highlight for `pane`. `None` clears it.
pub(crate) fn set_p_error_mark(w: &MainWindow, pane: usize, mark: Option<ErrorMark>) {
    // One gate for every caller: the mark stays in pane state either way, so
    // turning the preference back on can re-arm it without a re-run.
    let mark = mark.filter(|_| w.get_error_highlight());
    let m = mark.unwrap_or(ErrorMark {
        line: -1,
        from: -1,
        to: -1,
        col: 0,
        len: 0,
    });
    if pane == 0 {
        w.set_error_line(m.line);
        w.set_error_from(m.from);
        w.set_error_to(m.to);
        w.set_error_col(m.col);
        w.set_error_len(m.len);
    } else {
        w.set_p1_error_line(m.line);
        w.set_p1_error_from(m.from);
        w.set_p1_error_to(m.to);
        w.set_p1_error_col(m.col);
        w.set_p1_error_len(m.len);
    }
}

pub(crate) fn set_p_pending_count(w: &MainWindow, pane: usize, n: i32) {
    if pane == 0 {
        w.set_pending_count(n);
    } else {
        w.set_p1_pending_count(n);
    }
}

pub(crate) fn set_p_index_rows(w: &MainWindow, pane: usize, rows: ModelRc<IndexRow>) {
    if pane == 0 {
        w.set_index_rows(rows);
    } else {
        w.set_p1_index_rows(rows);
    }
}

pub(crate) fn set_p_results_meta(w: &MainWindow, pane: usize, s: SharedString) {
    if pane == 0 {
        w.set_results_meta(s);
    } else {
        w.set_p1_results_meta(s);
    }
}

pub(crate) fn set_p_chart_bars(w: &MainWindow, pane: usize, m: ModelRc<ChartBar>) {
    if pane == 0 {
        w.set_chart_bars(m);
    } else {
        w.set_p1_chart_bars(m);
    }
}

pub(crate) fn set_p_grid_sort(w: &MainWindow, pane: usize, col: i32, asc: bool) {
    if pane == 0 {
        w.set_grid_sort_col(col);
        w.set_grid_sort_asc(asc);
    } else {
        w.set_p1_grid_sort_col(col);
        w.set_p1_grid_sort_asc(asc);
    }
}

pub(crate) fn set_p_grid_col_filters(w: &MainWindow, pane: usize, m: ModelRc<SharedString>) {
    if pane == 0 {
        w.set_grid_col_filters(m);
    } else {
        w.set_p1_grid_col_filters(m);
    }
}

pub(crate) fn set_p_filter_columns(w: &MainWindow, pane: usize, m: ModelRc<SharedString>) {
    if pane == 0 {
        w.set_filter_columns(m);
    } else {
        w.set_p1_filter_columns(m);
    }
}

pub(crate) fn set_p_filter_col(w: &MainWindow, pane: usize, v: SharedString) {
    if pane == 0 {
        w.set_filter_col(v);
    } else {
        w.set_p1_filter_col(v);
    }
}

pub(crate) fn set_p_grid_filter(w: &MainWindow, pane: usize, v: SharedString) {
    if pane == 0 {
        w.set_grid_filter(v);
    } else {
        w.set_p1_grid_filter(v);
    }
}

pub(crate) fn set_p_detail_pretty(w: &MainWindow, pane: usize, m: ModelRc<SharedString>) {
    if pane == 0 {
        w.set_grid_detail_pretty(m);
    } else {
        w.set_p1_detail_pretty(m);
    }
}

pub(crate) fn set_p_selected_row(w: &MainWindow, pane: usize, row: i32) {
    if pane == 0 {
        w.set_selected_row(row);
    } else {
        w.set_p1_selected_row(row);
    }
}

pub(crate) fn set_p_range_anchor(w: &MainWindow, pane: usize, row: i32) {
    if pane == 0 {
        w.set_range_anchor_row(row);
    } else {
        w.set_p1_range_anchor_row(row);
    }
}

pub(crate) fn set_p_range_anchor_col(w: &MainWindow, pane: usize, col: i32) {
    if pane == 0 {
        w.set_range_anchor_col(col);
    } else {
        w.set_p1_range_anchor_col(col);
    }
}

pub(crate) fn get_p_col_widths(w: &MainWindow, pane: usize) -> Vec<f32> {
    if pane == 0 {
        w.get_grid_col_widths().iter().collect()
    } else {
        w.get_p1_col_widths().iter().collect()
    }
}

pub(crate) fn set_p_editing(w: &MainWindow, pane: usize, row: i32, col: i32) {
    if pane == 0 {
        w.set_editing_row(row);
        w.set_editing_col(col);
    } else {
        w.set_p1_editing_row(row);
        w.set_p1_editing_col(col);
    }
}

pub(crate) fn set_p_editing_large(w: &MainWindow, pane: usize, large: bool) {
    if pane == 0 {
        w.set_editing_large(large);
    } else {
        w.set_p1_editing_large(large);
    }
}

pub(crate) fn set_p_doc_tree(w: &MainWindow, pane: usize, m: ModelRc<DocRow>) {
    if pane == 0 {
        w.set_doc_tree(m);
    } else {
        w.set_p1_doc_tree(m);
    }
}

pub(crate) fn set_p_query_running(w: &MainWindow, pane: usize, b: bool) {
    if pane == 0 {
        w.set_query_running(b);
    } else {
        w.set_p1_query_running(b);
    }
}

pub(crate) fn set_p_streaming(w: &MainWindow, pane: usize, b: bool) {
    if pane == 0 {
        w.set_streaming(b);
    } else {
        w.set_p1_streaming(b);
    }
}

pub(crate) fn set_p_completion_items(w: &MainWindow, pane: usize, m: ModelRc<PaletteItem>) {
    if pane == 0 {
        w.set_completion_items(m);
    } else {
        w.set_p1_completion_items(m);
    }
}

pub(crate) fn set_p_completion_visible(w: &MainWindow, pane: usize, b: bool) {
    if pane == 0 {
        w.set_completion_visible(b);
    } else {
        w.set_p1_completion_visible(b);
    }
}

pub(crate) fn get_p_completion_visible(w: &MainWindow, pane: usize) -> bool {
    if pane == 0 {
        w.get_completion_visible()
    } else {
        w.get_p1_completion_visible()
    }
}

pub(crate) fn set_p_completion_selected(w: &MainWindow, pane: usize, i: i32) {
    if pane == 0 {
        w.set_completion_selected(i);
    } else {
        w.set_p1_completion_selected(i);
    }
}

pub(crate) fn get_p_completion_selected(w: &MainWindow, pane: usize) -> i32 {
    if pane == 0 {
        w.get_completion_selected()
    } else {
        w.get_p1_completion_selected()
    }
}

pub(crate) fn p_completion_count(w: &MainWindow, pane: usize) -> i32 {
    if pane == 0 {
        w.get_completion_items().row_count() as i32
    } else {
        w.get_p1_completion_items().row_count() as i32
    }
}

pub(crate) fn set_p_find_open(w: &MainWindow, pane: usize, b: bool) {
    if pane == 0 {
        w.set_find_open(b);
    } else {
        w.set_p1_find_open(b);
    }
}

pub(crate) fn get_p_find_open(w: &MainWindow, pane: usize) -> bool {
    if pane == 0 {
        w.get_find_open()
    } else {
        w.get_p1_find_open()
    }
}

pub(crate) fn set_p_find_text(w: &MainWindow, pane: usize, s: SharedString) {
    if pane == 0 {
        w.set_find_text(s);
    } else {
        w.set_p1_find_text(s);
    }
}

pub(crate) fn get_p_find_text(w: &MainWindow, pane: usize) -> String {
    if pane == 0 {
        w.get_find_text().to_string()
    } else {
        w.get_p1_find_text().to_string()
    }
}

pub(crate) fn set_p_find_status(w: &MainWindow, pane: usize, s: SharedString) {
    if pane == 0 {
        w.set_find_status(s);
    } else {
        w.set_p1_find_status(s);
    }
}

/// Nudge the editor to scroll the cursor into view (e.g. after a find jump).
/// A plain counter bump, not a bound value — see `scroll-request` in
/// code-editor.slint for why it has to be one-shot.
pub(crate) fn bump_p_scroll_request(w: &MainWindow, pane: usize) {
    if pane == 0 {
        w.set_scroll_request(w.get_scroll_request().wrapping_add(1));
    } else {
        w.set_p1_scroll_request(w.get_p1_scroll_request().wrapping_add(1));
    }
}

/// Send the grid's scroll offset back to the top. Same one-shot counter idiom
/// as `bump_p_scroll_request` — see `scroll-top-tick` in tabular-grid.slint.
pub(crate) fn bump_p_scroll_grid_top(w: &MainWindow, pane: usize) {
    if pane == 0 {
        w.set_scroll_top_tick(w.get_scroll_top_tick().wrapping_add(1));
    } else {
        w.set_p1_scroll_top_tick(w.get_p1_scroll_top_tick().wrapping_add(1));
    }
}

pub(crate) fn get_p_cursor(w: &MainWindow, pane: usize) -> (usize, usize) {
    if pane == 0 {
        (w.get_cursor_line() as usize, w.get_cursor_col() as usize)
    } else {
        (
            w.get_p1_cursor_line() as usize,
            w.get_p1_cursor_col() as usize,
        )
    }
}
