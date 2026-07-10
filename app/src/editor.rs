//! SQL editor state: a line-based text buffer with cursor, plus a small
//! per-line lexer that feeds the highlighted span model. Lines are lexed
//! independently (line comments and single-line strings only), which keeps
//! re-lexing incremental: only the edited line changes shape.

/// Span kinds, mirrored in code-editor.slint: 0 plain · 1 keyword ·
/// 2 string · 3 function · 4 comment · 5 number. `sel` marks the span as
/// inside the selection (background highlight).
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub text: String,
    pub kind: i32,
    pub sel: bool,
}

const KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "GROUP",
    "BY",
    "ORDER",
    "LIMIT",
    "OFFSET",
    "JOIN",
    "LEFT",
    "RIGHT",
    "INNER",
    "OUTER",
    "ON",
    "AS",
    "AND",
    "OR",
    "NOT",
    "IN",
    "IS",
    "NULL",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "CREATE",
    "TABLE",
    "FUNCTION",
    "REPLACE",
    "RETURNS",
    "LANGUAGE",
    "BEGIN",
    "END",
    "RETURN",
    "DESC",
    "ASC",
    "HAVING",
    "UNION",
    "ALL",
    "DISTINCT",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "LIKE",
    "ILIKE",
    "BETWEEN",
    "EXISTS",
    "WITH",
    "IMMUTABLE",
    "PARALLEL",
    "SAFE",
    "STRICT",
    "OR",
];

/// Redis / Mongo dialects reuse the SQL lexer with extra command words.
const COMMAND_WORDS: &[&str] = &[
    "GET",
    "MGET",
    "HGETALL",
    "LRANGE",
    "ZRANGE",
    "SMEMBERS",
    "SCAN",
    "TYPE",
    "TTL",
    "KEYS",
    "FIND",
    "AGGREGATE",
];

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// Lex one line into colored spans. Whitespace stays attached to plain spans
/// so concatenating span texts reproduces the line exactly.
/// True when `word` (already uppercased) is a SQL keyword.
pub fn is_keyword(word: &str) -> bool {
    KEYWORDS.contains(&word)
}

pub fn lex_line(line: &str) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    let push = |spans: &mut Vec<Span>, text: &str, kind: i32| {
        if text.is_empty() {
            return;
        }
        // merge adjacent same-kind spans to keep the model small
        if let Some(last) = spans.last_mut() {
            if last.kind == kind {
                last.text.push_str(text);
                return;
            }
        }
        spans.push(Span {
            text: text.to_string(),
            kind,
            sel: false,
        });
    };

    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        // line comment
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            let rest: String = chars[i..].iter().collect();
            push(&mut spans, &rest, 4);
            break;
        }
        // string literal
        if c == '\'' {
            let mut j = i + 1;
            while j < chars.len() && chars[j] != '\'' {
                j += 1;
            }
            let end = (j + 1).min(chars.len());
            let text: String = chars[i..end].iter().collect();
            push(&mut spans, &text, 2);
            i = end;
            continue;
        }
        // number
        if c.is_ascii_digit() {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '.') {
                j += 1;
            }
            let text: String = chars[i..j].iter().collect();
            push(&mut spans, &text, 5);
            i = j;
            continue;
        }
        // identifier / keyword / function call
        if is_ident(c) && !c.is_ascii_digit() {
            let mut j = i;
            while j < chars.len() && is_ident(chars[j]) {
                j += 1;
            }
            let word: String = chars[i..j].iter().collect();
            let upper = word.to_uppercase();
            let kind = if chars.get(j) == Some(&'(') && !KEYWORDS.contains(&upper.as_str()) {
                3
            } else if KEYWORDS.contains(&upper.as_str()) || COMMAND_WORDS.contains(&upper.as_str())
            {
                1
            } else {
                0
            };
            push(&mut spans, &word, kind);
            i = j;
            continue;
        }
        // everything else: punctuation / whitespace
        push(&mut spans, &c.to_string(), 0);
        i += 1;
    }
    if spans.is_empty() {
        spans.push(Span {
            text: String::new(),
            kind: 0,
            sel: false,
        });
    }
    spans
}

