# DBM MySQL / Redis / MongoDB Drivers Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement three `dbm_core::driver::Driver` impls — MySQL (`mysql_async`, SQL/tabular), Redis (`redis`, key-value), and MongoDB (`mongodb`, document) — each as its own workspace crate with real-engine integration tests via `testcontainers`.

**Architecture:** Each engine is an isolated crate (`crates/driver-mysql`, `crates/driver-redis`, `crates/driver-mongo`) depending only on `dbm-core` plus its native async client. Every crate implements the frozen `Driver` trait against the exact `dbm-core` types, handling only the `Query` variant it understands and returning `DbmError::UnsupportedQuery` for the rest. The UI never imports these crates directly — it depends on `dbm-core::Driver` alone, so adding an engine is purely additive.

**Tech Stack:** Rust, tokio, mysql_async, redis (tokio-comp), mongodb, async-trait, testcontainers

---

## Prerequisites

- Foundation plan (`2026-06-11-01-foundation-core.md`) is complete: `crates/core` exists, `dbm-core` exposes the frozen contract (`Driver`, `ConnConfig`, `SslMode`, `Query`, `MongoOp`, `MongoKind`, `ResultSet`, `Column`, `Row`, `Cell`, `RedisValue`, `Schema`, `Database`, `Container`, `ContainerKind`, `Field`, `DbmError`, `Result`).
- A Docker daemon is running locally (required by `testcontainers` integration tests). Integration tests are gated so a normal `cargo build` does not need Docker.

> **Version note:** Dependency versions below are known-good minimums as of 2026-06 (`mysql_async` 0.34.x, `redis` 0.27.x, `mongodb` 3.x, `testcontainers` 0.23.x, `testcontainers-modules` 0.11.x). Run `cargo update` and confirm latest compatible on first build. Where an exact API name is uncertain it is flagged "verify on first build" — the surrounding code is concrete.

---

# Part A — `crates/driver-mysql` (`dbm-driver-mysql`)

SQL engine via `mysql_async`. `Query::Sql` → `ResultSet::Tabular` for reads, `ResultSet::Affected` for writes. Schema from `information_schema`. Type mapping: integer types → `Cell::Int`, float/double/decimal → `Cell::Float`, char/varchar/text → `Cell::Text`, blob/binary → `Cell::Bytes`, NULL → `Cell::Null`, everything else → `Cell::Text` (stringified). `tinyint(1)` is read as `Int` (consistent, no special-casing).

## File Structure (Part A)

```
crates/driver-mysql/
├── Cargo.toml
├── src/
│   ├── lib.rs          # MysqlDriver + Driver impl
│   ├── convert.rs      # mysql_async::Value -> Cell, column type -> type_name
│   └── schema.rs       # information_schema -> Schema
└── tests/
    └── integration.rs  # testcontainers mysql: connect/ping/query/schema
```

---

### Task A1: Add `driver-mysql` to the workspace + crate skeleton

**Files:**
- Edit: `Cargo.toml` (workspace root)
- Create: `crates/driver-mysql/Cargo.toml`
- Create: `crates/driver-mysql/src/lib.rs`

- [ ] **Step 1: Add the member to the workspace root `Cargo.toml`**

Edit the `members` array so it reads:

```toml
members = ["crates/core", "crates/driver-mysql"]
```

- [ ] **Step 2: Create `crates/driver-mysql/Cargo.toml`**

```toml
[package]
name = "dbm-driver-mysql"
version = "0.1.0"
edition = "2021"

[dependencies]
dbm-core = { path = "../core" }
async-trait = "0.1"
mysql_async = "0.34"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }

[dev-dependencies]
testcontainers = "0.23"
testcontainers-modules = { version = "0.11", features = ["mysql"] }
```

- [ ] **Step 3: Create placeholder `crates/driver-mysql/src/lib.rs`**

```rust
//! dbm-driver-mysql: MySQL/MariaDB Driver impl via mysql_async.

mod convert;
mod schema;

pub use driver::MysqlDriver;

mod driver;
```

- [ ] **Step 4: Verify it fails to compile (modules not created yet)**

Run: `cargo build -p dbm-driver-mysql`
Expected: FAIL — `file not found for module 'convert'` / `'schema'` / `'driver'`. Confirms wiring is ahead of the files.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/driver-mysql/Cargo.toml crates/driver-mysql/src/lib.rs
git commit -m "chore(mysql): add dbm-driver-mysql crate skeleton to workspace"
```

---

### Task A2: Value + column-type conversion

**Files:**
- Create: `crates/driver-mysql/src/convert.rs`
- Test: inline `#[cfg(test)]` in `crates/driver-mysql/src/convert.rs`

- [ ] **Step 1: Write the failing test**

```rust
// at bottom of crates/driver-mysql/src/convert.rs
#[cfg(test)]
mod tests {
    use super::*;
    use dbm_core::result::Cell;
    use mysql_async::Value;

    #[test]
    fn null_maps_to_cell_null() {
        assert!(matches!(value_to_cell(&Value::NULL), Cell::Null));
    }

    #[test]
    fn int_maps_to_cell_int() {
        assert!(matches!(value_to_cell(&Value::Int(42)), Cell::Int(42)));
    }

    #[test]
    fn uint_maps_to_cell_int() {
        assert!(matches!(value_to_cell(&Value::UInt(7)), Cell::Int(7)));
    }

    #[test]
    fn float_and_double_map_to_cell_float() {
        assert!(matches!(value_to_cell(&Value::Float(1.5)), Cell::Float(_)));
        assert!(matches!(value_to_cell(&Value::Double(2.5)), Cell::Float(_)));
    }

    #[test]
    fn utf8_bytes_map_to_text_and_binary_maps_to_bytes() {
        // valid UTF-8 -> Text
        let t = value_to_cell(&Value::Bytes(b"hello".to_vec()));
        assert!(matches!(t, Cell::Text(ref s) if s == "hello"));
        // invalid UTF-8 -> Bytes
        let b = value_to_cell(&Value::Bytes(vec![0xff, 0xfe]));
        assert!(matches!(b, Cell::Bytes(_)));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dbm-driver-mysql convert`
