use async_trait::async_trait;
use native_tls::TlsConnector;
use postgres_native_tls::MakeTlsConnector;
use tokio::task::JoinHandle;
use tokio_postgres::{Client, NoTls};

use rdbs_core::conn::{ConnConfig, SslMode};
use rdbs_core::driver::Driver;
use rdbs_core::error::{RdbsError, Result};
use rdbs_core::query::Query;
use rdbs_core::result::{Column, ResultSet, Row};
use rdbs_core::schema::{Container, ContainerKind, Database, Field, Schema};
use rdbs_core::write::{TableRef, WriteOp};

use crate::conn_string::build_conn_string;
use crate::write_sql;

/// A `Driver` backed by tokio-postgres over a single connection.
///
/// TLS: `SslMode::Disable` uses a plaintext `NoTls` connection. `Prefer` and
/// `Require` use a native-TLS connector that encrypts the transport but does
/// NOT verify the server certificate or hostname — matching libpq's `require`
/// semantics (encrypt, don't validate). Certificate-validating modes
/// (verify-ca / verify-full) are a future addition.
pub struct PostgresDriver {
    client: Client,
    /// Handle to the spawned connection-driver task; aborted on `close`.
    conn_task: JoinHandle<()>,
}

#[async_trait]
impl Driver for PostgresDriver {
    async fn connect(cfg: &ConnConfig) -> Result<Self> {
        let conn_str = build_conn_string(cfg);
        // Disable -> plaintext. Prefer/Require -> TLS (negotiated per the
        // sslmode token in the conn string). The connector accepts any cert so
        // managed (DigitalOcean/Supabase/RDS) and self-signed servers both work.
        let (client, conn_task) = if matches!(cfg.sslmode, SslMode::Disable) {
            let (client, connection) = tokio_postgres::connect(&conn_str, NoTls)
                .await
                .map_err(|e| RdbsError::Connection(e.to_string()))?;
            let conn_task = tokio::spawn(async move {
                let _ = connection.await;
            });
            (client, conn_task)
        } else {
            let tls = TlsConnector::builder()
                .danger_accept_invalid_certs(true)
                .danger_accept_invalid_hostnames(true)
                .build()
                .map_err(|e| RdbsError::Connection(e.to_string()))?;
            let connector = MakeTlsConnector::new(tls);
            let (client, connection) = tokio_postgres::connect(&conn_str, connector)
                .await
                .map_err(|e| RdbsError::Connection(e.to_string()))?;
            let conn_task = tokio::spawn(async move {
                let _ = connection.await;
            });
            (client, conn_task)
        };
        Ok(PostgresDriver { client, conn_task })
    }

    async fn ping(&self) -> Result<()> {
        self.client
            .query("SELECT 1", &[])
            .await
            .map_err(|e| RdbsError::Connection(e.to_string()))?;
        Ok(())
    }

    async fn schema(&self) -> Result<Schema> {
        schema_impl(&self.client, "public").await
    }

    async fn schema_for(&self, schema: &str) -> Result<Schema> {
        schema_impl(&self.client, schema).await
    }

    async fn list_schemas(&self) -> Result<Vec<String>> {
        let rows = self
            .client
            .query(
                "SELECT schema_name FROM information_schema.schemata \
                 WHERE schema_name NOT LIKE 'pg_%' \
                 AND schema_name <> 'information_schema' ORDER BY 1",
                &[],
            )
            .await
            .map_err(|e| RdbsError::Schema(e.to_string()))?;
        Ok(rows.iter().map(|r| r.get::<_, String>(0)).collect())
    }

    async fn query(&self, q: &Query) -> Result<ResultSet> {
        query_impl(&self.client, q).await
    }

    async fn primary_key(&self, table: &TableRef) -> Result<Vec<String>> {
        let schema = table.schema.as_deref().unwrap_or("public");
        let rows = self
            .client
            .query(
                "SELECT a.attname \
                 FROM pg_index i \
                 JOIN pg_class c ON c.oid = i.indrelid \
                 JOIN pg_namespace n ON n.oid = c.relnamespace \
                 JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = ANY(i.indkey) \
                 WHERE i.indisprimary AND c.relname = $1 AND n.nspname = $2 \
                 ORDER BY array_position(i.indkey, a.attnum)",
                &[&table.name, &schema],
            )
            .await
            .map_err(|e| RdbsError::Schema(e.to_string()))?;
        Ok(rows.iter().map(|r| r.get::<_, String>(0)).collect())
    }

    async fn count(&self, table: &TableRef) -> Result<u64> {
        let sql = format!("SELECT count(*) FROM {}", write_sql::table_name(table));
        let row = self
            .client
            .query_one(&sql, &[])
            .await
            .map_err(|e| RdbsError::Query(e.to_string()))?;
        let n: i64 = row.get(0);
        Ok(n as u64)
    }

