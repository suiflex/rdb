//! SQL identifier autocomplete: given the text before the cursor and the
//! in-memory schema tree (`VmTreeNode` list), suggest keywords, table names, or
//! column names based on the SQL context. Columns resolve through a light
//! `FROM tbl alias` alias map so `alias.` offers that table's columns.

use crate::model::VmTreeNode;

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

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub label: String,
    pub kind: String, // "keyword" | "table" | "field"
    pub sub: String,  // owning table, for column hints
}

fn is_keyword(w: &str) -> bool {
    let u = w.to_uppercase();
    KEYWORDS.contains(&u.as_str())
}

/// Field nodes store `"name: type"` for the sidebar; completions insert the
/// bare column name only.
fn field_name(label: &str) -> &str {
    label.split(':').next().unwrap_or(label).trim()
}

fn keywords() -> Vec<Candidate> {
    KEYWORDS
        .iter()
        .map(|k| Candidate {
            label: (*k).to_string(),
            kind: "keyword".into(),
            sub: String::new(),
        })
        .collect()
}

/// mongosh collection methods, offered after `db.<collection>.` so Mongo
/// completion reaches parity with SQL column completion.
const MONGO_METHODS: &[&str] = &[
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

fn mongo_methods() -> Vec<Candidate> {
    MONGO_METHODS
        .iter()
        .map(|m| Candidate {
            label: (*m).to_string(),
            kind: "keyword".into(),
            sub: String::new(),
        })
        .collect()
}

/// Does `name` match a collection node (MongoDB) in the tree?
fn is_collection(nodes: &[VmTreeNode], name: &str) -> bool {
    let nl = name.to_lowercase();
    nodes
        .iter()
        .any(|n| n.kind == "collection" && n.label.to_lowercase() == nl)
}

/// Every column across the schema (deduped by name), for SELECT/WHERE contexts
/// where the owning table isn't yet known.
fn all_columns(nodes: &[VmTreeNode]) -> Vec<Candidate> {
    let mut seen = std::collections::HashSet::new();
    nodes
        .iter()
        .filter(|n| n.kind == "field")
        .filter_map(|n| {
            let name = field_name(&n.label).to_string();
            if seen.insert(name.to_lowercase()) {
                Some(Candidate {
                    label: name,
                    kind: "field".into(),
                    sub: String::new(),
                })
            } else {
                None
            }
        })
        .collect()
}

/// Map a table name or `FROM tbl alias` alias to its underlying table name.
/// `.` stays inside a token so a schema-qualified `schema.table alias` keeps the
/// table reference whole and the alias is read as the next token.
fn resolve_alias(stmt: &str, owner: &str) -> String {
    let ol = owner.to_lowercase();
    let words: Vec<&str> = stmt
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
        .filter(|w| !w.is_empty())
        .collect();
    for i in 0..words.len() {
        let w = words[i].to_uppercase();
        if (w == "FROM" || w == "JOIN") && i + 1 < words.len() {
            let table = words[i + 1];
            // `FROM tbl alias` — alias is the following non-keyword word.
            if let Some(alias) = words.get(i + 2) {
                if !is_keyword(alias) && alias.to_lowercase() == ol {
                    return table.to_string();
                }
            }
            if table.to_lowercase() == ol {
                return table.to_string();
            }
        }
    }
    owner.to_string()
}

/// How well `word` (already lowercased) matches `label`, lowest is best:
/// 0 = literal prefix, 1 = prefix of a `_`-delimited segment (`teknis` finds
/// `flag_teknis`), 2 = prefix once the underscores are squashed out of both
/// (`schemaoi` finds `schema_oi`). `None` when it doesn't match at all.
/// Doubles as the sort key so a fuzzier tier can't outrank a literal one.
fn match_rank(label: &str, word: &str) -> Option<u8> {
    let l = label.to_lowercase();
    if l.starts_with(word) {
        return Some(0);
    }
    if l.split('_').any(|seg| seg.starts_with(word)) {
        return Some(1);
    }
    if l.replace('_', "").starts_with(&word.replace('_', "")) {
        return Some(2);
    }
    None
}

/// The trailing run of identifier chars at the end of `s` (ASCII identifier).
pub fn trailing_word(s: &str) -> &str {
    let b = s.as_bytes();
    let mut i = b.len();
    while i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_') {
        i -= 1;
    }
    &s[i..]
}

