//! Identifier autocomplete: given the text before the cursor and the
//! in-memory schema tree (`VmTreeNode` list), suggest keywords, table names,
//! or column names based on the query context. Columns resolve through a
//! light `FROM tbl alias` alias map so `alias.` offers that table's columns.
//!
//! Dispatch is per-`QueryLanguage` (see the `sql`/`cql`/`mongo`/`command`
//! submodules) — each dialect owns its own keyword set and clause-position
//! logic; this file keeps only the tree helpers they share (table/column
//! lookup, fuzzy ranking) plus the top-level `suggest` dispatcher.

use crate::model::VmTreeNode;

pub mod command;
pub mod cql;
pub mod mongo;
pub mod sql;

#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub label: String,
    pub kind: String, // "keyword" | "table" | "field"
    pub sub: String,  // owning table, for column hints
}

/// Field nodes store `"name: type"` for the sidebar; completions insert the
/// bare column name only.
fn field_name(label: &str) -> &str {
    label.split(':').next().unwrap_or(label).trim()
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
fn resolve_alias(stmt: &str, owner: &str, language: rdb_connstore::QueryLanguage) -> String {
    for (table, alias) in table_refs(stmt, language) {
        if table.eq_ignore_ascii_case(owner)
            || table
                .rsplit('.')
                .next()
                .is_some_and(|t| t.eq_ignore_ascii_case(owner))
        {
            return table.to_string();
        }
        if alias.is_some_and(|alias| alias.eq_ignore_ascii_case(owner)) {
            return table.to_string();
        }
    }
    owner.to_string()
}

fn table_tokens(stmt: &str) -> Vec<&str> {
    let mut tokens = Vec::new();
    let mut start = None;
    for (i, ch) in stmt.char_indices() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '.' {
            start.get_or_insert(i);
            continue;
        }
        if let Some(s) = start.take() {
            tokens.push(&stmt[s..i]);
        }
        if ch == ',' {
            tokens.push(",");
        }
    }
    if let Some(s) = start {
        tokens.push(&stmt[s..]);
    }
    tokens
}

fn table_refs(stmt: &str, language: rdb_connstore::QueryLanguage) -> Vec<(&str, Option<&str>)> {
    let words = table_tokens(stmt);
    let mut refs = Vec::new();
    let mut i = 0;
    while i < words.len() {
        if !(words[i].eq_ignore_ascii_case("FROM") || words[i].eq_ignore_ascii_case("JOIN")) {
            i += 1;
            continue;
        }
        // Resume right after the last token this clause consumed, instead of
        // rescanning table/alias tokens table_list already read.
        let (list, next) = table_list(&words, i + 1, language);
        refs.extend(list);
        i = next;
    }
    refs
}

/// Every `table [AS] alias` entry starting at `start`, up to the next clause
/// keyword. Returns the entries plus the index to resume scanning at.
fn table_list<'a>(
    words: &[&'a str],
    start: usize,
    language: rdb_connstore::QueryLanguage,
) -> (Vec<(&'a str, Option<&'a str>)>, usize) {
    let mut refs = Vec::new();
    let mut j = start;
    while let Some((table, alias, next)) = next_table_ref(words, j, language) {
        refs.push((table, alias));
        j = next;
    }
    (refs, j)
}

/// Read one `table [AS] alias` entry at or after `j`, skipping a leading
/// comma. Returns the ref plus the index to resume at, or `None` once `j`
/// hits a keyword (end of the table list) or the end of `words`.
fn next_table_ref<'a>(
    words: &[&'a str],
    mut j: usize,
    language: rdb_connstore::QueryLanguage,
) -> Option<(&'a str, Option<&'a str>, usize)> {
    while words.get(j) == Some(&",") {
        j += 1;
    }
    let table = *words.get(j)?;
    if is_keyword_for(language, table) {
        return None;
    }
    let mut k = j + 1;
    if words.get(k).is_some_and(|w| w.eq_ignore_ascii_case("AS")) {
        k += 1;
    }
    let alias = words
        .get(k)
        .copied()
        .filter(|w| *w != "," && !is_keyword_for(language, w));
    let next = if alias.is_some() { k + 1 } else { k };
    Some((table, alias, next))
}

fn is_keyword_for(language: rdb_connstore::QueryLanguage, w: &str) -> bool {
    match language {
        rdb_connstore::QueryLanguage::Cql => cql::is_keyword(&w.to_uppercase()),
        _ => sql::is_keyword(w),
    }
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
    // Subsequence: the typed chars appear in order but not contiguously, so a
    // long name can be reached by skipping through it (`t_invl` finds
    // `t_invoice_line`). Last resort — every prefix tier outranks it.
    if is_subsequence(&l, word) {
        return Some(3);
    }
    // A schema-qualified label (`schema.table`) is matched on its table part
    // too, so a cross-schema table is reachable by its own name. Ranked below
    // the same tier on a bare label: an in-scope table wins the slot.
    if let Some((_, table)) = l.split_once('.') {
        if let Some(r) = match_rank(table, word) {
            return Some(r + 4);
        }
    }
    None
}

