//! Pure seam between the editor text box and a typed `rdbs_core::Query`.
//! The connected `Engine` decides how the text is interpreted, so a single
//! editor drives all four paradigms.

use rdbs_connstore::Engine;
use rdbs_core::query::{MongoKind, MongoOp, Query};

/// Turn raw editor text into a typed `Query` for the connected engine.
/// Returns a human-readable error string on malformed input (shown in the
/// result-status line; no driver call is made).
pub fn parse_query(engine: Engine, text: &str) -> Result<Query, String> {
    match engine {
        Engine::Postgres | Engine::MySql => Ok(Query::Sql(text.to_string())),
        Engine::Redis => {
            let tokens: Vec<String> = text.split_whitespace().map(|s| s.to_string()).collect();
            if tokens.is_empty() {
                return Err("empty Redis command".into());
            }
            Ok(Query::Command(tokens))
        }
        Engine::Mongo => parse_mongo(text),
    }
}

/// Editor placeholder/hint per engine (shown in the UI).
pub fn editor_hint(engine: Engine) -> &'static str {
    match engine {
        Engine::Postgres | Engine::MySql => "SQL — e.g. SELECT * FROM table",
        Engine::Redis => "Redis command — e.g. SET key value",
        Engine::Mongo => r#"Mongo JSON — {"collection":"c","op":"find","body":{}}"#,
    }
}

fn parse_mongo(text: &str) -> Result<Query, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("invalid Mongo JSON: {e}"))?;
    let collection = v
        .get("collection")
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing \"collection\"".to_string())?
        .to_string();
    let op = v
        .get("op")
        .and_then(|o| o.as_str())
        .ok_or_else(|| "missing \"op\"".to_string())?;
    let body = v.get("body").cloned().unwrap_or(serde_json::Value::Null);

    let kind = match op {
        "find" => MongoKind::Find(body),
        "insert" => MongoKind::Insert(body),
        "aggregate" => {
            let arr = body
                .as_array()
                .ok_or_else(|| "aggregate \"body\" must be a JSON array of stages".to_string())?;
            MongoKind::Aggregate(arr.clone())
        }
        other => {
            return Err(format!(
                "unknown Mongo op \"{other}\" (use find/insert/aggregate)"
            ))
        }
    };
    Ok(Query::Mongo(MongoOp { collection, kind }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdbs_connstore::Engine;
    use rdbs_core::query::{MongoKind, Query};

    #[test]
    fn sql_engines_pass_text_through() {
        for e in [Engine::Postgres, Engine::MySql] {
            match parse_query(e, "SELECT 1").unwrap() {
                Query::Sql(s) => assert_eq!(s, "SELECT 1"),
                _ => panic!("expected Sql"),
            }
        }
    }

    #[test]
    fn redis_splits_into_command_tokens() {
        match parse_query(Engine::Redis, "  SET   key   val ").unwrap() {
            Query::Command(toks) => assert_eq!(toks, vec!["SET", "key", "val"]),
            _ => panic!("expected Command"),
        }
    }

    #[test]
    fn redis_empty_is_error() {
        assert!(parse_query(Engine::Redis, "   ").is_err());
    }

    #[test]
    fn mongo_find_builds_op() {
        let text = r#"{ "collection": "users", "op": "find", "body": { "age": { "$gte": 18 } } }"#;
        match parse_query(Engine::Mongo, text).unwrap() {
            Query::Mongo(op) => {
                assert_eq!(op.collection, "users");
                assert!(matches!(op.kind, MongoKind::Find(_)));
            }
            _ => panic!("expected Mongo"),
        }
    }

    #[test]
    fn mongo_insert_builds_op() {
        let text = r#"{ "collection": "users", "op": "insert", "body": { "name": "a" } }"#;
        match parse_query(Engine::Mongo, text).unwrap() {
            Query::Mongo(op) => assert!(matches!(op.kind, MongoKind::Insert(_))),
            _ => panic!("expected Mongo"),
        }
    }

    #[test]
    fn mongo_aggregate_requires_array_body() {
        let ok = r#"{ "collection": "u", "op": "aggregate", "body": [ { "$count": "n" } ] }"#;
        match parse_query(Engine::Mongo, ok).unwrap() {
            Query::Mongo(op) => assert!(matches!(op.kind, MongoKind::Aggregate(_))),
            _ => panic!("expected Mongo"),
        }
        let bad = r#"{ "collection": "u", "op": "aggregate", "body": { "x": 1 } }"#;
        assert!(parse_query(Engine::Mongo, bad).is_err());
    }

    #[test]
    fn mongo_errors_on_bad_json_unknown_op_and_missing_collection() {
        assert!(parse_query(Engine::Mongo, "not json").is_err());
        assert!(parse_query(
            Engine::Mongo,
            r#"{ "collection": "u", "op": "drop", "body": {} }"#
        )
        .is_err());
        assert!(parse_query(Engine::Mongo, r#"{ "op": "find", "body": {} }"#).is_err());
    }
}