/// Split `spans` at the char columns `a..b` and mark the covered slice as
/// selected. `a >= b` returns the spans untouched.
pub fn overlay_selection(spans: Vec<Span>, a: usize, b: usize) -> Vec<Span> {
    if a >= b {
        return spans;
    }
    let mut out: Vec<Span> = Vec::with_capacity(spans.len() + 2);
    let mut col = 0usize;
    for sp in spans {
        let len = sp.text.chars().count();
        let (s, e) = (col, col + len);
        col = e;
        // split points inside this span, clamped to its bounds
        let cut_a = a.clamp(s, e) - s;
        let cut_b = b.clamp(s, e) - s;
        if cut_a >= cut_b {
            out.push(sp);
            continue;
        }
        let chars: Vec<char> = sp.text.chars().collect();
        let piece = |r: std::ops::Range<usize>, sel: bool| Span {
            text: chars[r].iter().collect(),
            kind: sp.kind,
            sel,
        };
        if cut_a > 0 {
            out.push(piece(0..cut_a, false));
        }
        out.push(piece(cut_a..cut_b, true));
        if cut_b < len {
            out.push(piece(cut_b..len, false));
        }
    }
    out.retain(|s| !s.text.is_empty());
    if out.is_empty() {
        out.push(Span {
            text: String::new(),
            kind: 0,
            sel: false,
        });
    }
    out
}

/// One undo step: the full buffer + cursor. Snapshots are cheap at editor
/// scale (a query is a few KB) and make undo/redo trivially correct.
#[derive(Clone)]
struct Snap {
    lines: Vec<String>,
    line: usize,
    col: usize,
}

/// Editor buffer + cursor. The text is the single source of truth; the UI
/// mirrors it through the span model rebuilt per edited line.
#[derive(Default)]
pub struct EditorState {
    pub lines: Vec<String>,
    pub line: usize, // cursor line
    pub col: usize,  // cursor column (chars)
    /// Selection anchor; the cursor is the moving end. None = no selection.
    sel: Option<(usize, usize)>,
    undo_stack: Vec<Snap>,
    redo_stack: Vec<Snap>,
    /// Last undo push came from plain typing — coalesce the next one.
    typing: bool,
}

const UNDO_CAP: usize = 200;

impl EditorState {
    pub fn from_text(text: &str) -> Self {
        let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        let line = lines.len() - 1;
        let col = lines[line].chars().count();
        EditorState {
            lines,
            line,
            col,
            ..Default::default()
        }
    }

    pub fn text(&self) -> String {
        self.lines.join("\n")
    }

    fn clamp(&mut self) {
        self.line = self.line.min(self.lines.len().saturating_sub(1));
        self.col = self.col.min(self.lines[self.line].chars().count());
    }

    fn byte_col(&self) -> usize {
        let l = &self.lines[self.line];
        l.char_indices()
            .nth(self.col)
            .map(|(b, _)| b)
            .unwrap_or(l.len())
    }

    /// Text on the current line up to the cursor — the autocomplete context.
    pub fn before_cursor(&self) -> &str {
        &self.lines[self.line][..self.byte_col()]
    }

    fn byte_at(l: &str, col: usize) -> usize {
        l.char_indices().nth(col).map(|(b, _)| b).unwrap_or(l.len())
    }

    // ----- undo / redo -----

    fn snap(&self) -> Snap {
        Snap {
            lines: self.lines.clone(),
            line: self.line,
            col: self.col,
        }
    }

    fn push_undo(&mut self, typing: bool) {
        if !(typing && self.typing) {
            self.undo_stack.push(self.snap());
            if self.undo_stack.len() > UNDO_CAP {
                self.undo_stack.remove(0);
            }
        }
        self.typing = typing;
        self.redo_stack.clear();
    }

    fn restore(&mut self, s: Snap) {
        self.lines = s.lines;
        self.line = s.line;
        self.col = s.col;
        self.sel = None;
        self.typing = false;
    }

    pub fn undo(&mut self) {
        if let Some(s) = self.undo_stack.pop() {
            self.redo_stack.push(self.snap());
            self.restore(s);
        }
    }

    pub fn redo(&mut self) {
        if let Some(s) = self.redo_stack.pop() {
            self.undo_stack.push(self.snap());
            self.restore(s);
        }
    }

    // ----- selection -----

    /// Called before every cursor move: shift extends (anchoring on first
    /// use), a plain move drops the selection.
    pub fn set_selecting(&mut self, shift: bool) {
        if shift {
            if self.sel.is_none() {
                self.sel = Some((self.line, self.col));
            }
        } else {
            self.sel = None;
        }
    }

    /// Normalized selection range (start ≤ end), None when empty.
    pub fn selection(&self) -> Option<((usize, usize), (usize, usize))> {
        let a = self.sel?;
        let b = (self.line, self.col);
        match a.cmp(&b) {
            std::cmp::Ordering::Less => Some((a, b)),
            std::cmp::Ordering::Greater => Some((b, a)),
            std::cmp::Ordering::Equal => None,
        }
    }

    pub fn select_all(&mut self) {
        self.sel = Some((0, 0));
        self.line = self.lines.len() - 1;
        self.col = self.lines[self.line].chars().count();
    }

