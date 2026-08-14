use async_trait::async_trait;
use rustls::ClientConfig;
use scylla::client::session::Session;
use scylla::client::session::TlsContext;
use scylla::client::session_builder::SessionBuilder;
use scylla::value::Row;
use std::sync::Arc;
use webpki_roots::TLS_SERVER_ROOTS;

use rdb_core::conn::{ConnConfig, SslMode};
use rdb_core::driver::Driver;
use rdb_core::error::{RdbError, Result};
use rdb_core::query::Query;
use rdb_core::result::{Column, ResultSet};
use rdb_core::schema::{Container, ContainerKind, Database, Field, Schema};
use rdb_core::write::{TableRef, WriteOp};

use crate::type_map;
use crate::write_cql;

fn tls_context() -> TlsContext {
    let roots = rustls::RootCertStore {
        roots: TLS_SERVER_ROOTS.to_vec(),
    };
    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    TlsContext::Rustls023(Arc::new(config))
}

/// System keyspaces hidden from the schema tree.
const SYSTEM_KEYSPACES: &[&str] = &[
    "system",
    "system_schema",
    "system_auth",
    "system_distributed",
    "system_traces",
    "system_distributed_everywhere",
    "system_views",
    "system_virtual_schema",
];

/// A `Driver` backed by a scylla `Session` (works against Cassandra + Scylla).
pub struct CassandraDriver {
    session: Session,
}

async fn connect_session(cfg: &ConnConfig, tls: bool) -> Result<Session> {
    let node = format!("{}:{}", cfg.host, cfg.port);
    let mut builder = SessionBuilder::new().known_node(node);
    if !cfg.user.is_empty() {
        builder = builder.user(cfg.user.clone(), cfg.password.clone().unwrap_or_default());
    }
    if tls {
        builder = builder.tls_context(Some(tls_context()));
    }
    if let Some(ks) = cfg.database.as_deref().filter(|s| !s.is_empty()) {
        builder = builder.use_keyspace(ks.to_string(), false);
    }
    builder
        .build()
        .await
        .map_err(|e| RdbError::Connection(e.to_string()))
}

#[async_trait]
impl Driver for CassandraDriver {
    async fn connect(cfg: &ConnConfig) -> Result<Self> {
        let session = match cfg.sslmode {
            SslMode::Disable => connect_session(cfg, false).await?,
            SslMode::Require => connect_session(cfg, true).await?,
            SslMode::Prefer => match connect_session(cfg, true).await {
                Ok(session) => session,
                Err(_) => connect_session(cfg, false).await?,
            },
        };
        Ok(CassandraDriver { session })
    }

    async fn ping(&self) -> Result<()> {
        self.session
            .query_unpaged("SELECT now() FROM system.local", ())
            .await
            .map_err(|e| RdbError::Connection(e.to_string()))?;
        Ok(())
    }
    async fn schema(&self) -> Result<Schema> {
        let res = self
            .session
            .query_unpaged("SELECT keyspace_name FROM system_schema.keyspaces", ())
            .await
            .map_err(|e| RdbError::Schema(e.to_string()))?
            .into_rows_result()
            .map_err(|e| RdbError::Schema(e.to_string()))?;
        let mut databases = Vec::new();
        for row in res
            .rows::<(String,)>()
            .map_err(|e| RdbError::Schema(e.to_string()))?
        {
            let (name,) = row.map_err(|e| RdbError::Schema(e.to_string()))?;
            if SYSTEM_KEYSPACES.contains(&name.as_str()) {
                continue;
            }
            databases.push(Database {
                name,
                containers: Vec::new(),
                functions: Vec::new(),
            });
        }
        Ok(Schema { databases })
    }

    async fn containers(&self, database: &str) -> Result<Vec<Container>> {
        // Tables of one keyspace, with their columns.
        let tables_res = self
            .session
            .query_unpaged(
                "SELECT table_name FROM system_schema.tables WHERE keyspace_name = ?",
                (database,),
            )
            .await
            .map_err(|e| RdbError::Schema(e.to_string()))?
            .into_rows_result()
            .map_err(|e| RdbError::Schema(e.to_string()))?;

        let mut containers = Vec::new();
        for row in tables_res
            .rows::<(String,)>()
            .map_err(|e| RdbError::Schema(e.to_string()))?
        {
            let (table,) = row.map_err(|e| RdbError::Schema(e.to_string()))?;
            let fields = self.columns_of(database, &table).await?;
            containers.push(Container {
                name: table,
                kind: ContainerKind::Table,
                fields,
            });
        }
        Ok(containers)
    }

