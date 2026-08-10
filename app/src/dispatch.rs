//! Boxed-enum driver dispatch. The UI holds an `AnyDriver`, never a `dyn Driver`
//! (the `Driver` trait is not object-safe because `connect`/`close` are
//! `where Self: Sized`). Each variant owns a concrete driver and forwards the
//! async trait methods. Construction is the ONLY place the app names a concrete
//! driver crate.

use rdb_connstore::Engine;
use rdb_core::conn::ConnConfig;
use rdb_core::driver::Driver;
use rdb_core::error::Result;
use rdb_core::query::Query;
use rdb_core::result::ResultSet;
use rdb_core::schema::{Container, Field, Schema};
use rdb_core::write::{TableRef, WriteOp};
use rdb_driver_cassandra::CassandraDriver;
use rdb_driver_clickhouse::ClickhouseDriver;
use rdb_driver_mongo::MongoDriver;
use rdb_driver_mssql::MssqlDriver;
use rdb_driver_mysql::MysqlDriver;
use rdb_driver_postgres::PostgresDriver;
use rdb_driver_redis::RedisDriver;
use rdb_driver_sqlite::SqliteDriver;

pub enum AnyDriver {
    Postgres(PostgresDriver),
    Mysql(MysqlDriver),
    Redis(RedisDriver),
    Mongo(MongoDriver),
    Sqlite(SqliteDriver),
    // Boxed: a scylla Session is far larger than the other drivers, so keep it
    // off the enum's inline footprint (clippy::large_enum_variant).
    Cassandra(Box<CassandraDriver>),
    // Boxed: a tiberius Client's connection buffers are far larger than the
    // other drivers, same reasoning as Cassandra above.
    Mssql(Box<MssqlDriver>),
    Clickhouse(ClickhouseDriver),
    /// In-process demo driver (RDB_MOCK=1); no network, seeded data.
    #[cfg(feature = "mock")]
    Mock(crate::mock::MockDriver),
}

