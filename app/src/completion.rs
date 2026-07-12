//! SQL identifier autocomplete: given the text before the cursor and the
//! in-memory schema tree (`VmTreeNode` list), suggest keywords, table names, or
//! column names based on the SQL context. Columns resolve through a light
//! `FROM tbl alias` alias map so `alias.` offers that table's columns.

use crate::model::VmTreeNode;

/// SQL keywords offered when the cursor is at a statement/clause boundary.
const KEYWORDS: &[&str] = &[
    "SELECT", "FROM", "WHERE", "INSERT", "INTO", "VALUES", "UPDATE", "SET", "DELETE", "JOIN",
    "INNER", "LEFT", "RIGHT", "OUTER", "FULL", "ON", "AS", "GROUP", "ORDER", "BY", "HAVING",
    "LIMIT", "OFFSET", "DISTINCT", "AND", "OR", "NOT", "NULL", "IS", "IN", "LIKE", "BETWEEN",
    "ASC", "DESC", "COUNT", "CREATE", "TABLE", "ALTER", "DROP", "INDEX",
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
fn resolve_alias(stmt: &str, owner: &str) -> String {
    let ol = owner.to_lowercase();
    let words: Vec<&str> = stmt
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
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

/// The trailing run of identifier chars at the end of `s` (ASCII identifier).
pub fn trailing_word(s: &str) -> &str {
    let b = s.as_bytes();
    let mut i = b.len();
    while i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_') {
        i -= 1;
    }
    &s[i..]
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

/// Columns are the `field` nodes that follow a matching table node in the flat
/// tree (the sidebar stores parent-then-children order).
fn columns_of(nodes: &[VmTreeNode], owner: &str) -> Vec<Candidate> {
    let owner_l = owner.to_lowercase();
    for (i, n) in nodes.iter().enumerate() {
        if (n.kind == "table" || n.kind == "collection") && n.label.to_lowercase() == owner_l {
            return nodes[i + 1..]
                .iter()
                .take_while(|f| f.kind == "field")
                .map(|f| Candidate {
                    label: field_name(&f.label).to_string(),
                    kind: "field".into(),
                    sub: owner.to_string(),
                })
                .collect();
        }
    }
    Vec::new()
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
pub fn suggest(before_cursor: &str, nodes: &[VmTreeNode]) -> (usize, Vec<Candidate>) {
    let word = trailing_word(before_cursor);
    let head = before_cursor.strip_suffix(word).unwrap_or(before_cursor);
    let mut cands = if let Some(before_dot) = head.strip_suffix('.') {
        // `table.` / `alias.` → that table's columns.
        let owner = resolve_alias(before_cursor, trailing_word(before_dot));
        columns_of(nodes, &owner)
    } else {
        match last_keyword(before_cursor).as_deref() {
            // table position
            Some("FROM") | Some("JOIN") | Some("INTO") | Some("UPDATE") | Some("TABLE") => {
                tables(nodes)
            }
            // column position: offer columns (and tables, for `table.col`)
            Some("SELECT") | Some("WHERE") | Some("AND") | Some("OR") | Some("ON")
            | Some("HAVING") | Some("SET") | Some("BY") | Some("VALUES") => {
                let mut c = all_columns(nodes);
                c.extend(tables(nodes));
                c
            }
            // statement start / no useful context: keywords + tables
            _ => {
                let mut c = keywords();
                c.extend(tables(nodes));
                c
            }
        }
    };
    let wl = word.to_lowercase();
    if !wl.is_empty() {
        cands.retain(|c| c.label.to_lowercase().starts_with(&wl));
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
    fn from_prefix_suggests_matching_table() {
        let (n, c) = suggest("select * from ste", &nodes());
        assert_eq!(n, 3);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["step_config"]
        );
    }

    #[test]
    fn dot_suggests_that_tables_columns() {
        let (n, c) = suggest("select * from step_config where step_config.", &nodes());
        assert_eq!(n, 0);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["config_id", "name"]
        );
    }

    #[test]
    fn bare_word_completes_keyword() {
        let (n, c) = suggest("sele", &nodes());
        assert_eq!(n, 4);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["SELECT"]
        );
    }

    #[test]
    fn select_context_offers_columns() {
        let (_, c) = suggest("select ", &nodes());
        let labels: Vec<&str> = c.iter().map(|x| x.label.as_str()).collect();
        assert!(labels.contains(&"config_id"));
        assert!(labels.contains(&"name"));
        assert!(labels.contains(&"id"));
    }

    #[test]
    fn alias_dot_resolves_to_table_columns() {
        let (_, c) = suggest("select * from step_config sc where sc.", &nodes());
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["config_id", "name"]
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
        let (_, c) = suggest("select * from users where users.", &typed);
        assert_eq!(c[0].label, "id");
    }
}
