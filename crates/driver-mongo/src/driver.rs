use async_trait::async_trait;
use futures::stream::TryStreamExt;
use mongodb::bson::{doc, Document};
use mongodb::{Client, Collection};

use dbm_core::conn::ConnConfig;
use dbm_core::driver::Driver;
use dbm_core::error::{DbmError, Result};
use dbm_core::query::{MongoKind, MongoOp, Query};
use dbm_core::result::ResultSet;
use dbm_core::schema::{Container, ContainerKind, Database, Schema};

use crate::convert::{document_to_json, json_to_document};

/// MongoDB driver over a `mongodb::Client`. The client is internally pooled and
/// cheap to clone, so we share `&self` directly.
pub struct MongoDriver {
    client: Client,
    /// Default database used when an op does not imply one.
    default_db: String,
}

fn build_uri(cfg: &ConnConfig) -> String {
    let auth = match &cfg.password {
        Some(pw) if !pw.is_empty() => format!("{}:{}@", cfg.user, pw),
        _ => String::new(),
    };
    format!("mongodb://{auth}{}:{}", cfg.host, cfg.port)
}

impl MongoDriver {
    fn collection(&self, name: &str) -> Collection<Document> {
        self.client
            .database(&self.default_db)
            .collection::<Document>(name)
    }
}

#[async_trait]
impl Driver for MongoDriver {
    async fn connect(cfg: &ConnConfig) -> Result<Self> {
        let client = Client::with_uri_str(build_uri(cfg))
            .await
            .map_err(|e| DbmError::Connection(e.to_string()))?;
        let default_db = cfg.database.clone().unwrap_or_else(|| "admin".to_string());
        Ok(MongoDriver { client, default_db })
    }

    async fn ping(&self) -> Result<()> {
        self.client
            .database("admin")
            .run_command(doc! { "ping": 1 })
            .await
            .map_err(|e| DbmError::Connection(e.to_string()))?;
        Ok(())
    }

    async fn schema(&self) -> Result<Schema> {
        let db_names = self
            .client
            .list_database_names()
            .await
            .map_err(|e| DbmError::Schema(e.to_string()))?;

        let mut databases = Vec::new();
        for db_name in db_names {
            let db = self.client.database(&db_name);
            let coll_names = db
                .list_collection_names()
                .await
                .map_err(|e| DbmError::Schema(e.to_string()))?;
            let containers = coll_names
                .into_iter()
                .map(|name| Container {
                    name,
                    kind: ContainerKind::Collection,
                    // Mongo is schemaless: no static field list to report.
                    fields: Vec::new(),
                })
                .collect();
            databases.push(Database {
                name: db_name,
                containers,
            });
        }
        Ok(Schema { databases })
    }

    async fn query(&self, q: &Query) -> Result<ResultSet> {
        let op: &MongoOp = match q {
            Query::Mongo(op) => op,
            _ => return Err(DbmError::UnsupportedQuery),
        };
        let coll = self.collection(&op.collection);

        match &op.kind {
            MongoKind::Find(filter) => {
                let filter_doc = json_to_document(filter)?;
                let cursor = coll
                    .find(filter_doc)
                    .await
                    .map_err(|e| DbmError::Query(e.to_string()))?;
                let docs: Vec<Document> = cursor
                    .try_collect()
                    .await
                    .map_err(|e| DbmError::Query(e.to_string()))?;
                Ok(ResultSet::Documents(
                    docs.into_iter().map(document_to_json).collect(),
                ))
            }
            MongoKind::Insert(payload) => {
                let doc = json_to_document(payload)?;
                coll.insert_one(doc)
                    .await
                    .map_err(|e| DbmError::Query(e.to_string()))?;
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
                    .map_err(|e| DbmError::Query(e.to_string()))?;
                let docs: Vec<Document> = cursor
                    .try_collect()
                    .await
                    .map_err(|e| DbmError::Query(e.to_string()))?;
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
