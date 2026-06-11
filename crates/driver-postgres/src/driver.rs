use async_trait::async_trait;
use tokio::task::JoinHandle;
use tokio_postgres::{Client, NoTls};

use dbm_core::conn::ConnConfig;
use dbm_core::driver::Driver;
use dbm_core::error::{DbmError, Result};
use dbm_core::query::Query;
use dbm_core::result::{Column, ResultSet, Row};
use dbm_core::schema::{Container, ContainerKind, Database, Field, Schema};

use crate::conn_string::build_conn_string;

/// A `Driver` backed by tokio-postgres over a single connection.
///
/// TLS limitation (MVP): connections always use `NoTls`. `SslMode::Require`
/// is accepted and written to the conn string but NOT enforced at the
/// transport layer — real enforcement needs `tokio-postgres-rustls` and is a
/// documented follow-up. `Disable`/`Prefer` behave correctly against a plain
/// server.
pub struct PostgresDriver {
    client: Client,
    /// Handle to the spawned connection-driver task; aborted on `close`.
    conn_task: JoinHandle<()>,
}

#[async_trait]
impl Driver for PostgresDriver {
    async fn connect(cfg: &ConnConfig) -> Result<Self> {
        let conn_str = build_conn_string(cfg);
        let (client, connection) = tokio_postgres::connect(&conn_str, NoTls)
            .await
            .map_err(|e| DbmError::Connection(e.to_string()))?;
        // The connection object drives the protocol; it must be polled on its
        // own task for the client to make progress.
        let conn_task = tokio::spawn(async move {
            let _ = connection.await;
        });
        Ok(PostgresDriver { client, conn_task })
    }

    async fn ping(&self) -> Result<()> {
        self.client
            .query("SELECT 1", &[])
            .await
            .map_err(|e| DbmError::Connection(e.to_string()))?;
        Ok(())
    }

    async fn schema(&self) -> Result<Schema> {
        schema_impl(&self.client).await
    }

    async fn query(&self, q: &Query) -> Result<ResultSet> {
        query_impl(&self.client, q).await
    }

    async fn close(self) -> Result<()> {
        // Drop the client first so the connection future can complete, then
        // abort the connection task to release the spawned future promptly.
        let PostgresDriver { client, conn_task } = self;
        drop(client);
        conn_task.abort();
        Ok(())
    }
}

async fn query_impl(client: &Client, q: &Query) -> Result<ResultSet> {
    let sql = match q {
        Query::Sql(s) => s,
        Query::Command(_) | Query::Mongo(_) => return Err(DbmError::UnsupportedQuery),
    };

    if is_row_returning(sql) {
        let rows = client
            .query(sql, &[])
            .await
            .map_err(|e| DbmError::Query(e.to_string()))?;
        let cols = column_meta(&rows);
        let mut out_rows: Vec<Row> = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut cells: Row = Vec::with_capacity(cols.len());
            for idx in 0..cols.len() {
                cells.push(crate::type_map::extract_cell(row, idx));
            }
            out_rows.push(cells);
        }
        Ok(ResultSet::Tabular { cols, rows: out_rows })
    } else {
        let affected = client
            .execute(sql, &[])
            .await
            .map_err(|e| DbmError::Query(e.to_string()))?;
        Ok(ResultSet::Affected(affected))
    }
}

/// Heuristic: does this statement return rows? Covers the common row-returning
/// leading keywords. Anything else (INSERT/UPDATE/DELETE/CREATE/DROP/...) is
/// treated as a write and routed to `execute` for an affected-row count.
fn is_row_returning(sql: &str) -> bool {
    let head = sql.trim_start().to_ascii_lowercase();
    head.starts_with("select")
        || head.starts_with("with")
        || head.starts_with("show")
        || head.starts_with("values")
        || head.starts_with("table")
        || head.starts_with("explain")
}

/// Build `Column` metadata from the first row (empty result -> no columns).
fn column_meta(rows: &[tokio_postgres::Row]) -> Vec<Column> {
    match rows.first() {
        Some(first) => first
            .columns()
            .iter()
            .map(|c| Column {
                name: c.name().to_string(),
                type_name: c.type_().name().to_string(),
            })
            .collect(),
        None => Vec::new(),
    }
}

async fn schema_impl(client: &Client) -> Result<Schema> {
    // The current database name groups everything under one logical Database.
    let db_row = client
        .query_one("SELECT current_database()", &[])
        .await
        .map_err(|e| DbmError::Schema(e.to_string()))?;
    let db_name: String = db_row
        .try_get(0)
        .map_err(|e| DbmError::Schema(e.to_string()))?;

    // User tables + columns from information_schema, public schema only.
    // Ordered so columns of the same table are contiguous for grouping.
    let rows = client
        .query(
            "SELECT c.table_name, c.column_name, c.data_type, c.is_nullable \
             FROM information_schema.columns c \
             JOIN information_schema.tables t \
               ON t.table_schema = c.table_schema AND t.table_name = c.table_name \
             WHERE c.table_schema = 'public' AND t.table_type = 'BASE TABLE' \
             ORDER BY c.table_name, c.ordinal_position",
            &[],
        )
        .await
        .map_err(|e| DbmError::Schema(e.to_string()))?;

    let mut containers: Vec<Container> = Vec::new();
    for row in &rows {
        let table: String = row.get(0);
        let column: String = row.get(1);
        let data_type: String = row.get(2);
        let is_nullable: String = row.get(3); // 'YES' | 'NO'
        let field = Field {
            name: column,
            type_name: data_type,
            nullable: is_nullable.eq_ignore_ascii_case("YES"),
        };
        match containers.last_mut() {
            Some(last) if last.name == table => last.fields.push(field),
            _ => containers.push(Container {
                name: table,
                kind: ContainerKind::Table,
                fields: vec![field],
            }),
        }
    }

    Ok(Schema {
        databases: vec![Database {
            name: db_name,
            containers,
        }],
    })
}
