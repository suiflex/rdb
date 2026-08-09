//! SQL Server / T-SQL driver backed by `tiberius`. `tiberius` speaks Tokio's
//! own I/O traits through `tokio_util::compat` (the one bit of connection
//! boilerplate `driver-mysql`/`driver-postgres` don't need — their client
//! crates own their I/O internally), and its `Client` methods take `&mut
//! self`, so a single connection is guarded by a `tokio::sync::Mutex` rather
//! than pooled like `driver-mysql`. v1 scope: SQL-auth only (no Windows/AD
//! auth — `ConnConfig` has no concept of it and no other driver here needs
//! one either), and `query_stream` uses the trait default (buffer-then-chunk)
//! rather than a true server-side-cursor override.

use async_trait::async_trait;
use futures_util::TryStreamExt;
use tiberius::{AuthMethod, Client, Config, EncryptionLevel, QueryItem};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};

use rdb_core::conn::{ConnConfig, SslMode};
use rdb_core::driver::Driver;
use rdb_core::error::{RdbError, Result};
use rdb_core::query::Query;
use rdb_core::result::{Cell, Column, ResultSet};
use rdb_core::schema::Schema;
use rdb_core::write::{TableRef, WriteOp};

use crate::convert::{column_data_to_cell, column_type_name};
use crate::schema::{fold_rows, SchemaRow, COLUMNS_QUERY};
use crate::write_sql;

type MssqlClient = Client<Compat<TcpStream>>;

/// SQL Server's default schema — the rough equivalent of Postgres's `public`.
const DEFAULT_SCHEMA: &str = "dbo";

/// SQL Server driver backed by a single `tiberius` connection.
pub struct MssqlDriver {
    client: Mutex<MssqlClient>,
}

fn build_config(cfg: &ConnConfig) -> Config {
    let mut config = Config::new();
    config.host(&cfg.host);
    config.port(cfg.port);
    config.authentication(AuthMethod::sql_server(
        &cfg.user,
        cfg.password.clone().unwrap_or_default(),
    ));
    if let Some(db) = &cfg.database {
        config.database(db);
    }
    match cfg.sslmode {
        SslMode::Disable => {
            config.encryption(EncryptionLevel::NotSupported);
        }
        // Prefer/Require: enable TLS. Accept invalid certs only to keep MVP
        // connectable against self-signed servers; tighten post-MVP — same
        // MVP posture driver-mysql/driver-postgres already take.
        SslMode::Prefer => {
            config.encryption(EncryptionLevel::On);
            config.trust_cert();
        }
        SslMode::Require => {
            config.encryption(EncryptionLevel::Required);
            config.trust_cert();
        }
    }
    config
}

#[async_trait]
impl Driver for MssqlDriver {
    async fn connect(cfg: &ConnConfig) -> Result<Self> {
        let config = build_config(cfg);
        let tcp = TcpStream::connect(config.get_addr())
            .await
            .map_err(|e| RdbError::Connection(e.to_string()))?;
        tcp.set_nodelay(true)
            .map_err(|e| RdbError::Connection(e.to_string()))?;
        let client = Client::connect(config, tcp.compat_write())
            .await
            .map_err(|e| RdbError::Connection(e.to_string()))?;
        Ok(MssqlDriver {
            client: Mutex::new(client),
        })
    }

    async fn ping(&self) -> Result<()> {
        let mut client = self.client.lock().await;
        client
            .simple_query("SELECT 1")
            .await
            .map_err(|e| RdbError::Connection(e.to_string()))?
            .into_row()
            .await
            .map_err(|e| RdbError::Connection(e.to_string()))?;
        Ok(())
    }

    async fn schema(&self) -> Result<Schema> {
        schema_impl(&self.client, DEFAULT_SCHEMA).await
    }

    async fn schema_for(&self, schema: &str) -> Result<Schema> {
        schema_impl(&self.client, schema).await
    }

