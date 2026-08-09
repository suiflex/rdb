//! CQL format spec — Cassandra/ScyllaDB. No JOIN/HAVING/UNION: CQL doesn't
//! support them, so they're absent from both the keyword set (see
//! `editor::cql`) and the clause starters here.

use super::Spec;

const CLAUSE_STARTERS: &[&str] = &["FROM", "WHERE", "AND", "ALLOW", "USING", "SET"];

pub const SPEC: Spec = Spec {
    is_keyword: crate::editor::cql::is_keyword,
    clause_starters: CLAUSE_STARTERS,
    join_qualifiers: &[],
};