    /// Set an explicit anchor→cursor selection (used by find-in-editor to
    /// highlight a match). Both ends are clamped to the buffer.
    pub fn set_selection(&mut self, anchor: (usize, usize), cursor: (usize, usize)) {
        let clamp = |l: usize, c: usize, lines: &[String]| {
            let l = l.min(lines.len() - 1);
            (l, c.min(lines[l].chars().count()))
        };
        let a = clamp(anchor.0, anchor.1, &self.lines);
        let cur = clamp(cursor.0, cursor.1, &self.lines);
        self.sel = Some(a);
        self.line = cur.0;
        self.col = cur.1;
    }

    pub fn selected_text(&self) -> Option<String> {
        let ((sl, sc), (el, ec)) = self.selection()?;
        if sl == el {
            let l = &self.lines[sl];
            let (a, b) = (Self::byte_at(l, sc), Self::byte_at(l, ec));
            return Some(l[a..b].to_string());
        }
        let mut out = self.lines[sl][Self::byte_at(&self.lines[sl], sc)..].to_string();
        for l in &self.lines[sl + 1..el] {
            out.push('\n');
            out.push_str(l);
        }
        out.push('\n');
        out.push_str(&self.lines[el][..Self::byte_at(&self.lines[el], ec)]);
        Some(out)
    }

    /// Remove the selected range; cursor lands at its start. False when
    /// there was nothing selected.
    fn remove_selection(&mut self) -> bool {
        let Some(((sl, sc), (el, ec))) = self.selection() else {
            self.sel = None;
            return false;
        };
        let tail = self.lines[el][Self::byte_at(&self.lines[el], ec)..].to_string();
        let keep = Self::byte_at(&self.lines[sl], sc);
        self.lines[sl].truncate(keep);
        self.lines[sl].push_str(&tail);
        self.lines.drain(sl + 1..=el);
        self.line = sl;
        self.col = sc;
        self.sel = None;
        true
    }

    // ----- mutations (all undo-recorded, all selection-aware) -----

    pub fn insert(&mut self, s: &str) {
        let one_char = s.chars().count() == 1 && !s.contains('\n');
        self.push_undo(one_char && self.sel.is_none());
        self.remove_selection();
        self.insert_raw(s);
    }

    fn insert_raw(&mut self, s: &str) {
        if s.contains('\n') {
            for (i, part) in s.split('\n').enumerate() {
                if i > 0 {
                    self.newline_raw();
                }
                self.insert_raw(part);
            }
            return;
        }
        let b = self.byte_col();
        self.lines[self.line].insert_str(b, s);
        self.col += s.chars().count();
    }

    pub fn newline(&mut self) {
        self.push_undo(false);
        self.remove_selection();
        self.newline_raw();
    }

    fn newline_raw(&mut self) {
        let b = self.byte_col();
        let rest = self.lines[self.line].split_off(b);
        self.lines.insert(self.line + 1, rest);
        self.line += 1;
        self.col = 0;
    }

    pub fn backspace(&mut self) {
        self.push_undo(false);
        if self.remove_selection() {
            return;
        }
        if self.col > 0 {
            self.col -= 1;
            let b = self.byte_col();
            self.lines[self.line].remove(b);
        } else if self.line > 0 {
            let cur = self.lines.remove(self.line);
            self.line -= 1;
            self.col = self.lines[self.line].chars().count();
            self.lines[self.line].push_str(&cur);
        }
    }

    pub fn delete(&mut self) {
        self.push_undo(false);
        if self.remove_selection() {
            return;
        }
        let len = self.lines[self.line].chars().count();
        if self.col < len {
            let b = self.byte_col();
            self.lines[self.line].remove(b);
        } else if self.line + 1 < self.lines.len() {
            let next = self.lines.remove(self.line + 1);
            self.lines[self.line].push_str(&next);
        }
    }

    /// Cut helper: caller copies `selected_text()` first.
    pub fn cut_selection(&mut self) {
        if self.selection().is_some() {
            self.push_undo(false);
            self.remove_selection();
        }
    }

