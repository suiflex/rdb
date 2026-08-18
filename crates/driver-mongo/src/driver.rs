use async_trait::async_trait;
use futures::stream::TryStreamExt;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use mongodb::bson::{doc, Document};
use mongodb::options::{ClientOptions, ServerMonitoringMode};
use mongodb::{Client, Collection};

use rdb_core::conn::{ConnConfig, SslMode};
use rdb_core::driver::Driver;
use rdb_core::error::{RdbError, Result};
use rdb_core::query::{MongoKind, MongoOp, Query};
use rdb_core::result::{Cell, ResultSet};
use rdb_core::schema::{Container, ContainerKind, Database, Field, Schema};
use rdb_core::write::{TableRef, WriteOp};

use crate::convert::{document_to_json, json_to_document};

/// MongoDB driver over a `mongodb::Client`. The client is internally pooled and
/// cheap to clone, so we share `&self` directly.
pub struct MongoDriver {
    client: Client,
    /// Default database used when an op does not imply one.
    default_db: String,
    /// Max collections listed per database (the sidebar cap). Interior-mutable
    /// so the UI can push the user's NoSQL limit onto a live connection.
    collection_limit: Arc<AtomicUsize>,
}

/// Fallback collection cap when the UI has not pushed a limit yet.
const DEFAULT_COLLECTION_LIMIT: usize = 200;

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
    // TLS is opt-in for Mongo: only `Require` enforces it (with no cert/hostname
    // validation, matching the other drivers). `Prefer` means opportunistic TLS,
    // but the mongodb driver has no plaintext fallback, so forcing tls=true on
    // Prefer resets every plaintext server. Treat Prefer as no-TLS; users who
    // need mandatory TLS pick Require.
    let mut query: Vec<String> = Vec::new();
    if matches!(cfg.sslmode, SslMode::Require) {
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
    /// Push the user's NoSQL collection cap onto this live connection. Affects
    /// subsequent sidebar refreshes.
    pub fn set_collection_limit(&self, n: usize) {
        self.collection_limit.store(n.max(1), Ordering::Relaxed);
    }

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
            .map_err(|e| RdbError::Connection(e.to_string()))?;
        // Fail fast on an unreachable host/replica set instead of the 30s default
        // server-selection hang.
        options.server_selection_timeout = Some(Duration::from_secs(8));
        options.connect_timeout = Some(Duration::from_secs(8));
        // The driver's default streaming (awaitable isMaster) SDAM monitor
        // keeps a long-poll socket open per server and re-arms it in a tight
        // loop; on Windows that socket churn shows up as sustained >10% CPU
        // even while idle. A desktop client doesn't need sub-second topology
        // change detection, so fall back to plain interval polling and widen
        // the interval well past the driver's 10s default.
        options.server_monitoring_mode = Some(ServerMonitoringMode::Poll);
        options.heartbeat_freq = Some(Duration::from_secs(30));
        let client =
            Client::with_options(options).map_err(|e| RdbError::Connection(e.to_string()))?;
        let default_db = cfg.database.clone().unwrap_or_else(|| "admin".to_string());
        Ok(MongoDriver {
            client,
            default_db,
            collection_limit: Arc::new(AtomicUsize::new(DEFAULT_COLLECTION_LIMIT)),
        })
    }

    async fn ping(&self) -> Result<()> {
        self.client
            .database("admin")
            .run_command(doc! { "ping": 1 })
            .await
            .map_err(|e| RdbError::Connection(e.to_string()))?;
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
            .map_err(|e| RdbError::Schema(e.to_string()))?;
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

    /// All non-system databases, backing the "schema:" switcher so the user can
    /// still reach every database even when the tree is scoped to one on connect.
    async fn list_databases(&self) -> Result<Vec<String>> {
        let mut names: Vec<String> = self
            .client
            .list_database_names()
            .await
            .map_err(|e| RdbError::Schema(e.to_string()))?
            .into_iter()
            .filter(|n| !is_system_db(n))
            .collect();
        // Alphabetical, matching how the SQL drivers ORDER BY name.
        names.sort();
        Ok(names)
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

    /// Collections of one database, capped by the user's NoSQL collection limit
    /// so a database with thousands of collections stays light.
    async fn containers(&self, database: &str) -> Result<Vec<Container>> {
        let limit = self.collection_limit.load(Ordering::Relaxed);
        let mut coll_names = self
            .client
            .database(database)
            .list_collection_names()
            .await
            .map_err(|e| RdbError::Schema(e.to_string()))?;
        // Sort before the limit so the A–Z cap is a stable prefix, not a
        // random slice — mirrors ORDER BY name in the SQL drivers.
        coll_names.sort();
        Ok(coll_names
            .into_iter()
            .take(limit)
            .map(|name| Container {
                name,
                kind: ContainerKind::Collection,
                // Mongo is schemaless: no static field list to report.
                fields: Vec::new(),
            })
            .collect())
    }

    /// Mongo has no real schema, so completion falls back to sampling: union
    /// the top-level keys (plus one level of nested-object dot notation,
    /// e.g. `address.city`) across up to `sample_size` documents.
    async fn sample_fields(
        &self,
        database: &str,
        container: &str,
        sample_size: u32,
    ) -> Result<Vec<Field>> {
        let coll: Collection<Document> = self.client.database(database).collection(container);
        let mut cursor = coll
            .find(doc! {})
            .limit(sample_size as i64)
            .await
            .map_err(|e| RdbError::Schema(e.to_string()))?;
        let mut order: Vec<String> = Vec::new();
        let mut types: std::collections::HashMap<String, &'static str> =
            std::collections::HashMap::new();
        while let Some(d) = cursor
            .try_next()
            .await
            .map_err(|e| RdbError::Schema(e.to_string()))?
        {
            collect_keys(&d, "", &mut order, &mut types);
        }
        Ok(order
            .into_iter()
            .map(|name| Field {
                type_name: types.get(&name).copied().unwrap_or("mixed").to_string(),
                name,
                nullable: true,
                pk: false,
                fk: false,
            })
            .collect())
    }

    async fn query(&self, q: &Query) -> Result<ResultSet> {
        let op: &MongoOp = match q {
            Query::Mongo(op) => op,
            _ => return Err(RdbError::UnsupportedQuery),
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
                if let Some(sort) = &op.sort {
                    find = find.sort(json_to_document(sort)?);
                }
                let cursor = find.await.map_err(|e| RdbError::Query(e.to_string()))?;
                let docs: Vec<Document> = cursor
                    .try_collect()
                    .await
                    .map_err(|e| RdbError::Query(e.to_string()))?;
                Ok(ResultSet::Documents(
                    docs.into_iter().map(document_to_json).collect(),
                ))
            }
            MongoKind::Insert(payload) => {
                let doc = json_to_document(payload)?;
                coll.insert_one(doc)
                    .await
                    .map_err(|e| RdbError::Query(e.to_string()))?;
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
                    .map_err(|e| RdbError::Query(e.to_string()))?;
                let docs: Vec<Document> = cursor
                    .try_collect()
                    .await
                    .map_err(|e| RdbError::Query(e.to_string()))?;
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
            .map_err(|e| RdbError::Query(e.to_string()))
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
                    return Err(RdbError::Query(format!(
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
        .ok_or_else(|| RdbError::Query("write op without a row identity".into()))?;
    if let Cell::Text(s) = cell {
        if let Ok(oid) = mongodb::bson::oid::ObjectId::parse_str(s) {
            return Ok(mongodb::bson::Bson::ObjectId(oid));
        }
    }
    Ok(cell_to_bson(cell))
}

/// Insert-order union of a document's keys into `order`/`types`, one level
/// of nested objects deep (`address.city`), used by [`MongoDriver::sample_fields`].
fn collect_keys(
    doc: &Document,
    prefix: &str,
    order: &mut Vec<String>,
    types: &mut std::collections::HashMap<String, &'static str>,
) {
    for (key, value) in doc {
        let full = format!("{prefix}{key}");
        if !types.contains_key(&full) {
            order.push(full.clone());
        }
        types.insert(full.clone(), bson_type_name(value));
        if let mongodb::bson::Bson::Document(sub) = value {
            collect_keys(sub, &format!("{full}."), order, types);
        }
    }
}

fn bson_type_name(b: &mongodb::bson::Bson) -> &'static str {
    use mongodb::bson::Bson;
    match b {
        Bson::Double(_) => "double",
        Bson::String(_) => "string",
        Bson::Array(_) => "array",
        Bson::Document(_) => "object",
        Bson::Boolean(_) => "bool",
        Bson::Null => "null",
        Bson::Int32(_) => "int32",
        Bson::Int64(_) => "int64",
        Bson::ObjectId(_) => "objectId",
        Bson::DateTime(_) => "date",
        _ => "mixed",
    }
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
            ssh: None,
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
    fn build_uri_prefer_does_not_force_tls() {
        // Prefer is opportunistic; Mongo has no fallback, so it must stay plaintext.
        assert_eq!(
            build_uri(&cfg(None, SslMode::Prefer)),
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
    fn collect_keys_unions_top_level_and_one_nested_level() {
        let mut order = Vec::new();
        let mut types = std::collections::HashMap::new();
        collect_keys(
            &doc! { "name": "a", "age": 1, "address": { "city": "x" } },
            "",
            &mut order,
            &mut types,
        );
        collect_keys(
            &doc! { "name": "b", "email": "b@x.com", "address": { "zip": "1" } },
            "",
            &mut order,
            &mut types,
        );
        assert_eq!(
            order,
            vec![
                "name",
                "age",
                "address",
                "address.city",
                "email",
                "address.zip"
            ]
        );
        assert_eq!(types["age"], "int32");
        assert_eq!(types["address"], "object");
        assert_eq!(types["address.city"], "string");
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
