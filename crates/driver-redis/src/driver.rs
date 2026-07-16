use async_trait::async_trait;
use redis::aio::MultiplexedConnection;
use redis::{Client, Value};
use tokio::sync::Mutex;

use rdb_core::conn::ConnConfig;
use rdb_core::driver::Driver;
use rdb_core::error::{RdbsError, Result};
use rdb_core::query::Query;
use rdb_core::result::RedisValue;
use rdb_core::result::{Cell, ResultSet};
use rdb_core::schema::{Container, ContainerKind, Database, Schema};
use rdb_core::write::{TableRef, WriteOp};

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
                functions: Vec::new(),
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

        // `BROWSE <key> [offset limit]` is a UI-only convention (not a real
        // Redis verb): read a key with the right command for its type and
        // return typed rows, windowed for pagination when offset/limit given.
        if tokens[0].eq_ignore_ascii_case("BROWSE") {
            let key = tokens
                .get(1)
                .ok_or_else(|| RdbsError::Query("BROWSE needs a key".into()))?;
            let offset = tokens.get(2).and_then(|t| t.parse::<usize>().ok());
            let limit = tokens.get(3).and_then(|t| t.parse::<usize>().ok());
            return self.browse_key(key, offset, limit).await;
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

    /// Row identity per key type: hashes edit by field, lists by index,
    /// sets/zsets by member, strings are the key's single value.
    async fn primary_key(&self, table: &TableRef) -> Result<Vec<String>> {
        let kind = self.key_type(&table.name).await?;
        Ok(match kind.as_str() {
            "hash" => vec!["field".into()],
            "list" => vec!["index".into()],
            "set" | "zset" => vec!["member".into()],
            "string" => vec!["key".into()],
            _ => Vec::new(),
        })
    }

    async fn count(&self, table: &TableRef) -> Result<u64> {
        let kind = self.key_type(&table.name).await?;
        let cmd = match kind.as_str() {
            "hash" => "HLEN",
            "list" => "LLEN",
            "set" => "SCARD",
            "zset" => "ZCARD",
            "string" => return Ok(1),
            _ => return Ok(0),
        };
        let mut conn = self.conn.lock().await;
        redis::cmd(cmd)
            .arg(&table.name)
            .query_async(&mut *conn)
            .await
            .map_err(|e| RdbsError::Query(e.to_string()))
    }

    /// Sequential type-aware writes; stops at the first failure and reports
    /// how many ops applied (Redis has no cross-command transaction here).
    async fn commit(&self, ops: &[WriteOp]) -> Result<u64> {
        let mut applied = 0u64;
        for op in ops {
            let key = &op.table().name;
            let kind = self.key_type(key).await?;
            self.apply_op(key, &kind, op).await.map_err(|e| {
                RdbsError::Query(format!("{e} (applied {applied} of {} ops)", ops.len()))
            })?;
            applied += 1;
        }
        Ok(applied)
    }

    async fn close(self) -> Result<()> {
        // MultiplexedConnection closes when dropped; nothing to await.
        drop(self.conn);
        Ok(())
    }
}

/// An owned `Cmd` from name + string args (redis-rs `arg` chains return
/// `&mut Cmd`, which cannot leave a match arm).
fn build_cmd(name: &str, args: &[&str]) -> redis::Cmd {
    let mut c = redis::cmd(name);
    for a in args {
        c.arg(*a);
    }
    c
}

/// Find a named value in op payload pairs, rendered to its string form.
fn pair_text(pairs: &[(String, Cell)], name: &str) -> Option<String> {
    pairs.iter().find(|(k, _)| k == name).map(|(_, c)| match c {
        Cell::Null => String::new(),
        other => other.render(),
    })
}

impl RedisDriver {
    async fn key_type(&self, key: &str) -> Result<String> {
        let mut conn = self.conn.lock().await;
        redis::cmd("TYPE")
            .arg(key)
            .query_async(&mut *conn)
            .await
            .map_err(|e| RdbsError::Query(e.to_string()))
    }

