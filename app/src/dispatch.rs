//! Boxed-enum driver dispatch. The UI holds an `AnyDriver`, never a `dyn Driver`
//! (the `Driver` trait is not object-safe because `connect`/`close` are
//! `where Self: Sized`). Each variant owns a concrete driver and forwards the
//! async trait methods. Construction is the ONLY place the app names a concrete
//! driver crate.

use dbm_connstore::Engine;
use dbm_core::conn::ConnConfig;
use dbm_core::driver::Driver;
use dbm_core::error::Result;
use dbm_core::query::Query;
use dbm_core::result::ResultSet;
use dbm_core::schema::Schema;
use dbm_driver_mongo::MongoDriver;
use dbm_driver_mysql::MysqlDriver;
use dbm_driver_postgres::PostgresDriver;
use dbm_driver_redis::RedisDriver;

pub enum AnyDriver {
    Postgres(PostgresDriver),
    Mysql(MysqlDriver),
    Redis(RedisDriver),
    Mongo(MongoDriver),
}

impl AnyDriver {
    /// Human label for an engine (used in the sidebar + palette).
    pub fn label(engine: Engine) -> &'static str {
        match engine {
            Engine::Postgres => "PostgreSQL",
            Engine::MySql => "MySQL",
            Engine::Redis => "Redis",
            Engine::Mongo => "MongoDB",
        }
    }

    /// Whether this build can connect the engine (all four are wired).
    pub fn is_supported(_engine: Engine) -> bool {
        true
    }

    /// Connect using the concrete driver for `engine`.
    pub async fn connect(engine: Engine, cfg: &ConnConfig) -> Result<Self> {
        Ok(match engine {
            Engine::Postgres => AnyDriver::Postgres(PostgresDriver::connect(cfg).await?),
            Engine::MySql => AnyDriver::Mysql(MysqlDriver::connect(cfg).await?),
            Engine::Redis => AnyDriver::Redis(RedisDriver::connect(cfg).await?),
            Engine::Mongo => AnyDriver::Mongo(MongoDriver::connect(cfg).await?),
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
        }
    }

    pub async fn schema(&self) -> Result<Schema> {
        match self {
            AnyDriver::Postgres(d) => d.schema().await,
            AnyDriver::Mysql(d) => d.schema().await,
            AnyDriver::Redis(d) => d.schema().await,
            AnyDriver::Mongo(d) => d.schema().await,
        }
    }

    pub async fn query(&self, q: &Query) -> Result<ResultSet> {
        match self {
            AnyDriver::Postgres(d) => d.query(q).await,
            AnyDriver::Mysql(d) => d.query(q).await,
            AnyDriver::Redis(d) => d.query(q).await,
            AnyDriver::Mongo(d) => d.query(q).await,
        }
    }

    /// Consumes and closes the driver; called on disconnect/app teardown.
    #[allow(dead_code)]
    pub async fn close(self) -> Result<()> {
        match self {
            AnyDriver::Postgres(d) => d.close().await,
            AnyDriver::Mysql(d) => d.close().await,
            AnyDriver::Redis(d) => d.close().await,
            AnyDriver::Mongo(d) => d.close().await,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbm_connstore::Engine;

    #[test]
    fn engine_maps_to_human_label() {
        assert_eq!(AnyDriver::label(Engine::Postgres), "PostgreSQL");
        assert_eq!(AnyDriver::label(Engine::Redis), "Redis");
    }

    #[test]
    fn all_four_engines_supported_and_labeled() {
        for e in [
            Engine::Postgres,
            Engine::MySql,
            Engine::Redis,
            Engine::Mongo,
        ] {
            assert!(AnyDriver::is_supported(e), "{e:?} should be supported");
        }
        assert_eq!(AnyDriver::label(Engine::MySql), "MySQL");
        assert_eq!(AnyDriver::label(Engine::Mongo), "MongoDB");
    }
}
