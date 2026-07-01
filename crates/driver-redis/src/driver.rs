use async_trait::async_trait;
use redis::aio::MultiplexedConnection;
use redis::{Client, Value};
use tokio::sync::Mutex;

use rdbs_core::conn::ConnConfig;
use rdbs_core::driver::Driver;
use rdbs_core::error::{RdbsError, Result};
use rdbs_core::query::Query;
use rdbs_core::result::RedisValue;
use rdbs_core::result::ResultSet;
use rdbs_core::schema::{Container, ContainerKind, Database, Schema};

use crate::convert::{connection_url, pairs_from_flat, pairs_from_members, value_to_resultset};

/// Upper bound on keys surfaced when a Redis database is expanded.
/// ponytail: single SCAN pass, wire the returned cursor for paging if needed.
const MAX_KEYS: usize = 200;

/// Redis driver over a multiplexed async connection. The connection is shared
/// behind a Mutex because commands take `&mut` and the trait exposes `&self`.
pub struct RedisDriver {
    conn: Mutex<MultiplexedConnection>,
    /// Numbered DB index this connection is bound to (for schema labeling).
    db: String,
}

#[async_trait]
impl Driver for RedisDriver {
    async fn connect(cfg: &ConnConfig) -> Result<Self> {
        let client =
            Client::open(connection_url(cfg)).map_err(|e| RdbsError::Connection(e.to_string()))?;
        let conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| RdbsError::Connection(e.to_string()))?;
        let db = cfg.database.clone().unwrap_or_else(|| "0".to_string());
        Ok(RedisDriver {
            conn: Mutex::new(conn),
            db,
        })
    }

    async fn ping(&self) -> Result<()> {
        let mut conn = self.conn.lock().await;
        let reply: String = redis::cmd("PING")
            .query_async(&mut *conn)
            .await
            .map_err(|e| RdbsError::Connection(e.to_string()))?;
        if reply == "PONG" {
            Ok(())
        } else {
            Err(RdbsError::Connection(format!(
                "unexpected PING reply: {reply}"
            )))
        }
    }

    async fn schema(&self) -> Result<Schema> {
        // One expandable database header; keys load lazily via `containers()` the
        // first time it is expanded (mirrors Mongo's database→collection tree).
        Ok(Schema {
            databases: vec![Database {
                name: format!("db{}", self.db),
                containers: Vec::new(),
            }],
        })
    }

    async fn containers(&self, _database: &str) -> Result<Vec<Container>> {
        let mut conn = self.conn.lock().await;
        // SCAN over one bounded pass; COUNT is a hint, not a hard limit.
        let (_cursor, keys): (String, Vec<String>) = redis::cmd("SCAN")
            .arg(0)
            .arg("COUNT")
            .arg(MAX_KEYS as i64)
            .query_async(&mut *conn)
            .await
            .map_err(|e| RdbsError::Schema(e.to_string()))?;
        Ok(keys
            .into_iter()
            .map(|k| Container {
                name: k,
                kind: ContainerKind::Keyspace,
                fields: Vec::new(),
            })
            .collect())
    }

    async fn query(&self, q: &Query) -> Result<ResultSet> {
        let tokens = match q {
            Query::Command(tokens) => tokens,
            _ => return Err(RdbsError::UnsupportedQuery),
        };
        if tokens.is_empty() {
            return Err(RdbsError::Query("empty command".into()));
        }

        // `BROWSE <key>` is a UI-only convention (not a real Redis verb): read a
        // key with the right command for its type and return typed rows.
        if tokens[0].eq_ignore_ascii_case("BROWSE") {
            let key = tokens
                .get(1)
                .ok_or_else(|| RdbsError::Query("BROWSE needs a key".into()))?;
            return self.browse_key(key).await;
        }

        let mut cmd = redis::cmd(&tokens[0]);
        for arg in &tokens[1..] {
            cmd.arg(arg);
        }

        let mut conn = self.conn.lock().await;
        let value: Value = cmd
            .query_async(&mut *conn)
            .await
            .map_err(|e| RdbsError::Query(e.to_string()))?;

        Ok(value_to_resultset(tokens[0].to_uppercase(), value))
    }

    async fn close(self) -> Result<()> {
        // MultiplexedConnection closes when dropped; nothing to await.
        drop(self.conn);
        Ok(())
    }
}

impl RedisDriver {
    /// Read one key using the command appropriate to its type, returning typed
    /// key/value rows: string→value, list/set→index→member, hash→field→value,
    /// zset→member→score.
    async fn browse_key(&self, key: &str) -> Result<ResultSet> {
        let qerr = |e: redis::RedisError| RdbsError::Query(e.to_string());
        let mut conn = self.conn.lock().await;
        let kind: String = redis::cmd("TYPE")
            .arg(key)
            .query_async(&mut *conn)
            .await
            .map_err(qerr)?;
        let rows = match kind.as_str() {
            "string" => {
                let v: Option<String> = redis::cmd("GET")
                    .arg(key)
                    .query_async(&mut *conn)
                    .await
                    .map_err(qerr)?;
                vec![(
                    key.to_string(),
                    v.map(RedisValue::Str).unwrap_or(RedisValue::Nil),
                )]
            }
            "list" => {
                let items: Vec<String> = redis::cmd("LRANGE")
                    .arg(key)
                    .arg(0)
                    .arg(-1)
                    .query_async(&mut *conn)
                    .await
                    .map_err(qerr)?;
                pairs_from_members(items)
            }
            "set" => {
                let items: Vec<String> = redis::cmd("SMEMBERS")
                    .arg(key)
                    .query_async(&mut *conn)
                    .await
                    .map_err(qerr)?;
                pairs_from_members(items)
            }
            "hash" => {
                let flat: Vec<String> = redis::cmd("HGETALL")
                    .arg(key)
                    .query_async(&mut *conn)
                    .await
                    .map_err(qerr)?;
                pairs_from_flat(flat)
            }
            "zset" => {
                let flat: Vec<String> = redis::cmd("ZRANGE")
                    .arg(key)
                    .arg(0)
                    .arg(-1)
                    .arg("WITHSCORES")
                    .query_async(&mut *conn)
                    .await
                    .map_err(qerr)?;
                pairs_from_flat(flat)
            }
            "none" => vec![(key.to_string(), RedisValue::Nil)],
            other => vec![(
                key.to_string(),
                RedisValue::Str(format!("unsupported type: {other}")),
            )],
        };
        Ok(ResultSet::KeyValue(rows))
    }
}
