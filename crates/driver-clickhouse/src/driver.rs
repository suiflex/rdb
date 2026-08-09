//! ClickHouse driver backed by the `clickhouse` crate's HTTP client. Reads go
//! through `Query::fetch_bytes("JSON")` + manual `serde_json` parsing rather
//! than the crate's typed `Row`/derive-macro path: that path needs the
//! result shape known at compile time, which doesn't fit a browser that runs
//! arbitrary user SQL against unknown tables. ClickHouse's `FORMAT JSON`
//! response is self-describing (`{"meta": [...], "data": [...]}`) and
//! already stringifies anything that could lose precision as plain JSON, so
//! one generic parse covers every query shape — see `convert::json_value_to_cell`.

use async_trait::async_trait;
use clickhouse::Client;
use serde_json::Value as Json;

use rdb_core::conn::{ConnConfig, SslMode};
use rdb_core::driver::Driver;
use rdb_core::error::{RdbError, Result};
use rdb_core::query::Query;
use rdb_core::result::{Cell, Column, ResultSet};
use rdb_core::schema::Schema;
use rdb_core::write::{TableRef, WriteOp};

use crate::convert::json_value_to_cell;
use crate::schema::{columns_query, databases_query, fold_rows, SchemaRow};
use crate::write_sql;

/// ClickHouse driver. The `clickhouse` crate's `Client` is a thin `Arc`-backed
/// HTTP client handle (cheap to hold directly, no pool/mutex needed — every
/// other driver here guards a stateful connection, this one doesn't have one).
pub struct ClickhouseDriver {
    client: Client,
    database: Option<String>,
}

fn build_client(cfg: &ConnConfig) -> Client {
    let scheme = match cfg.sslmode {
        SslMode::Disable => "http",
        SslMode::Prefer | SslMode::Require => "https",
    };
    let mut client = Client::default()
        .with_url(format!("{scheme}://{}:{}", cfg.host, cfg.port))
        .with_user(&cfg.user);
    if let Some(pass) = &cfg.password {
        client = client.with_password(pass);
    }
    if let Some(db) = &cfg.database {
        client = client.with_database(db);
    }
    client
}

/// Run `sql` (which must itself end in `FORMAT JSON` for data-returning
/// statements) and parse the response body as JSON.
async fn fetch_json(client: &Client, sql: &str) -> Result<Json> {
    let bytes = client
        .query(sql)
        .fetch_bytes("JSON")
        .map_err(|e| RdbError::Query(e.to_string()))?
        .collect()
        .await
        .map_err(|e| RdbError::Query(e.to_string()))?;
    serde_json::from_slice(&bytes).map_err(|e| RdbError::Query(e.to_string()))
}

fn json_str<'a>(row: &'a Json, key: &str) -> &'a str {
    row.get(key).and_then(Json::as_str).unwrap_or_default()
}

#[async_trait]
impl Driver for ClickhouseDriver {
    async fn connect(cfg: &ConnConfig) -> Result<Self> {
        let client = build_client(cfg);
        // Eagerly validate the connection so connect() fails fast.
        fetch_json(&client, "SELECT 1 FORMAT JSON").await?;
        Ok(ClickhouseDriver {
            client,
            database: cfg.database.clone(),
        })
    }

    async fn ping(&self) -> Result<()> {
        fetch_json(&self.client, "SELECT 1 FORMAT JSON")
            .await
            .map_err(|e| RdbError::Connection(e.to_string()))?;
        Ok(())
    }

    async fn schema(&self) -> Result<Schema> {
        let db = self.database.as_deref().unwrap_or("default");
        schema_impl(&self.client, db).await
    }

    async fn list_databases(&self) -> Result<Vec<String>> {
        let body = fetch_json(&self.client, databases_query())
            .await
            .map_err(|e| RdbError::Schema(e.to_string()))?;
        let rows = body["data"].as_array().cloned().unwrap_or_default();
        Ok(rows
            .iter()
            .map(|r| json_str(r, "name").to_string())
            .collect())
    }

    async fn containers(&self, database: &str) -> Result<Vec<rdb_core::schema::Container>> {
        Ok(schema_impl(&self.client, database)
            .await?
            .databases
            .into_iter()
            .next()
            .map(|d| d.containers)
            .unwrap_or_default())
    }

