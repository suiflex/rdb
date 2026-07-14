//! Boxed-enum driver dispatch. The UI holds an `AnyDriver`, never a `dyn Driver`
//! (the `Driver` trait is not object-safe because `connect`/`close` are
//! `where Self: Sized`). Each variant owns a concrete driver and forwards the
//! async trait methods. Construction is the ONLY place the app names a concrete
//! driver crate.

use rdbs_connstore::Engine;
use rdbs_core::conn::ConnConfig;
use rdbs_core::driver::Driver;
use rdbs_core::error::Result;
use rdbs_core::query::Query;
use rdbs_core::result::ResultSet;
use rdbs_core::schema::{Container, Schema};
use rdbs_core::write::{TableRef, WriteOp};
use rdbs_driver_cassandra::CassandraDriver;
use rdbs_driver_mongo::MongoDriver;
use rdbs_driver_mysql::MysqlDriver;
use rdbs_driver_postgres::PostgresDriver;
use rdbs_driver_redis::RedisDriver;
use rdbs_driver_sqlite::SqliteDriver;

pub enum AnyDriver {
    Postgres(PostgresDriver),
    Mysql(MysqlDriver),
    Redis(RedisDriver),
    Mongo(MongoDriver),
    Sqlite(SqliteDriver),
    // Boxed: a scylla Session is far larger than the other drivers, so keep it
    // off the enum's inline footprint (clippy::large_enum_variant).
    Cassandra(Box<CassandraDriver>),
    /// In-process demo driver (RDBS_MOCK=1); no network, seeded data.
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
        }
    }

    /// Connect using the concrete driver for `engine`.
    pub async fn connect(engine: Engine, cfg: &ConnConfig) -> Result<Self> {
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
        })
    }

    /// Part of the driver surface; wired into the UI status check later.
    #[allow(dead_code)]
    pub async fn ping(&self) -> Result<()> {
        match self {
            AnyDriver::Postgres(d) => d.ping().await,
            AnyDriver::Mysql(d) => d.ping().await,
            AnyDriver::Redis(d) => d.ping().await,
            AnyDriver::Mongo(d) => d.ping().await,
            AnyDriver::Sqlite(d) => d.ping().await,
            AnyDriver::Cassandra(d) => d.ping().await,
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
            AnyDriver::Mock(d) => d.schema().await,
        }
    }

    #[allow(dead_code)]
    pub async fn schema_for(&self, schema: &str) -> Result<Schema> {
        match self {
            AnyDriver::Postgres(d) => d.schema_for(schema).await,
            AnyDriver::Mysql(d) => d.schema_for(schema).await,
            AnyDriver::Redis(d) => d.schema_for(schema).await,
            AnyDriver::Mongo(d) => d.schema_for(schema).await,
            AnyDriver::Sqlite(d) => d.schema_for(schema).await,
            AnyDriver::Cassandra(d) => d.schema_for(schema).await,
            AnyDriver::Mock(d) => d.schema_for(schema).await,
        }
    }

    #[allow(dead_code)]
    pub async fn list_schemas(&self) -> Result<Vec<String>> {
        match self {
            AnyDriver::Postgres(d) => d.list_schemas().await,
            AnyDriver::Mysql(d) => d.list_schemas().await,
            AnyDriver::Redis(d) => d.list_schemas().await,
            AnyDriver::Mongo(d) => d.list_schemas().await,
            AnyDriver::Sqlite(d) => d.list_schemas().await,
            AnyDriver::Cassandra(d) => d.list_schemas().await,
            AnyDriver::Mock(d) => d.list_schemas().await,
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
            AnyDriver::Mock(d) => d.containers(database).await,
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
            AnyDriver::Mock(d) => d.query(q).await,
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
            AnyDriver::Mock(d) => d.commit(ops).await,
        }
    }
}

