//! Redis command completion. Redis is line-based (one command per
//! statement), so completion is offered only when the word being typed is
//! the first thing on the line — no clause-position logic like SQL/CQL
//! needs, and no table/column tree since Redis has no schema in that sense.

use super::{match_rank, trailing_word, Candidate};

pub fn suggest(before_cursor: &str) -> (usize, Vec<Candidate>) {
    let cur_line = before_cursor.rsplit('\n').next().unwrap_or(before_cursor);
    let word = trailing_word(cur_line);
    if word.is_empty() {
        return (0, Vec::new());
    }
    let head = cur_line.strip_suffix(word).unwrap_or(cur_line);
    if !head.trim().is_empty() {
        // Not at line start — a command's arguments aren't completed.
        return (0, Vec::new());
    }
    let wl = word.to_lowercase();
    let mut cands: Vec<Candidate> = crate::editor::command::KEYWORDS
        .iter()
        .filter(|k| match_rank(k, &wl).is_some())
        .map(|k| Candidate {
            label: (*k).to_string(),
            kind: "keyword".into(),
            sub: String::new(),
        })
        .collect();
    cands.sort_by_key(|c| match_rank(&c.label, &wl).unwrap_or(u8::MAX));
    cands.truncate(20);
    (word.chars().count(), cands)
}
