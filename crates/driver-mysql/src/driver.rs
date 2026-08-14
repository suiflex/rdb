use async_trait::async_trait;
use mysql_async::prelude::Queryable;
use mysql_async::{OptsBuilder, Pool, Row, SslOpts};

use rdb_core::conn::{ConnConfig, SslMode};
use rdb_core::driver::Driver;
use rdb_core::error::{RdbError, Result};
use rdb_core::query::Query;
use rdb_core::result::{Column, ResultSet};
use rdb_core::schema::Schema;
use rdb_core::write::{TableRef, WriteOp};

use crate::convert::{column_type_name, value_to_cell};
use crate::schema::{columns_query, fold_rows, SchemaRow};
use crate::write_sql;

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
            .map_err(|e| RdbError::Connection(e.to_string()))?;
        drop(conn.ping().await);
        Ok(MysqlDriver { pool })
    }

    async fn ping(&self) -> Result<()> {
        let mut conn = self
            .pool
            .get_conn()
            .await
            .map_err(|e| RdbError::Connection(e.to_string()))?;
        let _: Vec<i64> = conn
            .query("SELECT 1")
            .await
            .map_err(|e| RdbError::Query(e.to_string()))?;
        Ok(())
    }

    async fn schema(&self) -> Result<Schema> {
        let mut conn = self
            .pool
            .get_conn()
            .await
            .map_err(|e| RdbError::Connection(e.to_string()))?;
        let rows: Vec<SchemaRow> = conn
            .query_map(
                columns_query(),
                |(db, table, col, dtype, is_nullable, is_pk, is_fk): (
                    String,
                    String,
                    String,
                    String,
                    String,
                    String,
                    String,
                )| {
                    (
                        db,
                        table,
                        col,
                        dtype,
                        is_nullable.eq_ignore_ascii_case("YES"),
                        is_pk.eq_ignore_ascii_case("YES"),
                        is_fk.eq_ignore_ascii_case("YES"),
                    )
                },
            )
            .await
            .map_err(|e| RdbError::Schema(e.to_string()))?;
        Ok(fold_rows(rows))
    }

    async fn query(&self, q: &Query) -> Result<ResultSet> {
        let sql = match q {
            Query::Sql(s) => s,
            _ => return Err(RdbError::UnsupportedQuery),
        };

        let mut conn = self
            .pool
            .get_conn()
            .await
            .map_err(|e| RdbError::Connection(e.to_string()))?;

        let mut result = conn
            .query_iter(sql.as_str())
            .await
            .map_err(|e| RdbError::Query(my_err(&e.to_string())))?;

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
                .map_err(|e| RdbError::Query(e.to_string()))?;
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
            .map_err(|e| RdbError::Query(e.to_string()))?;

        let rows = mysql_rows
            .into_iter()
            .map(|r| {
                (0..r.len())
                    .map(|i| match r.as_ref(i) {
                        Some(v) => value_to_cell(v),
                        None => rdb_core::result::Cell::Null,
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();

        Ok(ResultSet::Tabular { cols, rows })
    }

    async fn primary_key(&self, table: &TableRef) -> Result<Vec<String>> {
        let mut conn = self
            .pool
            .get_conn()
            .await
            .map_err(|e| RdbError::Connection(e.to_string()))?;
        let cols: Vec<String> = conn
            .exec(
                "SELECT column_name FROM information_schema.key_column_usage \
                 WHERE table_name = ? AND constraint_name = 'PRIMARY' \
                   AND table_schema = COALESCE(?, DATABASE()) \
                 ORDER BY ordinal_position",
                (&table.name, &table.database),
            )
            .await
            .map_err(|e| RdbError::Schema(e.to_string()))?;
        Ok(cols)
    }

    async fn count(&self, table: &TableRef) -> Result<u64> {
        let mut conn = self
            .pool
            .get_conn()
            .await
            .map_err(|e| RdbError::Connection(e.to_string()))?;
        let n: Option<u64> = conn
            .query_first(format!(
                "SELECT COUNT(*) FROM {}",
                write_sql::table_name(table)
            ))
            .await
            .map_err(|e| RdbError::Query(e.to_string()))?;
        Ok(n.unwrap_or(0))
    }

    async fn commit(&self, ops: &[WriteOp]) -> Result<u64> {
        if ops.is_empty() {
            return Ok(0);
        }
        let mut conn = self
            .pool
            .get_conn()
            .await
            .map_err(|e| RdbError::Connection(e.to_string()))?;
        let mut tx = conn
            .start_transaction(mysql_async::TxOpts::default())
            .await
            .map_err(|e| RdbError::Query(e.to_string()))?;
        let mut affected = 0u64;
        for op in ops {
            let (sql, params) = match op {
                WriteOp::Update { table, pk, changes } => write_sql::update_sql(table, pk, changes),
                WriteOp::Insert { table, values } => write_sql::insert_sql(table, values),
                WriteOp::Delete { table, pk } => write_sql::delete_sql(table, pk),
            };
            // A failed statement drops `tx`, which rolls the batch back.
            tx.exec_drop(sql, params)
                .await
                .map_err(|e| RdbError::Query(e.to_string()))?;
            affected += tx.affected_rows();
        }
        tx.commit()
            .await
            .map_err(|e| RdbError::Query(e.to_string()))?;
        Ok(affected)
    }

    async fn close(self) -> Result<()> {
        self.pool
            .disconnect()
            .await
            .map_err(|e| RdbError::Connection(e.to_string()))?;
        Ok(())
    }
}

/// MySQL reports a syntax error's location only inside the message text
/// ("... near 'slect' at line 2"), where the line is relative to the statement
/// it was given. Lift it into the `[[rdb-line:N]]` marker the UI reads to
/// highlight the failing line in the query editor; a message without one is
/// passed through unchanged.
fn my_err(msg: &str) -> String {
    match msg
        .rsplit_once(" at line ")
        .map(|(_, tail)| tail.trim_start())
        .map(|tail| {
            tail.chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
        })
        .filter(|digits| !digits.is_empty())
    {
        Some(line) => format!("[[rdb-line:{line}]] {msg}"),
        None => msg.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::my_err;

    #[test]
    fn my_err_lifts_the_line_out_of_the_message() {
        assert_eq!(
            my_err("You have an error in your SQL syntax; ... near 'slect' at line 3"),
            "[[rdb-line:3]] You have an error in your SQL syntax; ... near 'slect' at line 3"
        );
        assert_eq!(
            my_err("Table 'db.nope' doesn't exist"),
            "Table 'db.nope' doesn't exist"
        );
        assert_eq!(my_err("weird at line abc"), "weird at line abc");
    }
}
