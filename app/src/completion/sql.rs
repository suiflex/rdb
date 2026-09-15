//! SQL clause-position completion — Postgres/MySQL/SQLite.

use super::{all_columns, from_table_columns, tables, Candidate};
use crate::model::VmTreeNode;
use rdb_connstore::{QueryDialect, SqlDialect};

pub fn keywords(dialect: SqlDialect) -> Vec<Candidate> {
    crate::editor::sql::keywords(dialect)
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
    dialect: SqlDialect,
) -> Vec<Candidate> {
    match super::last_keyword(head, QueryDialect::Sql(dialect)).as_deref() {
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
            let (has_scope, mut c) = from_table_columns(stmt, nodes, QueryDialect::Sql(dialect));
            if !has_scope {
                c.extend(all_columns(scope));
            }
            c.extend(tables(scope));
            c.extend(keywords(dialect));
            c
        }
        // statement start / no useful context: keywords + tables
        _ => {
            let mut c = keywords(dialect);
            c.extend(tables(scope));
            c
        }
    }
}
