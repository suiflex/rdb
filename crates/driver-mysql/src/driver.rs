use async_trait::async_trait;
use mysql_async::prelude::Queryable;
use mysql_async::{OptsBuilder, Pool, Row, SslOpts};

use dbm_core::conn::{ConnConfig, SslMode};
use dbm_core::driver::Driver;
use dbm_core::error::{DbmError, Result};
use dbm_core::query::Query;
use dbm_core::result::{Column, ResultSet};
use dbm_core::schema::Schema;

use crate::convert::{column_type_name, value_to_cell};
use crate::schema::{columns_query, fold_rows, SchemaRow};

/// MySQL / MariaDB driver backed by a small mysql_async pool.
pub struct MysqlDriver {
    pool: Pool,
}

fn build_opts(cfg: &ConnConfig) -> OptsBuilder {
    let mut opts = OptsBuilder::default()
        .ip_or_hostname(cfg.host.clone())
        .tcp_port(cfg.port)
        .user(Some(cfg.user.clone()))
        .pass(cfg.password.clone());

    if let Some(db) = &cfg.database {
        opts = opts.db_name(Some(db.clone()));
    }

    match cfg.sslmode {
        SslMode::Disable => opts,
        // Prefer/Require: enable TLS. Accept invalid certs only to keep MVP
        // connectable against self-signed servers; tighten post-MVP.
        SslMode::Prefer | SslMode::Require => opts.ssl_opts(Some(
            SslOpts::default().with_danger_accept_invalid_certs(true),
        )),
    }
}

#[async_trait]
impl Driver for MysqlDriver {
    async fn connect(cfg: &ConnConfig) -> Result<Self> {
        let pool = Pool::new(build_opts(cfg));
        // Eagerly validate the connection so connect() fails fast.
        let mut conn = pool
            .get_conn()
            .await
            .map_err(|e| DbmError::Connection(e.to_string()))?;
        drop(conn.ping().await);
        Ok(MysqlDriver { pool })
    }

    async fn ping(&self) -> Result<()> {
        let mut conn = self
            .pool
            .get_conn()
            .await
            .map_err(|e| DbmError::Connection(e.to_string()))?;
        let _: Vec<i64> = conn
            .query("SELECT 1")
            .await
            .map_err(|e| DbmError::Query(e.to_string()))?;
        Ok(())
    }

    async fn schema(&self) -> Result<Schema> {
        let mut conn = self
            .pool
            .get_conn()
            .await
            .map_err(|e| DbmError::Connection(e.to_string()))?;
        let rows: Vec<SchemaRow> = conn
            .query_map(
                columns_query(),
                |(db, table, col, dtype, is_nullable): (String, String, String, String, String)| {
                    (
                        db,
                        table,
                        col,
                        dtype,
                        is_nullable.eq_ignore_ascii_case("YES"),
                    )
                },
            )
            .await
            .map_err(|e| DbmError::Schema(e.to_string()))?;
        Ok(fold_rows(rows))
    }

    async fn query(&self, q: &Query) -> Result<ResultSet> {
        let sql = match q {
            Query::Sql(s) => s,
            _ => return Err(DbmError::UnsupportedQuery),
        };

        let mut conn = self
            .pool
            .get_conn()
            .await
            .map_err(|e| DbmError::Connection(e.to_string()))?;

        let mut result = conn
            .query_iter(sql.as_str())
            .await
            .map_err(|e| DbmError::Query(e.to_string()))?;

        // A statement with no result set (INSERT/UPDATE/DELETE/DDL) reports
        // affected rows and yields no columns.
        let columns = result.columns();
        let has_cols = columns.as_ref().map(|c| !c.is_empty()).unwrap_or(false);

        if !has_cols {
            let affected = result.affected_rows();
            // Drain to release the connection cleanly.
            result
                .drop_result()
                .await
                .map_err(|e| DbmError::Query(e.to_string()))?;
            return Ok(ResultSet::Affected(affected));
        }

        let cols: Vec<Column> = columns
            .as_ref()
            .map(|cs| {
                cs.iter()
                    .map(|c| Column {
                        name: c.name_str().to_string(),
                        type_name: column_type_name(c.column_type()),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let mysql_rows: Vec<Row> = result
            .collect()
            .await
            .map_err(|e| DbmError::Query(e.to_string()))?;

        let rows = mysql_rows
            .into_iter()
            .map(|r| {
                (0..r.len())
                    .map(|i| match r.as_ref(i) {
                        Some(v) => value_to_cell(v),
                        None => dbm_core::result::Cell::Null,
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        Ok(ResultSet::Tabular { cols, rows })
    }

    async fn close(self) -> Result<()> {
        self.pool
            .disconnect()
            .await
            .map_err(|e| DbmError::Connection(e.to_string()))?;
        Ok(())
    }
}
