//! SQL editor state: a line-based text buffer with cursor, plus a small
//! per-line lexer that feeds the highlighted span model. Lines are lexed
//! independently (line comments and single-line strings only), which keeps
//! re-lexing incremental: only the edited line changes shape.

/// Span kinds, mirrored in code-editor.slint: 0 plain · 1 keyword ·
/// 2 string · 3 function · 4 comment · 5 number.
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub text: String,
    pub kind: i32,
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
        });
    }
    spans
}

/// Editor buffer + cursor. The text is the single source of truth; the UI
/// mirrors it through the span model rebuilt per edited line.
#[derive(Default)]
pub struct EditorState {
    pub lines: Vec<String>,
    pub line: usize, // cursor line
    pub col: usize,  // cursor column (chars)
}

impl EditorState {
    pub fn from_text(text: &str) -> Self {
        let mut lines: Vec<String> = text.split('\n').map(str::to_string).collect();
        if lines.is_empty() {
            lines.push(String::new());
        }
        let line = lines.len() - 1;
        let col = lines[line].chars().count();
        EditorState { lines, line, col }
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

    pub fn insert(&mut self, s: &str) {
        if s.contains('\n') {
            for (i, part) in s.split('\n').enumerate() {
                if i > 0 {
                    self.newline();
                }
                self.insert(part);
            }
            return;
        }
        let b = self.byte_col();
        self.lines[self.line].insert_str(b, s);
        self.col += s.chars().count();
    }

    pub fn newline(&mut self) {
        let b = self.byte_col();
        let rest = self.lines[self.line].split_off(b);
        self.lines.insert(self.line + 1, rest);
        self.line += 1;
        self.col = 0;
    }

    pub fn backspace(&mut self) {
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
        let len = self.lines[self.line].chars().count();
        if self.col < len {
            let b = self.byte_col();
            self.lines[self.line].remove(b);
        } else if self.line + 1 < self.lines.len() {
            let next = self.lines.remove(self.line + 1);
            self.lines[self.line].push_str(&next);
        }
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

    /// Current line text — the "Run Selection" fallback unit.
    #[allow(dead_code)] // wired up when Run Selection gains a real selection
    pub fn current_line(&self) -> &str {
        &self.lines[self.line]
    }
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
    fn cursor_moves_clamp() {
        let mut ed = EditorState::from_text("ab\ncd");
        ed.move_cursor(-1, 0);
        assert_eq!((ed.line, ed.col), (0, 2));
        ed.move_cursor(0, 1); // wraps to next line start
        assert_eq!((ed.line, ed.col), (1, 0));
        ed.move_cursor(0, -1); // wraps back to previous line end
        assert_eq!((ed.line, ed.col), (0, 2));
    }
}