    /// Toggle a line comment across the affected lines (the selection's line
    /// span, or the current line when nothing is selected). If every non-blank
    /// line already starts with `prefix` after its indent, uncomment them all;
    /// otherwise comment them all with `prefix + " "` at the first non-space
    /// column. Blank lines are left untouched. One undo entry.
    pub fn toggle_comment(&mut self, prefix: &str) {
        let (start, end, had_sel) = match self.selection() {
            Some(((sl, _), (el, _))) => (sl, el, true),
            None => (self.line, self.line, false),
        };
        self.push_undo(false);
        // Leading whitespace is ASCII, so byte offset == char offset here.
        let all_commented = (start..=end).all(|i| {
            let t = self.lines[i].trim_start();
            t.is_empty() || t.starts_with(prefix)
        });
        for i in start..=end {
            let indent = self.lines[i].len() - self.lines[i].trim_start().len();
            let body = self.lines[i][indent..].to_string();
            if body.is_empty() {
                continue;
            }
            let head = &self.lines[i][..indent];
            self.lines[i] = if all_commented {
                let rest = body.strip_prefix(prefix).unwrap_or(&body);
                let rest = rest.strip_prefix(' ').unwrap_or(rest);
                format!("{head}{rest}")
            } else {
                format!("{head}{prefix} {body}")
            };
        }
        // Keep the affected lines selected so repeated Cmd+/ toggles them back.
        if had_sel {
            self.sel = Some((start, 0));
            self.line = end;
            self.col = self.lines[end].chars().count();
        } else {
            self.sel = None;
        }
        self.clamp();
    }

    /// Place the cursor at (line, col), clamped to the buffer. `extend`
    /// keeps/creates the selection anchor (drag or shift-click); otherwise the
    /// selection is dropped (a plain click).
    pub fn move_to(&mut self, line: i32, col: i32, extend: bool) {
        self.set_selecting(extend);
        self.line = (line.max(0) as usize).min(self.lines.len() - 1);
        self.col = (col.max(0) as usize).min(self.lines[self.line].chars().count());
    }

    /// Select the identifier/word under (line, col) — double-click behaviour.
    /// Falls back to placing the cursor when the spot isn't on a word char.
    pub fn select_word_at(&mut self, line: i32, col: i32) {
        self.move_to(line, col, false);
        let chars: Vec<char> = self.lines[self.line].chars().collect();
        if chars.is_empty() {
            return;
        }
        let at = self.col.min(chars.len().saturating_sub(1));
        if !is_ident(chars[at]) {
            return;
        }
        let mut s = at;
        while s > 0 && is_ident(chars[s - 1]) {
            s -= 1;
        }
        let mut e = at + 1;
        while e < chars.len() && is_ident(chars[e]) {
            e += 1;
        }
        self.sel = Some((self.line, s));
        self.col = e;
    }

    pub fn move_cursor(&mut self, dl: i32, dc: i32) {
        if dc < 0 && self.col == 0 && self.line > 0 {
            self.line -= 1;
            self.col = self.lines[self.line].chars().count();
            return;
        }
        let len = self.lines[self.line].chars().count();
        if dc > 0 && self.col >= len && self.line + 1 < self.lines.len() {
            self.line += 1;
            self.col = 0;
            return;
        }
        self.line = (self.line as i64 + dl as i64).clamp(0, self.lines.len() as i64 - 1) as usize;
        self.col = (self.col as i64 + dc as i64).max(0) as usize;
        self.clamp();
    }

    pub fn home(&mut self) {
        self.col = 0;
    }

    pub fn end(&mut self) {
        self.col = self.lines[self.line].chars().count();
    }

    /// Word-wise horizontal motion (macOS Option+Arrow). `dir < 0` jumps to the
    /// start of the word on the left, `dir > 0` to the end of the word on the
    /// right, crossing a line boundary when already at the edge. Selection is
    /// anchored/dropped by the caller via `set_selecting`.
    pub fn move_word(&mut self, dir: i32) {
        let chars: Vec<char> = self.lines[self.line].chars().collect();
        if dir < 0 {
            if self.col == 0 {
                if self.line > 0 {
                    self.line -= 1;
                    self.col = self.lines[self.line].chars().count();
                }
                return;
            }
            let mut i = self.col;
            while i > 0 && !is_ident(chars[i - 1]) {
                i -= 1;
            }
            while i > 0 && is_ident(chars[i - 1]) {
                i -= 1;
            }
            self.col = i;
        } else {
            let n = chars.len();
            if self.col >= n {
                if self.line + 1 < self.lines.len() {
                    self.line += 1;
                    self.col = 0;
                }
                return;
            }
            let mut i = self.col;
            while i < n && !is_ident(chars[i]) {
                i += 1;
            }
            while i < n && is_ident(chars[i]) {
                i += 1;
            }
            self.col = i;
        }
    }

    /// Cursor to the very start / end of the buffer (macOS Cmd+Up / Cmd+Down).
    pub fn move_doc_start(&mut self) {
        self.line = 0;
        self.col = 0;
    }

    pub fn move_doc_end(&mut self) {
        self.line = self.lines.len() - 1;
        self.col = self.lines[self.line].chars().count();
    }