    async fn commit(&self, ops: &[WriteOp]) -> Result<u64> {
        if ops.is_empty() {
            return Ok(0);
        }
        let run = |sql: String| async move {
            self.client
                .execute(&sql, &[])
                .await
                .map_err(|e| RdbsError::Query(e.to_string()))
        };
        self.client
            .execute("BEGIN", &[])
            .await
            .map_err(|e| RdbsError::Query(e.to_string()))?;
        let mut affected = 0u64;
        for op in ops {
            let sql = match op {
                WriteOp::Update { table, pk, changes } => write_sql::update_sql(table, pk, changes),
                WriteOp::Insert { table, values } => write_sql::insert_sql(table, values),
                WriteOp::Delete { table, pk } => write_sql::delete_sql(table, pk),
            };
            match run(sql).await {
                Ok(n) => affected += n,
                Err(e) => {
                    let _ = self.client.execute("ROLLBACK", &[]).await;
                    return Err(e);
                }
            }
        }
        self.client
            .execute("COMMIT", &[])
            .await
            .map_err(|e| RdbsError::Query(e.to_string()))?;
        Ok(affected)
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
        Query::Command(_) | Query::Mongo(_) => return Err(RdbsError::UnsupportedQuery),
    };

    if is_row_returning(sql) {
        let rows = client
            .query(sql, &[])
            .await
            .map_err(|e| RdbsError::Query(e.to_string()))?;
        let cols = column_meta(&rows);
        let mut out_rows: Vec<Row> = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut cells: Row = Vec::with_capacity(cols.len());
            for idx in 0..cols.len() {
                cells.push(crate::type_map::extract_cell(row, idx));
            }
            out_rows.push(cells);
        }
        Ok(ResultSet::Tabular {
            cols,
            rows: out_rows,
        })
    } else {
        let affected = client
            .execute(sql, &[])
            .await
            .map_err(|e| RdbsError::Query(e.to_string()))?;
        Ok(ResultSet::Affected(affected))
    }
}

/// Heuristic: does this statement return rows? Covers the common row-returning
/// leading keywords. Anything else (INSERT/UPDATE/DELETE/CREATE/DROP/...) is
/// treated as a write and routed to `execute` for an affected-row count.
fn is_row_returning(sql: &str) -> bool {
    // Skip leading `-- line` comments and blank lines before classifying.
    let head = sql
        .lines()
        .map(str::trim_start)
        .find(|l| !l.is_empty() && !l.starts_with("--"))
        .unwrap_or("")
        .to_ascii_lowercase();
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

async fn schema_impl(client: &Client, schema: &str) -> Result<Schema> {
    // The current database name groups everything under one logical Database.
    let db_row = client
        .query_one("SELECT current_database()", &[])
        .await
        .map_err(|e| RdbsError::Schema(e.to_string()))?;
    let db_name: String = db_row
        .try_get(0)
        .map_err(|e| RdbsError::Schema(e.to_string()))?;

    // User tables + columns from information_schema, scoped to one schema.
    // Ordered so columns of the same table are contiguous for grouping.
    let rows = client
        .query(
            "SELECT c.table_name, c.column_name, c.data_type, c.is_nullable \
             FROM information_schema.columns c \
             JOIN information_schema.tables t \
               ON t.table_schema = c.table_schema AND t.table_name = c.table_name \
             WHERE c.table_schema = $1 AND t.table_type = 'BASE TABLE' \
             ORDER BY c.table_name, c.ordinal_position",
            &[&schema],
        )
        .await
        .map_err(|e| RdbsError::Schema(e.to_string()))?;

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

    // Stored functions (public schema): name + full CREATE source for the
    // function view. Per-row source failures (e.g. C-language internals that
    // pg_get_functiondef rejects) are skipped, never fatal.
    let mut functions: Vec<rdbs_core::schema::Function> = Vec::new();
    if let Ok(rows) = client
        .query(
            "SELECT p.proname, pg_catalog.pg_get_functiondef(p.oid) \
             FROM pg_catalog.pg_proc p \
             JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
             WHERE n.nspname = $1 AND p.prokind = 'f' \
             ORDER BY p.proname",
            &[&schema],
        )
        .await
    {
        for row in &rows {
            let name: String = row.get(0);
            if let Ok(def) = row.try_get::<_, String>(1) {
                functions.push(rdbs_core::schema::Function {
                    name,
                    definition: def,
                });
            }
        }
    }

    Ok(Schema {
        databases: vec![Database {
            functions,
            name: db_name,
            containers,
        }],
    })
}