    /// One buffered write against `key` of type `kind`. The op payload uses
    /// the browse-grid column names: identity under `field`/`index`/`member`/
    /// `key`, the new content under `value` (zsets: value = score).
    async fn apply_op(&self, key: &str, kind: &str, op: &WriteOp) -> Result<()> {
        let qerr = |e: redis::RedisError| RdbsError::Query(e.to_string());
        let missing = |what: &str| RdbsError::Query(format!("write op missing \"{what}\""));
        let mut conn = self.conn.lock().await;
        match op {
            WriteOp::Update { pk, changes, .. } => {
                let value = pair_text(changes, "value").ok_or_else(|| missing("value"))?;
                let cmd = match kind {
                    "string" => build_cmd("SET", &[key, &value]),
                    "hash" => {
                        let field = pair_text(pk, "field").ok_or_else(|| missing("field"))?;
                        build_cmd("HSET", &[key, &field, &value])
                    }
                    "list" => {
                        let idx = pair_text(pk, "index").ok_or_else(|| missing("index"))?;
                        build_cmd("LSET", &[key, &idx, &value])
                    }
                    "zset" => {
                        let member = pair_text(pk, "member").ok_or_else(|| missing("member"))?;
                        build_cmd("ZADD", &[key, &value, &member])
                    }
                    "set" => {
                        // A set member's "update" is remove-old + add-new.
                        let old = pair_text(pk, "member").ok_or_else(|| missing("member"))?;
                        build_cmd("SREM", &[key, &old])
                            .query_async::<()>(&mut *conn)
                            .await
                            .map_err(qerr)?;
                        build_cmd("SADD", &[key, &value])
                    }
                    other => return Err(RdbsError::Query(format!("cannot edit type {other}"))),
                };
                cmd.query_async::<()>(&mut *conn).await.map_err(qerr)
            }
            WriteOp::Insert { values, .. } => {
                let value = pair_text(values, "value").ok_or_else(|| missing("value"))?;
                let cmd = match kind {
                    // Inserting on a missing key creates it as a string.
                    "string" | "none" => build_cmd("SET", &[key, &value]),
                    "hash" => {
                        let field = pair_text(values, "field").ok_or_else(|| missing("field"))?;
                        build_cmd("HSET", &[key, &field, &value])
                    }
                    "list" => build_cmd("RPUSH", &[key, &value]),
                    "set" => build_cmd("SADD", &[key, &value]),
                    "zset" => {
                        let member =
                            pair_text(values, "member").ok_or_else(|| missing("member"))?;
                        build_cmd("ZADD", &[key, &value, &member])
                    }
                    other => {
                        return Err(RdbsError::Query(format!("cannot insert into type {other}")))
                    }
                };
                cmd.query_async::<()>(&mut *conn).await.map_err(qerr)
            }
            WriteOp::Delete { pk, .. } => {
                let cmd = match kind {
                    "string" => build_cmd("DEL", &[key]),
                    "hash" => {
                        let field = pair_text(pk, "field").ok_or_else(|| missing("field"))?;
                        build_cmd("HDEL", &[key, &field])
                    }
                    "list" => {
                        // Lists delete by index: LSET a sentinel, then LREM it.
                        let idx = pair_text(pk, "index").ok_or_else(|| missing("index"))?;
                        build_cmd("LSET", &[key, &idx, "__rdb_deleted__"])
                            .query_async::<()>(&mut *conn)
                            .await
                            .map_err(qerr)?;
                        build_cmd("LREM", &[key, "1", "__rdb_deleted__"])
                    }
                    "set" => {
                        let member = pair_text(pk, "member").ok_or_else(|| missing("member"))?;
                        build_cmd("SREM", &[key, &member])
                    }
                    "zset" => {
                        let member = pair_text(pk, "member").ok_or_else(|| missing("member"))?;
                        build_cmd("ZREM", &[key, &member])
                    }
                    other => {
                        return Err(RdbsError::Query(format!("cannot delete from type {other}")))
                    }
                };
                cmd.query_async::<()>(&mut *conn).await.map_err(qerr)
            }
        }
    }

    /// Read one key using the command appropriate to its type, returning typed
    /// key/value rows: string→value, list/set→index→member, hash→field→value,
    /// zset→member→score. `offset`/`limit` window the rows (lists/zsets read
    /// only the range; hashes/sets fetch-all then slice — cursor paging can
    /// replace that if huge keys become a real workload).
    async fn browse_key(
        &self,
        key: &str,
        offset: Option<usize>,
        limit: Option<usize>,
    ) -> Result<ResultSet> {
        let qerr = |e: redis::RedisError| RdbsError::Query(e.to_string());
        let off = offset.unwrap_or(0);
        // Range end for LRANGE/ZRANGE: inclusive; no limit → -1 (to the end).
        let end: i64 = match limit {
            Some(l) if l > 0 => off as i64 + l as i64 - 1,
            _ => -1,
        };
        // In-driver window for the fetch-all types (hash/set).
        let window = |rows: Vec<(String, RedisValue)>| match limit {
            Some(l) => rows.into_iter().skip(off).take(l).collect(),
            None if off > 0 => rows.into_iter().skip(off).collect(),
            None => rows,
        };
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
                    .arg(off as i64)
                    .arg(end)
                    .query_async(&mut *conn)
                    .await
                    .map_err(qerr)?;
                // absolute indices: list edits target LSET by index
                crate::convert::pairs_from_members_at(items, off)
            }
            "set" => {
                let items: Vec<String> = redis::cmd("SMEMBERS")
                    .arg(key)
                    .query_async(&mut *conn)
                    .await
                    .map_err(qerr)?;
                window(pairs_from_members(items))
            }
            "hash" => {
                let flat: Vec<String> = redis::cmd("HGETALL")
                    .arg(key)
                    .query_async(&mut *conn)
                    .await
                    .map_err(qerr)?;
                window(pairs_from_flat(flat))
            }
            "zset" => {
                let flat: Vec<String> = redis::cmd("ZRANGE")
                    .arg(key)
                    .arg(off as i64)
                    .arg(end)
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
