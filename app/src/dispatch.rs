//! Boxed-enum driver dispatch. The UI holds an `AnyDriver`, never a `dyn Driver`
//! (the `Driver` trait is not object-safe because `connect`/`close` are
//! `where Self: Sized`). Each variant owns a concrete driver and forwards the
//! async trait methods. Construction is the ONLY place the app names a concrete
//! driver crate.

use dbm_connstore::Engine;
use dbm_core::conn::ConnConfig;
use dbm_core::driver::Driver;
use dbm_core::error::{DbmError, Result};
use dbm_core::query::Query;
use dbm_core::result::ResultSet;
use dbm_core::schema::Schema;
use dbm_driver_postgres::PostgresDriver;

pub enum AnyDriver {
    Postgres(PostgresDriver),
    // Mysql/Redis/Mongo arms land when those driver crates are wired; each is a
    // one-line forward below.
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

    /// Whether this build can connect the engine (MVP wires Postgres only).
    pub fn is_supported(engine: Engine) -> bool {
        matches!(engine, Engine::Postgres)
    }

    /// Connect using the concrete driver for `engine`.
    pub async fn connect(engine: Engine, cfg: &ConnConfig) -> Result<Self> {
        match engine {
            Engine::Postgres => Ok(AnyDriver::Postgres(PostgresDriver::connect(cfg).await?)),
            other => Err(DbmError::Connection(format!(
                "{} not supported in this build yet",
                Self::label(other)
            ))),
        }
    }

    /// Part of the driver surface; wired into the UI status check later.
    #[allow(dead_code)]
    pub async fn ping(&self) -> Result<()> {
        match self {
            AnyDriver::Postgres(d) => d.ping().await,
        }
    }

    pub async fn schema(&self) -> Result<Schema> {
        match self {
            AnyDriver::Postgres(d) => d.schema().await,
        }
    }

    pub async fn query(&self, q: &Query) -> Result<ResultSet> {
        match self {
            AnyDriver::Postgres(d) => d.query(q).await,
        }
    }

    /// Consumes and closes the driver; called on disconnect/app teardown.
    #[allow(dead_code)]
    pub async fn close(self) -> Result<()> {
        match self {
            AnyDriver::Postgres(d) => d.close().await,
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
    fn non_postgres_engines_are_unsupported_in_mvp_dispatch() {
        assert!(AnyDriver::is_supported(Engine::Postgres));
        assert!(!AnyDriver::is_supported(Engine::MySql));
    }
}