    async fn list_databases(&self) -> Result<Vec<String>> {
        let mut client = self.client.lock().await;
        let rows = client
            .query(
                "SELECT name FROM sys.databases \
                 WHERE database_id > 4 ORDER BY name",
                &[],
            )
            .await
            .map_err(|e| RdbError::Schema(e.to_string()))?
            .into_first_result()
            .await
            .map_err(|e| RdbError::Schema(e.to_string()))?;
        Ok(rows
            .iter()
            .filter_map(|r| r.get::<&str, _>(0))
            .map(str::to_string)
            .collect())
    }

    async fn list_schemas(&self) -> Result<Vec<String>> {
        let mut client = self.client.lock().await;
        let rows = client
            .query(
                "SELECT name FROM sys.schemas \
                 WHERE name NOT IN ( \
                     'sys', 'guest', 'INFORMATION_SCHEMA', \
                     'db_owner', 'db_accessadmin', 'db_securityadmin', 'db_ddladmin', \
                     'db_backupoperator', 'db_datareader', 'db_datawriter', \
                     'db_denydatareader', 'db_denydatawriter' \
                 ) ORDER BY name",
                &[],
            )
            .await
            .map_err(|e| RdbError::Schema(e.to_string()))?
            .into_first_result()
            .await
            .map_err(|e| RdbError::Schema(e.to_string()))?;
        Ok(rows
            .iter()
            .filter_map(|r| r.get::<&str, _>(0))
            .map(str::to_string)
            .collect())
    }

    async fn query(&self, q: &Query) -> Result<ResultSet> {
        let sql = match q {
            Query::Sql(s) => s,
            Query::Cql(_) | Query::Command(_) | Query::Mongo(_) => {
                return Err(RdbError::UnsupportedQuery)
            }
        };
        let mut client = self.client.lock().await;
        let mut stream = client
            .simple_query(sql.as_str())
            .await
            .map_err(|e| RdbError::Query(e.to_string()))?;

        let mut cols: Vec<Column> = Vec::new();
        let mut rows: Vec<Vec<Cell>> = Vec::new();
        let mut saw_metadata = false;
        while let Some(item) = stream
            .try_next()
            .await
            .map_err(|e| RdbError::Query(e.to_string()))?
        {
            match item {
                QueryItem::Metadata(meta) => {
                    if !saw_metadata {
                        cols = meta
                            .columns()
                            .iter()
                            .map(|c| Column {
                                name: c.name().to_string(),
                                type_name: column_type_name(c.column_type()),
                            })
                            .collect();
                        saw_metadata = true;
                    }
                }
                QueryItem::Row(row) => {
                    rows.push(
                        row.cells()
                            .map(|(_, data)| column_data_to_cell(data))
                            .collect(),
                    );
                }
            }
        }

        if !saw_metadata {
            // DDL/DML statements produce no result set; the QueryItem stream
            // only surfaces row/metadata tokens, not the DONE token's row
            // count, so — like the Cassandra driver — report 0 rather than
            // guess (ponytail: exact affected-count would need switching to
            // client.execute(), which loses the ability to also read rows
            // back from a statement that DOES return one).
            return Ok(ResultSet::Affected(0));
        }
        Ok(ResultSet::Tabular { cols, rows })
    }

    async fn primary_key(&self, table: &TableRef) -> Result<Vec<String>> {
        let schema = table.schema.as_deref().unwrap_or(DEFAULT_SCHEMA);
        let mut client = self.client.lock().await;
        let rows = client
            .query(
                "SELECT c.name FROM sys.indexes i \
                 JOIN sys.index_columns ic ON ic.object_id = i.object_id AND ic.index_id = i.index_id \
                 JOIN sys.columns c ON c.object_id = ic.object_id AND c.column_id = ic.column_id \
                 JOIN sys.tables t ON t.object_id = i.object_id \
                 JOIN sys.schemas s ON s.schema_id = t.schema_id \
                 WHERE i.is_primary_key = 1 AND t.name = @P1 AND s.name = @P2 \
                 ORDER BY ic.key_ordinal",
                &[&table.name.as_str(), &schema],
            )
            .await
            .map_err(|e| RdbError::Schema(e.to_string()))?
            .into_first_result()
            .await
            .map_err(|e| RdbError::Schema(e.to_string()))?;
        Ok(rows
            .iter()
            .filter_map(|r| r.get::<&str, _>(0))
            .map(str::to_string)
            .collect())
    }

