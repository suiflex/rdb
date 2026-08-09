//! MongoDB completion. Mongo has no SQL keywords, so bare-word completion
//! offers the `db` handle and the active database's collections instead of
//! SELECT/DELETE/… noise, and `db.<collection>.` offers mongosh methods.

use crate::model::VmTreeNode;

use super::{tables, Candidate};

/// mongosh collection methods, offered after `db.<collection>.` so Mongo
/// completion reaches parity with SQL column completion.
const METHODS: &[&str] = &[
    "find",
    "findOne",
    "aggregate",
    "countDocuments",
    "distinct",
    "insertOne",
    "insertMany",
    "updateOne",
    "updateMany",
    "deleteOne",
    "deleteMany",
];

pub fn methods() -> Vec<Candidate> {
    METHODS
        .iter()
        .map(|m| Candidate {
            label: (*m).to_string(),
            kind: "keyword".into(),
            sub: String::new(),
        })
        .collect()
}

/// Does `name` match a collection node in the tree?
pub fn is_collection(nodes: &[VmTreeNode], name: &str) -> bool {
    let nl = name.to_lowercase();
    nodes
        .iter()
        .any(|n| n.kind == "collection" && n.label.to_lowercase() == nl)
}

/// Completion when the cursor is on a bare word (no `owner.` prefix).
pub fn bare_word(scope: &[VmTreeNode]) -> Vec<Candidate> {
    let mut c = vec![Candidate {
        label: "db".into(),
        kind: "keyword".into(),
        sub: String::new(),
    }];
    c.extend(tables(scope));
    c
}
