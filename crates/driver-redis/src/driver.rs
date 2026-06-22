use async_trait::async_trait;
use redis::aio::MultiplexedConnection;
use redis::{Client, Value};
use tokio::sync::Mutex;

use rdbs_core::conn::ConnConfig;
use rdbs_core::driver::Driver;
use rdbs_core::error::{RdbsError, Result};
use rdbs_core::query::Query;
use rdbs_core::result::ResultSet;
use rdbs_core::schema::{Container, ContainerKind, Database, Field, Schema};

use crate::convert::{connection_url, value_to_resultset};

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
        let mut conn = self.conn.lock().await;
        let size: i64 = redis::cmd("DBSIZE")
            .query_async(&mut *conn)
            .await
            .map_err(|e| RdbsError::Schema(e.to_string()))?;
        drop(conn);

        // Redis is schemaless: surface one keyspace container summarizing key count.
        let container = Container {
            name: format!("keys ({size})"),
            kind: ContainerKind::Keyspace,
            fields: Vec::<Field>::new(),
        };
        Ok(Schema {
            databases: vec![Database {
                name: format!("db{}", self.db),
                containers: vec![container],
            }],
        })
    }

    async fn query(&self, q: &Query) -> Result<ResultSet> {
        let tokens = match q {
            Query::Command(tokens) => tokens,
            _ => return Err(RdbsError::UnsupportedQuery),
        };
        if tokens.is_empty() {
            return Err(RdbsError::Query("empty command".into()));
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
