//! Oracle driver backed by the `oracle` crate (ODPI-C over OCI).
//!
//! **Why this crate and not a pure-Rust one.** The pure-Rust `oracle-rs` was
//! tried first, since it would have kept RDB free of a native dependency.
//! Measured against a real 23ai server it silently truncated every result set
//! at the server's first 100-row batch, dropped the connection on *any* SQL
//! error without surfacing the `ORA-` message, and returned no primary keys —
//! so tables could not be edited. Those are upstream bugs (issues #8, #12),
//! not something a caller can work around, and silent row loss is the worst
//! failure mode a database browser can have. ODPI-C is compiled into this
//! binary; the Oracle client library it needs (`libclntsh`) is loaded
//! lazily at *runtime*, so builds, tests and CI need nothing installed —
//! only actually connecting to Oracle does.
//!
//! **Blocking client on an async trait.** OCI is synchronous, so every call
//! runs on `spawn_blocking` with the connection behind a `std::sync::Mutex`.
//! That mutex is held only inside the blocking closure, never across an
//! await, so it cannot deadlock the runtime.
//!
//! v1 scope: database (username/password) auth only — no OS auth, Kerberos,
//! wallet or SYSDBA; service-name connect only (`ConnConfig.database` is the
//! service name, e.g. `FREEPDB1`), not SID; and `cancel_running` keeps the
//! trait's no-op default. Oracle 12c or later is assumed, for `OFFSET`/`FETCH`
//! pagination.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use oracle::sql_type::ToSql;
use oracle::Connection;

use rdb_core::conn::{ConnConfig, SslMode};
use rdb_core::driver::Driver;
use rdb_core::error::{RdbError, Result};
use rdb_core::query::Query;
use rdb_core::result::{Cell, Column, ResultSet};
use rdb_core::schema::Schema;
use rdb_core::write::{TableRef, WriteOp};

use crate::convert::{column_type_name, sql_value_to_cell};
use crate::schema::{fold_rows, SchemaRow, COLUMNS_QUERY};
use crate::write_sql;

pub struct OracleDriver {
    conn: Arc<Mutex<Connection>>,
    /// The session's current schema, resolved once at connect. Oracle has no
    /// fixed default-schema name (no `public`, no `dbo`) — it is whatever
    /// user connected — so it has to be asked for rather than hardcoded.
    schema: String,
}

/// Oracle's Easy Connect string. `tcps` is a distinct endpoint rather than an
/// upgrade of `tcp`, so there is no opportunistic mode: Prefer and Require
/// both mean TLS.
fn connect_string(cfg: &ConnConfig) -> String {
    let service = cfg.database.as_deref().unwrap_or("FREEPDB1");
    let proto = match cfg.sslmode {
        SslMode::Disable => "tcp",
        SslMode::Prefer | SslMode::Require => "tcps",
    };
    format!("{proto}://{}:{}/{}", cfg.host, cfg.port, service)
}

/// Run a blocking OCI call on the blocking pool.
///
/// The lock lives entirely inside the closure — it is taken and dropped on
/// the blocking thread — so no guard is ever held across an `.await`.
async fn on_conn<T, F>(conn: &Arc<Mutex<Connection>>, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&Connection) -> std::result::Result<T, oracle::Error> + Send + 'static,
{
    let conn = Arc::clone(conn);
    tokio::task::spawn_blocking(move || {
        let guard = conn
            .lock()
            .map_err(|_| RdbError::Connection("connection lock poisoned".into()))?;
        f(&guard).map_err(|e| RdbError::Query(ora_err(&e)))
    })
    .await
    .map_err(|e| RdbError::Connection(format!("worker thread failed: {e}")))?
}

/// Rows of a single-column query, as text. Used by the several catalog
/// lookups that all want the same shape.
fn one_column(conn: &Connection, sql: &str, params: &[&dyn ToSql]) -> oracle::Result<Vec<String>> {
    let mut out = Vec::new();
    for row in conn.query(sql, params)? {
        let row = row?;
        out.push(row.get::<usize, String>(0).unwrap_or_default());
    }
    Ok(out)
}

