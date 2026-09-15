//! SQL format spec — Postgres/MySQL/SQLite.

use super::Spec;

// Formatting only needs clause-boundary keywords for indent/spacing
// decisions, not a connection-specific vendor list — Postgres' common-superset
// table is a fine stand-in for every SQL dialect here.
fn is_keyword(word: &str) -> bool {
    crate::editor::sql::is_keyword(rdb_connstore::SqlDialect::Postgres, word)
}

const CLAUSE_STARTERS: &[&str] = &[
    "FROM", "WHERE", "GROUP", "HAVING", "ORDER", "LIMIT", "OFFSET", "JOIN", "LEFT", "RIGHT",
    "INNER", "FULL", "UNION", "VALUES", "SET",
];

const JOIN_QUALIFIERS: &[&str] = &["LEFT", "RIGHT", "INNER", "FULL", "OUTER", "CROSS"];

pub const SPEC: Spec = Spec {
    is_keyword,
    clause_starters: CLAUSE_STARTERS,
    join_qualifiers: JOIN_QUALIFIERS,
};
