//! The SQL editor itself: the Rust-owned text buffer, the lexer feeding the
//! span model, statement folding, and autocomplete.
//!
//! Unlike the other wiring modules this one returns two handles. `sync_editor`
//! (repaint a pane from the buffer) and `load_editor_text` (replace a pane's
//! text) are built here but needed by nearly every other module, so `main`
//! takes them back and passes them on through `AppFns`.
//!
//! Split out of `main`; the handler bodies are unchanged.

use std::rc::Rc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};

use crate::*;

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
            let language = cur_engine
                .borrow()
                .map(rdb_connstore::Engine::language)
                .unwrap_or(rdb_connstore::QueryLanguage::Sql);
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
                    let error_line = error_line == Some(li);
                    let spans: Vec<Span> = spans
                        .into_iter()
                        .map(|mut sp| {
                            if error_line && sp.kind == 0 {
                                sp.kind = 6;
                            }
                            Span {
                                cols: sp.text.chars().count() as i32,
                                text: sp.text.into(),
                                kind: sp.kind,
                                sel: sp.sel,
                            }
                        })
                        .collect();
                    ModelRc::from(Rc::new(VecModel::from(spans)))
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
            let mut hidden = vec![false; n];
            let mut fold_state = vec![0i32; n];
            let folded = folded_heads.borrow();
            for (h, e) in editor::fold_regions(&ed.lines) {
                let closed = folded.contains(&h);
                fold_state[h] = if closed { 2 } else { 1 };
                if closed {
                    for hl in hidden.iter_mut().take(e + 1).skip(h + 1) {
                        *hl = true;
                    }
                }
            }
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
            let language = cur_engine
                .borrow()
                .map(rdb_connstore::Engine::language)
                .unwrap_or(rdb_connstore::QueryLanguage::Sql);
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
                    let language = cur_engine
                        .borrow()
                        .map(rdb_connstore::Engine::language)
                        .unwrap_or(rdb_connstore::QueryLanguage::Sql);
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
                                // Delete both sides of an empty pair in one
                                // keystroke, mirroring the auto-close below.
                                let before = ed
                                    .col
                                    .checked_sub(1)
                                    .and_then(|c| ed.lines[ed.line].chars().nth(c));
                                let after = ed.lines[ed.line].chars().nth(ed.col);
                                let is_empty_pair = ed.selection().is_none()
                                    && matches!(
                                        (before, after),
                                        (Some('('), Some(')'))
                                            | (Some('['), Some(']'))
                                            | (Some('{'), Some('}'))
                                            | (Some('"'), Some('"'))
                                            | (Some('\''), Some('\''))
                                    );
                                ed.backspace();
                                if is_empty_pair {
                                    ed.delete();
                                }
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
                                ed.insert(&format!("{c}{closer}"));
                                ed.move_cursor(0, -1);
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

    (sync_editor, load_editor_text)
}