    async fn count(&self, table: &TableRef) -> Result<u64> {
        let sql = format!("SELECT COUNT(*) FROM {}", write_sql::table_name(table));
        let mut client = self.client.lock().await;
        let row = client
            .simple_query(sql)
            .await
            .map_err(|e| RdbError::Query(e.to_string()))?
            .into_row()
            .await
            .map_err(|e| RdbError::Query(e.to_string()))?
            .ok_or_else(|| RdbError::Query("COUNT(*) returned no row".into()))?;
        let n: i32 = row.get(0).unwrap_or(0);
        Ok(n.max(0) as u64)
    }

    async fn commit(&self, ops: &[WriteOp]) -> Result<u64> {
        if ops.is_empty() {
            return Ok(0);
        }
        let mut client = self.client.lock().await;
        client
            .simple_query("BEGIN TRANSACTION")
            .await
            .map_err(|e| RdbError::Query(e.to_string()))?
            .into_results()
            .await
            .map_err(|e| RdbError::Query(e.to_string()))?;

        let mut affected = 0u64;
        for op in ops {
            let sql = match op {
                WriteOp::Update { table, pk, changes } => write_sql::update_sql(table, pk, changes),
                WriteOp::Insert { table, values } => write_sql::insert_sql(table, values),
                WriteOp::Delete { table, pk } => write_sql::delete_sql(table, pk),
            };
            match client.execute(sql, &[]).await {
                Ok(res) => affected += res.total(),
                Err(e) => {
                    // A failed statement leaves the transaction open on the
                    // server; roll it back explicitly rather than leaking it
                    // (tiberius has no Drop-based auto-rollback like a typed
                    // transaction handle from mysql_async/tokio-postgres).
                    let _ = client.simple_query("ROLLBACK TRANSACTION").await;
                    return Err(RdbError::Query(e.to_string()));
                }
            }
        }

        client
            .simple_query("COMMIT TRANSACTION")
            .await
            .map_err(|e| RdbError::Query(e.to_string()))?
            .into_results()
            .await
            .map_err(|e| RdbError::Query(e.to_string()))?;
        Ok(affected)
    }

    async fn close(self) -> Result<()> {
        self.client
            .into_inner()
            .close()
            .await
            .map_err(|e| RdbError::Connection(e.to_string()))?;
        Ok(())
    }
}

async fn schema_impl(client: &Mutex<MssqlClient>, schema: &str) -> Result<Schema> {
    let mut client = client.lock().await;
    let rows = client
        .query(COLUMNS_QUERY, &[&schema])
        .await
        .map_err(|e| RdbError::Schema(e.to_string()))?
        .into_first_result()
        .await
        .map_err(|e| RdbError::Schema(e.to_string()))?;

    let schema_rows: Vec<SchemaRow> = rows
        .iter()
        .map(|r| {
            (
                r.get::<&str, _>(0).unwrap_or_default().to_string(),
                r.get::<&str, _>(1).unwrap_or_default().to_string(),
                r.get::<&str, _>(2).unwrap_or_default().to_string(),
                r.get::<i32, _>(3).unwrap_or(0) != 0,
                r.get::<i32, _>(4).unwrap_or(0) != 0,
                r.get::<i32, _>(5).unwrap_or(0) != 0,
            )
        })
        .collect();
    Ok(fold_rows(schema, schema_rows))
}
