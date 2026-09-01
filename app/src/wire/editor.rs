//! The SQL editor itself: the Rust-owned text buffer, the lexer feeding the
//! span model, statement folding, and autocomplete.
//!
//! Unlike the other wiring modules this one returns two handles. `sync_editor`
//! (repaint a pane from the buffer) and `load_editor_text` (replace a pane's
//! text) are built here but needed by nearly every other module, so `main`
//! takes them back and passes them on through `AppFns`.
//!
//! Split out of `main`; the handler bodies are unchanged.

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::*;

/// The dialect the editor lexes and completes in: the active tab's own engine
/// first, the live connection only as a fallback. Highlighting and completion
/// both go through here so a tab can never be coloured in one dialect and
/// completed in another.
fn editor_language(
    w: &MainWindow,
    pane: usize,
    cur_engine: &RefCell<Option<rdb_connstore::Engine>>,
) -> rdb_connstore::QueryLanguage {
    crate::active_tab_language(w, pane).unwrap_or_else(|| {
        cur_engine
            .borrow()
            .map(rdb_connstore::Engine::language)
            .unwrap_or(rdb_connstore::QueryLanguage::Sql)
    })
}

/// Drop the completion popup and the replace-length it was built with. Anything
/// that moves the caret or rewrites the text under it without rebuilding the
/// list has to call this: `accept_completion` trusts the stored length and would
/// otherwise backspace over characters that are no longer the word being typed.
fn hide_completion(w: &MainWindow, group: &GroupRuntime, pane: usize) {
    set_p_completion_visible(w, pane, false);
    *group.completion_ctx.borrow_mut() = (0, Vec::new());
}

/// `folded_heads` stores fold state as raw line indices, but edits that add
/// or remove lines above a fold (Enter, Backspace/Delete merges, multi-line
/// paste) don't otherwise touch it — so a stale index stops matching the
/// head `fold_regions` recomputes on the next repaint, and the block reads
/// as open again. Re-anchor every head at or past the edit point by the
/// line-count delta so a fold stays glued to its statement.
fn shift_folded_heads(
    folded_heads: &RefCell<HashSet<usize>>,
    old_line: usize,
    old_len: usize,
    new_line: usize,
    new_len: usize,
) {
    if new_len == old_len {
        return;
    }
    let anchor = old_line.min(new_line) + 1;
    let diff = new_len as isize - old_len as isize;
    let mut folded = folded_heads.borrow_mut();
    let shifted: Vec<usize> = folded.drain().collect();
    for h in shifted {
        let h = if h >= anchor {
            (h as isize + diff).max(anchor as isize - 1) as usize
        } else {
            h
        };
        folded.insert(h);
    }
}

/// Lexed spans -> UI spans, carrying each span's start column so the renderer
/// can place it at an absolute `col * charw` instead of stacking rounded
/// widths in a layout (that rounding drifted the text off the caret grid).
/// `error_line` repaints plain tokens on the failing line in the error color.
pub(crate) fn ui_spans(spans: Vec<editor::Span>, error_line: bool) -> Vec<Span> {
    let mut col = 0;
    spans
        .into_iter()
        .map(|mut sp| {
            if error_line && sp.kind == 0 {
                sp.kind = 6;
            }
            let cols = sp.text.chars().count() as i32;
            let start = col;
            col += cols;
            // A tab has no glyph in IBM Plex Mono and draws as a tofu box. It
            // already sits in exactly one grid cell, so swapping it for a
            // space on the way to the renderer keeps every column where it is
            // and only changes what is painted — the buffer, and the text
            // handed to the driver, keep the tab the user pasted.
            let text = if sp.text.contains('\t') {
                sp.text.replace('\t', " ")
            } else {
                sp.text
            };
            Span {
                col: start,
                cols,
                text: text.into(),
                kind: sp.kind,
                sel: sp.sel,
            }
        })
        .collect()
}

/// Body lines collapsed under a closed fold head — the rows the editor draws
/// at zero height.
fn hidden_lines(lines: &[String], folded: &HashSet<usize>) -> Vec<bool> {
    let mut hidden = vec![false; lines.len()];
    for (head, end) in editor::fold_regions(lines) {
        if folded.contains(&head) {
            for h in hidden.iter_mut().take(end + 1).skip(head + 1) {
                *h = true;
            }
        }
    }
    hidden
}