    /// Current line text — the "Run Selection" fallback unit.
    pub fn current_line(&self) -> &str {
        &self.lines[self.line]
    }

    /// Statement under the cursor: the `;`-delimited segment containing the
    /// cursor, ignoring semicolons inside single-quoted literals. Falls back
    /// to the current line when the segment is empty.
    pub fn current_statement(&self) -> String {
        let text = self.text();
        let chars: Vec<char> = text.chars().collect();
        // cursor position as a char offset into the joined text
        let mut offset = 0;
        for (i, l) in self.lines.iter().enumerate() {
            if i == self.line {
                offset += self.col.min(l.chars().count());
                break;
            }
            offset += l.chars().count() + 1; // +1 for the newline
        }
        // split into statements on top-level semicolons
        let mut segments: Vec<(usize, usize)> = Vec::new();
        let mut seg_start = 0;
        let mut in_str = false;
        for (i, &c) in chars.iter().enumerate() {
            if c == '\'' {
                in_str = !in_str;
            }
            if c == ';' && !in_str {
                segments.push((seg_start, i + 1));
                seg_start = i + 1;
            }
        }
        if seg_start < chars.len() {
            segments.push((seg_start, chars.len()));
        }
        let (s, e) = segments
            .iter()
            .copied()
            .find(|&(s, e)| offset >= s && offset < e)
            .or_else(|| segments.last().copied())
            .unwrap_or((0, chars.len()));
        let stmt: String = chars[s..e].iter().collect();
        let stmt = stmt.trim();
        if stmt.is_empty() {
            self.current_line().trim().to_string()
        } else {
            stmt.to_string()
        }
    }
}

/// Split SQL text into individual statements on top-level semicolons,
/// ignoring semicolons inside single-quoted literals and `-- line comments`.
/// Trailing `;` and blank/comment-only segments are dropped. Text with no
/// real statement yields an empty vec; a single statement (no `;`) yields one.
pub fn split_statements(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut seg_start = 0usize;
    let mut in_str = false;
    let mut i = 0usize;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            in_str = !in_str;
        } else if !in_str && c == '-' && chars.get(i + 1) == Some(&'-') {
            // skip to end of line
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            continue;
        } else if c == ';' && !in_str {
            let seg: String = chars[seg_start..i].iter().collect();
            if !is_blank_sql(&seg) {
                out.push(seg.trim().to_string());
            }
            seg_start = i + 1;
        }
        i += 1;
    }
    if seg_start < chars.len() {
        let seg: String = chars[seg_start..].iter().collect();
        if !is_blank_sql(&seg) {
            out.push(seg.trim().to_string());
        }
    }
    out
}

/// Case-insensitive find of every occurrence of `needle` across `lines`.
/// Returns `(line, start_col, end_col)` in char columns. Empty needle → none.
/// Overlapping matches are not reported (scan advances past each hit).
pub fn find_matches(lines: &[String], needle: &str) -> Vec<(usize, usize, usize)> {
    let mut out = Vec::new();
    if needle.is_empty() {
        return out;
    }
    let nlow: Vec<char> = needle.to_lowercase().chars().collect();
    let nlen = nlow.len();
    for (li, line) in lines.iter().enumerate() {
        let hay: Vec<char> = line.to_lowercase().chars().collect();
        if hay.len() < nlen {
            continue;
        }
        let mut i = 0;
        while i + nlen <= hay.len() {
            if hay[i..i + nlen] == nlow[..] {
                out.push((li, i, i + nlen));
                i += nlen;
            } else {
                i += 1;
            }
        }
    }
    out
}

/// Inclusive `(first_line, last_line)` of each SQL statement, for the editor's
/// gutter fold arrows. Uses the same string/comment rules as
/// [`split_statements`] (a `;` inside a `'literal'` or after `--` doesn't end a
/// statement), carrying string state across lines. Blank leading lines are
/// skipped so a block starts at its first content line. A block is foldable
/// only when `last_line > first_line`.
pub fn statement_line_spans(lines: &[String]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    let mut in_str = false;
    let mut start: Option<usize> = None;
    for (li, line) in lines.iter().enumerate() {
        if start.is_none() && !line.trim().is_empty() {
            start = Some(li);
        }
        let chars: Vec<char> = line.chars().collect();
        let mut i = 0;
        let mut ends = false;
        while i < chars.len() {
            let c = chars[i];
            if c == '\'' {
                in_str = !in_str;
            } else if !in_str && c == '-' && chars.get(i + 1) == Some(&'-') {
                break; // rest of the line is a comment
            } else if c == ';' && !in_str {
                ends = true;
            }
            i += 1;
        }
        if ends {
            if let Some(s) = start.take() {
                out.push((s, li));
            }
        }
    }
    // Trailing statement without a terminating ';'.
    if let (Some(s), false) = (start, lines.is_empty()) {
        out.push((s, lines.len() - 1));
    }
    out
}

