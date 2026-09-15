//! SQL format spec — one per `SqlDialect` so a vendor keyword (MySQL's
//! `AUTO_INCREMENT`, MSSQL's `TOP`, ClickHouse's `PREWHERE`, Oracle's
//! `ROWNUM`, …) gets uppercased on that dialect's Format button, not just
//! the common superset.

use super::Spec;
use rdb_connstore::SqlDialect;

// Each arm is a non-capturing closure, so it coerces to the plain
// `fn(&str) -> bool` `Spec` expects — no boxing, no dialect passed at
// format time.
fn is_keyword_for(dialect: SqlDialect) -> fn(&str) -> bool {
    match dialect {
        SqlDialect::Postgres => |w| crate::editor::sql::is_keyword(SqlDialect::Postgres, w),
        SqlDialect::MySql => |w| crate::editor::sql::is_keyword(SqlDialect::MySql, w),
        SqlDialect::Sqlite => |w| crate::editor::sql::is_keyword(SqlDialect::Sqlite, w),
        SqlDialect::Mssql => |w| crate::editor::sql::is_keyword(SqlDialect::Mssql, w),
        SqlDialect::Clickhouse => |w| crate::editor::sql::is_keyword(SqlDialect::Clickhouse, w),
        SqlDialect::Oracle => |w| crate::editor::sql::is_keyword(SqlDialect::Oracle, w),
    }
}

const CLAUSE_STARTERS: &[&str] = &[
    "FROM", "WHERE", "GROUP", "HAVING", "ORDER", "LIMIT", "OFFSET", "JOIN", "LEFT", "RIGHT",
    "INNER", "FULL", "UNION", "VALUES", "SET",
];

const JOIN_QUALIFIERS: &[&str] = &["LEFT", "RIGHT", "INNER", "FULL", "OUTER", "CROSS"];

pub fn spec(dialect: SqlDialect) -> Spec {
    Spec {
        is_keyword: is_keyword_for(dialect),
        clause_starters: CLAUSE_STARTERS,
        join_qualifiers: JOIN_QUALIFIERS,
    }
}