impl AnyDriver {
    /// Human label for an engine (used in the sidebar + palette).
    pub fn label(engine: Engine) -> &'static str {
        match engine {
            Engine::Postgres => "PostgreSQL",
            Engine::MySql => "MySQL",
            Engine::Redis => "Redis",
            Engine::Mongo => "MongoDB",
            Engine::Sqlite => "SQLite",
            Engine::Cassandra => "Cassandra",
            Engine::Mssql => "SQL Server",
            Engine::Clickhouse => "ClickHouse",
        }
    }

    /// Stable lowercase key the UI's `DbBadge` switches on.
    pub fn badge(engine: Engine) -> &'static str {
        match engine {
            Engine::Postgres => "postgres",
            Engine::MySql => "mysql",
            Engine::Redis => "redis",
            Engine::Mongo => "mongo",
            Engine::Sqlite => "sqlite",
            Engine::Cassandra => "cassandra",
            Engine::Mssql => "mssql",
            Engine::Clickhouse => "clickhouse",
        }
    }

    /// Connect using the concrete driver for `engine`.
    pub async fn connect(engine: Engine, cfg: &ConnConfig) -> Result<Self> {
        #[cfg(feature = "mock")]
        if crate::mock::mock_mode() {
            return Ok(AnyDriver::Mock(
                crate::mock::MockDriver::connect(cfg).await?,
            ));
        }
        Ok(match engine {
            Engine::Postgres => AnyDriver::Postgres(PostgresDriver::connect(cfg).await?),
            Engine::MySql => AnyDriver::Mysql(MysqlDriver::connect(cfg).await?),
            Engine::Redis => AnyDriver::Redis(RedisDriver::connect(cfg).await?),
            Engine::Mongo => AnyDriver::Mongo(MongoDriver::connect(cfg).await?),
            Engine::Sqlite => AnyDriver::Sqlite(SqliteDriver::connect(cfg).await?),
            Engine::Cassandra => {
                AnyDriver::Cassandra(Box::new(CassandraDriver::connect(cfg).await?))
            }
            Engine::Mssql => AnyDriver::Mssql(Box::new(MssqlDriver::connect(cfg).await?)),
            Engine::Clickhouse => AnyDriver::Clickhouse(ClickhouseDriver::connect(cfg).await?),
        })
    }

    /// Push the user's NoSQL collection cap onto a live connection. Only NoSQL
    /// engines (MongoDB) have a sidebar collection cap; RDBMS variants no-op.
    pub fn set_collection_limit(&self, n: usize) {
        if let AnyDriver::Mongo(d) = self {
            d.set_collection_limit(n);
        }
    }

    /// Part of the driver surface; wired into the UI status check later.
    pub async fn ping(&self) -> Result<()> {
        match self {
            AnyDriver::Postgres(d) => d.ping().await,
            AnyDriver::Mysql(d) => d.ping().await,
            AnyDriver::Redis(d) => d.ping().await,
            AnyDriver::Mongo(d) => d.ping().await,
            AnyDriver::Sqlite(d) => d.ping().await,
            AnyDriver::Cassandra(d) => d.ping().await,
            AnyDriver::Mssql(d) => d.ping().await,
            AnyDriver::Clickhouse(d) => d.ping().await,
            #[cfg(feature = "mock")]
            AnyDriver::Mock(d) => d.ping().await,
        }
    }

    pub async fn schema(&self) -> Result<Schema> {
        match self {
            AnyDriver::Postgres(d) => d.schema().await,
            AnyDriver::Mysql(d) => d.schema().await,
            AnyDriver::Redis(d) => d.schema().await,
            AnyDriver::Mongo(d) => d.schema().await,
            AnyDriver::Sqlite(d) => d.schema().await,
            AnyDriver::Cassandra(d) => d.schema().await,
            AnyDriver::Mssql(d) => d.schema().await,
            AnyDriver::Clickhouse(d) => d.schema().await,
            #[cfg(feature = "mock")]
            AnyDriver::Mock(d) => d.schema().await,
        }
    }

    pub async fn schema_for(&self, schema: &str) -> Result<Schema> {
        match self {
            AnyDriver::Postgres(d) => d.schema_for(schema).await,
            AnyDriver::Mysql(d) => d.schema_for(schema).await,
            AnyDriver::Redis(d) => d.schema_for(schema).await,
            AnyDriver::Mongo(d) => d.schema_for(schema).await,
            AnyDriver::Sqlite(d) => d.schema_for(schema).await,
            AnyDriver::Cassandra(d) => d.schema_for(schema).await,
            AnyDriver::Mssql(d) => d.schema_for(schema).await,
            AnyDriver::Clickhouse(d) => d.schema_for(schema).await,
            #[cfg(feature = "mock")]
            AnyDriver::Mock(d) => d.schema_for(schema).await,
        }
    }

    pub async fn list_schemas(&self) -> Result<Vec<String>> {
        match self {
            AnyDriver::Postgres(d) => d.list_schemas().await,
            AnyDriver::Mysql(d) => d.list_schemas().await,
            AnyDriver::Redis(d) => d.list_schemas().await,
            AnyDriver::Mongo(d) => d.list_schemas().await,
            AnyDriver::Sqlite(d) => d.list_schemas().await,
            AnyDriver::Cassandra(d) => d.list_schemas().await,
            AnyDriver::Mssql(d) => d.list_schemas().await,
            AnyDriver::Clickhouse(d) => d.list_schemas().await,
            #[cfg(feature = "mock")]
            AnyDriver::Mock(d) => d.list_schemas().await,
        }
    }

    pub async fn list_databases(&self) -> Result<Vec<String>> {
        match self {
            AnyDriver::Postgres(d) => d.list_databases().await,
            AnyDriver::Mysql(d) => d.list_databases().await,
            AnyDriver::Redis(d) => d.list_databases().await,
            AnyDriver::Mongo(d) => d.list_databases().await,
            AnyDriver::Sqlite(d) => d.list_databases().await,
            AnyDriver::Cassandra(d) => d.list_databases().await,
            AnyDriver::Mssql(d) => d.list_databases().await,
            AnyDriver::Clickhouse(d) => d.list_databases().await,
            #[cfg(feature = "mock")]
            AnyDriver::Mock(d) => d.list_databases().await,
        }
    }

    pub async fn containers(&self, database: &str) -> Result<Vec<Container>> {
        match self {
            AnyDriver::Postgres(d) => d.containers(database).await,
            AnyDriver::Mysql(d) => d.containers(database).await,
            AnyDriver::Redis(d) => d.containers(database).await,
            AnyDriver::Mongo(d) => d.containers(database).await,
            AnyDriver::Sqlite(d) => d.containers(database).await,
            AnyDriver::Cassandra(d) => d.containers(database).await,
            AnyDriver::Mssql(d) => d.containers(database).await,
            AnyDriver::Clickhouse(d) => d.containers(database).await,
            #[cfg(feature = "mock")]
            AnyDriver::Mock(d) => d.containers(database).await,
        }
    }

    pub async fn sample_fields(
        &self,
        database: &str,
        container: &str,
        sample_size: u32,
    ) -> Result<Vec<Field>> {
        match self {
            AnyDriver::Postgres(d) => d.sample_fields(database, container, sample_size).await,
            AnyDriver::Mysql(d) => d.sample_fields(database, container, sample_size).await,
            AnyDriver::Redis(d) => d.sample_fields(database, container, sample_size).await,
            AnyDriver::Mongo(d) => d.sample_fields(database, container, sample_size).await,
            AnyDriver::Sqlite(d) => d.sample_fields(database, container, sample_size).await,
            AnyDriver::Cassandra(d) => d.sample_fields(database, container, sample_size).await,
            AnyDriver::Mssql(d) => d.sample_fields(database, container, sample_size).await,
            AnyDriver::Clickhouse(d) => d.sample_fields(database, container, sample_size).await,
            #[cfg(feature = "mock")]
            AnyDriver::Mock(d) => d.sample_fields(database, container, sample_size).await,
        }
    }

    pub async fn query(&self, q: &Query) -> Result<ResultSet> {
        match self {
            AnyDriver::Postgres(d) => d.query(q).await,
            AnyDriver::Mysql(d) => d.query(q).await,
            AnyDriver::Redis(d) => d.query(q).await,
            AnyDriver::Mongo(d) => d.query(q).await,
            AnyDriver::Sqlite(d) => d.query(q).await,
            AnyDriver::Cassandra(d) => d.query(q).await,
            AnyDriver::Mssql(d) => d.query(q).await,
            AnyDriver::Clickhouse(d) => d.query(q).await,
            #[cfg(feature = "mock")]
            AnyDriver::Mock(d) => d.query(q).await,
        }
    }

    /// Streaming read: Postgres pulls from a server cursor, every other engine
    /// falls back to the trait default (one buffered `query`, chunked). Kept in
    /// sync with the trait so the "No limit" path streams instead of freezing.
    pub async fn query_stream(
        &self,
        q: &Query,
        batch: usize,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
        sink: tokio::sync::mpsc::Sender<rdb_core::result::StreamItem>,
    ) -> Result<()> {
        match self {
            AnyDriver::Postgres(d) => d.query_stream(q, batch, cancel, sink).await,
            AnyDriver::Mysql(d) => d.query_stream(q, batch, cancel, sink).await,
            AnyDriver::Redis(d) => d.query_stream(q, batch, cancel, sink).await,
            AnyDriver::Mongo(d) => d.query_stream(q, batch, cancel, sink).await,
            AnyDriver::Sqlite(d) => d.query_stream(q, batch, cancel, sink).await,
            AnyDriver::Cassandra(d) => d.query_stream(q, batch, cancel, sink).await,
            AnyDriver::Mssql(d) => d.query_stream(q, batch, cancel, sink).await,
            AnyDriver::Clickhouse(d) => d.query_stream(q, batch, cancel, sink).await,
            #[cfg(feature = "mock")]
            AnyDriver::Mock(d) => d.query_stream(q, batch, cancel, sink).await,
        }
    }

    pub async fn primary_key(&self, table: &TableRef) -> Result<Vec<String>> {
        match self {
            AnyDriver::Postgres(d) => d.primary_key(table).await,
            AnyDriver::Mysql(d) => d.primary_key(table).await,
            AnyDriver::Redis(d) => d.primary_key(table).await,
            AnyDriver::Mongo(d) => d.primary_key(table).await,
            AnyDriver::Sqlite(d) => d.primary_key(table).await,
            AnyDriver::Cassandra(d) => d.primary_key(table).await,
            AnyDriver::Mssql(d) => d.primary_key(table).await,
            AnyDriver::Clickhouse(d) => d.primary_key(table).await,
            #[cfg(feature = "mock")]
            AnyDriver::Mock(d) => d.primary_key(table).await,
        }
    }

    pub async fn count(&self, table: &TableRef) -> Result<u64> {
        match self {
            AnyDriver::Postgres(d) => d.count(table).await,
            AnyDriver::Mysql(d) => d.count(table).await,
            AnyDriver::Redis(d) => d.count(table).await,
            AnyDriver::Mongo(d) => d.count(table).await,
            AnyDriver::Sqlite(d) => d.count(table).await,
            AnyDriver::Cassandra(d) => d.count(table).await,
            AnyDriver::Mssql(d) => d.count(table).await,
            AnyDriver::Clickhouse(d) => d.count(table).await,
            #[cfg(feature = "mock")]
            AnyDriver::Mock(d) => d.count(table).await,
        }
    }

    pub async fn commit(&self, ops: &[WriteOp]) -> Result<u64> {
        match self {
            AnyDriver::Postgres(d) => d.commit(ops).await,
            AnyDriver::Mysql(d) => d.commit(ops).await,
            AnyDriver::Redis(d) => d.commit(ops).await,
            AnyDriver::Mongo(d) => d.commit(ops).await,
            AnyDriver::Sqlite(d) => d.commit(ops).await,
            AnyDriver::Cassandra(d) => d.commit(ops).await,
            AnyDriver::Mssql(d) => d.commit(ops).await,
            AnyDriver::Clickhouse(d) => d.commit(ops).await,
            #[cfg(feature = "mock")]
            AnyDriver::Mock(d) => d.commit(ops).await,
        }
    }
}

