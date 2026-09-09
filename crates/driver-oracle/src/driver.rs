//! Oracle driver backed by Oracle's own pure-Rust `oracledb` crate.
//!
//! **Why this crate.** RDB shipped Oracle first on `oracle` (ODPI-C over OCI),
//! which made Oracle the only engine that needed something installed before it
//! could connect: the Oracle Instant Client, loaded at runtime. A pure-Rust
//! third-party crate had been tried before that and rejected on measurement —
//! it silently truncated every result set at the server's first 100-row batch
//! and returned no primary keys. `oracledb` is Oracle's own thin driver: it
//! speaks the wire protocol directly, so there is no client library and no
//! native dependency, and Oracle maintains it. Oracle is now an engine like
//! any other.
//!
//! **Blocking client on an async trait.** `oracledb`'s API is synchronous, so
//! every call runs on `spawn_blocking` with the connection behind a
//! `std::sync::Mutex`. That mutex is held only inside the blocking closure,
//! never across an await, so it cannot deadlock the runtime.
//!
//! v1 scope: database (username/password) auth only — no OS auth, Kerberos,
//! wallet or SYSDBA; service-name connect only (`ConnConfig.database` is the
//! service name, e.g. `FREEPDB1`), not SID; and `cancel_running` keeps the
//! trait's no-op default. Oracle 12c or later is assumed, for `OFFSET`/`FETCH`
//! pagination.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use oracledb::{Config, Connection, ErrorKind, ToDbValue};

use rdb_core::conn::{ConnConfig, SslMode};
use rdb_core::driver::Driver;
use rdb_core::error::{RdbError, Result};
use rdb_core::query::Query;
use rdb_core::result::{Cell, Column, ResultSet};
use rdb_core::schema::Schema;
use rdb_core::write::{TableRef, WriteOp};

use crate::convert::{cell_at, column_type_name};
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

/// Run a blocking database call on the blocking pool.
///
/// The lock lives entirely inside the closure — it is taken and dropped on
/// the blocking thread — so no guard is ever held across an `.await`. The
/// closure gets `&mut` because a few calls (`close`) need it and the rest do
/// not care.
async fn on_conn<T, F>(conn: &Arc<Mutex<Connection>>, f: F) -> Result<T>
where
    T: Send + 'static,
    F: FnOnce(&mut Connection) -> std::result::Result<T, oracledb::Error> + Send + 'static,
{
    let conn = Arc::clone(conn);
    tokio::task::spawn_blocking(move || {
        let mut guard = conn
            .lock()
            .map_err(|_| RdbError::Connection("connection lock poisoned".into()))?;
        f(&mut guard).map_err(|e| RdbError::Query(ora_err(&e)))
    })
    .await
    .map_err(|e| RdbError::Connection(format!("worker thread failed: {e}")))?
}

