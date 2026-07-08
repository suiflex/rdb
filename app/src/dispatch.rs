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
            AnyDriver::Mock(d) => d.schema().await,
        }
    }

    pub async fn containers(&self, database: &str) -> Result<Vec<Container>> {
        match self {
            AnyDriver::Postgres(d) => d.containers(database).await,
            AnyDriver::Mysql(d) => d.containers(database).await,
            AnyDriver::Redis(d) => d.containers(database).await,
            AnyDriver::Mongo(d) => d.containers(database).await,
            AnyDriver::Sqlite(d) => d.containers(database).await,
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
            AnyDriver::Mock(d) => d.commit(ops).await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdbs_connstore::Engine;

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
}