#[async_trait]
impl Driver for OracleDriver {
    async fn connect(cfg: &ConnConfig) -> Result<Self> {
        let user = cfg.user.clone();
        let password = cfg.password.clone().unwrap_or_default();
        let dsn = connect_string(cfg);
        let fallback_schema = cfg.user.to_uppercase();

        // Connecting is itself blocking, and it is also where a missing
        // Oracle client library surfaces — reword that one, because the raw
        // ODPI-C message is a wall of URLs.
        tokio::task::spawn_blocking(move || {
            let conn = Connection::connect(&user, &password, &dsn)
                .map_err(|e| RdbError::Connection(connect_err(&e)))?;
            let schema = conn
                .query_row_as::<String>(
                    "SELECT SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA') FROM DUAL",
                    &[],
                )
                .ok()
                .filter(|s| !s.is_empty())
                // Unless the session ran ALTER SESSION SET CURRENT_SCHEMA,
                // the current schema is the connecting user, upper-cased.
                .unwrap_or(fallback_schema);
            Ok(OracleDriver {
                conn: Arc::new(Mutex::new(conn)),
                schema,
            })
        })
        .await
        .map_err(|e| RdbError::Connection(format!("worker thread failed: {e}")))?
    }

    async fn ping(&self) -> Result<()> {
        on_conn(&self.conn, |c| c.ping())
            .await
            .map_err(|e| RdbError::Connection(e.to_string()))
    }

    async fn schema(&self) -> Result<Schema> {
        self.schema_for(&self.schema.clone()).await
    }

    async fn schema_for(&self, schema: &str) -> Result<Schema> {
        // Unquoted identifiers live upper-cased in the data dictionary, so a
        // lower-case schema name from the sidebar would match nothing.
        let owner = schema.to_uppercase();
        let bind = owner.clone();
        let rows: Vec<SchemaRow> = on_conn(&self.conn, move |c| {
            let mut out = Vec::new();
            for row in c.query(COLUMNS_QUERY, &[&bind])? {
                let row = row?;
                out.push((
                    row.get::<usize, String>(0).unwrap_or_default(),
                    row.get::<usize, String>(1).unwrap_or_default(),
                    row.get::<usize, String>(2).unwrap_or_default(),
                    row.get::<usize, i64>(3).unwrap_or(0) != 0,
                    row.get::<usize, i64>(4).unwrap_or(0) != 0,
                    row.get::<usize, i64>(5).unwrap_or(0) != 0,
                ));
            }
            Ok(out)
        })
        .await
        .map_err(|e| RdbError::Schema(e.to_string()))?;
        Ok(fold_rows(&owner, rows))
    }

    async fn list_databases(&self) -> Result<Vec<String>> {
        // v$pdbs exists only on a container database, and reading it needs a
        // privilege an ordinary application user will not have. Neither case
        // is an error worth surfacing — it just means this connection has no
        // database list to switch between.
        Ok(on_conn(&self.conn, |c| {
            one_column(
                c,
                "SELECT name FROM v$pdbs WHERE open_mode = 'READ WRITE' ORDER BY name",
                &[],
            )
        })
        .await
        .unwrap_or_default())
    }

    async fn list_schemas(&self) -> Result<Vec<String>> {
        // In Oracle a schema *is* a user, so all_users doubles as the schema
        // list — minus the several dozen accounts the database ships with,
        // which would otherwise bury the one or two a user cares about.
        // Oracle flags those itself in all_users.oracle_maintained (12c+),
        // which beats hand-maintaining a name list.
        on_conn(&self.conn, |c| {
            one_column(
                c,
                "SELECT username FROM all_users \
                 WHERE oracle_maintained = 'N' ORDER BY username",
                &[],
            )
        })
        .await
        .map_err(|e| RdbError::Schema(e.to_string()))
    }