/// Rows of a single-column query, as text. Used by the several catalog
/// lookups that all want the same shape.
fn one_column(
    conn: &Connection,
    sql: &str,
    params: &[&dyn ToDbValue],
) -> std::result::Result<Vec<String>, oracledb::Error> {
    let mut out = Vec::new();
    for row in conn.query(sql, params)? {
        let row = row?;
        out.push(row.get::<Option<String>>(0)?.unwrap_or_default());
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

        // Connecting is itself blocking.
        tokio::task::spawn_blocking(move || {
            let config = Config::default()
                .set_credentials(&user, &password)
                .set_connect_string(&dsn)
                .map_err(|e| RdbError::Connection(ora_err(&e)))?;
            let conn = oracledb::connect(config).map_err(|e| RdbError::Connection(ora_err(&e)))?;
            let schema = conn
                .query_row(
                    "SELECT SYS_CONTEXT('USERENV', 'CURRENT_SCHEMA') FROM DUAL",
                    &[],
                )
                .and_then(|r| r.get::<Option<String>>(0))
                .ok()
                .flatten()
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
                    row.get::<Option<String>>(0)?.unwrap_or_default(),
                    row.get::<Option<String>>(1)?.unwrap_or_default(),
                    row.get::<Option<String>>(2)?.unwrap_or_default(),
                    row.get::<Option<i64>>(3)?.unwrap_or(0) != 0,
                    row.get::<Option<i64>>(4)?.unwrap_or(0) != 0,
                    row.get::<Option<i64>>(5)?.unwrap_or(0) != 0,
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
        on_conn(&self.conn, move |c| {
            // DDL, DML and PL/SQL have no result set to iterate; they report a
            // row count instead.
            if !is_query(&sql) {
                return Ok(ResultSet::Affected(c.execute(&sql, &[])?.rows_affected()));
            }
            let cursor = c.query(&sql, &[])?;
            let meta = cursor.columns().clone();
            let cols: Vec<Column> = meta
                .iter()
                .map(|m| Column {
                    name: m.name().to_string(),
                    type_name: column_type_name(m),
                })
                .collect();
            let mut out: Vec<Vec<Cell>> = Vec::new();
            for row in cursor {
                let row = row?;
                out.push(
                    meta.iter()
                        .enumerate()
                        .map(|(i, m)| cell_at(&row, i, m))
                        .collect(),
                );
            }
            Ok(ResultSet::Tabular { cols, rows: out })
        })
        .await
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
        let n = on_conn(&self.conn, move |c| {
            c.query_row(&sql, &[])?.get::<Option<i64>>(0)
        })
        .await?;
        Ok(n.unwrap_or(0).max(0) as u64)
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
                    Ok(res) => affected += res.rows_affected(),
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

/// Whether a statement produces a result set to iterate rather than a row
/// count, decided from its leading keyword.
///
/// `oracledb` classifies statements this same way internally
/// (`Statement::determine_statement_type`, which routes on `SELECT`/`WITH`
/// versus DML/DDL/PL/SQL keywords) but keeps the answer crate-private, so
/// this mirrors that table rather than inventing a different one. `Cursor`
/// does report an empty column list for a non-query, but it carries no
/// affected-row count, and "3 rows updated" is the whole result of a DML
/// statement — so routing has to happen before execution, not after.
///
/// ponytail: leading keyword only. `TABLE(...)`, `(SELECT ...)` and other
/// rarities route to `execute`, which still runs them correctly but reports a
/// row count instead of the rows. Replace this with upstream's own answer if
/// it is ever exposed.
fn is_query(sql: &str) -> bool {
    matches!(
        leading_keyword(sql).to_uppercase().as_str(),
        "SELECT" | "WITH"
    )
}

/// The first bare word of a statement, skipping whitespace and both comment
/// forms. A query editor's buffer routinely opens with a `--` note above the
/// statement, so a naive `trim_start` would classify most saved queries wrong.
fn leading_keyword(sql: &str) -> &str {
    let mut rest = sql.trim_start();
    loop {
        if let Some(after) = rest.strip_prefix("--") {
            rest = after
                .split_once('\n')
                .map_or("", |(_, tail)| tail)
                .trim_start();
        } else if let Some(after) = rest.strip_prefix("/*") {
            rest = after
                .split_once("*/")
                .map_or("", |(_, tail)| tail)
                .trim_start();
        } else {
            break;
        }
    }
    let end = rest
        .find(|c: char| !c.is_alphanumeric() && c != '_')
        .unwrap_or(rest.len());
    &rest[..end]
}

/// `ORA-00942: table or view does not exist` — the server's own message,
/// which `oracledb` passes through verbatim in `ErrorKind::DbError`. Anything
/// else is a client-side failure and its `Display` is already the best text
/// available.
fn ora_err(e: &oracledb::Error) -> String {
    match e.kind() {
        ErrorKind::DbError(msg) => msg.trim().to_string(),
        _ => e.to_string(),
    }
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
    fn selects_and_ctes_are_queries() {
        assert!(is_query("SELECT * FROM dual"));
        assert!(is_query("  select 1 from dual"));
        assert!(is_query("WITH t AS (SELECT 1 FROM dual) SELECT * FROM t"));
    }

    #[test]
    fn dml_ddl_and_plsql_are_not_queries() {
        assert!(!is_query("UPDATE users SET name = 'x'"));
        assert!(!is_query("INSERT INTO users VALUES (1)"));
        assert!(!is_query("DELETE FROM users"));
        assert!(!is_query("MERGE INTO users USING dual ON (1=1)"));
        assert!(!is_query("CREATE TABLE t (id NUMBER)"));
        assert!(!is_query("TRUNCATE TABLE t"));
        assert!(!is_query("BEGIN NULL; END;"));
        assert!(!is_query(""));
    }

    #[test]
    fn a_leading_comment_does_not_hide_the_keyword() {
        // A saved query routinely opens with a note above the statement.
        assert!(is_query("-- daily totals\nSELECT * FROM dual"));
        assert!(is_query("/* daily totals */ SELECT * FROM dual"));
        assert!(is_query(
            "-- one\n-- two\n\n  /* three */\nSELECT 1 FROM dual"
        ));
        assert!(!is_query("-- careful\nDROP TABLE t"));
    }

    #[test]
    fn an_unterminated_comment_is_not_mistaken_for_a_query() {
        assert!(!is_query("/* never closed SELECT"));
        assert!(!is_query("-- only a comment"));
    }
}
