//! SQL keyword vocabulary shared by the lexer and completion.

use rdb_connstore::SqlDialect;

pub const COMMON_KEYWORDS: &[&str] = &[
    "SELECT",
    "FROM",
    "WHERE",
    "GROUP",
    "BY",
    "ORDER",
    "LIMIT",
    "OFFSET",
    "JOIN",
    "LEFT",
    "RIGHT",
    "INNER",
    "OUTER",
    "ON",
    "AS",
    "AND",
    "OR",
    "NOT",
    "IN",
    "IS",
    "NULL",
    "INSERT",
    "INTO",
    "VALUES",
    "UPDATE",
    "SET",
    "DELETE",
    "CREATE",
    "TABLE",
    "FUNCTION",
    "REPLACE",
    "RETURNS",
    "LANGUAGE",
    "BEGIN",
    "COMMIT",
    "ROLLBACK",
    "END",
    "RETURN",
    "DESC",
    "ASC",
    "HAVING",
    "UNION",
    "ALL",
    "DISTINCT",
    "CASE",
    "WHEN",
    "THEN",
    "ELSE",
    "LIKE",
    "BETWEEN",
    "EXISTS",
    "WITH",
    "EXTRACT",
    "NOW",
    "CURRENT_DATE",
    "CURRENT_TIMESTAMP",
    "INTERVAL",
    "COALESCE",
    "CAST",
    "SUM",
    "AVG",
    "MIN",
    "MAX",
];

const POSTGRES_KEYWORDS: &[&str] = &[
    "ILIKE",
    "DATE_TRUNC",
    "AGE",
    "IMMUTABLE",
    "PARALLEL",
    "SAFE",
    "STRICT",
];
const MYSQL_KEYWORDS: &[&str] = &["AUTO_INCREMENT", "REGEXP", "SHOW", "DESCRIBE"];
// ROWID is a hidden column, not a keyword (not in sqlite.org/lang_keywords.html).
const SQLITE_KEYWORDS: &[&str] = &["PRAGMA", "GLOB", "AUTOINCREMENT"];
// GO is not part of T-SQL: it's a batch separator SSMS/sqlcmd strip client-side
// before the query ever reaches the server, so it isn't valid syntax to send.
const MSSQL_KEYWORDS: &[&str] = &["TOP", "IDENTITY", "NVARCHAR"];
const CLICKHOUSE_KEYWORDS: &[&str] = &["PREWHERE", "ARRAY", "FINAL", "SAMPLE", "SETTINGS"];
// DUAL is an ordinary table name, not a reserved word (absent from Oracle's
// own SQL Reserved Words list).
const ORACLE_KEYWORDS: &[&str] = &["CONNECT", "START", "PRIOR", "ROWNUM", "MINUS"];

fn dialect_keywords(dialect: SqlDialect) -> &'static [&'static str] {
    match dialect {
        SqlDialect::Postgres => POSTGRES_KEYWORDS,
        SqlDialect::MySql => MYSQL_KEYWORDS,
        SqlDialect::Sqlite => SQLITE_KEYWORDS,
        SqlDialect::Mssql => MSSQL_KEYWORDS,
        SqlDialect::Clickhouse => CLICKHOUSE_KEYWORDS,
        SqlDialect::Oracle => ORACLE_KEYWORDS,
    }
}

pub fn is_keyword(dialect: SqlDialect, word: &str) -> bool {
    COMMON_KEYWORDS.contains(&word) || dialect_keywords(dialect).contains(&word)
}

pub fn keywords(dialect: SqlDialect) -> impl Iterator<Item = &'static str> {
    COMMON_KEYWORDS
        .iter()
        .copied()
        .chain(dialect_keywords(dialect).iter().copied())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_keywords_do_not_leak_between_dialects() {
        assert!(is_keyword(SqlDialect::Postgres, "ILIKE"));
        assert!(!is_keyword(SqlDialect::MySql, "ILIKE"));
        assert!(is_keyword(SqlDialect::Mssql, "TOP"));
        assert!(!is_keyword(SqlDialect::Postgres, "TOP"));
    }

    #[test]
    fn common_keywords_are_available_across_every_dialect() {
        for d in [
            SqlDialect::Postgres,
            SqlDialect::MySql,
            SqlDialect::Sqlite,
            SqlDialect::Mssql,
            SqlDialect::Clickhouse,
            SqlDialect::Oracle,
        ] {
            assert!(is_keyword(d, "SELECT"), "{d:?} should know SELECT");
        }
    }

    #[test]
    fn identifiers_and_client_directives_are_not_keywords() {
        // ROWID: SQLite hidden column, absent from lang_keywords.html.
        assert!(!is_keyword(SqlDialect::Sqlite, "ROWID"));
        // GO: sqlcmd/SSMS batch separator, never sent to the server as SQL.
        assert!(!is_keyword(SqlDialect::Mssql, "GO"));
        // DUAL: an ordinary Oracle table name, absent from its reserved words list.
        assert!(!is_keyword(SqlDialect::Oracle, "DUAL"));
    }
}
