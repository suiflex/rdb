//! Pure seam between the editor text box and a typed `rdb_core::Query`.
//! The connected `Engine` decides how the text is interpreted, so a single
//! editor drives all four paradigms.

use rdb_connstore::Engine;
use rdb_core::query::{MongoKind, MongoOp, Query};

/// Turn raw editor text into a typed `Query` for the connected engine.
/// Returns a human-readable error string on malformed input (shown in the
/// result-status line; no driver call is made).
pub fn parse_query(engine: Engine, text: &str) -> Result<Query, String> {
    match engine {
        // SQL engines parse `--` comments (inline and whole-line) natively, so
        // send the buffer verbatim — same as every other SQL editor. Stripping
        // comment lines here re-joined the surrounding lines and could shift a
        // clause onto the wrong statement (e.g. a `WHERE` whose only condition
        // was commented, followed by the real predicate two lines down).
        Engine::Postgres | Engine::MySql | Engine::Sqlite | Engine::Cassandra => {
            Ok(Query::Sql(text.to_string()))
        }
        // Redis/Mongo have no server-side line comments, so drop commented lines
        // (Cmd+/ toggles them) before interpreting.
        Engine::Redis => {
            let cleaned = strip_comment_lines(engine, text);
            let tokens: Vec<String> = cleaned.split_whitespace().map(|s| s.to_string()).collect();
            if tokens.is_empty() {
                return Err("empty Redis command".into());
            }
            Ok(Query::Command(tokens))
        }
        Engine::Mongo => parse_mongo(&strip_comment_lines(engine, text)),
    }
}

/// Best-effort recognition of a plain, single-table `SELECT ... FROM t
/// [WHERE ...]` (optionally schema/db-qualified: `schema.table`) — no
/// join/union/aggregation/multi-table FROM — so its result grid can be
/// offered for inline cell editing. There's no SQL parser in this
/// workspace, so this is a conservative text heuristic: anything ambiguous
/// returns `None` and the result just stays read-only, same as before this
/// existed. Returns `(qualifier, table)` — `qualifier` is the part before
/// the dot when the query names it explicitly (e.g. `"oss_rba_common"` for
/// `oss_rba_common.step_journal`), left for the caller to apply as
/// `TableRef.schema` (Postgres) or `.database` (MySQL/Sqlite), falling back
/// to the active connection's selection when the query didn't qualify it.
pub fn single_table_name(sql: &str) -> Option<(Option<String>, String)> {
    // Run always sends the whole `;`-delimited statement under the cursor, and
    // that span deliberately includes any commented-out lines above it (a `;`
    // inside a `--` line doesn't cut a statement). Without stripping them the
    // text starts with `--`, never `select`, and a commented-out `join` would
    // reject an otherwise clean query. Every engine reaching here (Postgres,
    // MySQL, SQLite) uses `--`, so one prefix covers all three.
    let sql = strip_comment_lines(Engine::Postgres, sql);
    let trimmed = sql.trim().trim_end_matches(';').trim();
    let lower = trimmed.to_lowercase();
    if !lower.starts_with("select") {
        return None;
    }
    for kw in [" join ", " union ", " group by", " having", "distinct"] {
        if lower.contains(kw) {
            return None;
        }
    }
    for agg in ["count(", "sum(", "avg(", "min(", "max("] {
        if lower.contains(agg) {
            return None;
        }
    }
    let from_idx = lower.find(" from ")?;
    let after_from = trimmed[from_idx + 6..].trim_start();
    let end = after_from
        .find(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ';')
        .unwrap_or(after_from.len());
    let ident = after_from[..end].trim();
    let rest = after_from[end..].trim_start();
    // A comma right after means a multi-table `FROM a, b` — bail.
    if ident.is_empty() || rest.starts_with(',') {
        return None;
    }
    fn unquote(s: &str) -> &str {
        s.trim_matches(|c| c == '"' || c == '`' || c == '\'')
    }
    let (qualifier, name) = match ident.split_once('.') {
        Some((q, n)) => {
            // `db.schema.table` (3-part) still has a dot in `n` — too
            // ambiguous to guess which part is which, so bail.
            if n.contains('.') {
                return None;
            }
            (Some(unquote(q).to_string()), unquote(n))
        }
        None => (None, unquote(ident)),
    };
    if name.is_empty() || qualifier.as_deref().is_some_and(str::is_empty) {
        return None;
    }
    Some((qualifier, name.to_string()))
}