/// Buffer line `rows` *visible* rows away from `from`. A drag is hit-tested in
/// screen rows, but a selection is addressed in buffer lines, and a line
/// collapsed under a closed fold occupies no row — counting buffer lines
/// instead landed the drag inside the folded block.
fn line_after_rows(hidden: &[bool], from: usize, rows: i32) -> usize {
    let step: isize = if rows < 0 { -1 } else { 1 };
    let mut at = from as isize;
    for _ in 0..rows.unsigned_abs() {
        let mut next = at + step;
        while next >= 0 && (next as usize) < hidden.len() && hidden[next as usize] {
            next += step;
        }
        if next < 0 || next as usize >= hidden.len() {
            break;
        }
        at = next;
    }
    at as usize
}

pub(crate) fn wire(window: &MainWindow, state: &AppState) -> (PaneFn, PaneTextFn) {
    let AppState {
        panes,
        cur_engine,
        settings,
        completion_nodes,
        ..
    } = state.clone();

    // ----- SQL editor: Rust-owned buffer + lexer feeding the span model -----
    // Statement head lines the user has folded closed live per-pane in
    // `panes[*].folded_heads`; sync_editor + the fold toggle read them by pane.
    let sync_editor: Rc<dyn Fn(usize)> = Rc::new({
        let weak = window.as_weak();
        let panes = panes.clone();
        let cur_engine = cur_engine.clone();
        // Persistent per-pane line models, mutated in place each sync so the
        // per-line TouchAreas survive (a fresh model would drop the one holding
        // an in-flight drag). See `sync_vec_model`.
        let line_models: [Rc<VecModel<ModelRc<Span>>>; 2] =
            [Rc::new(VecModel::default()), Rc::new(VecModel::default())];
        move |pane: usize| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let ed_state = panes[pane].ed_state.clone();
            let folded_heads = panes[pane].folded_heads.clone();
            let ed = ed_state.borrow();
            let sel = ed.selection();
            let error_mark = *panes[pane].error_mark.lock().unwrap();
            let error_line = error_mark.map(|m| m.line as usize);
            let language = editor_language(&w, pane, &cur_engine);
            let lines: Vec<ModelRc<Span>> = ed
                .lines
                .iter()
                .enumerate()
                .map(|(li, l)| {
                    let mut spans = editor::lex_line(language, l);
                    // selection highlight: char-col range covered on this line
                    if let Some(((sl, sc), (el, ec))) = sel {
                        if li >= sl && li <= el {
                            let a = if li == sl { sc } else { 0 };
                            let b = if li == el { ec } else { l.chars().count() };
                            spans = editor::overlay_selection(spans, a, b);
                        }
                    }
                    ModelRc::from(Rc::new(VecModel::from(ui_spans(
                        spans,
                        error_line == Some(li),
                    ))))
                })
                .collect();
            // Update the persistent model in place; same instance → the `for`
            // keeps its row items and the TouchArea grabbing a drag stays alive.
            sync_vec_model(&line_models[pane], lines);
            let lines = ModelRc::from(line_models[pane].clone());
            // Fold arrows: 1 = open head, 2 = closed head, 0 = plain line.
            // `hidden` blanks out the body lines of a closed region; nested
            // closed regions just union their ranges.
            let n = ed.lines.len();
            let mut fold_state = vec![0i32; n];
            let folded = folded_heads.borrow();
            for (h, _) in editor::fold_regions(&ed.lines) {
                fold_state[h] = if folded.contains(&h) { 2 } else { 1 };
            }
            let hidden = hidden_lines(&ed.lines, &folded);
            let cursor_visual_row = hidden
                .iter()
                .take(ed.line)
                .filter(|hidden| !**hidden)
                .count() as i32;
            let hidden = ModelRc::from(Rc::new(VecModel::from(hidden)));
            let fold_state = ModelRc::from(Rc::new(VecModel::from(fold_state)));
            if pane == 0 {
                w.set_editor_lines(lines);
                w.set_editor_line_hidden(hidden);
                w.set_editor_fold_state(fold_state);
                w.set_cursor_line(ed.line as i32);
                w.set_cursor_visual_row(cursor_visual_row);
                w.set_cursor_col(ed.col as i32);
                set_p_error_mark(&w, 0, error_mark);
                // query-text mirrors the focused editor for tab persistence; the
                // right pane's text lives in panes[1].ed_state (persisted via
                // p1-query in a later step).
                w.set_query_text(SharedString::from(ed.text()));
            } else {
                w.set_p1_editor_lines(lines);
                w.set_p1_editor_line_hidden(hidden);
                w.set_p1_editor_fold_state(fold_state);
                w.set_p1_cursor_line(ed.line as i32);
                w.set_p1_cursor_visual_row(cursor_visual_row);
                w.set_p1_cursor_col(ed.col as i32);
                set_p_error_mark(&w, 1, error_mark);
            }
            // Keep the caret in view on every edit/cursor move, not just a
            // find jump — typing off the bottom of the viewport otherwise
            // left the new text scrolled out of sight.
            bump_p_scroll_request(&w, pane);
        }
    });
    #[allow(clippy::type_complexity)]
    let load_editor_text: Rc<dyn Fn(usize, &str)> = {
        let panes = panes.clone();
        let sync_editor = sync_editor.clone();
        Rc::new(move |pane: usize, text: &str| {
            let mut ed = editor::EditorState::from_text(text);
            // Start at the top so a restored tab shows its query from the first
            // line with the caret in view — matching a fresh tab. Leaving the
            // caret on the last line (where `from_text` puts it) kept it, and the
            // autocomplete popup, scrolled out of sight for saved multi-line queries.
            ed.move_to(0, 0, false);
            *panes[pane].ed_state.borrow_mut() = ed;
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
                if let Some((_, e)) = editor::fold_regions(&ed.lines)
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
        let cur_engine = cur_engine.clone();
        Rc::new(move |pane| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let (before, stmt) = {
                let ed = panes[pane].ed_state.borrow();
                (ed.before_cursor_doc(), ed.current_statement())
            };
            let schema = w.get_schema_name().to_string();
            let language = editor_language(&w, pane, &cur_engine);
            let (word_len, cands) = completion::suggest(
                &before,
                &stmt,
                &completion_nodes.lock().unwrap(),
                &schema,
                language,
            );
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
                    color: theme::accent_or_default(""),
                    has_custom_color: false,
                    env_tag_label: SharedString::default(),
                    env_tag_color: theme::accent_or_default(""),
                    group: SharedString::default(),
                    expanded: false,
                    is_group_end: false,
                    depth: 0,
                })
                .collect();
            *panes[pane].completion_ctx.borrow_mut() = (
                word_len,
                cands
                    .iter()
                    .map(|c| (c.label.clone(), c.kind.clone()))
                    .collect(),
            );
            set_p_completion_items(&w, pane, ModelRc::from(Rc::new(VecModel::from(items))));
            set_p_completion_selected(&w, pane, 0);
            set_p_completion_visible(&w, pane, true);
        })
    };
    let accept_completion: Rc<dyn Fn(usize, i32)> = {
        let weak = window.as_weak();
        let panes = panes.clone();
        let sync_editor = sync_editor.clone();
        let settings = settings.clone();
        let cur_engine = cur_engine.clone();
        Rc::new(move |pane, idx| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            let (word_len, entries) = panes[pane].completion_ctx.borrow().clone();
            let Some((label, kind)) = entries.get(idx.max(0) as usize).cloned() else {
                return;
            };
            {
                let mut ed = panes[pane].ed_state.borrow_mut();
                for _ in 0..word_len {
                    ed.backspace();
                }
                ed.insert(&label);
                // Auto-append alias when accepting a table name in FROM/JOIN position.
                if kind == "table" && settings.borrow().get().editor.auto_table_alias {
                    let language = editor_language(&w, pane, &cur_engine);
                    let before = ed.before_cursor_doc();
                    let cur_line = before.rsplit('\n').next().unwrap_or(&before);
                    if completion::is_table_position(cur_line, language) {
                        let alias = completion::generate_alias(&label);
                        if !alias.is_empty() {
                            ed.insert(&format!(" {alias}"));
                        }
                    }
                }
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
                                    ed.move_cursor(-1, 0);
                                    let vis = fold_skip_line(
                                        &ed.lines,
                                        &panes[pane].folded_heads.borrow(),
                                        ed.line,
                                        false,
                                    );
                                    ed.line = vis;
                                    ed.col = ed.col.min(ed.lines[vis].chars().count());
                                }
                            }
                            "\u{f701}" => {
                                if meta {
                                    ed.move_doc_end()
                                } else {
                                    ed.move_cursor(1, 0);
                                    let vis = fold_skip_line(
                                        &ed.lines,
                                        &panes[pane].folded_heads.borrow(),
                                        ed.line,
                                        true,
                                    );
                                    ed.line = vis;
                                    ed.col = ed.col.min(ed.lines[vis].chars().count());
                                }
                            }
                            "\u{f729}" => ed.home(),
                            "\u{f72b}" => ed.end(),
                            _ => {}
                        }
                    }
                    sync_editor(pane);
                    // The caret left the word the popup was built for, so its
                    // stored replace-length no longer describes what accepting
                    // would overwrite.
                    if let Some(w) = weak.upgrade() {
                        hide_completion(&w, &panes[pane], pane);
                    }
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
                    if text.as_str() == "\\" {
                        if let Some(w) = weak.upgrade() {
                            w.invoke_run_new_tab();
                        }
                        return true;
                    }
                    // Editor-owned cmd combos; everything else bubbles up to the
                    // window shortcut scope (⌘S commit, ⌘R refresh, …).
                    let handled = {
                        let mut ed = ed_state.borrow_mut();
                        let (old_line, old_len) = (ed.line, ed.lines.len());
                        let handled = match text.as_str() {
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
                            // ⌘⌫ / ⌘⌦ delete to the start / end of the line.
                            // Without these the key falls through to the window
                            // scope, where Backspace toggles the delete mark on
                            // the selected results row — so this was not just
                            // missing, it was actively dangerous.
                            "\u{8}" => {
                                ed.delete_to_line_start();
                                true
                            }
                            "\u{7f}" => {
                                ed.delete_to_line_end();
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
                        };
                        // Only cut/paste splice lines in a way `fold_regions`
                        // will re-derive predictably; undo/redo restore an
                        // arbitrary prior buffer and are left alone.
                        if matches!(text.as_str(), "x" | "v" | "\u{8}" | "\u{7f}") {
                            shift_folded_heads(
                                &panes[pane].folded_heads,
                                old_line,
                                old_len,
                                ed.line,
                                ed.lines.len(),
                            );
                        }
                        handled
                    };
                    if handled {
                        sync_editor(pane);
                        // ⌘⌫/⌘⌦ change the text around the caret, so the
                        // completion context moved with it. The other cmd
                        // combos here don't edit, or replace the buffer whole.
                        if matches!(text.as_str(), "\u{8}" | "\u{7f}") {
                            refresh_completion(pane);
                        } else if matches!(text.as_str(), "x" | "v" | "z") {
                            // Cut/paste/undo rewrite the text under the caret
                            // without rebuilding the popup, so anything still
                            // showing describes a word that is no longer there.
                            if let Some(w) = weak.upgrade() {
                                hide_completion(&w, &panes[pane], pane);
                            }
                        }
                    }
                    return handled;
                }
                let handled = {
                    let mut ed = ed_state.borrow_mut();
                    let (old_line, old_len) = (ed.line, ed.lines.len());
                    // movement keys: shift extends the selection, plain drops it
                    if matches!(
                        text.as_str(),
                        "\u{f700}" | "\u{f701}" | "\u{f702}" | "\u{f703}" | "\u{f729}" | "\u{f72b}"
                    ) {
                        ed.set_selecting(shift);
                    }
                    // Taken once per keystroke: anything that is not another
                    // auto-close disarms the pair unwrap below.
                    let armed_pair = ed.take_pair();
                    let mut it = text.chars();
                    let handled = match (it.next(), it.next()) {
                        (Some(c), None) => match c {
                            // ⌥⌫ deletes the previous word. Checked before the
                            // pair handling below: with alt held the user is
                            // deleting a word, not unwrapping an empty pair.
                            '\u{8}' if alt => {
                                ed.delete_word_left();
                                true
                            }
                            '\u{8}' => {
                                // Unwrap an empty pair in one keystroke, but
                                // only the one auto-closed below and only
                                // while the cursor still sits inside it.
                                // Matching on the surrounding characters alone
                                // ate both quotes of an `''` the user typed
                                // themselves.
                                ed.backspace_pair(armed_pair);
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
                            // Skip over an already-typed closer/quote instead
                            // of inserting a duplicate.
                            c @ (')' | ']' | '}' | '"' | '\'')
                                if ed.selection().is_none()
                                    && ed.lines[ed.line].chars().nth(ed.col) == Some(c) =>
                            {
                                ed.move_cursor(0, 1);
                                true
                            }
                            c @ ('(' | '[' | '{' | '"' | '\'') => {
                                let closer = match c {
                                    '(' => ')',
                                    '[' => ']',
                                    '{' => '}',
                                    other => other, // quotes close themselves
                                };
                                ed.insert_pair(c, closer);
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
                    };
                    shift_folded_heads(
                        &panes[pane].folded_heads,
                        old_line,
                        old_len,
                        ed.line,
                        ed.lines.len(),
                    );
                    handled
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
        let press: Rc<dyn Fn(usize, i32, i32, bool)> = Rc::new(move |pane, line, col, shift| {
            let Some(w) = weak.upgrade() else {
                return;
            };
            // Clicking a pane focuses it: drives the accent + which pane
            // global shortcuts fall back to.
            w.set_active_pane(pane as i32);
            set_p_completion_visible(&w, pane, false);
            // A plain click only moves the caret. sync_editor rebuilds the whole
            // lines model, which destroys every row's TouchArea — and that swallows
            // the second tap of a double-click, so word-select never fired. When
            // there is no selection to clear, the line spans are unchanged, so just
            // nudge the caret and leave the rows (and their TouchAreas) intact.
            // Shift+click extends from the caret (or the live anchor) to the
            // clicked spot instead, so click → shift-click selects end to end.
            let had_sel = panes[pane].ed_state.borrow().selection().is_some();
            panes[pane].ed_state.borrow_mut().move_to(line, col, shift);
            if had_sel || shift {
                sync_editor(pane);
            } else {
                let ed = panes[pane].ed_state.borrow();
                if pane == 0 {
                    w.set_cursor_line(ed.line as i32);
                    w.set_cursor_col(ed.col as i32);
                } else {
                    w.set_p1_cursor_line(ed.line as i32);
                    w.set_p1_cursor_col(ed.col as i32);
                }
            }
        });
        window.on_editor_press({
            let press = press.clone();
            move |line, col, shift| press(0, line, col, shift)
        });
        window.on_p1_editor_press({
            let press = press.clone();
            move |line, col, shift| press(1, line, col, shift)
        });
    }
    {
        let panes = panes.clone();
        let sync_editor = sync_editor.clone();
        // `rows` is how many rows *on screen* the pointer has travelled from
        // the line it was pressed on, so folded lines have to be skipped to
        // turn it back into a buffer line.
        let drag: Rc<dyn Fn(usize, i32, i32, i32)> = Rc::new(move |pane, line, rows, col| {
            let ed_state = panes[pane].ed_state.clone();
            let line = {
                let ed = ed_state.borrow();
                let hidden = hidden_lines(&ed.lines, &panes[pane].folded_heads.borrow());
                let from = (line.max(0) as usize).min(hidden.len().saturating_sub(1));
                line_after_rows(&hidden, from, rows) as i32
            };
            ed_state.borrow_mut().move_to(line, col, true);
            sync_editor(pane);
        });
        window.on_editor_drag({
            let drag = drag.clone();
            move |line, rows, col| drag(0, line, rows, col)
        });
        window.on_p1_editor_drag({
            let drag = drag.clone();
            move |line, rows, col| drag(1, line, rows, col)
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

    (sync_editor, load_editor_text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_rows_skip_the_lines_a_closed_fold_collapsed() {
        // select / a, / b are one region: folding line 0 hides lines 1 and 2,
        // so the rows on screen read select, from, t.
        let lines: Vec<String> = "select\n  a,\n  b\nfrom\n  t"
            .split('\n')
            .map(str::to_string)
            .collect();
        let folded: HashSet<usize> = [0].into_iter().collect();
        let hidden = hidden_lines(&lines, &folded);
        assert_eq!(hidden, vec![false, true, true, false, false]);

        // One row down from `select` is `from` (line 3), not `  a,` (line 1).
        assert_eq!(line_after_rows(&hidden, 0, 1), 3);
        assert_eq!(line_after_rows(&hidden, 0, 2), 4);
        // Past the end clamps to the last visible line.
        assert_eq!(line_after_rows(&hidden, 0, 9), 4);
        // Upwards, and standing still.
        assert_eq!(line_after_rows(&hidden, 4, -2), 0);
        assert_eq!(line_after_rows(&hidden, 3, 0), 3);

        // Nothing folded: rows and buffer lines agree again.
        let hidden = hidden_lines(&lines, &HashSet::new());
        assert_eq!(line_after_rows(&hidden, 0, 2), 2);
    }

    #[test]
    fn tabs_are_painted_as_spaces_without_moving_columns() {
        let spans = editor::lex_line(rdb_connstore::QueryLanguage::Sql, "select\ta");
        let ui = ui_spans(spans, false);
        let joined: String = ui.iter().map(|s| s.text.as_str()).collect();
        assert_eq!(joined, "select a");
        // Each span still starts where it did: a tab is one cell, not eight.
        assert_eq!(ui.last().map(|s| s.col + s.cols), Some(8));
    }
}
