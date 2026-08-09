//! Query formatter: uppercase keywords, one clause per line, collapsed
//! whitespace. String literals, quoted identifiers and comments pass through
//! untouched. Not a parser — just enough for an editor tidy-up button.
//!
//! The clause-splitting algorithm is identical across dialects — only the
//! keyword set and which keywords start a new line differ — so `sql`/`cql`
//! just supply a `Spec` to the shared `run` loop instead of duplicating it.

pub mod cql;
pub mod sql;

use rdb_connstore::QueryLanguage;

/// Per-dialect knobs for the shared formatting loop.
pub struct Spec {
    pub is_keyword: fn(&str) -> bool,
    /// Keywords that start a new line. BY/ON etc. stay inline after their opener.
    pub clause_starters: &'static [&'static str],
    /// Qualifiers (LEFT/INNER/…) after which a following JOIN stays on the
    /// same line instead of starting a new one.
    pub join_qualifiers: &'static [&'static str],
}

/// Format `text` for `language`. `None` for Redis/Mongo — the Format button
/// stays hidden for those (see `sql_capable` in main.rs), this is
/// defense-in-depth against a stray call.
pub fn dispatch(language: QueryLanguage, text: &str) -> Option<String> {
    match language {
        QueryLanguage::Sql => Some(run(&sql::SPEC, text)),
        QueryLanguage::Cql => Some(run(&cql::SPEC, text)),
        QueryLanguage::Command | QueryLanguage::Mongo => None,
    }
}

fn run(spec: &Spec, text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut prev_word = String::new();
    while i < chars.len() {
        let c = chars[i];
        if c == '-' && chars.get(i + 1) == Some(&'-') {
            let start = i;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            out.extend(chars[start..i].iter());
            continue;
        }
        if c == '/' && chars.get(i + 1) == Some(&'*') {
            let start = i;
            i += 2;
            while i < chars.len() && !(chars[i] == '*' && chars.get(i + 1) == Some(&'/')) {
                i += 1;
            }
            i = (i + 2).min(chars.len());
            out.extend(chars[start..i].iter());
            continue;
        }
        if c == '\'' || c == '"' {
            let start = i;
            i += 1;
            while i < chars.len() {
                if chars[i] == c {
                    i += 1;
                    // doubled quote = escaped, keep scanning
                    if chars.get(i) == Some(&c) {
                        i += 1;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            out.extend(chars[start..i].iter());
            continue;
        }
        if c.is_whitespace() {
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            if !out.is_empty() && !out.ends_with('\n') {
                out.push(' ');
            }
            continue;
        }
        if c.is_alphanumeric() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            let upper = word.to_uppercase();
            if (spec.is_keyword)(&upper) {
                let joins_previous =
                    upper == "JOIN" && spec.join_qualifiers.contains(&prev_word.as_str());
                if spec.clause_starters.contains(&upper.as_str())
                    && !out.is_empty()
                    && !joins_previous
                {
                    while out.ends_with(' ') {
                        out.pop();
                    }
                    if !out.ends_with('\n') {
                        out.push('\n');
                    }
                }
                out.push_str(&upper);
                prev_word = upper;
            } else {
                out.push_str(&word);
                prev_word = String::new();
            }
            continue;
        }
        out.push(c);
        prev_word = String::new();
        i += 1;
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn format(sql: &str) -> String {
        dispatch(QueryLanguage::Sql, sql).unwrap()
    }

    #[test]
    fn uppercases_and_splits_clauses() {
        assert_eq!(
            format("select  a, b from t where a = 1 order by b desc limit 10"),
            "SELECT a, b\nFROM t\nWHERE a = 1\nORDER BY b DESC\nLIMIT 10"
        );
    }

    #[test]
    fn join_qualifier_stays_with_join() {
        assert_eq!(
            format("select * from a left join b on a.id = b.id"),
            "SELECT *\nFROM a\nLEFT JOIN b ON a.id = b.id"
        );
    }

    #[test]
    fn literals_and_comments_untouched() {
        assert_eq!(
            format("select 'from x' as \"From\" -- from here\nfrom t"),
            "SELECT 'from x' AS \"From\" -- from here\nFROM t"
        );
    }

    #[test]
    fn formats_a_full_multi_statement_selection() {
        assert_eq!(
            format("select a from t where a=1;"),
            "SELECT a\nFROM t\nWHERE a=1;"
        );
    }

    #[test]
    fn command_and_mongo_are_not_formatted() {
        assert!(dispatch(QueryLanguage::Command, "GET k").is_none());
        assert!(dispatch(QueryLanguage::Mongo, "db.t.find()").is_none());
    }

    #[test]
    fn cql_splits_where_and_allow_filtering() {
        let got = dispatch(
            QueryLanguage::Cql,
            "select * from ks.t where k=1 allow filtering",
        )
        .unwrap();
        assert_eq!(got, "SELECT *\nFROM ks.t\nWHERE k=1\nALLOW FILTERING");
    }

    #[test]
    fn cql_does_not_break_on_create_table_primary_key() {
        let got = dispatch(
            QueryLanguage::Cql,
            "create table t (id int, primary key (id))",
        )
        .unwrap();
        assert_eq!(got, "CREATE TABLE t (id int, PRIMARY KEY (id))");
    }
}