/// Statements executed by the table metadata path. Keep these beside concrete
/// driver dispatch so the Slint layer never needs to know driver SQL builders.
pub fn table_metadata_statements(engine: Engine, table: &TableRef) -> Vec<String> {
    match engine {
        Engine::Postgres => {
            let schema = table.schema.as_deref().unwrap_or("public");
            let esc = |s: &str| s.replace('\'', "''");
            vec![
                format!(
                    "SELECT count(*) FROM {}",
                    rdbs_driver_postgres::write_sql::table_name(table)
                ),
                format!(
                    "SELECT a.attname FROM pg_index i JOIN pg_class c ON c.oid = i.indrelid \
                     JOIN pg_namespace n ON n.oid = c.relnamespace \
                     JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum = ANY(i.indkey) \
                     WHERE i.indisprimary AND c.relname = $1 AND n.nspname = $2 \
                     ORDER BY array_position(i.indkey, a.attnum)\n-- $1 = '{}', $2 = '{}'",
                    table.name.replace('\'', "''"),
                    schema.replace('\'', "''")
                ),
                format!(
                    "SELECT indexname, indexdef FROM pg_indexes \
                     WHERE tablename = '{}' AND schemaname = '{}' ORDER BY 1",
                    esc(&table.name),
                    esc(schema)
                ),
            ]
        }
        Engine::MySql => {
            let esc = |s: &str| s.replace('\'', "''");
            vec![
                format!(
                    "SELECT COUNT(*) FROM {}",
                    rdbs_driver_mysql::write_sql::table_name(table)
                ),
                format!(
                    "SELECT column_name FROM information_schema.key_column_usage \
                     WHERE table_name = ? AND constraint_name = 'PRIMARY' \
                     AND table_schema = COALESCE(?, DATABASE()) ORDER BY ordinal_position\n\
                     -- params: '{}', '{}'",
                    esc(&table.name),
                    esc(table.database.as_deref().unwrap_or(""))
                ),
                format!(
                    "SELECT index_name, GROUP_CONCAT(column_name ORDER BY seq_in_index \
                     SEPARATOR ', ') FROM information_schema.statistics \
                     WHERE table_name = '{}' AND table_schema = \
                     COALESCE(NULLIF('{}', ''), DATABASE()) GROUP BY index_name ORDER BY 1",
                    esc(&table.name),
                    esc(table.database.as_deref().unwrap_or(""))
                ),
            ]
        }
        Engine::Sqlite => vec![
            format!(
                "SELECT count(*) FROM {}",
                rdbs_driver_sqlite::write_sql::table_name(table)
            ),
            format!(
                "PRAGMA table_info({})",
                rdbs_driver_sqlite::write_sql::quote_ident(&table.name)
            ),
        ],
        Engine::Cassandra => vec![
            format!(
                "SELECT count(*) FROM {}",
                rdbs_driver_cassandra::write_cql::table_name(table)
            ),
            format!(
                "SELECT column_name, kind, position FROM system_schema.columns \
                 WHERE keyspace_name = ? AND table_name = ?\n-- params: '{}', '{}'",
                table.database.as_deref().unwrap_or_default(),
                table.name
            ),
        ],
        Engine::Mongo => vec![format!(
            "db.getSiblingDB('{}').getCollection('{}').countDocuments({{}})",
            table.database.as_deref().unwrap_or_default(),
            table.name
        )],
        Engine::Redis => vec![format!("TYPE {}", table.name)],
    }
}