/// Editor placeholder/hint per engine (shown in the UI).
pub fn editor_hint(engine: Engine) -> &'static str {
    match engine {
        Engine::Postgres | Engine::MySql | Engine::Sqlite => "SELECT * FROM table",
        Engine::Cassandra => "SELECT * FROM keyspace.table",
        Engine::Redis => "SET key value",
        Engine::Mongo => r#"db.coll.find({ })  ·  or JSON envelope"#,
    }
}

/// Line-comment marker for the engine's query language. Used by the editor's
/// Cmd+/ toggle and to strip commented lines before a query runs.
pub fn comment_prefix(engine: Engine) -> &'static str {
    match engine {
        Engine::Postgres | Engine::MySql | Engine::Sqlite | Engine::Cassandra => "--",
        Engine::Redis => "#",
        Engine::Mongo => "//",
    }
}

/// Remove whole-line comments (lines whose first non-whitespace run is the
/// engine's comment marker). Inline trailing comments are left for the engine
/// itself to handle.
fn strip_comment_lines(engine: Engine, text: &str) -> String {
    let prefix = comment_prefix(engine);
    text.lines()
        .filter(|l| !l.trim_start().starts_with(prefix))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Mongo has two accepted editor forms: the mongosh line syntax people know
/// (`db.coll.find({...}).limit(5).sort({...})`) and the original JSON envelope
/// (`{"collection":"c","op":"find","body":{}}`). Anything starting with `db.`
/// is the line form; everything else is the envelope.
fn parse_mongo(text: &str) -> Result<Query, String> {
    // An optional leading `use('db')` switches the target database, mongosh-style.
    let (db_override, body) = strip_use(text.trim());
    let trimmed = body.trim().trim_end_matches(';').trim_end();
    if trimmed.starts_with("db.") {
        parse_mongo_line(trimmed, db_override)
    } else {
        parse_mongo_envelope(text)
    }
}

/// Strip an optional leading `use('name')` / `use("name")` statement. Returns
/// the database name (if any) and the remaining text after it.
fn strip_use(s: &str) -> (Option<String>, &str) {
    let t = s.trim_start();
    let Some(rest) = t.strip_prefix("use") else {
        return (None, s);
    };
    let Some(open) = rest.find('(') else {
        return (None, s);
    };
    // Only whitespace may sit between `use` and `(`.
    if !rest[..open].trim().is_empty() {
        return (None, s);
    }
    let Some(rel_close) = rest[open..].find(')') else {
        return (None, s);
    };
    let close = open + rel_close;
    let name = rest[open + 1..close]
        .trim()
        .trim_matches(|c| c == '\'' || c == '"')
        .to_string();
    if name.is_empty() {
        return (None, s);
    }
    let tail = rest[close + 1..].trim_start().trim_start_matches(';');
    (Some(name), tail)
}

/// Parse `db.<collection>.<op>(<json>)` with optional chained
/// `.limit(n)` / `.skip(n)` / `.sort({...})`. Bodies are permissive JSON
/// (json5: bare keys, single quotes) but not full mongosh — `ObjectId(...)`
/// still errors. `database` overrides the connection default when a `use(...)`
/// preceded the query.
fn parse_mongo_line(s: &str, database: Option<String>) -> Result<Query, String> {
    let rest = &s[3..]; // drop "db."
    let dot = rest
        .find('.')
        .ok_or_else(|| "expected db.<collection>.<op>(...)".to_string())?;
    let collection = rest[..dot].trim().to_string();
    if collection.is_empty() {
        return Err("missing collection in db.<collection>.<op>(...)".into());
    }

    let (method, arg, mut cursor) = next_call(&rest[dot..])?;
    let kind = match method {
        "find" => MongoKind::Find(parse_json_arg(arg, "find")?),
        "insertOne" | "insert" => MongoKind::Insert(parse_json_arg(arg, method)?),
        "aggregate" => {
            let v = parse_json_arg(arg, "aggregate")?;
            let arr = v
                .as_array()
                .ok_or_else(|| "aggregate([...]) needs an array of stages".to_string())?;
            MongoKind::Aggregate(arr.clone())
        }
        other => {
            return Err(format!(
                "unknown Mongo method \"{other}\" (use find/aggregate/insertOne)"
            ))
        }
    };

    // Trailing chained modifiers, in any order.
    let (mut limit, mut skip, mut sort) = (None, None, None);
    cursor = cursor.trim();
    while !cursor.is_empty() {
        let (m, a, rest) = next_call(cursor)?;
        match m {
            "limit" => limit = Some(parse_int_arg(a, "limit")?),
            "skip" => skip = Some(parse_int_arg(a, "skip")?),
            "sort" => sort = Some(parse_json_arg(a, "sort")?),
            other => {
                return Err(format!(
                    "unknown modifier \".{other}()\" (use limit/skip/sort)"
                ))
            }
        }
        cursor = rest.trim();
    }

    Ok(Query::Mongo(Box::new(MongoOp {
        collection,
        database,
        limit,
        skip,
        sort,
        kind,
    })))
}

/// Parse one `.name(arg)` at the start of `s`. Returns the method name, the raw
/// text between its parens, and the remainder after the closing `)`.
fn next_call(s: &str) -> Result<(&str, &str, &str), String> {
    let s = s
        .strip_prefix('.')
        .ok_or_else(|| format!("expected '.method(...)' at \"{}\"", s.trim()))?;
    let open = s
        .find('(')
        .ok_or_else(|| "expected '(' after method name".to_string())?;
    let name = s[..open].trim();
    let (inner, end) = extract_parens(s, open)?;
    Ok((name, inner, &s[end..]))
}

/// Extract the balanced `(...)` whose opening paren is at byte index `open`,
/// respecting JSON string literals so parens inside strings don't unbalance the
/// scan. Returns the inner text and the byte index just past the closing `)`.
/// Parens/quotes are ASCII, so all slice boundaries fall on char boundaries.
fn extract_parens(s: &str, open: usize) -> Result<(&str, usize), String> {
    let b = s.as_bytes();
    let (mut depth, mut in_str, mut esc, mut i) = (0i32, false, false, open);
    while i < b.len() {
        let c = b[i];
        if in_str {
            if esc {
                esc = false;
            } else if c == b'\\' {
                esc = true;
            } else if c == b'"' {
                in_str = false;
            }
        } else {
            match c {
                b'"' => in_str = true,
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok((&s[open + 1..i], i + 1));
                    }
                }
                _ => {}
            }
        }
        i += 1;
    }
    Err("unbalanced parentheses".into())
}

