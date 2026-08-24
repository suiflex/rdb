//! SQL clause-position completion — Postgres/MySQL/SQLite.

use crate::model::VmTreeNode;

use super::{all_columns, from_table_columns, tables, Candidate};

/// SQL keywords offered when the cursor is at a statement/clause boundary.
const KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "JOIN",
    "INNER",
    "LEFT",
    "RIGHT",
    "OUTER",
    "FULL",
    "ON",
    "AS",
    "GROUP",
    "ORDER",
    "BY",
    "HAVING",
    "LIMIT",
    "OFFSET",
    "DISTINCT",
    "AND",
    "OR",
    "NOT",
    "NULL",
    "IS",
    "IN",
    "LIKE",
    "BETWEEN",
    "ASC",
    "DESC",
    "COUNT",
    "CREATE",
    "TABLE",
    "ALTER",
    "DROP",
    "INDEX",
    // Date/aggregate functions — widely-portable names only (e.g. not
    // MySQL's DATE_FORMAT or Postgres' TO_CHAR specifically).
    "EXTRACT",
    "DATE_TRUNC",
    "NOW",
    "CURRENT_DATE",
    "CURRENT_TIMESTAMP",
    "INTERVAL",
    "AGE",
    "COALESCE",
    "CAST",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
];

pub fn is_keyword(w: &str) -> bool {
    let u = w.to_uppercase();
    KEYWORDS.contains(&u.as_str())
}

pub fn keywords() -> Vec<Candidate> {
    KEYWORDS
        .iter()
        .map(|k| Candidate {
            label: (*k).to_string(),
            kind: "keyword".into(),
            sub: String::new(),
        })
        .collect()
}

/// Completion when the cursor is on a bare word (no `owner.` prefix) — the
/// SQL clause-position dispatch: table names after FROM/JOIN/…, columns
/// after SELECT/WHERE/…, keywords + tables as the statement-start fallback.
pub fn bare_word(
    head: &str,
    stmt: &str,
    nodes: &[VmTreeNode],
    scope: &[VmTreeNode],
    active_schema: &str,
) -> Vec<Candidate> {
    match super::last_keyword(head, rdb_connstore::QueryLanguage::Sql).as_deref() {
        // table position: active-schema tables, every schema name, and every
        // other schema's tables pre-qualified, so a cross-schema table can be
        // completed from its own name without typing the schema first.
        Some("FROM") | Some("JOIN") | Some("INTO") | Some("UPDATE") | Some("TABLE") => {
            let mut c = tables(scope);
            c.extend(super::schemas(nodes));
            c.extend(super::qualified_tables(nodes, active_schema));
            c
        }
        // column position: offer columns and tables, plus keywords so the
        // next clause (FROM/WHERE/…) is always reachable, e.g. after `*`.
        Some("SELECT") | Some("WHERE") | Some("AND") | Some("OR") | Some("ON") | Some("HAVING")
        | Some("SET") | Some("BY") | Some("VALUES") => {
            // Columns of the statement's own FROM/JOIN tables come first (they
            // are what's actually in scope, cross-schema included), then the
            // active-schema columns/tables and keywords as a fallback.
            let mut c = from_table_columns(stmt, nodes);
            c.extend(all_columns(scope));
            c.extend(tables(scope));
            c.extend(keywords());
            c
        }
        // statement start / no useful context: keywords + tables
        _ => {
            let mut c = keywords();
            c.extend(tables(scope));
            c
        }
    }
}
