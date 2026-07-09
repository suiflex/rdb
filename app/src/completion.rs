//! SQL identifier autocomplete: given the text before the cursor and the
//! in-memory schema tree (`VmTreeNode` list), suggest table or column names.
//! Deliberately small — context is just "after a dot" (columns) or "after
//! FROM/JOIN/… or a partial word" (tables). No keyword completion.

use crate::model::VmTreeNode;

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub label: String,
    pub kind: String, // "table" | "field"
    pub sub: String,  // owning table, for column hints
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
                    label: f.label.clone(),
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
        columns_of(nodes, trailing_word(before_dot))
    } else {
        match last_keyword(before_cursor).as_deref() {
            Some("FROM") | Some("JOIN") | Some("INTO") | Some("UPDATE") | Some("TABLE") => {
                tables(nodes)
            }
            _ if !word.is_empty() => tables(nodes),
            _ => return (word.chars().count(), Vec::new()),
        }
    };
    let wl = word.to_lowercase();
    if !wl.is_empty() {
        cands.retain(|c| c.label.to_lowercase().starts_with(&wl));
    }
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
    fn bare_word_without_context_is_empty() {
        let (_, c) = suggest("sele", &nodes());
        // "sele" is a partial word → tables filtered by prefix (none match)
        assert!(c.is_empty());
    }
}
