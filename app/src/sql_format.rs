//! Minimal SQL formatter: uppercase keywords, one clause per line, collapsed
//! whitespace. String literals, quoted identifiers and comments pass through
//! untouched. Not a parser — just enough for an editor tidy-up button.

use crate::editor;

/// Keywords that start a new line. BY/ON etc. stay inline after their opener.
const CLAUSE_STARTERS: &[&str] = &[
    "FROM", "WHERE", "GROUP", "HAVING", "ORDER", "LIMIT", "OFFSET", "JOIN", "LEFT", "RIGHT",
    "INNER", "FULL", "UNION", "VALUES", "SET",
];

/// Join qualifiers: a following JOIN stays on the same line.
const JOIN_QUALIFIERS: &[&str] = &["LEFT", "RIGHT", "INNER", "FULL", "OUTER", "CROSS"];

pub fn format(sql: &str) -> String {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = String::with_capacity(sql.len());
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
            if editor::is_keyword(&upper) {
                let joins_previous =
                    upper == "JOIN" && JOIN_QUALIFIERS.contains(&prev_word.as_str());
                if CLAUSE_STARTERS.contains(&upper.as_str()) && !out.is_empty() && !joins_previous {
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

/// Format one physical editor line without introducing new editor lines.
pub fn format_line(sql: &str) -> String {
    format(sql).replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn format_line_keeps_a_query_on_one_physical_line() {
        assert_eq!(
            format_line("select * from users where active = true"),
            "SELECT * FROM users WHERE active = true"
        );
    }
}