    async fn query(&self, q: &Query) -> Result<ResultSet> {
        let sql = match q {
            Query::Sql(s) => s.clone(),
            Query::Cql(_) | Query::Command(_) | Query::Mongo(_) => {
                return Err(RdbError::UnsupportedQuery)
            }
        };
        let for_err = sql.clone();
        on_conn(&self.conn, move |c| {
            let mut stmt = c.statement(&sql).build()?;
            // DDL and DML have no result set to iterate; they report a row
            // count instead. `is_query` comes from Oracle's own parse of the
            // statement, so it needs no guessing from the SQL text.
            if !stmt.is_query() {
                stmt.execute(&[])?;
                return Ok(ResultSet::Affected(stmt.row_count().unwrap_or(0)));
            }
            let rows = stmt.query(&[])?;
            let cols: Vec<Column> = rows
                .column_info()
                .iter()
                .map(|c| Column {
                    name: c.name().to_string(),
                    type_name: column_type_name(c.oracle_type()),
                })
                .collect();
            let mut out: Vec<Vec<Cell>> = Vec::new();
            for row in rows {
                let row = row?;
                out.push(row.sql_values().iter().map(sql_value_to_cell).collect());
            }
            Ok(ResultSet::Tabular { cols, rows: out })
        })
        .await
        .map_err(|e| RdbError::Query(with_offset_line(&json_hint(&e.to_string()), &for_err)))
    }

    async fn primary_key(&self, table: &TableRef) -> Result<Vec<String>> {
        let owner = table
            .schema
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.schema)
            .to_uppercase();
        let name = table.name.to_uppercase();
        on_conn(&self.conn, move |c| {
            one_column(
                c,
                "SELECT cc.column_name FROM all_constraints c \
                 JOIN all_cons_columns cc \
                   ON cc.owner = c.owner AND cc.constraint_name = c.constraint_name \
                 WHERE c.constraint_type = 'P' AND c.owner = :1 AND c.table_name = :2 \
                 ORDER BY cc.position",
                &[&owner, &name],
            )
        })
        .await
        .map_err(|e| RdbError::Schema(e.to_string()))
    }

    async fn count(&self, table: &TableRef) -> Result<u64> {
        let sql = format!("SELECT COUNT(*) FROM {}", write_sql::table_name(table));
        let n = on_conn(&self.conn, move |c| c.query_row_as::<i64>(&sql, &[])).await?;
        Ok(n.max(0) as u64)
    }

    async fn commit(&self, ops: &[WriteOp]) -> Result<u64> {
        if ops.is_empty() {
            return Ok(0);
        }
        let stmts: Vec<String> = ops
            .iter()
            .map(|op| match op {
                WriteOp::Update { table, pk, changes } => write_sql::update_sql(table, pk, changes),
                WriteOp::Insert { table, values } => write_sql::insert_sql(table, values),
                WriteOp::Delete { table, pk } => write_sql::delete_sql(table, pk),
            })
            .collect();

        on_conn(&self.conn, move |c| {
            // Oracle opens a transaction implicitly on the first DML and
            // holds it until commit or rollback, so there is no BEGIN to
            // issue — only the obligation to end it on both paths.
            let mut affected = 0u64;
            for sql in &stmts {
                match c.execute(sql, &[]) {
                    Ok(stmt) => affected += stmt.row_count().unwrap_or(0),
                    Err(e) => {
                        let _ = c.rollback();
                        return Err(e);
                    }
                }
            }
            c.commit()?;
            Ok(affected)
        })
        .await
    }

    async fn close(self) -> Result<()> {
        on_conn(&self.conn, |c| c.close())
            .await
            .map_err(|e| RdbError::Connection(e.to_string()))
    }
}

/// A missing Oracle client library is the one error a new user is most likely
/// to hit, and ODPI-C reports it as several lines of help URLs. Say the one
/// thing they need to do instead.
fn connect_err(e: &oracle::Error) -> String {
    let msg = e.to_string();
    if msg.contains("DPI-1047") {
        return "Oracle Client library not found. Install Oracle Instant Client \
                (Basic or Basic Light) and make sure it is on the library path."
            .to_string();
    }
    ora_err(e)
}

/// `ORA-00942: table or view does not exist` — the code and message, without
/// ODPI-C's trailing `fn_name`/`action` noise.
fn ora_err(e: &oracle::Error) -> String {
    match e.db_error() {
        Some(db) => format!("ORA-{:05}: {}", db.code(), db.message().trim()),
        None => e.to_string(),
    }
}

