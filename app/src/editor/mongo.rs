//! mongosh method words for the editor lexer. `lex_line` uppercases every
//! identifier before matching (shared across all four dialects), so these
//! stay uppercase even though real mongosh calls are lowercase/camelCase.

pub const KEYWORDS: &[&str] = &["FIND", "AGGREGATE"];

/// True when `word` (already uppercased) is a highlighted Mongo keyword.
pub fn is_keyword(word: &str) -> bool {
    KEYWORDS.contains(&word)
}