/// Does `name` match a schema/database node in the tree? Used to treat
/// `schema.` as "list that schema's tables" rather than a table's columns.
fn is_database(nodes: &[VmTreeNode], name: &str) -> bool {
    let nl = name.to_lowercase();
    nodes
        .iter()
        .any(|n| n.kind == "database" && n.label.to_lowercase() == nl)
}

/// The nodes belonging to `schema`: from its `database` node up to the next
/// `database` node (the tree is flat, parent-then-children). An empty schema or
/// no match falls back to the whole tree so completion still works.
fn schema_scope<'a>(nodes: &'a [VmTreeNode], schema: &str) -> &'a [VmTreeNode] {
    if schema.is_empty() {
        return nodes;
    }
    let sl = schema.to_lowercase();
    let Some(start) = nodes
        .iter()
        .position(|n| n.kind == "database" && n.label.to_lowercase() == sl)
    else {
        return nodes;
    };
    let body = &nodes[start + 1..];
    let end = body
        .iter()
        .position(|n| n.kind == "database")
        .unwrap_or(body.len());
    &body[..end]
}

fn tables(nodes: &[VmTreeNode]) -> Vec<Candidate> {
    nodes
        .iter()
        .filter(|n| n.kind == "table" || n.kind == "collection")
        .map(|n| Candidate {
            label: n.label.clone(),
            kind: "table".into(),
            sub: String::new(),
        })
        .collect()
}

/// Schema/database names, offered in table position so a cross-schema
/// `schema.table` name can be started (their tables aren't in the active scope).
fn schemas(nodes: &[VmTreeNode]) -> Vec<Candidate> {
    nodes
        .iter()
        .filter(|n| n.kind == "database")
        .map(|n| Candidate {
            label: n.label.clone(),
            kind: "database".into(),
            sub: String::new(),
        })
        .collect()
}

/// Columns are the `field` nodes that follow a matching table node in the flat
/// tree (the sidebar stores parent-then-children order).
fn columns_of(nodes: &[VmTreeNode], owner: &str) -> Vec<Candidate> {
    // Tree labels are bare table names; a schema-qualified owner
    // (`schema.table`) matches on its last segment.
    let owner_l = owner.rsplit('.').next().unwrap_or(owner).to_lowercase();
    for (i, n) in nodes.iter().enumerate() {
        if (n.kind == "table" || n.kind == "collection") && n.label.to_lowercase() == owner_l {
            return nodes[i + 1..]
                .iter()
                .take_while(|f| f.kind == "field")
                .map(|f| Candidate {
                    label: field_name(&f.label).to_string(),
                    kind: "field".into(),
                    // Bare table name only — the schema prefix just crowds the
                    // row and pushes the field label into an ellipsis.
                    sub: owner.rsplit('.').next().unwrap_or(owner).to_string(),
                })
                .collect();
        }
    }
    Vec::new()
}

/// Columns of every table named by a `FROM`/`JOIN` in the current statement, so
/// a `WHERE`/`SELECT` completion offers the real columns in scope — including
/// cross-schema tables the active-schema `all_columns` would miss. `stmt` is the
/// whole statement under the cursor, not just the text before it: the `FROM` is
/// usually already written when the user goes back to replace the `SELECT *`.
fn from_table_columns(stmt: &str, nodes: &[VmTreeNode]) -> Vec<Candidate> {
    let words: Vec<&str> = stmt
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.'))
        .filter(|w| !w.is_empty())
        .collect();
    let mut cols = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for i in 0..words.len() {
        let w = words[i].to_uppercase();
        if (w == "FROM" || w == "JOIN") && i + 1 < words.len() {
            for c in columns_of(nodes, words[i + 1]) {
                if seen.insert(c.label.to_lowercase()) {
                    cols.push(c);
                }
            }
        }
    }
    cols
}

/// The last SQL keyword token on `line`, uppercased (via the editor lexer).
fn last_keyword(line: &str) -> Option<String> {
    crate::editor::lex_line(line)
        .into_iter()
        .rev()
        .find(|s| s.kind == 1)
        .map(|s| s.text.to_uppercase())
}