/// Exact SQL/CQL text (or native command description) produced for buffered
/// writes. SQL builders are the same functions used by each driver's commit.
pub fn write_statements(engine: Engine, ops: &[WriteOp]) -> Vec<String> {
    ops.iter()
        .map(|op| match (engine, op) {
            (Engine::Postgres, WriteOp::Update { table, pk, changes }) => {
                rdb_driver_postgres::write_sql::update_sql(table, pk, changes)
            }
            (Engine::Postgres, WriteOp::Insert { table, values }) => {
                rdb_driver_postgres::write_sql::insert_sql(table, values)
            }
            (Engine::Postgres, WriteOp::Delete { table, pk }) => {
                rdb_driver_postgres::write_sql::delete_sql(table, pk)
            }
            (Engine::MySql, WriteOp::Update { table, pk, changes }) => {
                rdb_driver_mysql::write_sql::update_sql(table, pk, changes).0
            }
            (Engine::MySql, WriteOp::Insert { table, values }) => {
                rdb_driver_mysql::write_sql::insert_sql(table, values).0
            }
            (Engine::MySql, WriteOp::Delete { table, pk }) => {
                rdb_driver_mysql::write_sql::delete_sql(table, pk).0
            }
            (Engine::Sqlite, WriteOp::Update { table, pk, changes }) => {
                rdb_driver_sqlite::write_sql::update_sql(table, pk, changes)
            }
            (Engine::Sqlite, WriteOp::Insert { table, values }) => {
                rdb_driver_sqlite::write_sql::insert_sql(table, values)
            }
            (Engine::Sqlite, WriteOp::Delete { table, pk }) => {
                rdb_driver_sqlite::write_sql::delete_sql(table, pk)
            }
            (Engine::Cassandra, WriteOp::Update { table, pk, changes }) => {
                rdb_driver_cassandra::write_cql::update_sql(table, pk, changes)
            }
            (Engine::Cassandra, WriteOp::Insert { table, values }) => {
                rdb_driver_cassandra::write_cql::insert_sql(table, values)
            }
            (Engine::Cassandra, WriteOp::Delete { table, pk }) => {
                rdb_driver_cassandra::write_cql::delete_sql(table, pk)
            }
            (Engine::Mssql, WriteOp::Update { table, pk, changes }) => {
                rdb_driver_mssql::write_sql::update_sql(table, pk, changes)
            }
            (Engine::Mssql, WriteOp::Insert { table, values }) => {
                rdb_driver_mssql::write_sql::insert_sql(table, values)
            }
            (Engine::Mssql, WriteOp::Delete { table, pk }) => {
                rdb_driver_mssql::write_sql::delete_sql(table, pk)
            }
            (Engine::Clickhouse, WriteOp::Insert { table, values }) => {
                rdb_driver_clickhouse::write_sql::insert_sql(table, values)
            }
            // ClickHouse has no update_sql/delete_sql (see write_sql.rs) —
            // commit() rejects these outright, this is preview text only.
            (Engine::Clickhouse, op) => format!("ClickHouse write (unsupported): {op:?}"),
            (Engine::Mongo, op) => format!("MongoDB write: {op:?}"),
            (Engine::Redis, op) => format!("Redis write: {op:?}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdb_connstore::Engine;
    use rdb_core::result::Cell;

    #[test]
    fn engine_maps_to_human_label() {
        assert_eq!(AnyDriver::label(Engine::Postgres), "PostgreSQL");
        assert_eq!(AnyDriver::label(Engine::Redis), "Redis");
    }

    #[test]
    fn all_four_engines_labeled() {
        assert_eq!(AnyDriver::label(Engine::MySql), "MySQL");
        assert_eq!(AnyDriver::label(Engine::Mongo), "MongoDB");
    }

    #[test]
    fn console_uses_the_same_postgres_write_builder_as_commit() {
        let table = TableRef {
            database: Some("app".into()),
            schema: Some("public".into()),
            name: "users".into(),
        };
        let ops = [WriteOp::Update {
            table,
            pk: vec![("id".into(), Cell::Int(7))],
            changes: vec![("name".into(), Cell::Text("Ada".into()))],
        }];
        assert_eq!(
            write_statements(Engine::Postgres, &ops),
            vec!["UPDATE \"public\".\"users\" SET \"name\" = 'Ada' WHERE \"id\" = 7"]
        );
    }
}