Expected: FAIL — `value_to_cell` not defined.

- [ ] **Step 3: Write minimal implementation (above the test module)**

```rust
use dbm_core::result::Cell;
use mysql_async::consts::ColumnType;
use mysql_async::Value;

/// Map a mysql_async cell value into dbm-core's `Cell`.
///
/// Bytes are treated as text when valid UTF-8 (covers CHAR/VARCHAR/TEXT and
/// DECIMAL, which mysql returns as bytes), otherwise as raw `Bytes`
/// (covers BLOB/BINARY). Date/time values come back as `Bytes` from the
/// driver in their default form, so they render as text here.
pub fn value_to_cell(v: &Value) -> Cell {
    match v {
        Value::NULL => Cell::Null,
        Value::Int(i) => Cell::Int(*i),
        Value::UInt(u) => Cell::Int(*u as i64),
        Value::Float(f) => Cell::Float(*f as f64),
        Value::Double(d) => Cell::Float(*d),
        Value::Bytes(b) => match std::str::from_utf8(b) {
            Ok(s) => Cell::Text(s.to_string()),
            Err(_) => Cell::Bytes(b.clone()),
        },
        // Date/time variants: stringify via Value's Display.
        other => Cell::Text(other.as_sql(true).trim_matches('\'').to_string()),
    }
}

/// Human-readable type name for a result column, used to fill `Column.type_name`.
pub fn column_type_name(ct: ColumnType) -> String {
    format!("{ct:?}")
}
```

> "verify on first build": `Value::as_sql(true)` quotes string/temporal values; `trim_matches('\'')` strips the wrapping quotes. If `as_sql` is unavailable in the pinned version, replace with `format!("{other:?}")`. The numeric/null/bytes paths are stable and are what the tests cover.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p dbm-driver-mysql convert`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/driver-mysql/src/convert.rs
git commit -m "feat(mysql): map mysql_async Value to dbm-core Cell"
```

---

### Task A3: Schema query builder (pure SQL string)

**Files:**
- Create: `crates/driver-mysql/src/schema.rs`
- Test: inline `#[cfg(test)]` in `crates/driver-mysql/src/schema.rs`

We unit-test the pure pieces (the SQL strings and the row-folding logic) without a database; the network call is exercised in the integration test.

- [ ] **Step 1: Write the failing test**

```rust
// at bottom of crates/driver-mysql/src/schema.rs
#[cfg(test)]
mod tests {
    use super::*;
    use dbm_core::schema::ContainerKind;

    #[test]
    fn columns_query_targets_information_schema() {
        let sql = columns_query();
        assert!(sql.to_lowercase().contains("information_schema.columns"));
        assert!(sql.to_lowercase().contains("table_schema not in"));
    }

    #[test]
    fn fold_rows_groups_columns_under_tables_and_databases() {
        // (db, table, column, type, nullable)
        let rows = vec![
            ("app".to_string(), "users".to_string(), "id".to_string(), "int".to_string(), false),
            ("app".to_string(), "users".to_string(), "name".to_string(), "varchar".to_string(), true),
            ("app".to_string(), "orders".to_string(), "id".to_string(), "int".to_string(), false),
        ];
        let schema = fold_rows(rows);
        assert_eq!(schema.databases.len(), 1);
        let db = &schema.databases[0];
        assert_eq!(db.name, "app");
        assert_eq!(db.containers.len(), 2);
        let users = db.containers.iter().find(|c| c.name == "users").unwrap();
        assert_eq!(users.kind, ContainerKind::Table);
        assert_eq!(users.fields.len(), 2);
        assert!(users.fields.iter().any(|f| f.name == "name" && f.nullable));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dbm-driver-mysql schema`
Expected: FAIL — `columns_query` / `fold_rows` not defined.

- [ ] **Step 3: Write minimal implementation (above the test module)**

```rust
use dbm_core::schema::{Container, ContainerKind, Database, Field, Schema};

/// One row of the schema query: (database, table, column, type_name, nullable).
pub type SchemaRow = (String, String, String, String, bool);

/// SQL pulling every user column. System schemas are excluded so the tree is
/// the user's data, not server internals.
pub fn columns_query() -> String {
    "SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, DATA_TYPE, IS_NULLABLE \
     FROM INFORMATION_SCHEMA.COLUMNS \
     WHERE TABLE_SCHEMA NOT IN ('mysql','information_schema','performance_schema','sys') \
     ORDER BY TABLE_SCHEMA, TABLE_NAME, ORDINAL_POSITION"
        .to_string()
}

/// Fold flat (db, table, column, ...) rows into the nested `Schema` tree.
/// Rows are assumed ordered by db, then table (the query's ORDER BY guarantees this).
pub fn fold_rows(rows: Vec<SchemaRow>) -> Schema {
    let mut databases: Vec<Database> = Vec::new();

    for (db_name, table, col, type_name, nullable) in rows {
        let db = match databases.iter_mut().find(|d| d.name == db_name) {
            Some(d) => d,
            None => {
                databases.push(Database {
                    name: db_name.clone(),
                    containers: Vec::new(),
                });
                databases.last_mut().unwrap()
            }
        };

        let container = match db.containers.iter_mut().find(|c| c.name == table) {
            Some(c) => c,
            None => {
                db.containers.push(Container {
                    name: table.clone(),
                    kind: ContainerKind::Table,
                    fields: Vec::new(),
                });
                db.containers.last_mut().unwrap()
            }
        };

        container.fields.push(Field {
            name: col,
            type_name,
            nullable,
        });
    }

    Schema { databases }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p dbm-driver-mysql schema`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/driver-mysql/src/schema.rs