/// Suggest completions for the text before the cursor. Returns the char length
/// of the partial word to replace on accept, plus the (prefix-filtered, capped)
/// candidates. An empty list means "no popup".
/// `stmt` is the whole statement under the cursor (text on both sides of it);
/// it resolves `FROM`/`JOIN` tables and aliases that the user hasn't typed
/// *yet* at this point but has already written further along the statement.
pub fn suggest(
    before_cursor: &str,
    stmt: &str,
    nodes: &[VmTreeNode],
    active_schema: &str,
    mongo: bool,
) -> (usize, Vec<Candidate>) {
    // Default table/column suggestions come from the active schema only, so the
    // popup follows the connected schema without the user picking it first.
    let scope = schema_scope(nodes, active_schema);
    let word = trailing_word(before_cursor);
    let head = before_cursor.strip_suffix(word).unwrap_or(before_cursor);
    // Type-triggered: don't pop up on an empty line or right after whitespace —
    // only once the user has typed at least one char of a word. `table.`/`alias.`
    // is an explicit request for that table's columns, so it still fires.
    if word.is_empty() && !head.ends_with('.') {
        return (0, Vec::new());
    }
    let mut cands = if let Some(before_dot) = head.strip_suffix('.') {
        // `table.` / `alias.` → that table's columns. When the name before the
        // dot is a schema/database, offer that schema's tables instead.
        // Explicit `schema.` uses the whole tree so other schemas stay reachable.
        let owner_word = trailing_word(before_dot);
        let owner = resolve_alias(stmt, owner_word);
        let cols = columns_of(nodes, &owner);
        if !cols.is_empty() {
            cols
        } else if mongo && owner_word.eq_ignore_ascii_case("db") {
            // MongoDB: `db.` is the current database — offer its collections.
            tables(scope)
        } else if mongo && is_collection(nodes, owner_word) {
            // MongoDB: `db.<collection>.` — offer collection methods.
            mongo_methods()
        } else if is_database(nodes, owner_word) {
            tables(schema_scope(nodes, owner_word))
        } else {
            cols
        }
    } else if mongo {
        // MongoDB has no SQL keywords: offer the `db` handle and the active
        // database's collections instead of DELETE/SELECT/… noise.
        let mut c = vec![Candidate {
            label: "db".into(),
            kind: "keyword".into(),
            sub: String::new(),
        }];
        c.extend(tables(scope));
        c
    } else {
        // Keyword context comes from the current line; `before_cursor` may span
        // several lines (alias resolution needs the whole statement).
        let cur_line = before_cursor.rsplit('\n').next().unwrap_or(before_cursor);
        match last_keyword(cur_line).as_deref() {
            // table position: active-schema tables plus every schema name, so a
            // `schema.table` from another namespace can be started.
            Some("FROM") | Some("JOIN") | Some("INTO") | Some("UPDATE") | Some("TABLE") => {
                let mut c = tables(scope);
                c.extend(schemas(nodes));
                c
            }
            // column position: offer columns and tables, plus keywords so the
            // next clause (FROM/WHERE/…) is always reachable, e.g. after `*`.
            Some("SELECT") | Some("WHERE") | Some("AND") | Some("OR") | Some("ON")
            | Some("HAVING") | Some("SET") | Some("BY") | Some("VALUES") => {
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
    };
    let wl = word.to_lowercase();
    if !wl.is_empty() {
        cands.retain(|c| match_rank(&c.label, &wl).is_some());
        // Literal prefixes rank above `_`-segment matches, which rank above
        // underscore-squashed ones, so the most literal completion stays on
        // top; the sort is stable, so order within a tier is unchanged.
        cands.sort_by_key(|c| match_rank(&c.label, &wl).unwrap_or(u8::MAX));
    }
    // dedup by label (a column name may appear across tables), keep first.
    let mut seen = std::collections::HashSet::new();
    cands.retain(|c| seen.insert((c.kind.clone(), c.label.to_lowercase())));
    cands.truncate(20);
    (word.chars().count(), cands)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The common case: nothing typed past the cursor yet, so the statement
    /// and the before-cursor text are the same. Tests that care about text
    /// *after* the cursor call `suggest` directly.
    fn sug(
        before: &str,
        nodes: &[VmTreeNode],
        active_schema: &str,
        mongo: bool,
    ) -> (usize, Vec<Candidate>) {
        suggest(before, before, nodes, active_schema, mongo)
    }

    fn nodes() -> Vec<VmTreeNode> {
        let mk = |l: &str, k: &str| VmTreeNode {
            label: l.into(),
            kind: k.into(),
        };
        vec![
            mk("public", "database"),
            mk("step_config", "table"),
            mk("config_id", "field"),
            mk("name", "field"),
            mk("users", "table"),
            mk("id", "field"),
        ]
    }

    #[test]
    fn empty_and_whitespace_context_suppress_popup() {
        assert!(sug("", &nodes(), "public", false).1.is_empty());
        assert!(sug("   ", &nodes(), "public", false).1.is_empty());
        // trailing space after a keyword: wait for the user to start typing
        assert!(sug("select ", &nodes(), "public", false).1.is_empty());
    }

    #[test]
    fn from_prefix_suggests_matching_table() {
        let (n, c) = sug("select * from ste", &nodes(), "public", false);
        assert_eq!(n, 3);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["step_config"]
        );
    }

    #[test]
    fn dot_suggests_that_tables_columns() {
        let (n, c) = sug(
            "select * from step_config where step_config.",
            &nodes(),
            "public",
            false,
        );
        assert_eq!(n, 0);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["config_id", "name"]
        );
    }

    #[test]
    fn mongo_db_dot_suggests_collections() {
        // MongoDB: `db.` is the current database, so it must surface the active
        // schema's collections even though `db` is not a schema node.
        let mut ns = nodes();
        ns.push(VmTreeNode {
            label: "log_inbound".into(),
            kind: "collection".into(),
        });
        let (n, c) = sug("db.log", &ns, "public", true);
        assert_eq!(n, 3);
        assert!(c.iter().any(|x| x.label == "log_inbound"));
    }

    #[test]
    fn mongo_collection_dot_suggests_methods() {
        // `db.<collection>.` offers mongosh methods (parity with SQL columns).
        let mut ns = nodes();
        ns.push(VmTreeNode {
            label: "log_inbound".into(),
            kind: "collection".into(),
        });
        let (_, c) = sug("db.log_inbound.fi", &ns, "public", true);
        assert!(c.iter().any(|x| x.label == "find"));
        assert!(c.iter().any(|x| x.label == "findOne"));
    }

    #[test]
    fn mongo_bare_word_suggests_db_not_sql_keywords() {
        // On MongoDB, typing a letter must offer `db` and collections — never SQL
        // keywords like DELETE.
        let mut ns = nodes();
        ns.push(VmTreeNode {
            label: "log_inbound".into(),
            kind: "collection".into(),
        });
        let (_, c) = sug("d", &ns, "public", true);
        assert!(c.iter().any(|x| x.label == "db"));
        assert!(!c.iter().any(|x| x.label == "DELETE"));
    }

    /// Typing a mid-word `_` segment finds the identifier, and a true prefix
    /// still ranks above the subword hit.
    #[test]
    fn subword_matches_underscore_segment() {
        let n = vec![
            VmTreeNode {
                label: "public".into(),
                kind: "database".into(),
            },
            VmTreeNode {
                label: "licenses".into(),
                kind: "table".into(),
            },
            VmTreeNode {
                label: "flag_teknis".into(),
                kind: "field".into(),
            },
            VmTreeNode {
                label: "teknis_id".into(),
                kind: "field".into(),
            },
        ];
        let (_, c) = sug("select teknis", &n, "public", false);
        let labels: Vec<&str> = c.iter().map(|x| x.label.as_str()).collect();
        assert!(labels.contains(&"flag_teknis"));
        // `teknis_id` is a real prefix → ranks ahead of the mid-word match.
        assert_eq!(labels.first(), Some(&"teknis_id"));
    }

    /// WHERE on a cross-schema table (not the active schema) still offers that
    /// table's columns, resolved from the statement's FROM clause.
    #[test]
    fn where_offers_from_table_columns_cross_schema() {
        let mk = |l: &str, k: &str| VmTreeNode {
            label: l.into(),
            kind: k.into(),
        };
        let two = vec![
            mk("public", "database"),
            mk("users", "table"),
            mk("id", "field"),
            mk("oss_rba_common", "database"),
            mk("step_journal", "table"),
            mk("step_id", "field"),
            mk("journal_id", "field"),
        ];
        // Active schema is public; the query reads oss_rba_common.step_journal.
        let (_, c) = sug(
            "select * from oss_rba_common.step_journal where step",
            &two,
            "public",
            false,
        );
        let labels: Vec<&str> = c.iter().map(|x| x.label.as_str()).collect();
        assert!(labels.contains(&"step_id"));
        // A prior statement's tables can't leak in: the caller scopes `stmt` to
        // the statement under the cursor (`EditorState::current_statement`).
        let (_, c2) = suggest(
            "select * from users;\nselect * from oss_rba_common.step_journal where step",
            "select * from oss_rba_common.step_journal where step",
            &two,
            "public",
            false,
        );
        assert!(c2.iter().any(|x| x.label == "step_id"));
        assert!(!c2.iter().any(|x| x.label == "id"));
    }

    /// Going back to replace the `*` in an already-written
    /// `select * from schema.table`: the FROM is *after* the cursor, so the
    /// columns only resolve if the whole statement is consulted.
    #[test]
    fn select_offers_columns_from_a_from_after_the_cursor() {
        let mk = |l: &str, k: &str| VmTreeNode {
            label: l.into(),
            kind: k.into(),
        };
        let two = vec![
            mk("public", "database"),
            mk("users", "table"),
            mk("id", "field"),
            mk("oss_rba_common", "database"),
            mk("step_config", "table"),
            mk("config_id", "field"),
            mk("taint", "field"),
        ];
        let (n, c) = suggest(
            "select con",
            "select con from oss_rba_common.step_config",
            &two,
            "public",
            false,
        );
        assert_eq!(n, 3);
        assert_eq!(c.first().map(|x| x.label.as_str()), Some("config_id"));
    }

    /// Same blind spot for an alias declared further along the statement.
    #[test]
    fn alias_dot_resolves_an_alias_declared_after_the_cursor() {
        let (_, c) = suggest(
            "select a.",
            "select a. from step_config a",
            &nodes(),
            "public",
            false,
        );
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["config_id", "name"]
        );
    }

    #[test]
    fn matching_ignores_missing_underscores() {
        let mk = |l: &str, k: &str| VmTreeNode {
            label: l.into(),
            kind: k.into(),
        };
        let n = vec![
            mk("oss_rba_common", "database"),
            mk("step_config", "table"),
            mk("config_id", "field"),
        ];
        // Schema name typed without its underscores.
        let (_, c) = sug("select * from ossrbacommon", &n, "oss_rba_common", false);
        assert!(c.iter().any(|x| x.label == "oss_rba_common"));
        // Not schemas only — tables and columns too.
        let (_, c) = sug("select * from stepconfig", &n, "oss_rba_common", false);
        assert!(c.iter().any(|x| x.label == "step_config"));
        let (_, c) = sug(
            "select * from step_config where configid",
            &n,
            "oss_rba_common",
            false,
        );
        assert!(c.iter().any(|x| x.label == "config_id"));
    }

    #[test]
    fn literal_prefix_outranks_an_underscore_squashed_match() {
        let mk = |l: &str, k: &str| VmTreeNode {
            label: l.into(),
            kind: k.into(),
        };
        let n = vec![
            mk("public", "database"),
            mk("stepconfig", "table"),
            mk("step_config", "table"),
        ];
        let (_, c) = sug("select * from stepc", &n, "public", false);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["stepconfig", "step_config"]
        );
    }

    #[test]
    fn bare_word_completes_keyword() {
        let (n, c) = sug("sele", &nodes(), "public", false);
        assert_eq!(n, 4);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["SELECT"]
        );
    }

    #[test]
    fn select_context_offers_columns() {
        // Type-triggered: a prefix is required before the popup appears.
        let (_, c) = sug("select n", &nodes(), "public", false);
        let labels: Vec<&str> = c.iter().map(|x| x.label.as_str()).collect();
        assert!(labels.contains(&"name"));
    }

    #[test]
    fn alias_dot_resolves_to_table_columns() {
        let (_, c) = sug(
            "select * from step_config sc where sc.",
            &nodes(),
            "public",
            false,
        );
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["config_id", "name"]
        );
    }

    /// Schema-qualified table with an alias inside a JOIN: each alias resolves
    /// to its own table's columns (regression for empty suggestions on joins).
    #[test]
    fn schema_qualified_alias_join_resolves_columns() {
        let a = "select * from public.step_config a left join public.users b on a.";
        let (_, c) = sug(a, &nodes(), "public", false);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["config_id", "name"]
        );
        let b = "select * from public.step_config a left join public.users b on b.";
        let (_, c) = sug(b, &nodes(), "public", false);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["id"]
        );
    }

    /// Alias declared on an earlier line still resolves when completing a
    /// later line (before_cursor spans the whole statement).
    #[test]
    fn multiline_join_alias_resolves_columns() {
        let sql = "select * from public.step_config a\nleft join public.users b on a.";
        let (_, c) = sug(sql, &nodes(), "public", false);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["config_id", "name"]
        );
    }

    #[test]
    fn star_context_offers_from_keyword() {
        let (_, c) = sug("select * f", &nodes(), "public", false);
        let labels: Vec<&str> = c.iter().map(|x| x.label.as_str()).collect();
        assert!(labels.contains(&"FROM"));
    }

    #[test]
    fn schema_dot_suggests_its_tables() {
        let (n, c) = sug("select * from public.", &nodes(), "public", false);
        assert_eq!(n, 0);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["step_config", "users"]
        );
    }

    #[test]
    fn column_insert_strips_type_annotation() {
        let typed = vec![
            VmTreeNode {
                label: "users".into(),
                kind: "table".into(),
            },
            VmTreeNode {
                label: "id: int4".into(),
                kind: "field".into(),
            },
        ];
        let (_, c) = sug("select * from users where users.", &typed, "public", false);
        assert_eq!(c[0].label, "id");
    }

    /// Two schemas loaded: table suggestions follow the active schema only.
    #[test]
    fn scopes_tables_to_active_schema() {
        let mk = |l: &str, k: &str| VmTreeNode {
            label: l.into(),
            kind: k.into(),
        };
        let two = vec![
            mk("public", "database"),
            mk("t_users", "table"),
            mk("id", "field"),
            mk("other", "database"),
            mk("t_orders", "table"),
            mk("oid", "field"),
        ];
        // Type-triggered: a prefix ("t") is required before the popup appears.
        let (_, c) = sug("select * from t", &two, "public", false);
        let labels: Vec<&str> = c.iter().map(|x| x.label.as_str()).collect();
        assert!(labels.contains(&"t_users"));
        assert!(!labels.contains(&"t_orders"));

        let (_, c) = sug("select * from t", &two, "other", false);
        let labels: Vec<&str> = c.iter().map(|x| x.label.as_str()).collect();
        assert!(labels.contains(&"t_orders"));
        assert!(!labels.contains(&"t_users"));
    }

    fn two_schema_nodes() -> Vec<VmTreeNode> {
        let mk = |l: &str, k: &str| VmTreeNode {
            label: l.into(),
            kind: k.into(),
        };
        vec![
            mk("public", "database"),
            mk("users", "table"),
            mk("id", "field"),
            mk("analytics", "database"),
            mk("step_config", "table"),
            mk("cfg_id", "field"),
        ]
    }

    /// In table position, every schema name is offered so a cross-schema
    /// `schema.table` can be started even when it isn't the active schema.
    #[test]
    fn from_offers_schema_names() {
        // Type-triggered: "a" narrows to the analytics schema name.
        let (_, c) = sug("select * from a", &two_schema_nodes(), "public", false);
        let labels: Vec<&str> = c.iter().map(|x| x.label.as_str()).collect();
        assert!(labels.contains(&"analytics"));
    }

    /// `otherschema.` lists that schema's tables even while another schema is
    /// active (all schemas live in the completion tree).
    #[test]
    fn dot_lists_non_active_schema_tables() {
        let (n, c) = sug(
            "select * from analytics.",
            &two_schema_nodes(),
            "public",
            false,
        );
        assert_eq!(n, 0);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["step_config"]
        );
    }
}