/// JSON argument for a call; an empty argument means an empty object (so
/// `find()` browses all documents).
fn parse_json_arg(arg: &str, ctx: &str) -> Result<serde_json::Value, String> {
    let a = arg.trim();
    if a.is_empty() {
        return Ok(serde_json::Value::Object(Default::default()));
    }
    json5::from_str(a).map_err(|e| format!("invalid JSON in {ctx}(...): {e}"))
}

fn parse_int_arg(arg: &str, ctx: &str) -> Result<i64, String> {
    arg.trim()
        .parse::<i64>()
        .map_err(|_| format!("{ctx}(...) expects an integer"))
}

fn parse_mongo_envelope(text: &str) -> Result<Query, String> {
    let v: serde_json::Value =
        serde_json::from_str(text).map_err(|e| format!("invalid Mongo JSON: {e}"))?;
    let collection = v
        .get("collection")
        .and_then(|c| c.as_str())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "missing \"collection\"".to_string())?
        .to_string();
    // Optional: target a specific database; omitted falls back to the default.
    let database = v
        .get("database")
        .and_then(|d| d.as_str())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());
    let op = v
        .get("op")
        .and_then(|o| o.as_str())
        .ok_or_else(|| "missing \"op\"".to_string())?;
    let body = v.get("body").cloned().unwrap_or(serde_json::Value::Null);
    // Optional row cap / offset for `find`; omitted means unbounded / 0.
    let limit = v.get("limit").and_then(|l| l.as_i64());
    let skip = v.get("skip").and_then(|s| s.as_i64());

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
    Ok(Query::Mongo(Box::new(MongoOp {
        collection,
        database,
        limit,
        skip,
        sort: None,
        kind,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdb_connstore::Engine;
    use rdb_core::query::{MongoKind, Query};

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
    fn sql_is_sent_verbatim_comments_and_all() {
        // SQL engines parse `--` natively; the buffer must reach them unchanged
        // so comment lines can't reflow the surrounding SQL.
        let text = "-- keep this\nSELECT 1";
        match parse_query(Engine::Postgres, text).unwrap() {
            Query::Sql(s) => assert_eq!(s, text),
            _ => panic!("expected Sql"),
        }
    }

    #[test]
    fn sql_commented_where_condition_keeps_real_predicate() {
        // Regression: a WHERE whose only inline condition is commented, with the
        // real predicate two lines below (past more comment lines). Stripping
        // comment lines used to fuse `where` onto the wrong token; raw passthrough
        // keeps it valid for Postgres to parse.
        let text = "select a\nfrom t\nwhere -- x = 1\n-- y = 2\nb = 3";
        match parse_query(Engine::Postgres, text).unwrap() {
            Query::Sql(s) => assert_eq!(s, text),
            _ => panic!("expected Sql"),
        }
    }

    #[test]
    fn mongo_ignores_commented_line() {
        let text = "// old query\n{ \"collection\": \"c\", \"op\": \"find\", \"body\": {} }";
        match parse_query(Engine::Mongo, text).unwrap() {
            Query::Mongo(op) => assert_eq!(op.collection, "c"),
            _ => panic!("expected Mongo"),
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
    fn mongo_parses_optional_database() {
        let with = r#"{ "collection": "c", "database": "appdb", "op": "find", "body": {} }"#;
        match parse_query(Engine::Mongo, with).unwrap() {
            Query::Mongo(op) => assert_eq!(op.database.as_deref(), Some("appdb")),
            _ => panic!("expected Mongo"),
        }
        let without = r#"{ "collection": "c", "op": "find", "body": {} }"#;
        match parse_query(Engine::Mongo, without).unwrap() {
            Query::Mongo(op) => assert_eq!(op.database, None),
            _ => panic!("expected Mongo"),
        }
    }

    #[test]
    fn mongo_parses_optional_limit() {
        let with = r#"{ "collection": "c", "op": "find", "body": {}, "limit": 50 }"#;
        match parse_query(Engine::Mongo, with).unwrap() {
            Query::Mongo(op) => assert_eq!(op.limit, Some(50)),
            _ => panic!("expected Mongo"),
        }
        let without = r#"{ "collection": "c", "op": "find", "body": {} }"#;
        match parse_query(Engine::Mongo, without).unwrap() {
            Query::Mongo(op) => assert_eq!(op.limit, None),
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
    fn mongo_line_find_with_filter() {
        let text = r#"db.users.find({ "age": { "$gte": 18 } })"#;
        match parse_query(Engine::Mongo, text).unwrap() {
            Query::Mongo(op) => {
                assert_eq!(op.collection, "users");
                assert!(matches!(op.kind, MongoKind::Find(_)));
                assert_eq!(op.database, None);
            }
            _ => panic!("expected Mongo"),
        }
    }

    #[test]
    fn mongo_line_empty_find_is_browse_all() {
        match parse_query(Engine::Mongo, "db.c.find()").unwrap() {
            Query::Mongo(op) => match op.kind {
                MongoKind::Find(f) => assert_eq!(f, serde_json::json!({})),
                _ => panic!("expected Find"),
            },
            _ => panic!("expected Mongo"),
        }
    }

    #[test]
    fn mongo_line_chained_limit_skip_sort() {
        let text = r#"db.c.find({}).limit(5).skip(10).sort({ "_id": -1 });"#;
        match parse_query(Engine::Mongo, text).unwrap() {
            Query::Mongo(op) => {
                assert_eq!(op.limit, Some(5));
                assert_eq!(op.skip, Some(10));
                assert_eq!(op.sort, Some(serde_json::json!({ "_id": -1 })));
            }
            _ => panic!("expected Mongo"),
        }
    }

    #[test]
    fn mongo_line_aggregate() {
        let text = r#"db.u.aggregate([ { "$count": "n" } ])"#;
        match parse_query(Engine::Mongo, text).unwrap() {
            Query::Mongo(op) => assert!(matches!(op.kind, MongoKind::Aggregate(_))),
            _ => panic!("expected Mongo"),
        }
    }

    #[test]
    fn mongo_line_insert_one() {
        match parse_query(Engine::Mongo, r#"db.u.insertOne({ "name": "a" })"#).unwrap() {
            Query::Mongo(op) => assert!(matches!(op.kind, MongoKind::Insert(_))),
            _ => panic!("expected Mongo"),
        }
    }

    #[test]
    fn mongo_line_paren_inside_string_stays_balanced() {
        // A ')' inside a JSON string must not close the call early.
        let text = r#"db.c.find({ "note": "a)b" })"#;
        match parse_query(Engine::Mongo, text).unwrap() {
            Query::Mongo(op) => match op.kind {
                MongoKind::Find(f) => assert_eq!(f["note"], serde_json::json!("a)b")),
                _ => panic!("expected Find"),
            },
            _ => panic!("expected Mongo"),
        }
    }

    #[test]
    fn mongo_line_accepts_relaxed_json() {
        // Real mongosh idiom: unquoted keys, single quotes, negative sort.
        let text = r#"db.c.find({to_email:'a@b.com'}).sort({_id:-1})"#;
        match parse_query(Engine::Mongo, text).unwrap() {
            Query::Mongo(op) => {
                match &op.kind {
                    MongoKind::Find(f) => assert_eq!(f["to_email"], serde_json::json!("a@b.com")),
                    _ => panic!("expected Find"),
                }
                assert_eq!(op.sort, Some(serde_json::json!({ "_id": -1 })));
            }
            _ => panic!("expected Mongo"),
        }
    }

    #[test]
    fn mongo_use_switches_database() {
        let text = "use('shop');\ndb.orders.find({})";
        match parse_query(Engine::Mongo, text).unwrap() {
            Query::Mongo(op) => {
                assert_eq!(op.database.as_deref(), Some("shop"));
                assert_eq!(op.collection, "orders");
            }
            _ => panic!("expected Mongo"),
        }
    }

    #[test]
    fn mongo_line_errors_are_readable() {
        assert!(parse_query(Engine::Mongo, "db.c.find({ bad json })").is_err());
        assert!(parse_query(Engine::Mongo, "db.c.drop()").is_err());
        assert!(parse_query(Engine::Mongo, "db..find({})").is_err());
    }

    #[test]
    fn mongo_non_db_text_still_parses_as_envelope() {
        let text = r#"{ "collection": "c", "op": "find", "body": {} }"#;
        match parse_query(Engine::Mongo, text).unwrap() {
            Query::Mongo(op) => assert_eq!(op.collection, "c"),
            _ => panic!("expected Mongo"),
        }
    }

    #[test]
    fn single_table_select_extracts_bare_table() {
        assert_eq!(
            single_table_name("select * from users where id = 1"),
            Some((None, "users".into()))
        );
        assert_eq!(
            single_table_name("SELECT id, name FROM \"Users\";"),
            Some((None, "Users".into()))
        );
        assert_eq!(
            single_table_name("  select id from orders order by id limit 10  "),
            Some((None, "orders".into()))
        );
    }

    #[test]
    fn single_table_select_extracts_schema_qualified_table() {
        assert_eq!(
            single_table_name("select * from oss_rba_common.step_journal where x = 1"),
            Some((Some("oss_rba_common".into()), "step_journal".into()))
        );
        assert_eq!(
            single_table_name("SELECT * FROM \"public\".\"Users\";"),
            Some((Some("public".into()), "Users".into()))
        );
    }

    #[test]
    fn single_table_select_ignores_commented_out_lines() {
        // Run sends the whole statement span, commented-out lines included.
        assert_eq!(
            single_table_name(
                "-- update oss_rba_perizinan.t_permohonan_izin\n\
                 -- set status_respon = '50'\n\
                 -- where id_permohonan_izin = 'I-1';\n\
                 select * from oss_rba_common.step_config ;"
            ),
            Some((Some("oss_rba_common".into()), "step_config".into()))
        );
        // A reject keyword that only appears inside a comment doesn't count.
        assert_eq!(
            single_table_name("-- select * from a join b\nselect * from users"),
            Some((None, "users".into()))
        );
        assert_eq!(single_table_name("-- select * from users"), None);
    }

    #[test]
    fn single_table_select_rejects_ambiguous_queries() {
        assert_eq!(
            single_table_name("select * from a join b on a.id = b.a_id"),
            None
        );
        assert_eq!(single_table_name("select * from a, b"), None);
        assert_eq!(
            single_table_name("select * from a union select * from b"),
            None
        );
        assert_eq!(single_table_name("select count(*) from users"), None);
        assert_eq!(single_table_name("select distinct name from users"), None);
        assert_eq!(single_table_name("select * from db.schema.users"), None);
        assert_eq!(single_table_name("select * from (select 1) t"), None);
        assert_eq!(single_table_name("insert into users values (1)"), None);
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