/// True when a statement segment carries no executable SQL — only whitespace
/// and `--` line comments.
fn is_blank_sql(seg: &str) -> bool {
    seg.lines()
        .map(|l| l.split("--").next().unwrap_or("").trim())
        .all(|l| l.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(line: &str) -> Vec<(String, i32)> {
        lex_line(line)
            .into_iter()
            .map(|s| (s.text, s.kind))
            .collect()
    }

    fn spans(text: &str) -> Vec<(usize, usize)> {
        let lines: Vec<String> = text.lines().map(str::to_string).collect();
        statement_line_spans(&lines)
    }

    #[test]
    fn fold_spans_group_multiline_statements() {
        // Two statements: lines 0-2 (ends with ;) and lines 3-4 (no ;).
        assert_eq!(
            spans("select *\nfrom t\nwhere x=1;\nupdate t\nset a=1"),
            vec![(0, 2), (3, 4)]
        );
        // Semicolon inside a literal does not split.
        assert_eq!(spans("select 'a;b'\nfrom t;"), vec![(0, 1)]);
        // A semicolon after `--` is a comment, not a terminator.
        assert_eq!(spans("select 1 -- a;b\nfrom t;"), vec![(0, 1)]);
    }

    #[test]
    fn keywords_strings_functions_comments() {
        let ks = kinds("SELECT count(*) FROM emiten WHERE c = 'x' -- note");
        assert!(ks.contains(&("SELECT".into(), 1)));
        assert!(ks.contains(&("count".into(), 3)));
        assert!(ks.contains(&("'x'".into(), 2)));
        assert!(ks.iter().any(|(t, k)| t.starts_with("--") && *k == 4));
    }

    #[test]
    fn spans_roundtrip_line_text() {
        let line = "JOIN sectors s ON s.id = e.id_sector";
        let joined: String = lex_line(line).into_iter().map(|s| s.text).collect();
        assert_eq!(joined, line);
    }

    #[test]
    fn numbers_are_tagged() {
        let ks = kinds("LIMIT 500");
        assert!(ks.contains(&("500".into(), 5)));
    }

    #[test]
    fn editor_insert_newline_backspace() {
        let mut ed = EditorState::from_text("SELECT 1");
        ed.end();
        ed.insert(";");
        assert_eq!(ed.text(), "SELECT 1;");
        ed.newline();
        ed.insert("x");
        assert_eq!(ed.text(), "SELECT 1;\nx");
        ed.backspace();
        ed.backspace();
        assert_eq!(ed.text(), "SELECT 1;");
    }

    #[test]
    fn toggle_comment_is_idempotent() {
        let mut ed = EditorState::from_text("  SELECT 1");
        ed.toggle_comment("--");
        assert_eq!(ed.text(), "  -- SELECT 1");
        ed.toggle_comment("--");
        assert_eq!(ed.text(), "  SELECT 1");
    }

    #[test]
    fn toggle_comment_spans_selection() {
        let mut ed = EditorState::from_text("a\nb\nc");
        ed.set_selection((0, 0), (2, 1)); // all three lines
        ed.toggle_comment("//");
        assert_eq!(ed.text(), "// a\n// b\n// c");
        ed.toggle_comment("//");
        assert_eq!(ed.text(), "a\nb\nc");
    }

    #[test]
    fn cursor_moves_clamp() {
        let mut ed = EditorState::from_text("ab\ncd");
        ed.move_cursor(-1, 0);
        assert_eq!((ed.line, ed.col), (0, 2));
        ed.move_cursor(0, 1); // wraps to next line start
        assert_eq!((ed.line, ed.col), (1, 0));
        ed.move_cursor(0, -1); // wraps back to previous line end
        assert_eq!((ed.line, ed.col), (0, 2));
    }

    #[test]
    fn statement_under_cursor() {
        let mut ed = EditorState::from_text("SELECT 1;\nSELECT 2;\nSELECT 3");
        ed.line = 1;
        ed.col = 3;
        assert_eq!(ed.current_statement(), "SELECT 2;");
        ed.line = 2;
        ed.col = 0;
        assert_eq!(ed.current_statement(), "SELECT 3");
    }

    #[test]
    fn statement_ignores_semicolon_in_literal() {
        let mut ed = EditorState::from_text("SELECT 'a;b' FROM t; SELECT 2;");
        ed.line = 0;
        ed.col = 4;
        assert_eq!(ed.current_statement(), "SELECT 'a;b' FROM t;");
    }

    #[test]
    fn statement_without_semicolons_is_whole_text() {
        let ed = EditorState::from_text("SELECT *\nFROM t");
        assert_eq!(ed.current_statement(), "SELECT *\nFROM t");
    }

    // ----- selection -----

    #[test]
    fn shift_arrows_build_selection_plain_move_clears() {
        let mut ed = EditorState::from_text("SELECT 1");
        ed.line = 0;
        ed.col = 0;
        ed.set_selecting(true);
        ed.move_cursor(0, 6);
        assert_eq!(ed.selected_text().as_deref(), Some("SELECT"));
        ed.set_selecting(false);
        ed.move_cursor(0, 1);
        assert_eq!(ed.selected_text(), None);
    }

    #[test]
    fn multiline_selection_text_and_delete() {
        let mut ed = EditorState::from_text("abc\ndef\nghi");
        ed.line = 0;
        ed.col = 1;
        ed.set_selecting(true);
        ed.line = 2;
        ed.col = 2;
        assert_eq!(ed.selected_text().as_deref(), Some("bc\ndef\ngh"));
        ed.backspace(); // deletes the selection, not one char
        assert_eq!(ed.text(), "ai");
        assert_eq!((ed.line, ed.col), (0, 1));
    }

    #[test]
    fn typing_replaces_selection() {
        let mut ed = EditorState::from_text("SELECT 111");
        ed.line = 0;
        ed.col = 7;
        ed.set_selecting(true);
        ed.col = 10;
        ed.insert("2");
        assert_eq!(ed.text(), "SELECT 2");
    }

    #[test]
    fn select_all_covers_everything() {
        let mut ed = EditorState::from_text("a\nb");
        ed.select_all();
        assert_eq!(ed.selected_text().as_deref(), Some("a\nb"));
    }

    #[test]
    fn backward_selection_normalizes() {
        let mut ed = EditorState::from_text("hello");
        ed.end();
        ed.set_selecting(true);
        ed.move_cursor(0, -3);
        assert_eq!(ed.selected_text().as_deref(), Some("llo"));
    }

    // ----- undo / redo -----

    #[test]
    fn undo_redo_roundtrip() {
        let mut ed = EditorState::from_text("SELECT 1");
        ed.end();
        ed.insert(";");
        assert_eq!(ed.text(), "SELECT 1;");
        ed.undo();
        assert_eq!(ed.text(), "SELECT 1");
        ed.redo();
        assert_eq!(ed.text(), "SELECT 1;");
    }

    #[test]
    fn typing_coalesces_into_one_undo_step() {
        let mut ed = EditorState::from_text("");
        for c in ["a", "b", "c"] {
            ed.insert(c);
        }
        ed.undo();
        assert_eq!(ed.text(), "");
    }

    #[test]
    fn undo_restores_deleted_selection() {
        let mut ed = EditorState::from_text("abc\ndef");
        ed.select_all();
        ed.delete();
        assert_eq!(ed.text(), "");
        ed.undo();
        assert_eq!(ed.text(), "abc\ndef");
    }

    #[test]
    fn cut_removes_and_undo_restores() {
        let mut ed = EditorState::from_text("keep cut");
        ed.line = 0;
        ed.col = 4;
        ed.set_selecting(true);
        ed.end();
        assert_eq!(ed.selected_text().as_deref(), Some(" cut"));
        ed.cut_selection();
        assert_eq!(ed.text(), "keep");
        ed.undo();
        assert_eq!(ed.text(), "keep cut");
    }

    #[test]
    fn paste_multiline_via_insert() {
        let mut ed = EditorState::from_text("");
        ed.insert("SELECT *\nFROM t;");
        assert_eq!(ed.text(), "SELECT *\nFROM t;");
        assert_eq!((ed.line, ed.col), (1, 7));
    }

    // ----- selection overlay rendering -----

    #[test]
    fn overlay_selection_splits_spans() {
        let spans = lex_line("SELECT 1");
        let out = overlay_selection(spans, 3, 8);
        let joined: String = out.iter().map(|s| s.text.clone()).collect();
        assert_eq!(joined, "SELECT 1");
        let sel: String = out
            .iter()
            .filter(|s| s.sel)
            .map(|s| s.text.clone())
            .collect();
        assert_eq!(sel, "ECT 1");
    }

    #[test]
    fn overlay_selection_empty_range_noop() {
        let spans = lex_line("SELECT 1");
        let out = overlay_selection(spans.clone(), 4, 4);
        assert_eq!(out, spans);
    }

    // ----- statement splitting -----

    #[test]
    fn split_single_statement_no_semicolon() {
        assert_eq!(split_statements("SELECT 1"), vec!["SELECT 1"]);
    }

    #[test]
    fn split_multiple_and_drops_trailing_and_blank() {
        let got = split_statements("SELECT 1; SELECT 2;\n  ;\nSELECT 3;");
        assert_eq!(got, vec!["SELECT 1", "SELECT 2", "SELECT 3"]);
    }

    #[test]
    fn split_ignores_semicolon_in_literal() {
        let got = split_statements("INSERT INTO t VALUES ('a;b'); SELECT 2;");
        assert_eq!(got, vec!["INSERT INTO t VALUES ('a;b')", "SELECT 2"]);
    }

    #[test]
    fn split_ignores_semicolon_in_line_comment() {
        let got = split_statements("SELECT 1 -- a; b\n; SELECT 2");
        assert_eq!(got, vec!["SELECT 1 -- a; b", "SELECT 2"]);
    }

    #[test]
    fn split_comment_only_segment_is_dropped() {
        let got = split_statements("SELECT 1;\n-- just a comment\n");
        assert_eq!(got, vec!["SELECT 1"]);
    }

    #[test]
    fn split_empty_text_is_empty() {
        assert!(split_statements("   \n  ").is_empty());
        assert!(split_statements(";;;").is_empty());
    }

    // ----- mouse hit-test -----

    #[test]
    fn move_to_clamps_and_clears_selection() {
        let mut ed = EditorState::from_text("abc\nde");
        ed.select_all();
        ed.move_to(0, 99, false); // past line end, no extend
        assert_eq!((ed.line, ed.col), (0, 3));
        assert_eq!(ed.selected_text(), None);
    }

    #[test]
    fn move_to_extend_builds_selection() {
        let mut ed = EditorState::from_text("hello world");
        ed.move_to(0, 0, false);
        ed.move_to(0, 5, true); // drag to col 5
        assert_eq!(ed.selected_text().as_deref(), Some("hello"));
    }

    #[test]
    fn select_word_grabs_identifier() {
        let mut ed = EditorState::from_text("SELECT id_sector FROM t");
        ed.select_word_at(0, 10); // inside "id_sector"
        assert_eq!(ed.selected_text().as_deref(), Some("id_sector"));
    }

    #[test]
    fn select_word_on_space_just_moves() {
        let mut ed = EditorState::from_text("a  b");
        ed.select_word_at(0, 1); // on a space
        assert_eq!(ed.selected_text(), None);
    }

    // ----- find-in-editor -----

    #[test]
    fn find_matches_case_insensitive_multi_line() {
        let lines = vec!["SELECT id FROM t".to_string(), "where id = ID".to_string()];
        let m = find_matches(&lines, "id");
        // "id" in line0 col7, line1 "id"(6) and "ID"(11)
        assert_eq!(m, vec![(0, 7, 9), (1, 6, 8), (1, 11, 13)]);
    }

    #[test]
    fn find_matches_empty_needle_none() {
        let lines = vec!["abc".to_string()];
        assert!(find_matches(&lines, "").is_empty());
    }

    #[test]
    fn find_match_becomes_selection() {
        let mut ed = EditorState::from_text("SELECT id FROM t");
        let m = find_matches(&ed.lines, "from");
        assert_eq!(m.len(), 1);
        let (l, s, e) = m[0];
        ed.set_selection((l, s), (l, e));
        assert_eq!(ed.selected_text().as_deref(), Some("FROM"));
    }

    #[test]
    fn word_motion_jumps_over_whole_words() {
        let mut ed = EditorState::from_text("SELECT id FROM t");
        ed.move_doc_start();
        assert_eq!((ed.line, ed.col), (0, 0));
        ed.move_word(1); // over "SELECT"
        assert_eq!(ed.col, 6);
        ed.move_word(1); // skip space, over "id"
        assert_eq!(ed.col, 9);
        ed.move_word(-1); // back to start of "id"
        assert_eq!(ed.col, 7);
    }

    #[test]
    fn word_motion_and_doc_end_cross_lines() {
        let mut ed = EditorState::from_text("ab\ncd");
        ed.move_doc_start();
        ed.move_word(1); // to end of "ab"
        assert_eq!((ed.line, ed.col), (0, 2));
        ed.move_word(1); // at line end -> jump to next line start
        assert_eq!((ed.line, ed.col), (1, 0));
        ed.move_doc_end();
        assert_eq!((ed.line, ed.col), (1, 2));
    }

    #[test]
    fn shift_word_motion_builds_a_selection() {
        let mut ed = EditorState::from_text("hello world");
        ed.move_doc_start();
        ed.set_selecting(true);
        ed.move_word(1);
        assert_eq!(ed.selected_text().as_deref(), Some("hello"));
    }
}
