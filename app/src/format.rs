//! Query formatter: uppercase keywords, one clause per line, collapsed
//! whitespace. String literals, quoted identifiers and comments pass through
//! untouched. Not a parser — just enough for an editor tidy-up button.
//!
//! The clause-splitting algorithm is identical across dialects — only the
//! keyword set and which keywords start a new line differ — so `sql`/`cql`
//! just supply a `Spec` to the shared `run` loop instead of duplicating it.

pub mod cql;
pub mod sql;

use rdb_connstore::QueryDialect;

/// Per-dialect knobs for the shared formatting loop.
pub struct Spec {
    pub is_keyword: fn(&str) -> bool,
    /// Keywords that start a new line. BY/ON etc. stay inline after their opener.
    pub clause_starters: &'static [&'static str],
    /// Qualifiers (LEFT/INNER/…) after which a following JOIN stays on the
    /// same line instead of starting a new one.
    pub join_qualifiers: &'static [&'static str],
}

/// Format `text` for `dialect`. `None` for Redis/Mongo — the Format button
/// stays hidden for those (see `sql_capable` in main.rs), this is
/// defense-in-depth against a stray call.
pub fn dispatch(dialect: QueryDialect, text: &str) -> Option<String> {
    match dialect {
        QueryDialect::Sql(d) => Some(run(&sql::spec(d), text)),
        QueryDialect::Cql => Some(run(&cql::SPEC, text)),
        QueryDialect::Command | QueryDialect::Mongo => None,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum Tok {
    Word(String, String), // (original, upper)
    Quoted(String),
    LineComment(String),
    BlockComment(String),
    Punct(char, bool), // (punct, had_leading_whitespace)
}

fn tokenize(text: &str) -> Vec<Tok> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            let start = i;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            out.push(Tok::LineComment(chars[start..i].iter().collect()));
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            let start = i;
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i = (i + 2).min(chars.len());
            out.push(Tok::BlockComment(chars[start..i].iter().collect()));
            continue;
        }
        if c == '\'' || c == '"' || c == '`' {
            let start = i;
            i += 1;
            while i < chars.len() {
                if chars[i] == c {
                    i += 1;
                    if chars.get(i) == Some(&c) {
                        i += 1;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            out.push(Tok::Quoted(chars[start..i].iter().collect()));
            continue;
        }
        if c.is_alphanumeric() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let upper = word.to_uppercase();
            out.push(Tok::Word(word, upper));
            continue;
        }
        let had_space = i > 0 && chars[i - 1].is_whitespace();
        out.push(Tok::Punct(c, had_space));
        i += 1;
    }
    out
}

fn trim_trailing_spaces(out: &mut String) {
    let trimmed_len = out.trim_end_matches(' ').len();
    out.truncate(trimmed_len);
}

fn push_indent(out: &mut String, indent: usize) {
    out.push_str(&"    ".repeat(indent));
}

fn push_newline_indent(out: &mut String, indent: usize) {
    trim_trailing_spaces(out);
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    push_indent(out, indent);
}

fn push_sep(out: &mut String, indent: usize) {
    if out.is_empty() {
        return;
    }
    if out.ends_with('\n') {
        push_indent(out, indent);
    } else if !out.ends_with(' ') && !out.ends_with('(') && !out.ends_with('.') {
        out.push(' ');
    }
}

fn is_body_clause(upper: &str) -> bool {
    matches!(
        upper,
        "SELECT" | "FROM" | "WHERE" | "HAVING" | "LIMIT" | "OFFSET" | "VALUES" | "SET" | "JOIN"
    )
}

#[derive(Default)]
struct FmtState {
    out: String,
    indent: usize,
    in_cond_clause: bool,
    between_pending: bool,
    prev_kw: String,
    paren_stack: Vec<bool>,
}

impl FmtState {
    fn inside_inline_paren(&self) -> bool {
        self.paren_stack.iter().any(|&b| !b)
    }

    fn emit_open_paren(&mut self, had_space: bool) {
        let block_paren = self.in_cond_clause
            && matches!(self.prev_kw.as_str(), "AND" | "OR" | "WHERE" | "HAVING");
        if block_paren {
            self.indent += 1;
            push_newline_indent(&mut self.out, self.indent);
            self.out.push('(');
            self.indent += 1;
            push_newline_indent(&mut self.out, self.indent);
            self.paren_stack.push(true);
        } else {
            let needs_space = !self.out.is_empty()
                && !self.out.ends_with(['\n', ' ', '('])
                && (had_space || !self.prev_kw.is_empty());
            if needs_space {
                self.out.push(' ');
            }
            self.out.push('(');
            self.paren_stack.push(false);
        }
        self.prev_kw.clear();
    }

    fn emit_close_paren(&mut self) {
        if self.paren_stack.pop().unwrap_or(false) {
            self.indent = self.indent.saturating_sub(1);
            push_newline_indent(&mut self.out, self.indent);
            self.out.push(')');
            self.indent = self.indent.saturating_sub(1);
        } else {
            trim_trailing_spaces(&mut self.out);
            self.out.push(')');
        }
        self.prev_kw.clear();
    }

    fn emit_punct(&mut self, c: char, had_space: bool, has_more: bool) {
        match c {
            '(' => self.emit_open_paren(had_space),
            ')' => self.emit_close_paren(),
            ',' | '.' => {
                trim_trailing_spaces(&mut self.out);
                self.out.push(c);
                self.prev_kw.clear();
            }
            ';' => {
                trim_trailing_spaces(&mut self.out);
                self.out.push(';');
                self.indent = 0;
                self.in_cond_clause = false;
                self.between_pending = false;
                self.paren_stack.clear();
                self.prev_kw.clear();
                if has_more {
                    self.out.push('\n');
                }
            }
            other => {
                let attaches = self.out.ends_with(['<', '>', '!', ':', '-', '='])
                    && matches!(other, '=' | '>' | ':');
                if !attaches {
                    push_sep(&mut self.out, self.indent);
                }
                self.out.push(other);
                self.prev_kw.clear();
            }
        }
    }

    fn emit_clause_starter(&mut self, spec: &Spec, upper: &str, next_tok: Option<&Tok>) {
        self.indent = 0;
        self.in_cond_clause = matches!(upper, "WHERE" | "HAVING");
        self.between_pending = false;
        if !self.out.is_empty() {
            push_newline_indent(&mut self.out, 0);
        }
        self.out.push_str(upper);
        self.prev_kw = upper.to_string();

        let next_upper = next_tok.and_then(|t| match t {
            Tok::Word(_, u) if (spec.is_keyword)(u) => Some(u.as_str()),
            _ => None,
        });
        let stays_on_header = matches!(
            (upper, next_upper),
            ("SELECT", Some("DISTINCT" | "TOP"))
                | ("GROUP" | "ORDER", Some("BY"))
                | ("ALLOW", Some("FILTERING"))
                | ("UNION", Some("ALL"))
        ) || spec.join_qualifiers.contains(&upper);

        if !stays_on_header && is_body_clause(upper) && next_tok.is_some() {
            self.indent = 1;
            push_newline_indent(&mut self.out, self.indent);
        }
    }

    fn emit_keyword(&mut self, spec: &Spec, upper: &str, toks: &[Tok], idx: &mut usize) {
        if upper == "BETWEEN" {
            self.between_pending = true;
        }
        let joins_prev = (upper == "JOIN" && spec.join_qualifiers.contains(&self.prev_kw.as_str()))
            || (upper == "OUTER" && matches!(self.prev_kw.as_str(), "LEFT" | "RIGHT" | "FULL"));
        let inline = self.inside_inline_paren();
        let has_more = *idx + 1 < toks.len();

        if !inline && (upper == "SELECT" || spec.clause_starters.contains(&upper)) && !joins_prev {
            self.emit_clause_starter(spec, upper, toks.get(*idx + 1));
            return;
        }
        if !inline
            && ((matches!(upper, "DISTINCT" | "BY")
                && matches!(self.prev_kw.as_str(), "SELECT" | "GROUP" | "ORDER"))
                || (joins_prev && upper == "JOIN"))
        {
            push_sep(&mut self.out, self.indent);
            self.out.push_str(upper);
            self.prev_kw = upper.to_string();
            if has_more {
                self.indent = 1;
                push_newline_indent(&mut self.out, self.indent);
            }
            return;
        }
        if !inline && upper == "TOP" && self.prev_kw == "SELECT" {
            push_sep(&mut self.out, self.indent);
            self.out.push_str(upper);
            if let Some(Tok::Word(num, _)) = toks.get(*idx + 1) {
                if num.chars().all(|ch| ch.is_ascii_digit()) {
                    self.out.push(' ');
                    self.out.push_str(num);
                    *idx += 1;
                }
            }
            self.prev_kw = upper.to_string();
            if *idx + 1 < toks.len() {
                self.indent = 1;
                push_newline_indent(&mut self.out, self.indent);
            }
            return;
        }
        if !inline && self.in_cond_clause && matches!(upper, "AND" | "OR") {
            if upper == "AND" && self.between_pending {
                self.between_pending = false;
                push_sep(&mut self.out, self.indent);
            } else {
                push_newline_indent(&mut self.out, self.indent);
            }
            self.out.push_str(upper);
            self.prev_kw = upper.to_string();
            return;
        }
        push_sep(&mut self.out, self.indent);
        self.out.push_str(upper);
        self.prev_kw = upper.to_string();
    }
}

fn run(spec: &Spec, text: &str) -> String {
    let toks = tokenize(text);
    let mut st = FmtState {
        out: String::with_capacity(text.len() + 32),
        ..FmtState::default()
    };
    let mut idx = 0;
    while idx < toks.len() {
        match &toks[idx] {
            Tok::LineComment(s) => {
                push_sep(&mut st.out, st.indent);
                st.out.push_str(s);
                st.out.push('\n');
            }
            Tok::BlockComment(s) => {
                push_sep(&mut st.out, st.indent);
                st.out.push_str(s);
            }
            Tok::Quoted(s) => {
                push_sep(&mut st.out, st.indent);
                st.out.push_str(s);
                st.prev_kw.clear();
            }
            Tok::Punct(c, had_space) => {
                st.emit_punct(*c, *had_space, idx + 1 < toks.len());
            }
            Tok::Word(word, upper) => {
                if (spec.is_keyword)(upper) {
                    st.emit_keyword(spec, upper, &toks, &mut idx);
                } else {
                    push_sep(&mut st.out, st.indent);
                    st.out.push_str(word);
                    st.prev_kw.clear();
                }
            }
        }
        idx += 1;
    }
    st.out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdb_connstore::SqlDialect;

    fn format(sql: &str) -> String {
        dispatch(QueryDialect::Sql(SqlDialect::Postgres), sql).unwrap()
    }

    #[test]
    fn uppercases_and_splits_clauses() {
        assert_eq!(
            format("select  a, b from t where a = 1 order by b desc limit 10"),
            "SELECT\n    a, b\nFROM\n    t\nWHERE\n    a = 1\nORDER BY\n    b DESC\nLIMIT\n    10"
        );
    }

    #[test]
    fn indents_where_and_parenthesized_conditions() {
        let sql = "select * from oss_rba.col_logs icl where icl.tid = 'DELETE' and icl.tablename = 'oss_rba.permohonan' and (updated_at >= '2026-01-01' and updated_at < '2026-10-01') limit 10;";
        assert_eq!(
            format(sql),
            "SELECT\n    *\nFROM\n    oss_rba.col_logs icl\nWHERE\n    icl.tid = 'DELETE'\n    AND icl.tablename = 'oss_rba.permohonan'\n    AND\n        (\n            updated_at >= '2026-01-01'\n            AND updated_at < '2026-10-01'\n        )\nLIMIT\n    10;"
        );
    }

    #[test]
    fn join_qualifier_stays_with_join() {
        assert_eq!(
            format("select * from a left join b on a.id = b.id"),
            "SELECT\n    *\nFROM\n    a\nLEFT JOIN\n    b ON a.id = b.id"
        );
    }

    #[test]
    fn literals_and_comments_untouched() {
        assert_eq!(
            format("select 'from x' as \"From\" -- from here\nfrom t"),
            "SELECT\n    'from x' AS \"From\" -- from here\nFROM\n    t"
        );
    }

    #[test]
    fn formats_a_full_multi_statement_selection() {
        assert_eq!(
            format("select a from t where a=1;"),
            "SELECT\n    a\nFROM\n    t\nWHERE\n    a = 1;"
        );
    }

    #[test]
    fn command_and_mongo_are_not_formatted() {
        assert!(dispatch(QueryDialect::Command, "GET k").is_none());
        assert!(dispatch(QueryDialect::Mongo, "db.t.find()").is_none());
    }

    #[test]
    fn cql_splits_where_and_allow_filtering() {
        let got = dispatch(
            QueryDialect::Cql,
            "select * from ks.t where k=1 allow filtering",
        )
        .unwrap();
        assert_eq!(
            got,
            "SELECT\n    *\nFROM\n    ks.t\nWHERE\n    k = 1\nALLOW FILTERING"
        );
    }

    #[test]
    fn cql_does_not_break_on_create_table_primary_key() {
        let got = dispatch(
            QueryDialect::Cql,
            "create table t (id int, primary key (id))",
        )
        .unwrap();
        assert_eq!(got, "CREATE TABLE t (id int, PRIMARY KEY (id))");
    }

    #[test]
    fn full_join_stays_on_one_line_and_uppercase() {
        assert_eq!(
            format("select * from a full join b on a.id = b.id"),
            "SELECT\n    *\nFROM\n    a\nFULL JOIN\n    b ON a.id = b.id"
        );
    }

    #[test]
    fn vendor_keyword_uppercased_only_for_its_dialect() {
        let mssql = dispatch(
            QueryDialect::Sql(SqlDialect::Mssql),
            "select top 10 * from t",
        )
        .unwrap();
        assert_eq!(mssql, "SELECT TOP 10\n    *\nFROM\n    t");

        let postgres = dispatch(
            QueryDialect::Sql(SqlDialect::Postgres),
            "select top 10 * from t",
        )
        .unwrap();
        assert_eq!(postgres, "SELECT\n    top 10 *\nFROM\n    t");
    }
}
