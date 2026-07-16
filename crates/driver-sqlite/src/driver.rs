use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rusqlite::types::ValueRef;
use rusqlite::Connection;

use rdb_core::conn::ConnConfig;
use rdb_core::driver::Driver;
use rdb_core::error::{RdbError, Result};
use rdb_core::query::Query;
use rdb_core::result::{Cell, Column, ResultSet, Row};
use rdb_core::schema::{Container, ContainerKind, Database, Field, Schema};
use rdb_core::write::{TableRef, WriteOp};

use crate::write_sql;

/// A `Driver` backed by a single rusqlite connection.
///
/// ponytail: one global connection mutex. SQLite is a single local file with
/// no concurrency to exploit; a pool (r2d2) is the upgrade path only if
/// concurrent tabs ever contend.
pub struct SqliteDriver {
    conn: Arc<Mutex<Connection>>,
    /// Logical database name shown in the schema tree (the file stem).
    db_name: String,
}

impl SqliteDriver {
    /// Run a closure with the locked connection on a blocking thread.
    async fn with_conn<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
        T: Send + 'static,
    {
        let conn = self.conn.clone();
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().expect("sqlite mutex poisoned");
            f(&guard)
        })
        .await
        .map_err(|e| RdbError::Query(e.to_string()))?
    }
}

#[async_trait]
impl Driver for SqliteDriver {
    async fn connect(cfg: &ConnConfig) -> Result<Self> {
        // The file path travels in `database` (host/port/user are unused for a
        // local file). Empty or ":memory:" opens an in-memory database.
        let path = cfg.database.clone().unwrap_or_default();
        let db_name = db_name_from_path(&path);
        let conn = tokio::task::spawn_blocking(move || {
            if path.is_empty() || path == ":memory:" {
                Connection::open_in_memory()
            } else {
                Connection::open(&path)
            }
        })
        .await
        .map_err(|e| RdbError::Connection(e.to_string()))?
        .map_err(|e| RdbError::Connection(e.to_string()))?;
        Ok(SqliteDriver {
            conn: Arc::new(Mutex::new(conn)),
            db_name,
        })
    }

    async fn ping(&self) -> Result<()> {
        self.with_conn(|c| {
            c.query_row("SELECT 1", [], |_| Ok(()))
                .map_err(|e| RdbError::Connection(e.to_string()))
        })
        .await
    }

    async fn schema(&self) -> Result<Schema> {
        let db_name = self.db_name.clone();
        self.with_conn(move |c| schema_impl(c, &db_name)).await
    }

    async fn query(&self, q: &Query) -> Result<ResultSet> {
        let sql = match q {
            Query::Sql(s) => s.clone(),
            Query::Command(_) | Query::Mongo(_) => return Err(RdbError::UnsupportedQuery),
        };
        self.with_conn(move |c| run_sql(c, &sql)).await
    }

    async fn primary_key(&self, table: &TableRef) -> Result<Vec<String>> {
        let name = table.name.clone();
        self.with_conn(move |c| primary_key_impl(c, &name)).await
    }

    async fn count(&self, table: &TableRef) -> Result<u64> {
        let sql = format!("SELECT count(*) FROM {}", write_sql::table_name(table));
        self.with_conn(move |c| {
            let n: i64 = c
                .query_row(&sql, [], |r| r.get(0))
                .map_err(|e| RdbError::Query(e.to_string()))?;
            Ok(n as u64)
        })
        .await
    }

    async fn commit(&self, ops: &[WriteOp]) -> Result<u64> {
        if ops.is_empty() {
            return Ok(0);
        }
        let stmts: Vec<String> = ops
            .iter()
            .map(|op| match op {
                WriteOp::Update { table, pk, changes } => write_sql::update_sql(table, pk, changes),
                WriteOp::Insert { table, values } => write_sql::insert_sql(table, values),
                WriteOp::Delete { table, pk } => write_sql::delete_sql(table, pk),
            })
            .collect();
        self.with_conn(move |c| {
            c.execute_batch("BEGIN")
                .map_err(|e| RdbError::Query(e.to_string()))?;
            let mut affected = 0u64;
            for sql in &stmts {
                match c.execute(sql, []) {
                    Ok(n) => affected += n as u64,
                    Err(e) => {
                        let _ = c.execute_batch("ROLLBACK");
                        return Err(RdbError::Query(e.to_string()));
                    }
                }
            }
            c.execute_batch("COMMIT")
                .map_err(|e| RdbError::Query(e.to_string()))?;
            Ok(affected)
        })
        .await
    }

    async fn close(self) -> Result<()> {
        // Dropping the Arc drops the Connection when the last ref goes away.
        Ok(())
    }
}

