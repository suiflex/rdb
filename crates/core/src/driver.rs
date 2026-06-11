use async_trait::async_trait;

use crate::conn::ConnConfig;
use crate::error::Result;
use crate::query::Query;
use crate::result::ResultSet;
use crate::schema::Schema;

/// The single interface the UI depends on. The UI NEVER imports a concrete
/// driver crate — adding an engine is a new crate that implements this trait.
#[async_trait]
pub trait Driver: Send + Sync {
    /// Open a connection. `cfg.password` is expected to be populated by the
    /// caller (from the keychain) before this is called.
    async fn connect(cfg: &ConnConfig) -> Result<Self>
    where
        Self: Sized;

    /// Cheap liveness check.
    async fn ping(&self) -> Result<()>;

    /// Full schema tree (databases → containers → fields).
    async fn schema(&self) -> Result<Schema>;

    /// Run a query. Drivers handle the `Query` variant(s) they support and
    /// return `DbmError::UnsupportedQuery` for the rest.
    async fn query(&self, q: &Query) -> Result<ResultSet>;

    /// Close the connection, consuming the driver.
    async fn close(self) -> Result<()>
    where
        Self: Sized;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conn::ConnConfig;
    use crate::query::Query;
    use crate::result::ResultSet;

    struct FakeDriver;

    #[async_trait::async_trait]
    impl Driver for FakeDriver {
        async fn connect(_cfg: &ConnConfig) -> crate::error::Result<Self> {
            Ok(FakeDriver)
        }
        async fn ping(&self) -> crate::error::Result<()> {
            Ok(())
        }
        async fn schema(&self) -> crate::error::Result<crate::schema::Schema> {
            Ok(crate::schema::Schema { databases: vec![] })
        }
        async fn query(&self, q: &Query) -> crate::error::Result<ResultSet> {
            match q {
                Query::Sql(_) => Ok(ResultSet::Affected(0)),
                _ => Err(crate::error::DbmError::UnsupportedQuery),
            }
        }
        async fn close(self) -> crate::error::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn fake_driver_satisfies_trait_and_rejects_unsupported() {
        let cfg = ConnConfig {
            host: "x".into(),
            port: 0,
            user: "x".into(),
            database: None,
            password: None,
            sslmode: Default::default(),
        };
        let d = FakeDriver::connect(&cfg).await.unwrap();
        d.ping().await.unwrap();
        assert!(matches!(
            d.query(&Query::Sql("SELECT 1".into())).await.unwrap(),
            ResultSet::Affected(0)
        ));
        assert!(d.query(&Query::Command(vec!["GET".into()])).await.is_err());
        d.close().await.unwrap();
    }
}