git commit -m "feat(mysql): build Schema from information_schema rows"
```

---

### Task A4: `MysqlDriver` + `Driver` impl

**Files:**
- Create: `crates/driver-mysql/src/driver.rs`

This is wiring over a live pool — there is no pure logic to unit-test beyond what A2/A3 cover. Correctness is proven by the A5 integration test. We still confirm it compiles.

- [ ] **Step 1: Write `crates/driver-mysql/src/driver.rs`**

```rust
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
                    (db, table, col, dtype, is_nullable.eq_ignore_ascii_case("YES"))
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
                    .map(|i| {
                        // `r.as_ref(i)` borrows the underlying Value without consuming.
                        match r.as_ref(i) {
                            Some(v) => value_to_cell(v),
                            None => dbm_core::result::Cell::Null,
                        }
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
```

> "verify on first build" notes: (1) `query_iter` returns a `QueryResult`; `.columns()` returns `Option<Arc<[Column]>>`, `.affected_rows()` and `.collect()`/`.drop_result()` are the standard drain APIs. (2) `Row::as_ref(usize) -> Option<&Value>` is the borrowing accessor; if the pinned version exposes `take`/`get` instead, swap to `r.get::<mysql_async::Value, usize>(i)` and match by value. (3) `SslOpts::with_danger_accept_invalid_certs` exists in 0.34; if renamed, use the builder method the version provides. None of these changes the data flow.

- [ ] **Step 2: Verify the crate compiles**

Run: `cargo build -p dbm-driver-mysql`
Expected: PASS (compiles; warnings allowed at this step).

- [ ] **Step 3: Run unit tests + clippy**

Run: `cargo test -p dbm-driver-mysql --lib && cargo clippy -p dbm-driver-mysql -- -D warnings`
Expected: lib tests (convert + schema) PASS, no clippy warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/driver-mysql/src/driver.rs
git commit -m "feat(mysql): implement Driver trait over mysql_async pool"
```

---

### Task A5: Integration test against a real MySQL container

**Files:**
- Create: `crates/driver-mysql/tests/integration.rs`

- [ ] **Step 1: Write the integration test**

```rust
//! Real-engine test: spins a MySQL container, then exercises the driver.
//! Requires a running Docker daemon. Ignored by default so plain `cargo test`
//! stays offline; run with `cargo test -p dbm-driver-mysql -- --ignored`.

use dbm_core::conn::{ConnConfig, SslMode};
use dbm_core::driver::Driver;
use dbm_core::query::Query;
use dbm_core::result::{Cell, ResultSet};
use dbm_driver_mysql::MysqlDriver;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::mysql::Mysql;

#[tokio::test]
#[ignore = "requires docker"]
async fn connect_query_schema_against_real_mysql() {
    let container = Mysql::default().start().await.unwrap();
    let host = container.get_host().await.unwrap().to_string();
    let port = container.get_host_port_ipv4(3306).await.unwrap();

    // testcontainers-modules `Mysql` default: user `root`, no password, db `test`.
    let cfg = ConnConfig {
        host,
        port,
        user: "root".into(),
        database: Some("test".into()),
        password: None,
        sslmode: SslMode::Disable,
    };

    let driver = MysqlDriver::connect(&cfg).await.unwrap();
    driver.ping().await.unwrap();

    // DDL + write -> Affected.
    driver
        .query(&Query::Sql(
            "CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(50), score DOUBLE)".into(),
        ))
        .await
        .unwrap();
    let inserted = driver
        .query(&Query::Sql(
            "INSERT INTO users (id, name, score) VALUES (1, 'alice', 9.5), (2, NULL, 1.0)".into(),
        ))
        .await
        .unwrap();
    assert!(matches!(inserted, ResultSet::Affected(2)));

    // Read -> Tabular with correct cell mapping.
    let rs = driver
        .query(&Query::Sql("SELECT id, name, score FROM users ORDER BY id".into()))
        .await
        .unwrap();
    match rs {
        ResultSet::Tabular { cols, rows } => {
            assert_eq!(cols.len(), 3);
            assert_eq!(cols[0].name, "id");
            assert_eq!(rows.len(), 2);
            assert!(matches!(rows[0][0], Cell::Int(1)));
            assert!(matches!(rows[0][1], Cell::Text(ref s) if s == "alice"));
            assert!(matches!(rows[0][2], Cell::Float(_)));
            assert!(matches!(rows[1][1], Cell::Null));
        }
        other => panic!("expected Tabular, got {other:?}"),
    }

    // Schema includes our table.
    let schema = driver.schema().await.unwrap();
    let found = schema
        .databases
        .iter()
        .flat_map(|d| &d.containers)
        .any(|c| c.name == "users");
    assert!(found, "schema should contain the users table");

    // Non-SQL query is rejected.
    assert!(driver
        .query(&Query::Command(vec!["GET".into(), "k".into()]))
        .await
        .is_err());

    driver.close().await.unwrap();
}
```

- [ ] **Step 2: Run the integration test (needs Docker)**

Run: `cargo test -p dbm-driver-mysql -- --ignored`
Expected: PASS (1 test). If Docker is not available, the test is skipped by default; document this in the PR.

- [ ] **Step 3: Commit**

```bash
git add crates/driver-mysql/tests/integration.rs
git commit -m "test(mysql): integration test against real MySQL via testcontainers"
```

---

# Part B — `crates/driver-redis` (`dbm-driver-redis`)

Key-value engine via the `redis` crate (`tokio-comp`). `Query::Command(tokens)` → build a `redis::cmd`, run it on a `MultiplexedConnection`, map `redis::Value` → `ResultSet::KeyValue`. Redis has no schema; `schema()` returns one `Database` (the numbered DB) with a single `ContainerKind::Keyspace` container summarizing key count from `DBSIZE`. `ping()` issues `PING`.

## File Structure (Part B)

```
crates/driver-redis/
├── Cargo.toml
├── src/
│   ├── lib.rs          # RedisDriver + Driver impl
│   └── convert.rs      # redis::Value -> ResultSet::KeyValue; conn URL builder
└── tests/
    └── integration.rs  # testcontainers redis: connect/ping/command/schema
```

---

### Task B1: Add `driver-redis` to the workspace + crate skeleton

**Files:**
- Edit: `Cargo.toml` (workspace root)
- Create: `crates/driver-redis/Cargo.toml`
- Create: `crates/driver-redis/src/lib.rs`

- [ ] **Step 1: Add the member to the workspace root `Cargo.toml`**

Edit the `members` array so it reads:

```toml
members = ["crates/core", "crates/driver-mysql", "crates/driver-redis"]
```

- [ ] **Step 2: Create `crates/driver-redis/Cargo.toml`**

```toml
[package]
name = "dbm-driver-redis"
version = "0.1.0"
edition = "2021"

[dependencies]
dbm-core = { path = "../core" }
async-trait = "0.1"
redis = { version = "0.27", features = ["tokio-comp"] }
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }

[dev-dependencies]
testcontainers = "0.23"
testcontainers-modules = { version = "0.11", features = ["redis"] }
```

- [ ] **Step 3: Create placeholder `crates/driver-redis/src/lib.rs`**

```rust
//! dbm-driver-redis: Redis Driver impl via the redis crate.

mod convert;

pub use driver::RedisDriver;

mod driver;
```

- [ ] **Step 4: Verify it fails to compile (modules not created yet)**

Run: `cargo build -p dbm-driver-redis`
Expected: FAIL — `file not found for module 'convert'` / `'driver'`.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/driver-redis/Cargo.toml crates/driver-redis/src/lib.rs
git commit -m "chore(redis): add dbm-driver-redis crate skeleton to workspace"
```

---

### Task B2: URL builder + `redis::Value` → `ResultSet` conversion

**Files:**
- Create: `crates/driver-redis/src/convert.rs`
- Test: inline `#[cfg(test)]` in `crates/driver-redis/src/convert.rs`

- [ ] **Step 1: Write the failing test**

```rust
// at bottom of crates/driver-redis/src/convert.rs
#[cfg(test)]
mod tests {
    use super::*;
    use dbm_core::conn::{ConnConfig, SslMode};
    use dbm_core::result::{RedisValue, ResultSet};
    use redis::Value;

    fn cfg(pw: Option<&str>, db: Option<&str>) -> ConnConfig {
        ConnConfig {
            host: "localhost".into(),
            port: 6379,
            user: "default".into(),
            database: db.map(|s| s.to_string()),
            password: pw.map(|s| s.to_string()),
            sslmode: SslMode::Disable,
        }
    }

    #[test]
    fn url_without_password_or_db() {
        assert_eq!(connection_url(&cfg(None, None)), "redis://localhost:6379");
    }

    #[test]
    fn url_with_password_and_numeric_db() {
        assert_eq!(
            connection_url(&cfg(Some("s3cr3t"), Some("2"))),
            "redis://:s3cr3t@localhost:6379/2"
        );
    }

    #[test]
    fn simple_string_becomes_single_keyvalue_entry() {
        let label = "PING".to_string();
        let rs = value_to_resultset(label.clone(), Value::SimpleString("PONG".into()));
        match rs {
            ResultSet::KeyValue(pairs) => {
                assert_eq!(pairs.len(), 1);
                assert_eq!(pairs[0].0, "PING");
                assert!(matches!(pairs[0].1, RedisValue::Str(ref s) if s == "PONG"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn integer_becomes_int_redis_value() {
        let rs = value_to_resultset("DBSIZE".into(), Value::Int(7));
        match rs {
            ResultSet::KeyValue(pairs) => {
                assert!(matches!(pairs[0].1, RedisValue::Int(7)));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn nil_becomes_nil_redis_value() {
        let rs = value_to_resultset("GET".into(), Value::Nil);
        match rs {
            ResultSet::KeyValue(pairs) => assert!(matches!(pairs[0].1, RedisValue::Nil)),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn bulk_array_becomes_single_list_entry() {
        let arr = Value::Array(vec![
            Value::BulkString(b"a".to_vec()),
            Value::BulkString(b"b".to_vec()),
        ]);
        let rs = value_to_resultset("KEYS".into(), arr);
        match rs {
            ResultSet::KeyValue(pairs) => {
                assert_eq!(pairs.len(), 1);
                assert!(matches!(pairs[0].1, RedisValue::List(ref l) if l == &vec!["a".to_string(), "b".to_string()]));
            }
            _ => panic!("wrong variant"),
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dbm-driver-redis convert`
Expected: FAIL — `connection_url` / `value_to_resultset` not defined.

- [ ] **Step 3: Write minimal implementation (above the test module)**

```rust
use dbm_core::conn::ConnConfig;
use dbm_core::result::{RedisValue, ResultSet};
use redis::Value;

/// Build a `redis://[:password@]host:port[/db]` URL from connection config.
/// Redis auth historically has no username, so only the password is included.
pub fn connection_url(cfg: &ConnConfig) -> String {
    let auth = match &cfg.password {
        Some(pw) if !pw.is_empty() => format!(":{pw}@"),
        _ => String::new(),
    };
    let db = match &cfg.database {
        Some(d) if !d.is_empty() => format!("/{d}"),
        _ => String::new(),
    };
    format!("redis://{auth}{}:{}{db}", cfg.host, cfg.port)
}

/// Render a single `redis::Value` (the reply to one command) into a
/// `KeyValue` result. `label` is the key shown for the reply (we use the
/// command name). Scalars become one entry; bulk arrays become one `List`
/// entry; nested/other shapes are flattened to their debug string.
pub fn value_to_resultset(label: String, value: Value) -> ResultSet {
    ResultSet::KeyValue(vec![(label, value_to_redis(value))])
}

fn value_to_redis(value: Value) -> RedisValue {
    match value {
        Value::Nil => RedisValue::Nil,
        Value::Int(i) => RedisValue::Int(i),
        Value::SimpleString(s) => RedisValue::Str(s),
        Value::BulkString(bytes) => RedisValue::Str(String::from_utf8_lossy(&bytes).into_owned()),
        Value::Array(items) => RedisValue::List(items.into_iter().map(scalar_to_string).collect()),
        Value::Map(pairs) => RedisValue::List(
            pairs
                .into_iter()
                .flat_map(|(k, v)| [scalar_to_string(k), scalar_to_string(v)])
                .collect(),
        ),
        Value::Okay => RedisValue::Str("OK".to_string()),
        other => RedisValue::Str(format!("{other:?}")),
    }
}

/// Flatten one element of a bulk reply to a display string.
fn scalar_to_string(value: Value) -> String {
    match value {
        Value::Nil => "(nil)".to_string(),
        Value::Int(i) => i.to_string(),
        Value::SimpleString(s) => s,
        Value::BulkString(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Value::Okay => "OK".to_string(),
        other => format!("{other:?}"),
    }
}
```

> "verify on first build": redis 0.27 `Value` variants are `Nil`, `Int`, `BulkString(Vec<u8>)`, `SimpleString(String)`, `Okay`, `Array(Vec<Value>)`, `Map(Vec<(Value,Value)>)`, plus RESP3 variants (`Double`, `Boolean`, `BigNumber`, `Set`, `Push`, etc.) caught by the `other` arm. If a variant name differs in the pinned version, adjust the match arm; the fold strategy is unchanged.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p dbm-driver-redis convert`
Expected: PASS (6 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/driver-redis/src/convert.rs
git commit -m "feat(redis): URL builder + redis::Value to ResultSet mapping"
```

---

### Task B3: `RedisDriver` + `Driver` impl

**Files:**
- Create: `crates/driver-redis/src/driver.rs`

- [ ] **Step 1: Write `crates/driver-redis/src/driver.rs`**

```rust
use async_trait::async_trait;
use redis::aio::MultiplexedConnection;
use redis::{Client, Value};
use tokio::sync::Mutex;

use dbm_core::conn::ConnConfig;
use dbm_core::driver::Driver;
use dbm_core::error::{DbmError, Result};
use dbm_core::query::Query;
use dbm_core::result::{Container, ResultSet};
use dbm_core::schema::{ContainerKind, Database, Field, Schema};

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
            Client::open(connection_url(cfg)).map_err(|e| DbmError::Connection(e.to_string()))?;
        let conn = client
            .get_multiplexed_async_connection()
            .await
            .map_err(|e| DbmError::Connection(e.to_string()))?;
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
            .map_err(|e| DbmError::Connection(e.to_string()))?;
        if reply == "PONG" {
            Ok(())
        } else {
            Err(DbmError::Connection(format!("unexpected PING reply: {reply}")))
        }
    }

    async fn schema(&self) -> Result<Schema> {
        let mut conn = self.conn.lock().await;
        let size: i64 = redis::cmd("DBSIZE")
            .query_async(&mut *conn)
            .await
            .map_err(|e| DbmError::Schema(e.to_string()))?;
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
            _ => return Err(DbmError::UnsupportedQuery),
        };
        if tokens.is_empty() {
            return Err(DbmError::Query("empty command".into()));
        }

        let mut cmd = redis::cmd(&tokens[0]);
        for arg in &tokens[1..] {
            cmd.arg(arg);
        }

        let mut conn = self.conn.lock().await;
        let value: Value = cmd
            .query_async(&mut *conn)
            .await
            .map_err(|e| DbmError::Query(e.to_string()))?;

        Ok(value_to_resultset(tokens[0].to_uppercase(), value))
    }

    async fn close(self) -> Result<()> {
        // MultiplexedConnection closes when dropped; nothing to await.
        drop(self.conn);
        Ok(())
    }
}
```

> "verify on first build": redis 0.27 provides `Client::open(url)`, `get_multiplexed_async_connection().await`, and `redis::cmd("X").arg(..).query_async(&mut conn).await`. `Container`/`Field` are re-exported from `dbm_core::result` and `dbm_core::schema` respectively per the foundation crate; if `Container` lives only under `schema`, import it from `dbm_core::schema::Container` (single-line change).

- [ ] **Step 2: Verify the crate compiles**

Run: `cargo build -p dbm-driver-redis`
Expected: PASS (compiles).

- [ ] **Step 3: Run unit tests + clippy**

Run: `cargo test -p dbm-driver-redis --lib && cargo clippy -p dbm-driver-redis -- -D warnings`
Expected: lib tests (convert) PASS, no clippy warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/driver-redis/src/driver.rs
git commit -m "feat(redis): implement Driver trait over multiplexed connection"
```

---

### Task B4: Integration test against a real Redis container

**Files:**
- Create: `crates/driver-redis/tests/integration.rs`

- [ ] **Step 1: Write the integration test**

```rust
//! Real-engine test: spins a Redis container, then exercises the driver.
//! Requires Docker. Ignored by default; run with
//! `cargo test -p dbm-driver-redis -- --ignored`.

use dbm_core::conn::{ConnConfig, SslMode};
use dbm_core::driver::Driver;
use dbm_core::query::Query;
use dbm_core::result::{RedisValue, ResultSet};
use dbm_core::schema::ContainerKind;
use dbm_driver_redis::RedisDriver;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::redis::Redis;

#[tokio::test]
#[ignore = "requires docker"]
async fn connect_command_schema_against_real_redis() {
    let container = Redis::default().start().await.unwrap();
    let host = container.get_host().await.unwrap().to_string();
    let port = container.get_host_port_ipv4(6379).await.unwrap();

    let cfg = ConnConfig {
        host,
        port,
        user: "default".into(),
        database: Some("0".into()),
        password: None,
        sslmode: SslMode::Disable,
    };

    let driver = RedisDriver::connect(&cfg).await.unwrap();
    driver.ping().await.unwrap();

    // SET then GET.
    driver
        .query(&Query::Command(vec!["SET".into(), "greeting".into(), "hello".into()]))
        .await
        .unwrap();
    let got = driver
        .query(&Query::Command(vec!["GET".into(), "greeting".into()]))
        .await
        .unwrap();
    match got {
        ResultSet::KeyValue(pairs) => {
            assert_eq!(pairs.len(), 1);
            assert!(matches!(pairs[0].1, RedisValue::Str(ref s) if s == "hello"));
        }
        other => panic!("expected KeyValue, got {other:?}"),
    }

    // KEYS -> List.
    let keys = driver
        .query(&Query::Command(vec!["KEYS".into(), "*".into()]))
        .await
        .unwrap();
    match keys {
        ResultSet::KeyValue(pairs) => {
            assert!(matches!(pairs[0].1, RedisValue::List(ref l) if l.contains(&"greeting".to_string())));
        }
        other => panic!("expected KeyValue list, got {other:?}"),
    }

    // Schema: one keyspace container.
    let schema = driver.schema().await.unwrap();
    assert_eq!(schema.databases.len(), 1);
    assert_eq!(schema.databases[0].containers[0].kind, ContainerKind::Keyspace);

    // Non-Command query rejected.
    assert!(driver
        .query(&Query::Sql("SELECT 1".into()))
        .await
        .is_err());

    driver.close().await.unwrap();
}
```

- [ ] **Step 2: Run the integration test (needs Docker)**

Run: `cargo test -p dbm-driver-redis -- --ignored`
Expected: PASS (1 test).

- [ ] **Step 3: Commit**

```bash
git add crates/driver-redis/tests/integration.rs
git commit -m "test(redis): integration test against real Redis via testcontainers"
```

---

# Part C — `crates/driver-mongo` (`dbm-driver-mongo`)

Document engine via the `mongodb` crate (3.x). `Query::Mongo(op)` matches `MongoKind`: `Find` → `collection.find(filter)` → collect docs → `ResultSet::Documents`; `Insert` → `insert_one` → `ResultSet::Affected(1)`; `Aggregate` → `collection.aggregate(pipeline)` → `Documents`. BSON ⇄ JSON via `mongodb::bson::Bson`. `ping()` runs the `{ping: 1}` admin command. `schema()` lists databases and collections (`ContainerKind::Collection`, empty fields — Mongo is schemaless).

## File Structure (Part C)

```
crates/driver-mongo/
├── Cargo.toml
├── src/
│   ├── lib.rs          # MongoDriver + Driver impl
│   └── convert.rs      # serde_json::Value <-> bson::Document
└── tests/
    └── integration.rs  # testcontainers mongo: connect/ping/find/insert/aggregate/schema
```

---

### Task C1: Add `driver-mongo` to the workspace + crate skeleton

**Files:**
- Edit: `Cargo.toml` (workspace root)
- Create: `crates/driver-mongo/Cargo.toml`
- Create: `crates/driver-mongo/src/lib.rs`

- [ ] **Step 1: Add the member to the workspace root `Cargo.toml`**

Edit the `members` array so it reads:

```toml
members = [
    "crates/core",
    "crates/driver-mysql",
    "crates/driver-redis",
    "crates/driver-mongo",
]
```

- [ ] **Step 2: Create `crates/driver-mongo/Cargo.toml`**

```toml
[package]
name = "dbm-driver-mongo"
version = "0.1.0"
edition = "2021"

[dependencies]
dbm-core = { path = "../core" }
async-trait = "0.1"
mongodb = "3"
serde_json = "1"
futures = "0.3"
tokio = { version = "1", features = ["rt-multi-thread", "macros"] }

[dev-dependencies]
testcontainers = "0.23"
testcontainers-modules = { version = "0.11", features = ["mongo"] }
```

- [ ] **Step 3: Create placeholder `crates/driver-mongo/src/lib.rs`**

```rust
//! dbm-driver-mongo: MongoDB Driver impl via the mongodb crate.

mod convert;

pub use driver::MongoDriver;

mod driver;
```

- [ ] **Step 4: Verify it fails to compile (modules not created yet)**

Run: `cargo build -p dbm-driver-mongo`
Expected: FAIL — `file not found for module 'convert'` / `'driver'`.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/driver-mongo/Cargo.toml crates/driver-mongo/src/lib.rs
git commit -m "chore(mongo): add dbm-driver-mongo crate skeleton to workspace"
```

---

### Task C2: BSON ⇄ JSON conversion

**Files:**
- Create: `crates/driver-mongo/src/convert.rs`
- Test: inline `#[cfg(test)]` in `crates/driver-mongo/src/convert.rs`

- [ ] **Step 1: Write the failing test**

```rust
// at bottom of crates/driver-mongo/src/convert.rs
#[cfg(test)]
mod tests {
    use super::*;
    use mongodb::bson::doc;

    #[test]
    fn json_object_becomes_bson_document() {
        let json = serde_json::json!({ "name": "alice", "age": 30 });
        let doc = json_to_document(&json).unwrap();
        assert_eq!(doc.get_str("name").unwrap(), "alice");
        assert_eq!(doc.get_i64("age").or_else(|_| doc.get_i32("age").map(|v| v as i64)).unwrap(), 30);
    }

    #[test]
    fn empty_json_object_becomes_empty_document() {
        let json = serde_json::json!({});
        let doc = json_to_document(&json).unwrap();
        assert_eq!(doc.len(), 0);
    }

    #[test]
    fn non_object_json_is_rejected() {
        let json = serde_json::json!([1, 2, 3]);
        assert!(json_to_document(&json).is_err());
    }

    #[test]
    fn bson_document_roundtrips_to_json() {
        let d = doc! { "name": "bob", "score": 1.5 };
        let json = document_to_json(d);
        assert_eq!(json["name"], serde_json::json!("bob"));
        assert_eq!(json["score"], serde_json::json!(1.5));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dbm-driver-mongo convert`
Expected: FAIL — `json_to_document` / `document_to_json` not defined.

- [ ] **Step 3: Write minimal implementation (above the test module)**

```rust
use mongodb::bson::{Bson, Document};

use dbm_core::error::{DbmError, Result};

/// Convert a JSON object into a BSON `Document`. Filters, insert payloads, and
/// aggregation stages all arrive as JSON objects, so a non-object is a usage
/// error and is rejected.
pub fn json_to_document(value: &serde_json::Value) -> Result<Document> {
    let bson: Bson = value.clone().try_into().map_err(|e: mongodb::bson::ser::Error| {
        DbmError::Query(format!("invalid BSON: {e}"))
    })?;
    match bson {
        Bson::Document(d) => Ok(d),
        other => Err(DbmError::Query(format!(
            "expected a JSON object, got {:?}",
            other.element_type()
        ))),
    }
}

/// Convert a BSON `Document` back into a `serde_json::Value`. Uses BSON's
/// relaxed extended-JSON form so ordinary numbers/strings stay plain and only
/// exotic types (ObjectId, dates) carry `$`-prefixed wrappers.
pub fn document_to_json(doc: Document) -> serde_json::Value {
    Bson::Document(doc).into_relaxed_extjson()
}
```

> "verify on first build": mongodb 3.x re-exports `bson` as `mongodb::bson`. `serde_json::Value: TryInto<Bson>` is provided by `bson`'s serde integration (`Bson::try_from(value)`); if the blanket impl is not in scope, use `mongodb::bson::to_bson(value)` instead. `Bson::into_relaxed_extjson()` returns `serde_json::Value`; if named `into_relaxed_extended_json`, adjust. Behavior (object → doc, doc → json) is unchanged.

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p dbm-driver-mongo convert`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/driver-mongo/src/convert.rs
git commit -m "feat(mongo): BSON <-> JSON conversion helpers"
```

---

### Task C3: `MongoDriver` + `Driver` impl

**Files:**
- Create: `crates/driver-mongo/src/driver.rs`

- [ ] **Step 1: Write `crates/driver-mongo/src/driver.rs`**

```rust
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
```

> "verify on first build": mongodb 3.x uses the builder-future style — `coll.find(filter).await`, `coll.aggregate(pipeline).await`, `coll.insert_one(doc).await`, `db.run_command(doc).await`, `client.list_database_names().await`, `db.list_collection_names().await` (no `_options` suffix, no separate options arg in the basic form). `aggregate` on a `Collection<Document>` yields a `Cursor<Document>`. If the pinned 3.x patch requires `.session(None)` or differs on `list_*` return types, adjust the single call; the data flow is unchanged.

- [ ] **Step 2: Verify the crate compiles**

Run: `cargo build -p dbm-driver-mongo`
Expected: PASS (compiles).

- [ ] **Step 3: Run unit tests + clippy**

Run: `cargo test -p dbm-driver-mongo --lib && cargo clippy -p dbm-driver-mongo -- -D warnings`
Expected: lib tests (convert) PASS, no clippy warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/driver-mongo/src/driver.rs
git commit -m "feat(mongo): implement Driver trait over mongodb::Client"
```

---

### Task C4: Integration test against a real MongoDB container

**Files:**
- Create: `crates/driver-mongo/tests/integration.rs`

- [ ] **Step 1: Write the integration test**

```rust
//! Real-engine test: spins a MongoDB container, then exercises the driver.
//! Requires Docker. Ignored by default; run with
//! `cargo test -p dbm-driver-mongo -- --ignored`.

use dbm_core::conn::{ConnConfig, SslMode};
use dbm_core::driver::Driver;
use dbm_core::query::{MongoKind, MongoOp, Query};
use dbm_core::result::ResultSet;
use dbm_driver_mongo::MongoDriver;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::mongo::Mongo;

#[tokio::test]
#[ignore = "requires docker"]
async fn connect_insert_find_aggregate_schema_against_real_mongo() {
    let container = Mongo::default().start().await.unwrap();
    let host = container.get_host().await.unwrap().to_string();
    let port = container.get_host_port_ipv4(27017).await.unwrap();

    let cfg = ConnConfig {
        host,
        port,
        user: String::new(),
        database: Some("appdb".into()),
        password: None,
        sslmode: SslMode::Disable,
    };

    let driver = MongoDriver::connect(&cfg).await.unwrap();
    driver.ping().await.unwrap();

    // Insert two docs.
    for (name, age) in [("alice", 30), ("bob", 17)] {
        let inserted = driver
            .query(&Query::Mongo(MongoOp {
                collection: "users".into(),
                kind: MongoKind::Insert(serde_json::json!({ "name": name, "age": age })),
            }))
            .await
            .unwrap();
        assert!(matches!(inserted, ResultSet::Affected(1)));
    }

    // Find adults.
    let found = driver
        .query(&Query::Mongo(MongoOp {
            collection: "users".into(),
            kind: MongoKind::Find(serde_json::json!({ "age": { "$gte": 18 } })),
        }))
        .await
        .unwrap();
    match found {
        ResultSet::Documents(docs) => {
            assert_eq!(docs.len(), 1);
            assert_eq!(docs[0]["name"], serde_json::json!("alice"));
        }
        other => panic!("expected Documents, got {other:?}"),
    }

    // Aggregate: count by a group.
    let agg = driver
        .query(&Query::Mongo(MongoOp {
            collection: "users".into(),
            kind: MongoKind::Aggregate(vec![
                serde_json::json!({ "$group": { "_id": null, "total": { "$sum": 1 } } }),
            ]),
        }))
        .await
        .unwrap();
    match agg {
        ResultSet::Documents(docs) => {
            assert_eq!(docs.len(), 1);
            assert_eq!(docs[0]["total"], serde_json::json!(2));
        }
        other => panic!("expected Documents, got {other:?}"),
    }

    // Schema lists our collection.
    let schema = driver.schema().await.unwrap();
    let has_users = schema
        .databases
        .iter()
        .flat_map(|d| &d.containers)
        .any(|c| c.name == "users");
    assert!(has_users, "schema should contain the users collection");

    // Non-Mongo query rejected.
    assert!(driver.query(&Query::Sql("SELECT 1".into())).await.is_err());

    driver.close().await.unwrap();
}
```

- [ ] **Step 2: Run the integration test (needs Docker)**

Run: `cargo test -p dbm-driver-mongo -- --ignored`
Expected: PASS (1 test).

- [ ] **Step 3: Commit**

```bash
git add crates/driver-mongo/tests/integration.rs
git commit -m "test(mongo): integration test against real MongoDB via testcontainers"
```

---

### Task D1: Workspace-wide verification

- [ ] **Step 1: Build and unit-test the whole workspace (offline)**

Run: `cargo test --workspace --lib && cargo clippy --workspace -- -D warnings`
Expected: all crates compile, all lib/unit tests PASS, no clippy warnings.

- [ ] **Step 2: Run all driver integration tests (needs Docker)**

Run: `cargo test --workspace -- --ignored`
Expected: 3 integration tests PASS (mysql, redis, mongo), each spinning its own container.

- [ ] **Step 3: Commit any formatting fixes**

```bash
cargo fmt --all
git add -A
git commit -m "style: cargo fmt across driver crates" || echo "nothing to format"
```

---

## Self-Review

**Spec coverage**
- MySQL (Part A): `connect` via `OptsBuilder`; `Query::Sql` → `Tabular` (reads) / `Affected` (writes); non-Sql → `UnsupportedQuery`; type mapping ints→`Int`, float/double/decimal→`Float`, char/varchar/text→`Text`, blob/binary→`Bytes`, NULL→`Null`; `schema()` from `information_schema` with `ContainerKind::Table`; `ping()` via `SELECT 1`; integration test via testcontainers mysql. ✔
- Redis (Part B): `connect` builds `redis://[:password@]host:port/db`, `Client` + `MultiplexedConnection`; `Query::Command(tokens)` → `redis::cmd` → `redis::Value` → `KeyValue` (scalar = single entry keyed by command name; bulk = single `List` entry); non-Command → `UnsupportedQuery`; `ping()` via `PING`; `schema()` = one `Database` + one `ContainerKind::Keyspace` container summarizing `DBSIZE`; integration test via testcontainers redis. ✔
- MongoDB (Part C): `connect` via `Client::with_uri_str(mongodb://[user:pass@]host:port)`; `Query::Mongo(op)` → `Find`→`Documents`, `Insert`→`Affected(1)`, `Aggregate`→`Documents`; BSON⇄JSON via `bson::Bson`; non-Mongo → `UnsupportedQuery`; `ping()` via `{ping:1}` admin command; `schema()` from `list_database_names` + `list_collection_names` with `ContainerKind::Collection` and empty fields (schemaless, noted); integration test via testcontainers mongo. ✔
- Each engine is its own crate, its own integration test, its own container per test (separate `tests/integration.rs`, each calling `start()` once). ✔

**Type consistency vs frozen contract**
- All three `impl Driver` use the exact method signatures: `connect(&ConnConfig) -> Result<Self>`, `ping(&self) -> Result<()>`, `schema(&self) -> Result<Schema>`, `query(&self, &Query) -> Result<ResultSet>`, `close(self) -> Result<()>`.
- Only frozen variants are produced: `ResultSet::{Tabular, Documents, KeyValue, Affected}`, `Cell::{Null, Int, Float, Text, Bool, Bytes}`, `RedisValue::{Str, Int, List, Nil}`, `ContainerKind::{Table, Collection, Keyspace}`. `Cell::Bool` is not emitted by MySQL (tinyint(1)→`Int`), which is allowed and intentionally consistent.
- Errors use only `DbmError::{Connection, Query, UnsupportedQuery, Schema}`; `UnsupportedQuery` is returned by every driver for the variants it does not own.
- No driver renames or extends the contract; they depend on `dbm-core` by path.

**Placeholder scan**
- No `TODO`, no `unimplemented!()`, no `// similar to above`, no stubbed function bodies. Every code step is complete and compilable. The only deferred items are explicit "verify on first build" notes that name the exact API to confirm and give a concrete fallback — these are not placeholders, the surrounding code is whole.

**DRY / YAGNI / TDD**
- Pure logic (Cell mapping, schema folding, Redis value mapping, BSON⇄JSON) is isolated into `convert.rs`/`schema.rs` and unit-tested offline; live wiring is proven by one focused integration test per engine. No connection pooling beyond what each client offers by default (decision deferred per spec's open question, kept minimal).

## Execution Handoff

Implement Part A → Part B → Part C → Task D1, one task per fresh subagent, reviewing between tasks. Integration tests (`--ignored`) require a local Docker daemon; lib/unit tests and `cargo build` run fully offline. This plan assumes `2026-06-11-01-foundation-core.md` is already merged.