pub fn schema_statements(engine: Engine, schema: &str) -> Vec<String> {
    match engine {
        Engine::Postgres => vec![
            "SELECT current_database()".into(),
            format!(
                "SELECT c.table_name, c.column_name, c.data_type, c.is_nullable \
                 FROM information_schema.columns c JOIN information_schema.tables t \
                 ON t.table_schema = c.table_schema AND t.table_name = c.table_name \
                 WHERE c.table_schema = $1 AND t.table_type = 'BASE TABLE' \
                 ORDER BY c.table_name, c.ordinal_position\n-- $1 = '{}'",
                schema.replace('\'', "''")
            ),
            format!(
                "SELECT p.proname, pg_catalog.pg_get_functiondef(p.oid) \
                 FROM pg_catalog.pg_proc p JOIN pg_catalog.pg_namespace n ON n.oid = p.pronamespace \
                 WHERE n.nspname = $1 AND p.prokind = 'f' ORDER BY p.proname\n-- $1 = '{}'",
                schema.replace('\'', "''")
            ),
        ],
        Engine::MySql => vec![
            "SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, DATA_TYPE, IS_NULLABLE \
             FROM INFORMATION_SCHEMA.COLUMNS \
             WHERE TABLE_SCHEMA NOT IN ('mysql','information_schema','performance_schema','sys') \
             ORDER BY TABLE_SCHEMA, TABLE_NAME, ORDINAL_POSITION"
                .into(),
        ],
        Engine::Sqlite => vec![
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name"
                .into(),
        ],
        Engine::Cassandra => vec![
            "SELECT keyspace_name FROM system_schema.keyspaces".into(),
        ],
        Engine::Mongo => vec!["listDatabases + listCollections".into()],
        Engine::Redis => vec!["SCAN 0 MATCH *".into()],
    }
}

/// Exact SQL/CQL text (or native command description) produced for buffered
/// writes. SQL builders are the same functions used by each driver's commit.
pub fn write_statements(engine: Engine, ops: &[WriteOp]) -> Vec<String> {
    ops.iter()
        .map(|op| match (engine, op) {
            (Engine::Postgres, WriteOp::Update { table, pk, changes }) => {
                rdbs_driver_postgres::write_sql::update_sql(table, pk, changes)
            }
            (Engine::Postgres, WriteOp::Insert { table, values }) => {
                rdbs_driver_postgres::write_sql::insert_sql(table, values)
            }
            (Engine::Postgres, WriteOp::Delete { table, pk }) => {
                rdbs_driver_postgres::write_sql::delete_sql(table, pk)
            }
            (Engine::MySql, WriteOp::Update { table, pk, changes }) => {
                rdbs_driver_mysql::write_sql::update_sql(table, pk, changes).0
            }
            (Engine::MySql, WriteOp::Insert { table, values }) => {
                rdbs_driver_mysql::write_sql::insert_sql(table, values).0
            }
            (Engine::MySql, WriteOp::Delete { table, pk }) => {
                rdbs_driver_mysql::write_sql::delete_sql(table, pk).0
            }
            (Engine::Sqlite, WriteOp::Update { table, pk, changes }) => {
                rdbs_driver_sqlite::write_sql::update_sql(table, pk, changes)
            }
            (Engine::Sqlite, WriteOp::Insert { table, values }) => {
                rdbs_driver_sqlite::write_sql::insert_sql(table, values)
            }
            (Engine::Sqlite, WriteOp::Delete { table, pk }) => {
                rdbs_driver_sqlite::write_sql::delete_sql(table, pk)
            }
            (Engine::Cassandra, WriteOp::Update { table, pk, changes }) => {
                rdbs_driver_cassandra::write_cql::update_sql(table, pk, changes)
            }
            (Engine::Cassandra, WriteOp::Insert { table, values }) => {
                rdbs_driver_cassandra::write_cql::insert_sql(table, values)
            }
            (Engine::Cassandra, WriteOp::Delete { table, pk }) => {
                rdbs_driver_cassandra::write_cql::delete_sql(table, pk)
            }
            (Engine::Mongo, op) => format!("MongoDB write: {op:?}"),
            (Engine::Redis, op) => format!("Redis write: {op:?}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdbs_connstore::Engine;
    use rdbs_core::result::Cell;

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

    #[test]
    fn metadata_trace_contains_count_and_primary_key_lookup() {
        let sql = table_metadata_statements(Engine::Postgres, &TableRef::named("users"));
        assert!(sql[0].contains("SELECT count(*)"));
        assert!(sql[1].contains("i.indisprimary"));
    }
}
