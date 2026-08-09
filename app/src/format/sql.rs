//! SQL format spec — Postgres/MySQL/SQLite.

use super::Spec;

const CLAUSE_STARTERS: &[&str] = &[
    "FROM", "WHERE", "GROUP", "HAVING", "ORDER", "LIMIT", "OFFSET", "JOIN", "LEFT", "RIGHT",
    "INNER", "FULL", "UNION", "VALUES", "SET",
];

const JOIN_QUALIFIERS: &[&str] = &["LEFT", "RIGHT", "INNER", "FULL", "OUTER", "CROSS"];

pub const SPEC: Spec = Spec {
    is_keyword: crate::editor::sql::is_keyword,
    clause_starters: CLAUSE_STARTERS,
    join_qualifiers: JOIN_QUALIFIERS,
};