/// File stem for the schema-tree label; in-memory / empty -> "memory".
fn db_name_from_path(path: &str) -> String {
    if path.is_empty() || path == ":memory:" {
        return "memory".to_string();
    }
    std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("database")
        .to_string()
}

/// Map a rusqlite value to the unified `Cell`.
fn value_to_cell(v: ValueRef) -> Cell {
    match v {
        ValueRef::Null => Cell::Null,
        ValueRef::Integer(i) => Cell::Int(i),
        ValueRef::Real(f) => Cell::Float(f),
        ValueRef::Text(t) => Cell::Text(String::from_utf8_lossy(t).into_owned()),
        ValueRef::Blob(b) => Cell::Bytes(b.to_vec()),
    }
}

/// Row-returning heuristic (leading keyword after comment/blank lines).
fn is_row_returning(sql: &str) -> bool {
    let head = sql
        .lines()
        .map(str::trim_start)
        .find(|l| !l.is_empty() && !l.starts_with("--"))
        .unwrap_or("")
        .to_ascii_lowercase();
    head.starts_with("select")
        || head.starts_with("with")
        || head.starts_with("values")
        || head.starts_with("explain")
        || head.starts_with("pragma")
}

fn run_sql(c: &Connection, sql: &str) -> Result<ResultSet> {
    if is_row_returning(sql) {
        let mut stmt = c.prepare(sql).map_err(|e| RdbError::Query(e.to_string()))?;
        let cols: Vec<Column> = stmt
            .column_names()
            .into_iter()
            .map(|name| Column {
                name: name.to_string(),
                type_name: String::new(),
            })
            .collect();
        let ncol = cols.len();
        let mut rows = stmt.query([]).map_err(|e| RdbError::Query(e.to_string()))?;
        let mut out: Vec<Row> = Vec::new();
        while let Some(r) = rows.next().map_err(|e| RdbError::Query(e.to_string()))? {
            let mut cells: Row = Vec::with_capacity(ncol);
            for i in 0..ncol {
                let v = r.get_ref(i).map_err(|e| RdbError::Query(e.to_string()))?;
                cells.push(value_to_cell(v));
            }
            out.push(cells);
        }
        Ok(ResultSet::Tabular { cols, rows: out })
    } else {
        let n = c
            .execute(sql, [])
            .map_err(|e| RdbError::Query(e.to_string()))?;
        Ok(ResultSet::Affected(n as u64))
    }
}

fn schema_impl(c: &Connection, db_name: &str) -> Result<Schema> {
    let tables: Vec<String> = {
        let mut stmt = c
            .prepare(
                "SELECT name FROM sqlite_master \
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .map_err(|e| RdbError::Schema(e.to_string()))?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|e| RdbError::Schema(e.to_string()))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|e| RdbError::Schema(e.to_string()))?
    };

    let mut containers: Vec<Container> = Vec::with_capacity(tables.len());
    for table in tables {
        let pragma = format!("PRAGMA table_info({})", write_sql::quote_ident(&table));
        let mut stmt = c
            .prepare(&pragma)
            .map_err(|e| RdbError::Schema(e.to_string()))?;
        // table_info columns: cid, name, type, notnull, dflt_value, pk
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            })
            .map_err(|e| RdbError::Schema(e.to_string()))?;
        let mut fields = Vec::new();
        for row in rows {
            let (name, type_name, notnull) = row.map_err(|e| RdbError::Schema(e.to_string()))?;
            fields.push(Field {
                name,
                type_name,
                nullable: notnull == 0,
            });
        }
        containers.push(Container {
            name: table,
            kind: ContainerKind::Table,
            fields,
        });
    }

    Ok(Schema {
        databases: vec![Database {
            name: db_name.to_string(),
            containers,
            functions: Vec::new(),
        }],
    })
}

/// Primary-key columns in key order. Empty => not editable (rowid-only tables
/// have no PK column in `SELECT *`, so inline edits can't key on them).
fn primary_key_impl(c: &Connection, table: &str) -> Result<Vec<String>> {
    let pragma = format!("PRAGMA table_info({})", write_sql::quote_ident(table));
    let mut stmt = c
        .prepare(&pragma)
        .map_err(|e| RdbError::Schema(e.to_string()))?;
    // (name, pk-ordinal); pk == 0 means not part of the primary key.
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(1)?, r.get::<_, i64>(5)?)))
        .map_err(|e| RdbError::Schema(e.to_string()))?;
    let mut pk: Vec<(String, i64)> = Vec::new();
    for row in rows {
        let (name, ord) = row.map_err(|e| RdbError::Schema(e.to_string()))?;
        if ord > 0 {
            pk.push((name, ord));
        }
    }
    pk.sort_by_key(|(_, ord)| *ord);
    Ok(pk.into_iter().map(|(name, _)| name).collect())
}
