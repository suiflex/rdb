//! MongoDB completion. Mongo has no SQL keywords, so bare-word completion
//! offers the `db` handle and the active database's collections instead of
//! SELECT/DELETE/… noise, and `db.<collection>.` offers mongosh methods.

use crate::model::VmTreeNode;

use super::{columns_of, tables, Candidate};

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

/// mongosh's chained result modifiers, offered after a closed
/// `db.<collection>.<method>(...)` call (`db.coll.find().<here>`) — see
/// `call_context`'s `depth == 0` case. Matches `query_parse.rs`'s
/// `parse_mongo_line`, which parses this chain permissively after any method
/// (not just `find`), so offered the same way here rather than gating on it.
const CHAIN_MODIFIERS: &[&str] = &["limit", "skip", "sort"];

pub fn chain_modifiers() -> Vec<Candidate> {
    keyword_candidates(CHAIN_MODIFIERS)
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

/// Cursor position relative to the nearest `db.<coll>.<method>(...)` call
/// before it. Best-effort text scan, not a JSON parser — matches the rest of
/// this module's heuristic style (see `query_parse.rs`'s own stance on this).
/// Misfires on unusual nesting (e.g. `$or: [ { <here> } ]`) are acceptable;
/// extend the scan rather than reaching for a real parser.
pub struct CallCtx {
    pub collection: String,
    pub method: String,
    /// Unmatched `{`/`[` opened after the method's `(`.
    pub depth: u32,
    /// Key text immediately preceding the innermost still-open `{`/`[`, if
    /// any (e.g. `$set` in `{ $set: { |`).
    pub open_key: Option<String>,
    /// Top-level (`depth == 0`) commas seen since the method's `(` — which
    /// positional argument the cursor is in.
    pub arg_index: u32,
}

/// Cursor position, anchored at the last `db.` before it so an earlier,
/// unrelated call in the same document can't pollute the scan.
pub fn call_context(before_cursor: &str) -> Option<CallCtx> {
    let anchor = before_cursor.rfind("db.")?;
    let mut rest = &before_cursor[anchor + 3..];
    let collection = take_ident(&mut rest)?;
    rest = rest.strip_prefix('.')?;
    let method = take_ident(&mut rest)?;
    let rest = rest.strip_prefix('(')?;
    let (depth, open_key, arg_index) = scan_call_body(rest);
    Some(CallCtx {
        collection,
        method,
        depth,
        open_key,
        arg_index,
    })
}

fn take_ident(rest: &mut &str) -> Option<String> {
    let end = rest
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .unwrap_or(rest.len());
    if end == 0 {
        return None;
    }
    let ident = rest[..end].to_string();
    *rest = &rest[end..];
    Some(ident)
}

/// Scans everything after a method's `(` up to the cursor: brace/bracket
/// depth, the key that precedes the innermost still-open one, and how many
/// top-level commas (argument separators) have been seen. String-literal
/// aware so `{`/`}`/`,`/`:` inside a quoted value don't confuse the scan.
fn scan_call_body(rest: &str) -> (u32, Option<String>, u32) {
    let mut stack: Vec<Option<String>> = Vec::new();
    let mut pending = String::new();
    let mut pending_key: Option<String> = None;
    let mut arg_index: u32 = 0;
    let mut in_string: Option<char> = None;
    let mut escape = false;
    for c in rest.chars() {
        if let Some(q) = in_string {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == q {
                in_string = None;
            }
            continue;
        }
        match c {
            '"' | '\'' => {
                in_string = Some(c);
                pending.clear();
            }
            '{' | '[' => {
                stack.push(pending_key.take());
                pending.clear();
            }
            '}' | ']' => {
                stack.pop();
                pending.clear();
                pending_key = None;
            }
            ':' => {
                let key = pending.trim().to_string();
                pending_key = if key.is_empty() { None } else { Some(key) };
                pending.clear();
            }
            ',' => {
                if stack.is_empty() {
                    arg_index += 1;
                }
                pending.clear();
                pending_key = None;
            }
            c if c.is_ascii_alphanumeric() || c == '_' || c == '$' || c == '.' => {
                pending.push(c);
            }
            _ => pending.clear(),
        }
    }
    (
        stack.len() as u32,
        stack.last().cloned().flatten(),
        arg_index,
    )
}

/// MongoDB query operators, offered as filter values (`{ field: { <here> } }`).
const QUERY_OPS: &[&str] = &[
    "$eq",
    "$ne",
    "$gt",
    "$gte",
    "$lt",
    "$lte",
    "$in",
    "$nin",
    "$exists",
    "$regex",
    "$and",
    "$or",
    "$not",
    "$nor",
    "$type",
    "$size",
    "$all",
    "$elemMatch",
];

/// Query operators as completion candidates, for the standalone filter box.
pub fn query_ops() -> Vec<Candidate> {
    keyword_candidates(QUERY_OPS)
}

/// Update operators, offered as the top-level keys of an update document.
const UPDATE_OPS: &[&str] = &[
    "$set",
    "$unset",
    "$inc",
    "$push",
    "$pull",
    "$addToSet",
    "$rename",
    "$currentDate",
    "$min",
    "$max",
];

/// Aggregation pipeline stage names, offered inside a pipeline stage object.
const STAGE_NAMES: &[&str] = &[
    "$match",
    "$group",
    "$project",
    "$sort",
    "$limit",
    "$skip",
    "$unwind",
    "$lookup",
    "$count",
    "$addFields",
    "$facet",
    "$replaceRoot",
];

fn keyword_candidates(words: &[&str]) -> Vec<Candidate> {
    words
        .iter()
        .map(|w| Candidate {
            label: (*w).to_string(),
            kind: "keyword".into(),
            sub: String::new(),
        })
        .collect()
}

/// Filter-shaped keys: an open object under one of these offers field names
/// again rather than another operator (`{ $match: { <here> } }`).
fn is_filter_shaped(key: &str) -> bool {
    matches!(key, "$match" | "$set" | "$or" | "$and" | "$nor")
}

/// Completions once the cursor is inside a `db.<coll>.<method>(...)` call
/// (`ctx.depth >= 1`). Heuristic dispatch mirroring the SQL/CQL clause-
/// position `bare_word` functions: best real guess for the position, not a
/// guarantee.
pub fn in_call(ctx: &CallCtx, nodes: &[VmTreeNode]) -> Vec<Candidate> {
    let is_filter_method = matches!(
        ctx.method.as_str(),
        "find"
            | "findOne"
            | "countDocuments"
            | "deleteOne"
            | "deleteMany"
            | "updateOne"
            | "updateMany"
    );
    match &ctx.open_key {
        None if is_filter_method && ctx.arg_index == 0 => columns_of(nodes, &ctx.collection),
        None if matches!(ctx.method.as_str(), "updateOne" | "updateMany") && ctx.arg_index == 1 => {
            keyword_candidates(UPDATE_OPS)
        }
        None if ctx.method == "aggregate" && ctx.depth == 2 => keyword_candidates(STAGE_NAMES),
        Some(key) if is_filter_shaped(key) => columns_of(nodes, &ctx.collection),
        Some(key) if UPDATE_OPS.contains(&key.as_str()) => columns_of(nodes, &ctx.collection),
        _ => keyword_candidates(QUERY_OPS),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nodes() -> Vec<VmTreeNode> {
        let mk = |l: &str, k: &str| VmTreeNode {
            label: l.into(),
            kind: k.into(),
        };
        vec![
            mk("users", "collection"),
            mk("name", "field"),
            mk("age", "field"),
        ]
    }

    #[test]
    fn find_filter_position_offers_fields() {
        let ctx = call_context("db.users.find({ ").unwrap();
        assert_eq!(ctx.method, "find");
        assert_eq!(ctx.depth, 1);
        assert_eq!(ctx.open_key, None);
        let labels: Vec<String> = in_call(&ctx, &nodes())
            .into_iter()
            .map(|c| c.label)
            .collect();
        assert!(labels.iter().any(|l| l == "name"));
        assert!(labels.iter().any(|l| l == "age"));
    }

    #[test]
    fn update_second_arg_offers_update_operators() {
        let ctx = call_context("db.users.updateOne({ _id: 1 }, { ").unwrap();
        assert_eq!(ctx.arg_index, 1);
        assert_eq!(ctx.open_key, None);
        let labels: Vec<String> = in_call(&ctx, &nodes())
            .into_iter()
            .map(|c| c.label)
            .collect();
        assert!(labels.iter().any(|l| l == "$set"));
        assert!(!labels.iter().any(|l| l == "name"));
    }

    #[test]
    fn set_key_offers_fields_again() {
        let ctx = call_context("db.users.updateOne({}, { $set: { ").unwrap();
        assert_eq!(ctx.open_key, Some("$set".to_string()));
        let labels: Vec<String> = in_call(&ctx, &nodes())
            .into_iter()
            .map(|c| c.label)
            .collect();
        assert!(labels.iter().any(|l| l == "name"));
    }

    #[test]
    fn aggregate_pipeline_stage_offers_stage_names() {
        let ctx = call_context("db.users.aggregate([{ ").unwrap();
        assert_eq!(ctx.depth, 2);
        let labels: Vec<String> = in_call(&ctx, &nodes())
            .into_iter()
            .map(|c| c.label)
            .collect();
        assert!(labels.iter().any(|l| l == "$match"));
    }

    #[test]
    fn match_stage_offers_fields() {
        let ctx = call_context("db.users.aggregate([{ $match: { ").unwrap();
        assert_eq!(ctx.open_key, Some("$match".to_string()));
        let labels: Vec<String> = in_call(&ctx, &nodes())
            .into_iter()
            .map(|c| c.label)
            .collect();
        assert!(labels.iter().any(|l| l == "name"));
    }

    #[test]
    fn no_call_in_progress_returns_none() {
        assert!(call_context("db.users.find").is_none());
        assert!(call_context("db.").is_none());
    }
}