    async fn query(&self, q: &Query) -> Result<ResultSet> {
        let cql = match q {
            Query::Cql(s) => s,
            Query::Sql(_) | Query::Command(_) | Query::Mongo(_) => {
                return Err(RdbError::UnsupportedQuery)
            }
        };
        let res = self
            .session
            .query_unpaged(cql.as_str(), ())
            .await
            .map_err(|e| RdbError::Query(e.to_string()))?;
        if !res.is_rows() {
            // ponytail: CQL DML returns no affected-row count.
            return Ok(ResultSet::Affected(0));
        }
        let rows_res = res
            .into_rows_result()
            .map_err(|e| RdbError::Query(e.to_string()))?;
        let cols: Vec<Column> = rows_res
            .column_specs()
            .iter()
            .map(|c| Column {
                name: c.name().to_string(),
                type_name: String::new(),
            })
            .collect();
        let mut out = Vec::new();
        for row in rows_res
            .rows::<Row>()
            .map_err(|e| RdbError::Query(e.to_string()))?
        {
            let row = row.map_err(|e| RdbError::Query(e.to_string()))?;
            out.push(row.columns.iter().map(type_map::cell).collect());
        }
        Ok(ResultSet::Tabular { cols, rows: out })
    }

    async fn primary_key(&self, table: &TableRef) -> Result<Vec<String>> {
        let keyspace = table.database.as_deref().unwrap_or_default();
        let res = self
            .session
            .query_unpaged(
                "SELECT column_name, kind, position FROM system_schema.columns \
                 WHERE keyspace_name = ? AND table_name = ?",
                (keyspace, table.name.as_str()),
            )
            .await
            .map_err(|e| RdbError::Schema(e.to_string()))?
            .into_rows_result()
            .map_err(|e| RdbError::Schema(e.to_string()))?;
        // Partition keys first (by position), then clustering keys (by position).
        let mut partition: Vec<(i32, String)> = Vec::new();
        let mut clustering: Vec<(i32, String)> = Vec::new();
        for row in res
            .rows::<(String, String, i32)>()
            .map_err(|e| RdbError::Schema(e.to_string()))?
        {
            let (name, kind, position) = row.map_err(|e| RdbError::Schema(e.to_string()))?;
            match kind.as_str() {
                "partition_key" => partition.push((position, name)),
                "clustering" => clustering.push((position, name)),
                _ => {}
            }
        }
        partition.sort_by_key(|(p, _)| *p);
        clustering.sort_by_key(|(p, _)| *p);
        Ok(partition
            .into_iter()
            .chain(clustering)
            .map(|(_, name)| name)
            .collect())
    }

    async fn count(&self, table: &TableRef) -> Result<u64> {
        let cql = format!("SELECT count(*) FROM {}", write_cql::table_name(table));
        let res = self
            .session
            .query_unpaged(cql, ())
            .await
            .map_err(|e| RdbError::Query(e.to_string()))?
            .into_rows_result()
            .map_err(|e| RdbError::Query(e.to_string()))?;
        let (n,): (i64,) = res
            .first_row()
            .map_err(|e| RdbError::Query(e.to_string()))?;
        Ok(n as u64)
    }

    async fn commit(&self, ops: &[WriteOp]) -> Result<u64> {
        // No CQL transaction: apply sequentially, stop at the first failure
        // (mirrors the Mongo driver's contract).
        let mut applied = 0u64;
        for op in ops {
            let cql = match op {
                WriteOp::Update { table, pk, changes } => write_cql::update_sql(table, pk, changes),
                WriteOp::Insert { table, values } => write_cql::insert_sql(table, values),
                WriteOp::Delete { table, pk } => write_cql::delete_sql(table, pk),
            };
            match self.session.query_unpaged(cql, ()).await {
                Ok(_) => applied += 1,
                Err(e) => {
                    return Err(RdbError::Query(format!(
                        "{e} (applied {applied} of {} ops)",
                        ops.len()
                    )))
                }
            }
        }
        Ok(applied)
    }

    async fn close(self) -> Result<()> {
        // Dropping the session closes its connection pool.
        Ok(())
    }
}

impl CassandraDriver {
    /// Column definitions of one table.
    async fn columns_of(&self, keyspace: &str, table: &str) -> Result<Vec<Field>> {
        let res = self
            .session
            .query_unpaged(
                "SELECT column_name, type FROM system_schema.columns \
                 WHERE keyspace_name = ? AND table_name = ?",
                (keyspace, table),
            )
            .await
            .map_err(|e| RdbError::Schema(e.to_string()))?
            .into_rows_result()
            .map_err(|e| RdbError::Schema(e.to_string()))?;
        let mut fields = Vec::new();
        for row in res
            .rows::<(String, String)>()
            .map_err(|e| RdbError::Schema(e.to_string()))?
        {
            let (name, type_name) = row.map_err(|e| RdbError::Schema(e.to_string()))?;
            // CQL columns are nullable except primary-key parts; the schema
            // table doesn't flag that cheaply, so report nullable.
            fields.push(Field {
                name,
                type_name,
                nullable: true,
                ..Default::default()
            });
        }
        Ok(fields)
    }
}