/// Oracle 21c's native `JSON` column type has no binding in the `oracle`
/// crate yet (kubo/rust-oracle#107), so a `SELECT *` over a table containing
/// one fails before any row is read. The raw message says only "unsupported
/// Oracle type JSON", which leaves the user with nowhere to go — name the
/// workaround instead.
fn json_hint(msg: &str) -> String {
    if msg.contains("unsupported Oracle type JSON") {
        return "Oracle JSON columns are not supported by this driver yet. \
                Select the column as JSON_SERIALIZE(<col> RETURNING VARCHAR2) \
                to read it as text."
            .to_string();
    }
    msg.to_string()
}

/// Oracle reports where a statement broke as a character offset. Turn that
/// into the 1-based line the query editor highlights via its `[[rdb-line:N]]`
/// marker — the same contract `driver-mssql` fills from SQL Server's typed
/// line number.
fn with_offset_line(msg: &str, sql: &str) -> String {
    match offset_of(msg).and_then(|off| line_of_offset(sql, off)) {
        Some(n) => format!("[[rdb-line:{n}]] {msg}"),
        None => msg.to_string(),
    }
}

/// ODPI-C puts the offset in the error's `offset` field, which reaches us
/// only through the rendered message, as `... offset: N ...`.
fn offset_of(msg: &str) -> Option<usize> {
    let rest = msg.split("offset: ").nth(1)?;
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn line_of_offset(sql: &str, offset: usize) -> Option<u32> {
    if offset == 0 || offset > sql.len() {
        return None;
    }
    let line = sql[..offset].bytes().filter(|b| *b == b'\n').count() + 1;
    (line > 1).then_some(line as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(ssl: SslMode) -> ConnConfig {
        ConnConfig {
            host: "db.example.com".into(),
            port: 1521,
            user: "scott".into(),
            database: Some("ORCLPDB1".into()),
            password: Some("tiger".into()),
            sslmode: ssl,
            params: None,
            ssh: None,
        }
    }

    #[test]
    fn connect_string_uses_easy_connect_with_the_service_name() {
        assert_eq!(
            connect_string(&cfg(SslMode::Disable)),
            "tcp://db.example.com:1521/ORCLPDB1"
        );
    }

    #[test]
    fn tls_switches_the_protocol_not_just_a_flag() {
        // Oracle serves TLS on a separate endpoint, so Prefer cannot silently
        // fall back to plaintext the way Postgres's can.
        assert_eq!(
            connect_string(&cfg(SslMode::Require)),
            "tcps://db.example.com:1521/ORCLPDB1"
        );
        assert_eq!(
            connect_string(&cfg(SslMode::Prefer)),
            "tcps://db.example.com:1521/ORCLPDB1"
        );
    }

    #[test]
    fn missing_service_name_falls_back_rather_than_producing_an_empty_dsn() {
        let mut c = cfg(SslMode::Disable);
        c.database = None;
        assert!(connect_string(&c).ends_with("/FREEPDB1"));
    }

    #[test]
    fn a_multiline_statement_reports_the_failing_line() {
        let sql = "SELECT 1\nFROM dual\nWHERE bogus";
        // Offset 19 lands on line 3.
        let marked = with_offset_line("ORA-00904: bad, offset: 19 xyz", sql);
        assert_eq!(marked, "[[rdb-line:3]] ORA-00904: bad, offset: 19 xyz");
    }

    #[test]
    fn a_single_line_statement_gets_no_marker() {
        // Line 1 needs no highlight, and no offset at all must not invent one.
        assert_eq!(
            with_offset_line("ORA-00904: bad, offset: 3 x", "SELECT bogus"),
            "ORA-00904: bad, offset: 3 x"
        );
        assert_eq!(
            with_offset_line("ORA-00942: nope", "SELECT 1"),
            "ORA-00942: nope"
        );
    }

    #[test]
    fn a_json_column_failure_names_the_workaround() {
        let hinted = json_hint("internal error: unsupported Oracle type JSON");
        assert!(hinted.contains("JSON_SERIALIZE"));
        // Unrelated errors must pass through untouched.
        assert_eq!(json_hint("ORA-00942: nope"), "ORA-00942: nope");
    }

    #[test]
    fn offset_past_the_end_of_the_statement_is_ignored() {
        assert_eq!(line_of_offset("SELECT 1", 999), None);
    }
}
