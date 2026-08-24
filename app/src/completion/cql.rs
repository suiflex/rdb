//! CQL clause-position completion — Cassandra/ScyllaDB. Reuses the editor's
//! canonical CQL keyword list (`editor::cql::KEYWORDS`) so highlighting and
//! completion can't drift apart. Deliberately offers no JOIN/HAVING/GROUP BY
//! candidates — CQL has none — which is what actually distinguishes this
//! from `completion::sql`, not just a renamed copy of it.

use crate::model::VmTreeNode;

use super::{all_columns, from_table_columns, tables, Candidate};

pub use crate::editor::cql::is_keyword;

pub fn keywords() -> Vec<Candidate> {
    crate::editor::cql::KEYWORDS
        .iter()
        .map(|k| Candidate {
            label: (*k).to_string(),
            kind: "keyword".into(),
            sub: String::new(),
        })
        .collect()
}

/// Completion when the cursor is on a bare word (no `owner.` prefix).
pub fn bare_word(
    head: &str,
    stmt: &str,
    nodes: &[VmTreeNode],
    scope: &[VmTreeNode],
) -> Vec<Candidate> {
    match super::last_keyword(head, rdb_connstore::QueryLanguage::Cql).as_deref() {
        // table/keyspace position.
        Some("FROM") | Some("INTO") | Some("UPDATE") | Some("TABLE") => {
            let mut c = tables(scope);
            c.extend(super::schemas(nodes));
            c
        }
        // column/predicate position.
        Some("SELECT") | Some("WHERE") | Some("AND") | Some("OR") | Some("SET") | Some("BY")
        | Some("VALUES") | Some("USING") | Some("ALLOW") => {
            let mut c = from_table_columns(stmt, nodes);
            c.extend(all_columns(scope));
            c.extend(tables(scope));
            c.extend(keywords());
            c
        }
        _ => {
            let mut c = keywords();
            c.extend(tables(scope));
            c
        }
    }
}