    async fn query(&self, q: &Query) -> Result<ResultSet> {
        let sql = match q {
            Query::Sql(s) => s,
            Query::Cql(_) | Query::Command(_) | Query::Mongo(_) => {
                return Err(RdbError::UnsupportedQuery)
            }
        };
        let trimmed = sql.trim();
        let upper = trimmed.to_uppercase();
        // ponytail: fetch_bytes needs the response format decided before the
        // request is sent, and FORMAT is only valid on statements that
        // return rows, so sniff the leading keyword rather than trying every
        // statement both ways. mysql_async/tokio-postgres don't need this —
        // their protocols report "has columns" after the fact regardless of
        // statement type — but ClickHouse's HTTP+FORMAT interface doesn't
        // give us that. Good enough for v1; a real parser would be needed to
        // do better (e.g. `EXPLAIN`/CTEs that start with a DDL-looking word).
        let returns_rows = ["SELECT", "WITH", "SHOW", "DESCRIBE", "DESC", "EXISTS"]
            .iter()
            .any(|kw| upper.starts_with(kw));

        if !returns_rows {
            self.client
                .query(trimmed)
                .execute()
                .await
                .map_err(|e| RdbError::Query(e.to_string()))?;
            // ClickHouse's HTTP interface doesn't report an affected-row
            // count for DDL/INSERT — same situation as driver-cassandra and
            // driver-mssql.
            return Ok(ResultSet::Affected(0));
        }

        let body = fetch_json(&self.client, &format!("{trimmed} FORMAT JSON"))
            .await
            .map_err(|e| RdbError::Query(e.to_string()))?;
        let meta = body["meta"].as_array().cloned().unwrap_or_default();
        let cols: Vec<Column> = meta
            .iter()
            .map(|m| Column {
                name: json_str(m, "name").to_string(),
                type_name: json_str(m, "type").to_string(),
            })
            .collect();
        let data = body["data"].as_array().cloned().unwrap_or_default();
        let rows: Vec<Vec<Cell>> = data
            .iter()
            .map(|row| {
                cols.iter()
                    .map(|c| json_value_to_cell(row.get(&c.name).unwrap_or(&Json::Null)))
                    .collect()
            })
            .collect();
        Ok(ResultSet::Tabular { cols, rows })
    }

    async fn count(&self, table: &TableRef) -> Result<u64> {
        let sql = format!(
            "SELECT count() FROM {} FORMAT JSON",
            write_sql::table_name(table)
        );
        let body = fetch_json(&self.client, &sql)
            .await
            .map_err(|e| RdbError::Query(e.to_string()))?;
        let n = body["data"]
            .as_array()
            .and_then(|rows| rows.first())
            .and_then(|row| row.as_object().and_then(|m| m.values().next()))
            .and_then(|v| {
                v.as_str()
                    .and_then(|s| s.parse::<u64>().ok())
                    .or(v.as_u64())
            })
            .unwrap_or(0);
        Ok(n)
    }

    async fn commit(&self, ops: &[WriteOp]) -> Result<u64> {
        if ops.is_empty() {
            return Ok(0);
        }
        // Insert-only: ClickHouse's UPDATE/DELETE are async eventually-
        // consistent mutations (ALTER TABLE ... UPDATE/DELETE), not the
        // immediate row edits WriteOp implies elsewhere — reject the whole
        // batch up front rather than partially applying it.
        if ops.iter().any(|op| !matches!(op, WriteOp::Insert { .. })) {
            return Err(RdbError::UnsupportedQuery);
        }
        let mut affected = 0u64;
        for op in ops {
            let WriteOp::Insert { table, values } = op else {
                unreachable!("checked above");
            };
            let sql = write_sql::insert_sql(table, values);
            self.client
                .query(&sql)
                .execute()
                .await
                .map_err(|e| RdbError::Query(e.to_string()))?;
            affected += 1;
        }
        Ok(affected)
    }

    async fn close(self) -> Result<()> {
        Ok(())
    }
}

async fn schema_impl(client: &Client, database: &str) -> Result<Schema> {
    let body = fetch_json(client, &columns_query(database))
        .await
        .map_err(|e| RdbError::Schema(e.to_string()))?;
    let rows: Vec<SchemaRow> = body["data"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .map(|r| {
            (
                json_str(r, "table").to_string(),
                json_str(r, "name").to_string(),
                json_str(r, "type").to_string(),
            )
        })
        .collect();
    Ok(fold_rows(database, rows))
}
