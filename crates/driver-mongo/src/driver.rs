use async_trait::async_trait;
use futures::stream::TryStreamExt;
use mongodb::bson::{doc, Document};
use mongodb::{Client, Collection};

use rdbs_core::conn::{ConnConfig, SslMode};
use rdbs_core::driver::Driver;
use rdbs_core::error::{RdbsError, Result};
use rdbs_core::query::{MongoKind, MongoOp, Query};
use rdbs_core::result::ResultSet;
use rdbs_core::schema::{Container, ContainerKind, Database, Schema};

use crate::convert::{document_to_json, json_to_document};

/// MongoDB driver over a `mongodb::Client`. The client is internally pooled and
/// cheap to clone, so we share `&self` directly.
pub struct MongoDriver {
    client: Client,
    /// Default database used when an op does not imply one.
    default_db: String,
}

/// Mongo's internal databases. Hidden from the sidebar so the user's own
/// databases aren't buried, matching Compass/TablePlus defaults.
fn is_system_db(name: &str) -> bool {
    matches!(name, "admin" | "config" | "local")
}

fn build_uri(cfg: &ConnConfig) -> String {
    let auth = match &cfg.password {
        Some(pw) if !pw.is_empty() => format!("{}:{}@", cfg.user, pw),
        _ => String::new(),
    };
    // TLS: Prefer/Require enable TLS with `tlsInsecure=true` (no cert/hostname
    // validation) to match the other drivers; Disable stays plaintext.
    let tls = match cfg.sslmode {
        SslMode::Disable => "",
        SslMode::Prefer | SslMode::Require => "?tls=true&tlsInsecure=true",
    };
    format!("mongodb://{auth}{}:{}{tls}", cfg.host, cfg.port)
}

impl MongoDriver {
    /// Resolve a collection in the op's database, falling back to the
    /// connection's default database when the op names none.
    fn collection(&self, op: &MongoOp) -> Collection<Document> {
        let db = op.database.as_deref().unwrap_or(&self.default_db);
        self.client
            .database(db)
            .collection::<Document>(&op.collection)
    }
}

#[async_trait]
impl Driver for MongoDriver {
    async fn connect(cfg: &ConnConfig) -> Result<Self> {
        let client = Client::with_uri_str(build_uri(cfg))
            .await
            .map_err(|e| RdbsError::Connection(e.to_string()))?;
        let default_db = cfg.database.clone().unwrap_or_else(|| "admin".to_string());
        Ok(MongoDriver { client, default_db })
    }

    async fn ping(&self) -> Result<()> {
        self.client
            .database("admin")
            .run_command(doc! { "ping": 1 })
            .await
            .map_err(|e| RdbsError::Connection(e.to_string()))?;
        Ok(())
    }

    /// Lazy: list databases only (cheap), with empty containers. Collections are
    /// fetched per database via `containers()` when the user expands it, so a
    /// server with many large databases doesn't stall on connect.
    async fn schema(&self) -> Result<Schema> {
        let db_names = self
            .client
            .list_database_names()
            .await
            .map_err(|e| RdbsError::Schema(e.to_string()))?;
        let databases = db_names
            .into_iter()
            .filter(|n| !is_system_db(n))
            .map(|name| Database {
                name,
                containers: Vec::new(),
            })
            .collect();
        Ok(Schema { databases })
    }

    /// Collections of one database, capped so a database with thousands of
    /// collections stays light. ponytail: fixed cap, add paging if it matters.
    async fn containers(&self, database: &str) -> Result<Vec<Container>> {
        const MAX_COLLECTIONS: usize = 20;
        let coll_names = self
            .client
            .database(database)
            .list_collection_names()
            .await
            .map_err(|e| RdbsError::Schema(e.to_string()))?;
        Ok(coll_names
            .into_iter()
            .take(MAX_COLLECTIONS)
            .map(|name| Container {
                name,
                kind: ContainerKind::Collection,
                // Mongo is schemaless: no static field list to report.
                fields: Vec::new(),
            })
            .collect())
    }

    async fn query(&self, q: &Query) -> Result<ResultSet> {
        let op: &MongoOp = match q {
            Query::Mongo(op) => op,
            _ => return Err(RdbsError::UnsupportedQuery),
        };
        let coll = self.collection(op);

        match &op.kind {
            MongoKind::Find(filter) => {
                let filter_doc = json_to_document(filter)?;
                let mut find = coll.find(filter_doc);
                if let Some(n) = op.limit {
                    find = find.limit(n);
                }
                let cursor = find.await.map_err(|e| RdbsError::Query(e.to_string()))?;
                let docs: Vec<Document> = cursor
                    .try_collect()
                    .await
                    .map_err(|e| RdbsError::Query(e.to_string()))?;
                Ok(ResultSet::Documents(
                    docs.into_iter().map(document_to_json).collect(),
                ))
            }
            MongoKind::Insert(payload) => {
                let doc = json_to_document(payload)?;
                coll.insert_one(doc)
                    .await
                    .map_err(|e| RdbsError::Query(e.to_string()))?;
                Ok(ResultSet::Affected(1))
            }
            MongoKind::Aggregate(stages) => {
                let pipeline = stages
                    .iter()
                    .map(json_to_document)
                    .collect::<Result<Vec<Document>>>()?;
                let cursor = coll
                    .aggregate(pipeline)
                    .await
                    .map_err(|e| RdbsError::Query(e.to_string()))?;
                let docs: Vec<Document> = cursor
                    .try_collect()
                    .await
                    .map_err(|e| RdbsError::Query(e.to_string()))?;
                Ok(ResultSet::Documents(
                    docs.into_iter().map(document_to_json).collect(),
                ))
            }
        }
    }

    async fn close(self) -> Result<()> {
        // mongodb::Client has no explicit close; dropping it ends background tasks.
        drop(self.client);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::is_system_db;

    #[test]
    fn system_dbs_are_hidden() {
        assert!(is_system_db("admin"));
        assert!(is_system_db("config"));
        assert!(is_system_db("local"));
        assert!(!is_system_db("appdb"));
        assert!(!is_system_db("local_app"));
    }
}
