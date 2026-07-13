use async_trait::async_trait;
use futures::stream::TryStreamExt;
use std::time::Duration;

use mongodb::bson::{doc, Document};
use mongodb::options::ClientOptions;
use mongodb::{Client, Collection};

use rdbs_core::conn::{ConnConfig, SslMode};
use rdbs_core::driver::Driver;
use rdbs_core::error::{RdbsError, Result};
use rdbs_core::query::{MongoKind, MongoOp, Query};
use rdbs_core::result::{Cell, ResultSet};
use rdbs_core::schema::{Container, ContainerKind, Database, Schema};
use rdbs_core::write::{TableRef, WriteOp};

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
    // Full-URI override: paste a `mongodb://…` / `mongodb+srv://…` string into
    // the connection's params and it is used verbatim (Atlas SRV, custom auth,
    // replica sets — anything the host/port form can't express).
    if let Some(p) = cfg.params.as_deref() {
        let p = p.trim();
        if p.starts_with("mongodb://") || p.starts_with("mongodb+srv://") {
            return p.to_string();
        }
    }

    let auth = match &cfg.password {
        Some(pw) if !pw.is_empty() => format!("{}:{}@", cfg.user, pw),
        _ => String::new(),
    };
    // Query is the merge of TLS (Prefer/Require -> tls with no cert/hostname
    // validation, matching the other drivers) and any user-supplied options.
    let mut query: Vec<String> = Vec::new();
    if matches!(cfg.sslmode, SslMode::Prefer | SslMode::Require) {
        query.push("tls=true".into());
        query.push("tlsInsecure=true".into());
    }
    if let Some(p) = cfg.params.as_deref() {
        let p = p.trim().trim_start_matches('?');
        if !p.is_empty() {
            query.push(p.to_string());
        }
    }
    // The host/port form is always a single literal host, so default to a direct
    // connection (matching Compass): without it the driver runs topology
    // discovery, and a single RS member behind a NodePort/port-forward advertises
    // internal addresses that are unreachable from outside → server-selection
    // hang. Skip when the user already controls topology.
    let params_lower = cfg.params.as_deref().unwrap_or("").to_ascii_lowercase();
    if !params_lower.contains("directconnection") && !params_lower.contains("replicaset") {
        query.push("directConnection=true".into());
    }
    let q = if query.is_empty() {
        String::new()
    } else {
        format!("?{}", query.join("&"))
    };
    format!("mongodb://{auth}{}:{}/{q}", cfg.host, cfg.port)
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
        let mut options = ClientOptions::parse(build_uri(cfg))
            .await
            .map_err(|e| RdbsError::Connection(e.to_string()))?;
        // Fail fast on an unreachable host/replica set instead of the 30s default
        // server-selection hang.
        options.server_selection_timeout = Some(Duration::from_secs(8));
        options.connect_timeout = Some(Duration::from_secs(8));
        let client =
            Client::with_options(options).map_err(|e| RdbsError::Connection(e.to_string()))?;
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
                functions: Vec::new(),
                name,
                containers: Vec::new(),
            })
            .collect();
        Ok(Schema { databases })
    }

    /// Scope the sidebar to one database: return just that database with its
    /// collections loaded, so picking it in the schema picker shows only its
    /// collections instead of every database.
    async fn schema_for(&self, database: &str) -> Result<Schema> {
        let containers = self.containers(database).await?;
        Ok(Schema {
            databases: vec![Database {
                name: database.to_string(),
                containers,
                functions: Vec::new(),
            }],
        })
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
                if let Some(s) = op.skip {
                    find = find.skip(s.max(0) as u64);
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

    /// Documents are always identified by `_id`.
    async fn primary_key(&self, _table: &TableRef) -> Result<Vec<String>> {
        Ok(vec!["_id".to_string()])
    }

    async fn count(&self, table: &TableRef) -> Result<u64> {
        let db = table.database.as_deref().unwrap_or(&self.default_db);
        self.client
            .database(db)
            .collection::<Document>(&table.name)
            .count_documents(doc! {})
            .await
            .map_err(|e| RdbsError::Query(e.to_string()))
    }

    /// Sequential (no multi-doc transaction — standalone servers lack them);
    /// stops at the first failure and reports how many ops applied.
    async fn commit(&self, ops: &[WriteOp]) -> Result<u64> {
        let mut applied = 0u64;
        for op in ops {
            let table = op.table();
            let db = table.database.as_deref().unwrap_or(&self.default_db);
            let coll = self.client.database(db).collection::<Document>(&table.name);
            let res = match op {
                WriteOp::Update { pk, changes, .. } => {
                    let id = pk_id(pk)?;
                    let mut sets = Document::new();
                    for (field, val) in changes {
                        sets.insert(field.clone(), cell_to_bson(val));
                    }
                    coll.update_one(doc! { "_id": id }, doc! { "$set": sets })
                        .await
                        .map(|_| ())
                }
                WriteOp::Insert { values, .. } => {
                    let mut d = Document::new();
                    for (field, val) in values {
                        d.insert(field.clone(), cell_to_bson(val));
                    }
                    coll.insert_one(d).await.map(|_| ())
                }
                WriteOp::Delete { pk, .. } => {
                    let id = pk_id(pk)?;
                    coll.delete_one(doc! { "_id": id }).await.map(|_| ())
                }
            };
            match res {
                Ok(()) => applied += 1,
                Err(e) => {
                    return Err(RdbsError::Query(format!(
                        "{e} (applied {applied} of {} ops)",
                        ops.len()
                    )))
                }
            }
        }
        Ok(applied)
    }

    async fn close(self) -> Result<()> {
        // mongodb::Client has no explicit close; dropping it ends background tasks.
        drop(self.client);
        Ok(())
    }
}

/// The `_id` value from the op identity pairs. Hex ObjectId strings become
/// real ObjectIds (the flattened grid shows them as hex); anything else is
/// matched as its literal BSON value.
fn pk_id(pk: &[(String, Cell)]) -> Result<mongodb::bson::Bson> {
    let (_, cell) = pk
        .iter()
        .find(|(k, _)| k == "_id")
        .or_else(|| pk.first())
        .ok_or_else(|| RdbsError::Query("write op without a row identity".into()))?;
    if let Cell::Text(s) = cell {
        if let Ok(oid) = mongodb::bson::oid::ObjectId::parse_str(s) {
            return Ok(mongodb::bson::Bson::ObjectId(oid));
        }
    }
    Ok(cell_to_bson(cell))
}

fn cell_to_bson(c: &Cell) -> mongodb::bson::Bson {
    use mongodb::bson::Bson;
    match c {
        Cell::Null => Bson::Null,
        Cell::Int(i) => Bson::Int64(*i),
        Cell::Float(f) => Bson::Double(*f),
        Cell::Bool(b) => Bson::Boolean(*b),
        Cell::Text(s) => Bson::String(s.clone()),
        Cell::Bytes(b) => Bson::Binary(mongodb::bson::Binary {
            subtype: mongodb::bson::spec::BinarySubtype::Generic,
            bytes: b.clone(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_id_becomes_objectid_other_text_stays_string() {
        let oid = "657f1f77bcf86cd799439011";
        let b = pk_id(&[("_id".into(), Cell::Text(oid.into()))]).unwrap();
        assert!(matches!(b, mongodb::bson::Bson::ObjectId(_)));
        let b = pk_id(&[("_id".into(), Cell::Text("user-42".into()))]).unwrap();
        assert_eq!(b, mongodb::bson::Bson::String("user-42".into()));
    }

    #[test]
    fn pk_id_requires_an_identity() {
        assert!(pk_id(&[]).is_err());
    }

    #[test]
    fn cells_map_to_bson() {
        use mongodb::bson::Bson;
        assert_eq!(cell_to_bson(&Cell::Null), Bson::Null);
        assert_eq!(cell_to_bson(&Cell::Int(3)), Bson::Int64(3));
        assert_eq!(cell_to_bson(&Cell::Bool(true)), Bson::Boolean(true));
    }

    fn cfg(params: Option<&str>, ssl: SslMode) -> ConnConfig {
        ConnConfig {
            host: "db.internal".into(),
            port: 27017,
            user: "u".into(),
            database: Some("app".into()),
            password: Some("p".into()),
            sslmode: ssl,
            params: params.map(Into::into),
        }
    }

    #[test]
    fn build_uri_plain_has_trailing_slash_no_query() {
        assert_eq!(
            build_uri(&cfg(None, SslMode::Disable)),
            "mongodb://u:p@db.internal:27017/?directConnection=true"
        );
    }

    #[test]
    fn build_uri_only_authsource_still_gets_direct_connection() {
        let uri = build_uri(&cfg(Some("authSource=admin"), SslMode::Disable));
        assert_eq!(
            uri,
            "mongodb://u:p@db.internal:27017/?authSource=admin&directConnection=true"
        );
    }

    #[test]
    fn build_uri_merges_tls_and_params() {
        let uri = build_uri(&cfg(
            Some("?replicaSet=rs0&authSource=admin"),
            SslMode::Require,
        ));
        assert_eq!(
            uri,
            "mongodb://u:p@db.internal:27017/?tls=true&tlsInsecure=true&replicaSet=rs0&authSource=admin"
        );
    }

    #[test]
    fn build_uri_params_without_leading_question_mark() {
        let uri = build_uri(&cfg(Some("directConnection=true"), SslMode::Disable));
        assert_eq!(
            uri,
            "mongodb://u:p@db.internal:27017/?directConnection=true"
        );
    }

    #[test]
    fn build_uri_full_uri_override_is_verbatim() {
        let srv = "mongodb+srv://user:pass@cluster0.abc.mongodb.net/app?retryWrites=true";
        assert_eq!(build_uri(&cfg(Some(srv), SslMode::Require)), srv);
    }

    #[test]
    fn system_dbs_are_hidden() {
        assert!(is_system_db("admin"));
        assert!(is_system_db("config"));
        assert!(is_system_db("local"));
        assert!(!is_system_db("appdb"));
        assert!(!is_system_db("local_app"));
    }
}
