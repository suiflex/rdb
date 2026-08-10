use async_trait::async_trait;

use crate::conn::ConnConfig;
use crate::error::Result;
use crate::query::Query;
use crate::result::ResultSet;
use crate::schema::Schema;
use crate::write::{TableRef, WriteOp};

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

    /// Full schema tree (databases → containers → fields). Engines with many
    /// databases (e.g. Mongo) may return databases with empty `containers` and
    /// fill them lazily via [`Driver::containers`] when a database is expanded.
    async fn schema(&self) -> Result<Schema>;

    /// Switchable namespaces/schemas the sidebar can browse (e.g. Postgres
    /// schemas). Default empty: the engine has nothing to switch and the UI
    /// keeps its current schema name.
    async fn list_schemas(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// Databases the connection can switch between (e.g. every DB on a Postgres
    /// server). Default empty: the engine has a single/implicit database and the
    /// UI shows its current database without a switcher.
    async fn list_databases(&self) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// Schema tree scoped to one namespace (e.g. a specific Postgres schema).
    /// Default delegates to [`Driver::schema`] for engines without namespaces.
    async fn schema_for(&self, _schema: &str) -> Result<Schema> {
        self.schema().await
    }

    /// List the containers (tables/collections) of one database, fetched on
    /// demand. Default is empty for engines that return everything in `schema`.
    async fn containers(&self, _database: &str) -> Result<Vec<crate::schema::Container>> {
        Ok(Vec::new())
    }

    /// Best-effort field discovery for schema-less engines (Mongo): sample up
    /// to `sample_size` documents from `container` and union their keys into
    /// [`Field`](crate::schema::Field)s, for the query editor's completion
    /// popup. Engines with a real schema already get fields from
    /// [`Driver::schema`]/[`Driver::containers`] and can ignore this — default
    /// is empty, no sampling.
    async fn sample_fields(
        &self,
        _database: &str,
        _container: &str,
        _sample_size: u32,
    ) -> Result<Vec<crate::schema::Field>> {
        Ok(Vec::new())
    }

    /// Run a query. Drivers handle the `Query` variant(s) they support and
    /// return `RdbError::UnsupportedQuery` for the rest.
    async fn query(&self, q: &Query) -> Result<ResultSet>;

    /// Stream a row-returning query in batches so a huge result never buffers
    /// fully in memory or freezes the UI. Sends [`StreamItem::Meta`] (columns)
    /// first, then [`StreamItem::Batch`]es of at most `batch` rows; stops early
    /// when `cancel` flips true or the receiver is dropped.
    ///
    /// Default: run [`Driver::query`] once and chunk the buffered rows — correct
    /// for every engine, but only engines that OVERRIDE this (e.g. Postgres via
    /// a server cursor) actually stream from the server without buffering first.
    async fn query_stream(
        &self,
        q: &Query,
        batch: usize,
        cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
        sink: tokio::sync::mpsc::Sender<crate::result::StreamItem>,
    ) -> Result<()> {
        use crate::result::StreamItem;
        let batch = batch.max(1);
        match self.query(q).await? {
            ResultSet::Tabular { cols, rows } => {
                if sink.send(StreamItem::Meta(cols)).await.is_err() {
                    return Ok(());
                }
                for chunk in rows.chunks(batch) {
                    if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    if sink.send(StreamItem::Batch(chunk.to_vec())).await.is_err() {
                        break;
                    }
                }
                Ok(())
            }
            _ => Err(crate::error::RdbError::UnsupportedQuery),
        }
    }

    /// Primary-key column names of `table` (row identity for editing). An
    /// empty vec means the container is not editable. Default: not editable.
    async fn primary_key(&self, _table: &TableRef) -> Result<Vec<String>> {
        Ok(Vec::new())
    }

    /// Total rows/members of `table`, for the pagination footer.
    async fn count(&self, _table: &TableRef) -> Result<u64> {
        Err(crate::error::RdbError::UnsupportedQuery)
    }

    /// Apply buffered writes. SQL engines run the batch in one transaction
    /// (all-or-nothing); document/KV engines apply sequentially and stop at
    /// the first failure. Returns the number of ops applied.
    async fn commit(&self, _ops: &[WriteOp]) -> Result<u64> {
        Err(crate::error::RdbError::UnsupportedQuery)
    }

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
                _ => Err(crate::error::RdbError::UnsupportedQuery),
            }
        }
        async fn close(self) -> crate::error::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn write_api_defaults_are_read_only() {
        let d = FakeDriver;
        let t = crate::write::TableRef::named("users");
        assert!(d.primary_key(&t).await.unwrap().is_empty());
        assert!(d.count(&t).await.is_err());
        assert!(d.commit(&[]).await.is_err());
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
            params: None,
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