/// Are `word`'s chars present in `label`, in order but not necessarily
/// adjacent? Both are already lowercased.
fn is_subsequence(label: &str, word: &str) -> bool {
    let mut w = word.chars().peekable();
    for c in label.chars() {
        if w.peek() == Some(&c) {
            w.next();
        }
    }
    w.peek().is_none()
}

/// Like `trailing_word`, but keeps a `schema.` qualifier attached. `table.`
/// completion needs the whole dotted path: dropping the qualifier is what let
/// a table name repeated across schemas resolve to the wrong one.
fn trailing_path(s: &str) -> &str {
    let b = s.as_bytes();
    let mut i = b.len();
    while i > 0 && (b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_' || b[i - 1] == b'.') {
        i -= 1;
    }
    &s[i..]
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

/// Every table outside the active schema, labelled `schema.table` so it is
/// insertable as-is. The active schema's own tables are already offered bare
/// by `tables(scope)`, so they are skipped here rather than listed twice.
fn qualified_tables(nodes: &[VmTreeNode], active_schema: &str) -> Vec<Candidate> {
    let active = active_schema.to_lowercase();
    nodes
        .iter()
        .filter(|n| n.kind == "database" && n.label.to_lowercase() != active)
        .flat_map(|db| {
            schema_scope(nodes, &db.label)
                .iter()
                .filter(|n| n.kind == "table" || n.kind == "collection")
                .map(|t| Candidate {
                    label: format!("{}.{}", db.label, t.label),
                    kind: "table".into(),
                    sub: db.label.clone(),
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Columns are the `field` nodes that follow a matching table node in the flat
/// tree (the sidebar stores parent-then-children order).
fn columns_of(nodes: &[VmTreeNode], owner: &str) -> Vec<Candidate> {
    // Tree labels are bare table names; a schema-qualified owner
    // (`schema.table`) matches on its last segment.
    let owner_l = owner.rsplit('.').next().unwrap_or(owner).to_lowercase();
    // ...but the search itself has to stay inside the named schema, otherwise
    // the first table with that bare name anywhere in the tree wins and a
    // table name repeated across schemas answers with the wrong columns.
    let nodes = match owner.rsplit_once('.') {
        Some((schema, _)) if is_database(nodes, schema) => schema_scope(nodes, schema),
        _ => nodes,
    };
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
fn from_table_columns(
    stmt: &str,
    nodes: &[VmTreeNode],
    language: rdb_connstore::QueryLanguage,
) -> (bool, Vec<Candidate>) {
    let refs = table_refs(stmt, language);
    let mut seen = std::collections::HashSet::new();
    let cols = refs
        .iter()
        .flat_map(|(table, _)| columns_of(nodes, table))
        .filter(|c| seen.insert(c.label.to_lowercase()))
        .collect();
    (!refs.is_empty(), cols)
}

/// The last keyword token in `text` for `language`, uppercased (via the editor
/// lexer, so it agrees with what's actually highlighted). `text` is the
/// statement so far, not one line: a formatted query puts the clause keyword on
/// a line of its own, and reading only the cursor's line reported no clause at
/// all — the same query typed on one line completed differently.
fn last_keyword(text: &str, language: rdb_connstore::QueryLanguage) -> Option<String> {
    text.lines()
        .rev()
        .find_map(|line| {
            crate::editor::lex_line(language, line)
                .into_iter()
                .rev()
                .find(|s| s.kind == 1)
        })
        .map(|s| s.text.to_uppercase())
}

/// The current statement's text up to the cursor. `before_cursor` spans the
/// whole document, so everything before the last `;` belongs to statements the
/// clause context must not inherit.
fn stmt_before_cursor(before_cursor: &str) -> &str {
    // ponytail: naive `;` split, same rigor as `in_literal_or_comment` above —
    // a semicolon inside a string literal ends the context early. Upgrade to
    // the editor's `statement_bounds` if that ever bites.
    before_cursor
        .rsplit_once(';')
        .map(|(_, tail)| tail)
        .unwrap_or(before_cursor)
}

/// Whether the cursor is in a table-name position (after FROM, JOIN, INTO,
/// UPDATE, or TABLE). Used to decide whether to auto-append an alias.
/// True when the cursor is inside a quoted literal or a line comment.
fn in_literal_or_comment(line: &str) -> bool {
    let mut quote = false;
    let chars: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if !quote && chars[i] == '-' && chars.get(i + 1) == Some(&'-') {
            return true;
        }
        if chars[i] == '\'' {
            if quote && chars.get(i + 1) == Some(&'\'') {
                i += 2;
                continue;
            }
            quote = !quote;
        }
        i += 1;
    }
    quote
}

/// LIMIT/OFFSET arguments are values, not identifiers. Suppress completion
/// while typing their numeric or quoted argument.
fn sql_clause_argument(line: &str) -> bool {
    // `LIMIT`/`OFFSET` need not be the first token (`SELECT … LIMIT 10`).
    // The lexer gives us the actual last keyword, so quoted words and comments
    // cannot accidentally trigger this guard.
    matches!(
        last_keyword(line, rdb_connstore::QueryLanguage::Sql).as_deref(),
        Some("LIMIT") | Some("OFFSET")
    ) && line
        .rsplit_once(char::is_whitespace)
        .is_some_and(|(_, value)| !value.trim().is_empty())
}

pub fn is_table_position(line: &str, language: rdb_connstore::QueryLanguage) -> bool {
    matches!(
        last_keyword(line, language).as_deref(),
        Some("FROM") | Some("JOIN") | Some("INTO") | Some("UPDATE") | Some("TABLE")
    )
}

/// Generate a short alias from a table name by taking the first letter of
/// each underscore-delimited segment. `users` → `u`, `order_items` → `oi`.
pub fn generate_alias(table: &str) -> String {
    let mut alias = String::new();
    for part in table.split('_') {
        if let Some(ch) = part.chars().next() {
            alias.push(ch.to_ascii_lowercase());
        }
    }
    if alias.is_empty() {
        String::new()
    } else {
        alias
    }
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
    language: rdb_connstore::QueryLanguage,
) -> (usize, Vec<Candidate>) {
    // Redis has no table/column tree or dot-completion — dispatch entirely to
    // its own line-based command completion.
    if language == rdb_connstore::QueryLanguage::Command {
        return command::suggest(before_cursor);
    }
    // Default table/column suggestions come from the active schema only, so the
    // popup follows the connected schema without the user picking it first.
    let scope = schema_scope(nodes, active_schema);
    let is_mongo = language == rdb_connstore::QueryLanguage::Mongo;
    let mut word = trailing_word(before_cursor);
    let mut head = before_cursor.strip_suffix(word).unwrap_or(before_cursor);
    // Mongo operator/stage names are `$`-prefixed, but `$` isn't an identifier
    // char, so `trailing_word` stops right after it — extend the word (and
    // its replace length) back over the `$` so matching/insertion land on
    // the whole `$eq`-style token instead of duplicating the `$`.
    if is_mongo && head.ends_with('$') {
        word = &before_cursor[before_cursor.len() - word.len() - 1..];
        head = before_cursor.strip_suffix(word).unwrap_or(before_cursor);
    }
    // Never complete inside a string/comment. This matters especially for
    // values after LIMIT/OFFSET and other clause arguments: schema names are
    // not useful there and make the popup fight normal text entry.
    let cur_line = before_cursor.rsplit('\n').next().unwrap_or(before_cursor);
    if in_literal_or_comment(cur_line) || sql_clause_argument(cur_line) {
        return (0, Vec::new());
    }
    // Type-triggered: don't pop up on an empty line or right after whitespace —
    // only once the user has typed at least one char of a word. `table.`/`alias.`
    // (or a bare Mongo `$`) is an explicit request, so it still fires.
    if word.is_empty() && !head.ends_with('.') {
        return (0, Vec::new());
    }
    let cands = if let Some(before_dot) = head.strip_suffix('.') {
        // `table.` / `alias.` → that table's columns. When the name before the
        // dot is a schema/database, offer that schema's tables instead.
        // Explicit `schema.` uses the whole tree so other schemas stay reachable.
        let owner_word = trailing_word(before_dot);
        // The qualified path, so `schema.table.` keeps its schema; identical to
        // `owner_word` for a bare table or alias.
        let owner = resolve_alias(stmt, trailing_path(before_dot), language);
        // MongoDB's `db.` / `db.<collection>.` shapes are unambiguous and must
        // win over column completion: a collection's sampled fields (from
        // Driver::sample_fields) would otherwise satisfy the `!cols.is_empty()`
        // check below and hide the method list.
        if is_mongo && owner_word.eq_ignore_ascii_case("db") {
            // MongoDB: `db.` is the current database — offer its collections.
            tables(scope)
        } else if is_mongo && mongo::is_collection(nodes, owner_word) {
            // MongoDB: `db.<collection>.` — offer collection methods.
            mongo::methods()
        } else if is_mongo
            && owner_word.is_empty()
            && matches!(mongo::call_context(before_dot), Some(ctx) if ctx.depth == 0)
        {
            // MongoDB: `db.coll.find().` — the call closed (every `{`/`[` it
            // opened is matched), so offer chained modifiers instead of
            // falling through to column completion (there's no owner name
            // right before this dot to look up columns for anyway).
            mongo::chain_modifiers()
        } else {
            let cols = columns_of(nodes, &owner);
            if !cols.is_empty() {
                cols
            } else if is_database(nodes, owner_word) {
                tables(schema_scope(nodes, owner_word))
            } else {
                cols
            }
        }
    } else if is_mongo {
        match mongo::call_context(before_cursor) {
            Some(ctx) if ctx.depth >= 1 => mongo::in_call(&ctx, nodes),
            _ => mongo::bare_word(scope),
        }
    } else {
        // Keyword context comes from the statement so far, so a clause keyword
        // on an earlier line still counts; `stmt` stays the whole statement
        // (alias resolution needs the text after the cursor too).
        let head = stmt_before_cursor(before_cursor);
        match language {
            rdb_connstore::QueryLanguage::Cql => cql::bare_word(head, stmt, nodes, scope),
            _ => sql::bare_word(head, stmt, nodes, scope, active_schema),
        }
    };
    rank_and_cap(cands, word)
}

/// Prefix-filter, rank, dedup and cap a candidate list, and report how much of
/// the partial word the caller must replace on accept. Shared by `suggest` and
/// the filter-box entry points so every popup ranks the same way.
fn rank_and_cap(mut cands: Vec<Candidate>, word: &str) -> (usize, Vec<Candidate>) {
    let wl = word.to_lowercase();
    if !wl.is_empty() {
        let ranked: Vec<_> = cands
            .into_iter()
            .filter_map(|c| match_rank(&c.label, &wl).map(|r| (r, c)))
            .collect();
        let Some(best_rank) = ranked.iter().map(|(r, _)| *r).min() else {
            return (word.chars().count(), Vec::new());
        };
        cands = ranked
            .into_iter()
            .filter_map(|(r, c)| (r == best_rank).then_some(c))
            .collect();
    }
    // dedup by label (a column name may appear across tables), keep first.
    let mut seen = std::collections::HashSet::new();
    cands.retain(|c| seen.insert((c.kind.clone(), c.label.to_lowercase())));
    cands.truncate(20);
    (word.chars().count(), cands)
}

/// Completions for the Compass-style Mongo filter box.
///
/// That box holds a bare filter *document* (`{ "status": { $in: [...] } }`),
/// not a shell expression, so `suggest` is the wrong entry point: its
/// `in_literal_or_comment` guard sees the unterminated quote of a key being
/// typed and correctly refuses to complete inside a string — which is exactly
/// where a Mongo key lives. Two contexts are worth completing here:
///
/// - inside an unterminated `"` → the collection's sampled field names
/// - a `$`-prefixed token → query operators
///
/// Anywhere else returns nothing rather than guessing, so the popup never
/// fights ordinary value typing.
pub fn suggest_mongo_filter(
    before_cursor: &str,
    nodes: &[VmTreeNode],
    collection: &str,
) -> (usize, Vec<Candidate>) {
    let chars: Vec<char> = before_cursor.chars().collect();
    let mut in_str = false;
    let mut esc = false;
    let mut key_start = 0usize;
    for (i, ch) in chars.iter().enumerate() {
        if esc {
            esc = false;
            continue;
        }
        match ch {
            '\\' if in_str => esc = true,
            '"' => {
                in_str = !in_str;
                if in_str {
                    key_start = i + 1;
                }
            }
            _ => {}
        }
    }
    if in_str {
        let word: String = chars[key_start..].iter().collect();
        // Fall back to every known field when the collection has not been
        // sampled yet (Driver::sample_fields runs on expand, not on open).
        let mut cands = columns_of(nodes, collection);
        if cands.is_empty() {
            cands = all_columns(nodes);
        }
        return rank_and_cap(cands, &word);
    }
    // A `$` token: Mongo query operators. `$` is not an identifier char, so
    // walk it back explicitly the way `suggest` does for the shell path.
    let tail: String = chars
        .iter()
        .rev()
        .take_while(|c| c.is_ascii_alphanumeric() || **c == '$')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    if tail.starts_with('$') {
        return rank_and_cap(mongo::query_ops(), &tail);
    }
    (0, Vec::new())
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
        language: rdb_connstore::QueryLanguage,
    ) -> (usize, Vec<Candidate>) {
        suggest(before, before, nodes, active_schema, language)
    }

    fn nodes() -> Vec<VmTreeNode> {
        let mk = |l: &str, k: &str| VmTreeNode {
            label: l.into(),
            kind: k.into(),
        };
        vec![
            mk("public", "database"),
            mk("job_config", "table"),
            mk("config_id", "field"),
            mk("name", "field"),
            mk("users", "table"),
            mk("id", "field"),
        ]
    }

    /// A Mongo-shaped tree: one collection with sampled fields.
    fn mongo_filter_nodes() -> Vec<VmTreeNode> {
        let mk = |l: &str, k: &str| VmTreeNode {
            label: l.into(),
            kind: k.into(),
        };
        vec![
            mk("shop", "database"),
            mk("orders", "collection"),
            mk("status: string", "field"),
            mk("subtotal: double", "field"),
            mk("customer_id: objectId", "field"),
        ]
    }

    #[test]
    fn mongo_filter_completes_field_names_inside_quotes() {
        // The whole point of the separate entry point: `suggest` refuses to
        // complete inside an unterminated string, which is where a key lives.
        let (word_len, c) = suggest_mongo_filter("{ \"sta", &mongo_filter_nodes(), "orders");
        assert_eq!(word_len, 3);
        assert!(c.iter().any(|x| x.label == "status"));
        // The sidebar stores "name: type"; only the bare name is inserted.
        assert!(!c.iter().any(|x| x.label.contains(':')));
    }

    #[test]
    fn mongo_filter_completes_query_operators_after_dollar() {
        let (word_len, c) =
            suggest_mongo_filter("{ \"subtotal\": { $g", &mongo_filter_nodes(), "orders");
        assert_eq!(word_len, 2, "the $ is part of the replaced token");
        let labels: Vec<&str> = c.iter().map(|x| x.label.as_str()).collect();
        assert!(labels.contains(&"$gt"));
        assert!(labels.contains(&"$gte"));
    }

    #[test]
    fn mongo_filter_stays_quiet_while_typing_a_value() {
        // A bare value is not completable; popping up here would fight typing.
        let (_, c) = suggest_mongo_filter("{ \"subtotal\": 12", &mongo_filter_nodes(), "orders");
        assert!(c.is_empty());
    }

    #[test]
    fn mongo_filter_closed_quote_is_not_a_key_context() {
        // Both quotes balanced: the caret is past the key, not inside it.
        let (_, c) = suggest_mongo_filter("{ \"status\": ", &mongo_filter_nodes(), "orders");
        assert!(c.is_empty());
    }

    #[test]
    fn mongo_filter_falls_back_to_all_fields_for_an_unsampled_collection() {
        // sample_fields runs on expand, so a freshly opened collection has no
        // fields of its own yet — offer what the tree does know.
        let (_, c) = suggest_mongo_filter("{ \"cust", &mongo_filter_nodes(), "not_sampled_yet");
        assert!(c.iter().any(|x| x.label == "customer_id"));
    }

    /// Two schemas, so a table outside the active one has to be reached by
    /// its qualified name.
    fn cross_schema_nodes() -> Vec<VmTreeNode> {
        let mk = |l: &str, k: &str| VmTreeNode {
            label: l.into(),
            kind: k.into(),
        };
        vec![
            mk("public", "database"),
            mk("users", "table"),
            mk("id", "field"),
            mk("billing", "database"),
            mk("t_invoice_line", "table"),
            mk("id_invoice", "field"),
        ]
    }

    /// The same table name in two schemas: a qualified `schema.table.` has to
    /// answer with that schema's columns, not whichever copy the flat node
    /// list happened to hold first.
    #[test]
    fn qualified_dot_picks_the_named_schemas_table() {
        let mk = |l: &str, k: &str| VmTreeNode {
            label: l.into(),
            kind: k.into(),
        };
        let dupes = vec![
            mk("public", "database"),
            mk("audit_log", "table"),
            mk("public_col", "field"),
            mk("archive", "database"),
            mk("audit_log", "table"),
            mk("archive_col", "field"),
        ];
        let (_, c) = sug(
            "select * from archive.audit_log where archive.audit_log.",
            &dupes,
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["archive_col"]
        );
    }

    #[test]
    fn table_position_offers_other_schemas_tables_pre_qualified() {
        let (_, c) = sug(
            "select * from t_invoi",
            &cross_schema_nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert!(
            c.iter().any(|c| c.label == "billing.t_invoice_line"),
            "got {:?}",
            c.iter().map(|c| &c.label).collect::<Vec<_>>()
        );
    }

    /// The typed chars skip through the name rather than prefixing it —
    /// `t_invl` reaches `t_invoice_line`.
    #[test]
    fn qualified_table_matches_a_subsequence_of_its_name() {
        let (_, c) = sug(
            "select * from t_invl",
            &cross_schema_nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert!(
            c.iter().any(|c| c.label == "billing.t_invoice_line"),
            "got {:?}",
            c.iter().map(|c| &c.label).collect::<Vec<_>>()
        );
    }

    #[test]
    fn active_schema_tables_outrank_qualified_ones() {
        let (_, c) = sug(
            "select * from u",
            &cross_schema_nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert_eq!(c.first().map(|c| c.label.as_str()), Some("users"));
    }

    #[test]
    fn qualified_tables_skip_the_active_schema() {
        let (_, c) = sug(
            "select * from user",
            &cross_schema_nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert!(!c.iter().any(|c| c.label == "public.users"), "got {c:?}");
    }

    #[test]
    fn empty_and_whitespace_context_suppress_popup() {
        assert!(
            sug("", &nodes(), "public", rdb_connstore::QueryLanguage::Sql)
                .1
                .is_empty()
        );
        assert!(
            sug("   ", &nodes(), "public", rdb_connstore::QueryLanguage::Sql)
                .1
                .is_empty()
        );
        // trailing space after a keyword: wait for the user to start typing
        assert!(sug(
            "select ",
            &nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql
        )
        .1
        .is_empty());
    }

    #[test]
    fn completion_stays_quiet_in_literals_and_limit_arguments() {
        let nodes = nodes();
        let (_, literal) = sug(
            "SELECT * FROM users WHERE name = 'us",
            &nodes,
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert!(literal.is_empty());
        let (_, limit) = sug(
            "SELECT * FROM users LIMIT 10",
            &nodes,
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert!(limit.is_empty());
    }

    #[test]
    fn completion_still_works_after_sql_clause_keyword() {
        let nodes = nodes();
        let (_, candidates) = sug(
            "SELECT us",
            &nodes,
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert!(candidates.iter().any(|c| c.label == "users"));
    }

    #[test]
    fn from_prefix_suggests_matching_table() {
        let (n, c) = sug(
            "select * from job",
            &nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert_eq!(n, 3);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["job_config"]
        );
    }

    #[test]
    fn dot_suggests_that_tables_columns() {
        let (n, c) = sug(
            "select * from job_config where job_config.",
            &nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert_eq!(n, 0);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["config_id", "name"]
        );
    }

    #[test]
    fn scoped_column_completion_does_not_leak_other_tables() {
        let (_, c) = sug(
            "select * from users where config",
            &nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert!(!c.iter().any(|x| x.label == "config_id"), "got {c:?}");
    }

    #[test]
    fn from_scope_reads_unaliased_comma_tables() {
        let mut n = nodes();
        n.push(VmTreeNode {
            label: "orders".into(),
            kind: "table".into(),
        });
        n.push(VmTreeNode {
            label: "order_id".into(),
            kind: "field".into(),
        });
        let (_, c) = sug(
            "select * from users, orders where ord",
            &n,
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert!(c.iter().any(|x| x.label == "order_id"), "got {c:?}");
    }

    #[test]
    fn prefix_match_drops_weaker_match_tiers() {
        let cand = |label: &str| Candidate {
            label: label.into(),
            kind: "field".into(),
            sub: String::new(),
        };
        let cands = vec![cand("name"), cand("business_name")];
        let (_, c) = rank_and_cap(cands, "n");
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["name"]
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
        let (n, c) = sug("db.log", &ns, "public", rdb_connstore::QueryLanguage::Mongo);
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
        let (_, c) = sug(
            "db.log_inbound.fi",
            &ns,
            "public",
            rdb_connstore::QueryLanguage::Mongo,
        );
        assert!(c.iter().any(|x| x.label == "find"));
        assert!(c.iter().any(|x| x.label == "findOne"));
    }

    /// Regression: once `Driver::sample_fields` populates a collection's
    /// fields, `db.<collection>.` must still offer methods, not those fields
    /// (the `!cols.is_empty()` check must not win over the Mongo dot-branch).
    #[test]
    fn mongo_collection_dot_with_sampled_fields_still_suggests_methods() {
        let mut ns = nodes();
        ns.push(VmTreeNode {
            label: "log_inbound".into(),
            kind: "collection".into(),
        });
        ns.push(VmTreeNode {
            label: "source".into(),
            kind: "field".into(),
        });
        let (_, c) = sug(
            "db.log_inbound.fi",
            &ns,
            "public",
            rdb_connstore::QueryLanguage::Mongo,
        );
        assert!(c.iter().any(|x| x.label == "find"));
        assert!(!c.iter().any(|x| x.label == "source"));
    }

    #[test]
    fn mongo_closed_call_dot_offers_chain_modifiers() {
        // `db.coll.find().s` — the call already closed, so `.` should offer
        // limit/skip/sort, not fall through to (empty) column completion.
        let mut ns = nodes();
        ns.push(VmTreeNode {
            label: "log_inbound".into(),
            kind: "collection".into(),
        });
        let (n, c) = sug(
            "db.log_inbound.find().s",
            &ns,
            "public",
            rdb_connstore::QueryLanguage::Mongo,
        );
        assert_eq!(n, 1);
        assert!(c.iter().any(|x| x.label == "sort"));
        assert!(!c.iter().any(|x| x.label == "limit"));
    }

    #[test]
    fn mongo_find_filter_offers_field_names() {
        let mut ns = nodes();
        ns.push(VmTreeNode {
            label: "log_inbound".into(),
            kind: "collection".into(),
        });
        ns.push(VmTreeNode {
            label: "source".into(),
            kind: "field".into(),
        });
        let (_, c) = sug(
            "db.log_inbound.find({ sou",
            &ns,
            "public",
            rdb_connstore::QueryLanguage::Mongo,
        );
        assert!(c.iter().any(|x| x.label == "source"));
    }

    #[test]
    fn mongo_dollar_offers_query_operators_and_replaces_whole_token() {
        let mut ns = nodes();
        ns.push(VmTreeNode {
            label: "log_inbound".into(),
            kind: "collection".into(),
        });
        let (n, c) = sug(
            "db.log_inbound.find({ age: { $g",
            &ns,
            "public",
            rdb_connstore::QueryLanguage::Mongo,
        );
        // Replace length covers the `$` too, so accepting "$gt" doesn't
        // duplicate the `$` the user already typed.
        assert_eq!(n, 2);
        assert!(c.iter().any(|x| x.label == "$gt"));
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
        let (_, c) = sug("d", &ns, "public", rdb_connstore::QueryLanguage::Mongo);
        assert!(c.iter().any(|x| x.label == "db"));
        assert!(!c.iter().any(|x| x.label == "DELETE"));
    }

    #[test]
    fn redis_bare_word_suggests_commands_not_sql_keywords() {
        // Regression: Redis previously fell into the SQL keyword branch and
        // got offered SELECT/DELETE/etc, which don't exist in Redis.
        let (_, c) = sug("GE", &[], "public", rdb_connstore::QueryLanguage::Command);
        assert!(c.iter().any(|x| x.label == "GET"));
        assert!(!c.iter().any(|x| x.label == "SELECT"));
        assert!(!c.iter().any(|x| x.label == "DELETE"));
    }

    #[test]
    fn redis_completion_only_at_line_start() {
        // A command's arguments (the key name) aren't completed against the
        // command list.
        let (_, c) = sug(
            "GET k",
            &[],
            "public",
            rdb_connstore::QueryLanguage::Command,
        );
        assert!(c.is_empty());
    }

    #[test]
    fn cql_bare_word_offers_no_join_or_having() {
        // CQL has no JOIN/HAVING — offering them would be SQL leaking through.
        // (SQL would suggest JOIN/HAVING here since both are keyword prefixes
        // of "j"/"h" reachable from the SELECT column-position branch.)
        let (_, c) = sug(
            "select j",
            &nodes(),
            "public",
            rdb_connstore::QueryLanguage::Cql,
        );
        assert!(!c.iter().any(|x| x.label == "JOIN"));
        let (_, c) = sug(
            "select h",
            &nodes(),
            "public",
            rdb_connstore::QueryLanguage::Cql,
        );
        assert!(!c.iter().any(|x| x.label == "HAVING"));
    }

    #[test]
    fn cql_from_prefix_suggests_matching_table() {
        let (n, c) = sug(
            "select * from job",
            &nodes(),
            "public",
            rdb_connstore::QueryLanguage::Cql,
        );
        assert_eq!(n, 3);
        assert!(c.iter().any(|x| x.label == "job_config"));
    }

    /// Typing a mid-word `_` segment finds the identifier unless a true prefix
    /// already answers the word.
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
        let (_, c) = sug(
            "select teknis",
            &n,
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        let labels: Vec<&str> = c.iter().map(|x| x.label.as_str()).collect();
        // `teknis_id` is a real prefix → ranks ahead of the mid-word match.
        assert_eq!(labels, ["teknis_id"]);
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
            mk("analytics_core", "database"),
            mk("event_journal", "table"),
            mk("step_id", "field"),
            mk("journal_id", "field"),
        ];
        // Active schema is public; the query reads analytics_core.event_journal.
        let (_, c) = sug(
            "select * from analytics_core.event_journal where step",
            &two,
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        let labels: Vec<&str> = c.iter().map(|x| x.label.as_str()).collect();
        assert!(labels.contains(&"step_id"));
        // A prior statement's tables can't leak in: the caller scopes `stmt` to
        // the statement under the cursor (`EditorState::current_statement`).
        let (_, c2) = suggest(
            "select * from users;\nselect * from analytics_core.event_journal where step",
            "select * from analytics_core.event_journal where step",
            &two,
            "public",
            rdb_connstore::QueryLanguage::Sql,
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
            mk("analytics_core", "database"),
            mk("job_config", "table"),
            mk("config_id", "field"),
            mk("taint", "field"),
        ];
        let (n, c) = suggest(
            "select con",
            "select con from analytics_core.job_config",
            &two,
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert_eq!(n, 3);
        assert_eq!(c.first().map(|x| x.label.as_str()), Some("config_id"));
    }

    /// Same blind spot for an alias declared further along the statement.
    #[test]
    fn alias_dot_resolves_an_alias_declared_after_the_cursor() {
        let (_, c) = suggest(
            "select a.",
            "select a. from job_config a",
            &nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql,
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
            mk("analytics_core", "database"),
            mk("job_config", "table"),
            mk("config_id", "field"),
        ];
        // Schema name typed without its underscores.
        let (_, c) = sug(
            "select * from analyticscore",
            &n,
            "analytics_core",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert!(c.iter().any(|x| x.label == "analytics_core"));
        // Not schemas only — tables and columns too.
        let (_, c) = sug(
            "select * from jobconfig",
            &n,
            "analytics_core",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert!(c.iter().any(|x| x.label == "job_config"));
        let (_, c) = sug(
            "select * from job_config where configid",
            &n,
            "analytics_core",
            rdb_connstore::QueryLanguage::Sql,
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
            mk("jobconfig", "table"),
            mk("job_config", "table"),
        ];
        let (_, c) = sug(
            "select * from jobc",
            &n,
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["jobconfig"]
        );
    }

    #[test]
    fn bare_word_completes_keyword() {
        let (n, c) = sug(
            "sele",
            &nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert_eq!(n, 4);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["SELECT"]
        );
    }

    #[test]
    fn select_context_offers_columns() {
        // Type-triggered: a prefix is required before the popup appears.
        let (_, c) = sug(
            "select n",
            &nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        let labels: Vec<&str> = c.iter().map(|x| x.label.as_str()).collect();
        assert!(labels.contains(&"name"));
    }

    #[test]
    fn alias_dot_resolves_to_table_columns() {
        let (_, c) = sug(
            "select * from job_config sc where sc.",
            &nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["config_id", "name"]
        );
    }

    /// A formatted query puts the clause keyword on its own line, and the
    /// clause context used to be read from the cursor's line alone — so the
    /// same query completed columns when typed on one line and only keywords
    /// when typed across several.
    #[test]
    fn multiline_select_still_offers_columns() {
        let (_, c) = sug(
            "select\n    na",
            &nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        let labels: Vec<&str> = c.iter().map(|x| x.label.as_str()).collect();
        assert!(labels.contains(&"name"), "got {labels:?}");
    }

    /// Same shape one clause further in: WHERE on an earlier line.
    #[test]
    fn multiline_where_still_offers_columns() {
        let (_, c) = sug(
            "select *\nfrom job_config\nwhere\n    conf",
            &nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        let labels: Vec<&str> = c.iter().map(|x| x.label.as_str()).collect();
        assert!(labels.contains(&"config_id"), "got {labels:?}");
    }

    /// The clause context stops at the statement boundary — a finished
    /// statement above must not make the next one look like it is mid-SELECT.
    #[test]
    fn clause_context_stops_at_the_previous_statement() {
        let (_, c) = sug(
            "select name from job_config;\nsel",
            &nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        let labels: Vec<&str> = c.iter().map(|x| x.label.as_str()).collect();
        assert!(labels.contains(&"SELECT"), "got {labels:?}");
    }

    /// `AS` is itself a keyword, so the alias scan used to stop on it and
    /// leave an explicit `FROM tbl AS a` alias unresolvable — no popup at all.
    #[test]
    fn as_alias_resolves_to_table_columns() {
        let (_, c) = sug(
            "select * from job_config as jc where jc.",
            &nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["config_id", "name"]
        );
    }

    /// Only the first entry of a comma-separated FROM list was examined, so
    /// the second table's alias resolved to nothing.
    #[test]
    fn comma_joined_from_resolves_the_second_alias() {
        let (_, c) = sug(
            "select * from job_config a, users b where b.",
            &nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["id"]
        );
    }

    /// Schema-qualified table with an alias inside a JOIN: each alias resolves
    /// to its own table's columns (regression for empty suggestions on joins).
    #[test]
    fn schema_qualified_alias_join_resolves_columns() {
        let a = "select * from public.job_config a left join public.users b on a.";
        let (_, c) = sug(a, &nodes(), "public", rdb_connstore::QueryLanguage::Sql);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["config_id", "name"]
        );
        let b = "select * from public.job_config a left join public.users b on b.";
        let (_, c) = sug(b, &nodes(), "public", rdb_connstore::QueryLanguage::Sql);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["id"]
        );
    }

    /// Alias declared on an earlier line still resolves when completing a
    /// later line (before_cursor spans the whole statement).
    #[test]
    fn multiline_join_alias_resolves_columns() {
        let sql = "select * from public.job_config a\nleft join public.users b on a.";
        let (_, c) = sug(sql, &nodes(), "public", rdb_connstore::QueryLanguage::Sql);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["config_id", "name"]
        );
    }

    #[test]
    fn star_context_offers_from_keyword() {
        let (_, c) = sug(
            "select * f",
            &nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        let labels: Vec<&str> = c.iter().map(|x| x.label.as_str()).collect();
        assert!(labels.contains(&"FROM"));
    }

    #[test]
    fn schema_dot_suggests_its_tables() {
        let (n, c) = sug(
            "select * from public.",
            &nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        assert_eq!(n, 0);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["job_config", "users"]
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
        let (_, c) = sug(
            "select * from users where users.",
            &typed,
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
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
        let (_, c) = sug(
            "select * from t",
            &two,
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
        let labels: Vec<&str> = c.iter().map(|x| x.label.as_str()).collect();
        assert!(labels.contains(&"t_users"));
        assert!(!labels.contains(&"t_orders"));

        let (_, c) = sug(
            "select * from t",
            &two,
            "other",
            rdb_connstore::QueryLanguage::Sql,
        );
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
            mk("job_config", "table"),
            mk("cfg_id", "field"),
        ]
    }

    /// In table position, every schema name is offered so a cross-schema
    /// `schema.table` can be started even when it isn't the active schema.
    #[test]
    fn from_offers_schema_names() {
        // Type-triggered: "a" narrows to the analytics schema name.
        let (_, c) = sug(
            "select * from a",
            &two_schema_nodes(),
            "public",
            rdb_connstore::QueryLanguage::Sql,
        );
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
            rdb_connstore::QueryLanguage::Sql,
        );
        assert_eq!(n, 0);
        assert_eq!(
            c.iter().map(|x| x.label.as_str()).collect::<Vec<_>>(),
            ["job_config"]
        );
    }

    #[test]
    fn generate_alias_single_word() {
        assert_eq!(generate_alias("users"), "u");
    }

    #[test]
    fn generate_alias_underscore_separated() {
        assert_eq!(generate_alias("order_items"), "oi");
        assert_eq!(generate_alias("t_invoice_line"), "til");
    }

    #[test]
    fn generate_alias_preserves_case_in_initials() {
        // Each segment's first char is lowercased.
        assert_eq!(generate_alias("UserSessions"), "u");
        assert_eq!(generate_alias("order_Items"), "oi");
    }

    #[test]
    fn generate_alias_empty_fallback() {
        // Edge case: empty string or just underscores.
        assert_eq!(generate_alias(""), "");
        assert_eq!(generate_alias("_"), "");
    }

    #[test]
    fn is_table_position_detects_from_and_join() {
        assert!(is_table_position(
            "select * from ",
            rdb_connstore::QueryLanguage::Sql
        ));
        assert!(is_table_position(
            "select * from t left join ",
            rdb_connstore::QueryLanguage::Sql
        ));
        assert!(is_table_position(
            "insert into ",
            rdb_connstore::QueryLanguage::Sql
        ));
        assert!(is_table_position(
            "update ",
            rdb_connstore::QueryLanguage::Sql
        ));
        // Not table position:
        assert!(!is_table_position(
            "select ",
            rdb_connstore::QueryLanguage::Sql
        ));
        assert!(!is_table_position(
            "select * from users where ",
            rdb_connstore::QueryLanguage::Sql
        ));
    }
}
