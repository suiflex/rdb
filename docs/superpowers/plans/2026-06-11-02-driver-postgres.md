# DBM PostgreSQL Driver Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the `dbm-driver-postgres` crate — a concrete `dbm_core::driver::Driver` backed by `tokio-postgres` that connects, pings, runs SQL queries, introspects schema, and closes cleanly, verified by integration tests against a real Postgres container.

**Architecture:** `PostgresDriver` holds a `tokio_postgres::Client` plus the `JoinHandle` of the spawned connection-driver task. `connect` builds a libpq-style key=value conn string from `ConnConfig`, opens the connection with `NoTls` (MVP), and spawns the connection future. `query` accepts only `Query::Sql`, mapping pg column types to `dbm_core::result::Cell` and returning `Tabular` for selects or `Affected` for writes; all non-SQL variants return `DbmError::UnsupportedQuery`. The UI never imports this crate — it depends only on the `dbm-core` trait.

**Tech Stack:** Rust, tokio, tokio-postgres, async-trait, testcontainers

---

> **Frozen type contract:** This crate implements `dbm_core::driver::Driver` against the EXACT types defined in `2026-06-11-01-foundation-core.md` (`DbmError`, `Result`, `ConnConfig`, `SslMode`, `Query`, `ResultSet`, `Column`, `Row`, `Cell`, `Schema`, `Database`, `Container`, `ContainerKind`, `Field`). It does NOT redefine them — it `use`s them from `dbm_core`. Prerequisite: the `core` crate plan is fully implemented and committed.

> **Verified API baseline (2026-06-11):** `tokio-postgres` 0.7.17, `testcontainers` 0.27.x, `testcontainers-modules` 0.15.x (`postgres` feature). Confirmed from docs.rs: `tokio_postgres::connect(conn_str, NoTls) -> Result<(Client, Connection)>`; spawn `connection` on its own task; `Client::query(sql, &[]) -> Result<Vec<Row>>`; `Client::execute(sql, &[]) -> Result<u64>` (rows affected); `Row::columns() -> &[Column]`; `Column::type_() -> &Type`; `Column::name() -> &str`; `Row::try_get::<I, T>(idx) -> Result<T, Error>`; `Type` from `tokio_postgres::types`; testcontainers async pattern `Image.start().await -> ContainerAsync<_>` via `use testcontainers::runners::AsyncRunner`. **Verify on first build:** the exact `testcontainers_modules::postgres::Postgres` host/port accessor names (`get_host().await` and `get_host_port_ipv4(5432).await`) and that the module's default user/password/db are `postgres`/`postgres`/`postgres`. If a name differs, adjust the test helper only — driver code is unaffected.

---

## File Structure

```
dbm/
├── Cargo.toml                          # workspace root — ADD member (modify)
└── crates/
    └── driver-postgres/
        ├── Cargo.toml
        ├── src/
        │   ├── lib.rs                  # re-export PostgresDriver
        │   ├── conn_string.rs          # ConnConfig -> libpq conn string (pure, unit-tested)
        │   ├── type_map.rs             # pg Type + Row -> Cell (pure-ish, unit-tested)
        │   └── driver.rs               # PostgresDriver + impl Driver
        └── tests/
            └── integration.rs          # testcontainers: real Postgres
```

---

### Task 1: Add crate to workspace + Cargo.toml

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Create: `crates/driver-postgres/Cargo.toml`
- Create: `crates/driver-postgres/src/lib.rs`

- [ ] **Step 1: Add the member to the workspace root `Cargo.toml`**

Edit the `members` array (currently `["crates/core"]`) to:

```toml
members = ["crates/core", "crates/driver-postgres"]
```

- [ ] **Step 2: Create `crates/driver-postgres/Cargo.toml`**

```toml
[package]
name = "dbm-driver-postgres"
version = "0.1.0"
edition = "2021"

[dependencies]
dbm-core = { path = "../core" }
async-trait = "0.1"
tokio = { version = "1", features = ["full"] }
tokio-postgres = "0.7"

[dev-dependencies]
testcontainers = "0.27"
testcontainers-modules = { version = "0.15", features = ["postgres"] }
tokio = { version = "1", features = ["full", "macros", "rt-multi-thread"] }
```

> Versions are minimums known good as of 2026-06; run `cargo update` and confirm latest compatible on first build. `tokio-postgres` 0.7.x is the verified current line.

- [ ] **Step 3: Create placeholder `crates/driver-postgres/src/lib.rs`**

```rust
//! dbm-driver-postgres: a `dbm_core::driver::Driver` backed by tokio-postgres.

mod conn_string;
mod driver;
mod type_map;

pub use driver::PostgresDriver;
```

- [ ] **Step 4: Verify it fails to compile (modules not created yet)**

Run: `cargo build -p dbm-driver-postgres`
Expected: FAIL — `file not found for module 'conn_string'` (and `driver`, `type_map`). This confirms `lib.rs` wiring is correct ahead of the module files.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml crates/driver-postgres/Cargo.toml crates/driver-postgres/src/lib.rs
git commit -m "chore(driver-postgres): add crate skeleton to workspace"
```

---

### Task 2: Connection string builder

Pure function: `ConnConfig` → libpq key=value conn string. Unit-testable, no network.

**Files:**
- Create: `crates/driver-postgres/src/conn_string.rs`
- Test: inline `#[cfg(test)]` in `crates/driver-postgres/src/conn_string.rs`

- [ ] **Step 1: Write the failing test**

```rust
// at bottom of crates/driver-postgres/src/conn_string.rs
#[cfg(test)]
mod tests {
    use super::*;
    use dbm_core::conn::{ConnConfig, SslMode};

    fn base() -> ConnConfig {
        ConnConfig {
            host: "localhost".into(),
            port: 5432,
            user: "postgres".into(),
            database: Some("app".into()),
            password: Some("secret".into()),
            sslmode: SslMode::Prefer,
        }
    }

    #[test]
    fn builds_full_conn_string() {
        let s = build_conn_string(&base());
        assert_eq!(
            s,
            "host=localhost port=5432 user=postgres dbname=app password=secret sslmode=prefer"
        );
    }

    #[test]
    fn omits_dbname_when_absent() {
        let mut cfg = base();
        cfg.database = None;
        let s = build_conn_string(&cfg);
        assert!(!s.contains("dbname="));
        assert!(s.contains("host=localhost"));
    }

    #[test]
    fn omits_password_when_absent() {
        let mut cfg = base();
        cfg.password = None;
        let s = build_conn_string(&cfg);
        assert!(!s.contains("password="));
    }

    #[test]
    fn maps_sslmode_to_libpq_token() {
        let mut cfg = base();
        cfg.sslmode = SslMode::Disable;
        assert!(build_conn_string(&cfg).contains("sslmode=disable"));
        cfg.sslmode = SslMode::Require;
        assert!(build_conn_string(&cfg).contains("sslmode=require"));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dbm-driver-postgres conn_string`
Expected: FAIL — `build_conn_string` not defined.

- [ ] **Step 3: Write minimal implementation (above the test module)**

```rust
use dbm_core::conn::{ConnConfig, SslMode};

/// Map our `SslMode` to a libpq `sslmode` token.
///
/// MVP TLS limitation: `connect()` always uses `NoTls`, so `Require` is
/// advisory only — it is written into the conn string but a `NoTls`
/// connection cannot actually negotiate TLS. Real `Require` enforcement needs
/// `tokio-postgres-rustls` and is a documented follow-up (see driver.rs).
fn sslmode_token(mode: SslMode) -> &'static str {
    match mode {
        SslMode::Disable => "disable",
        SslMode::Prefer => "prefer",
        SslMode::Require => "require",
    }
}

/// Build a libpq-style `key=value` connection string from `ConnConfig`.
/// `dbname` and `password` are only emitted when present.
pub fn build_conn_string(cfg: &ConnConfig) -> String {
    let mut parts = vec![
        format!("host={}", cfg.host),
        format!("port={}", cfg.port),
        format!("user={}", cfg.user),
    ];
    if let Some(db) = &cfg.database {
        parts.push(format!("dbname={db}"));
    }
    if let Some(pw) = &cfg.password {
        parts.push(format!("password={pw}"));
    }
    parts.push(format!("sslmode={}", sslmode_token(cfg.sslmode)));
    parts.join(" ")
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p dbm-driver-postgres conn_string`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/driver-postgres/src/conn_string.rs
git commit -m "feat(driver-postgres): ConnConfig -> libpq conn string builder"
```

---

### Task 3: Type mapping (pg Row → Cell)

Pure-logic mapping of a `tokio_postgres::Row` cell into a `dbm_core::result::Cell`, keyed on the pg `Type`. Tested directly against `Type` constants (no network: `Type` is just a value).

**Files:**
- Create: `crates/driver-postgres/src/type_map.rs`
- Test: inline `#[cfg(test)]` in `crates/driver-postgres/src/type_map.rs`

- [ ] **Step 1: Write the failing test**

We test the type→category classifier in isolation (the per-row extraction needs a live `Row`, which the integration tests cover). The classifier is the part with real branching logic.

```rust
// at bottom of crates/driver-postgres/src/type_map.rs
#[cfg(test)]
mod tests {
    use super::*;
    use tokio_postgres::types::Type;

    #[test]
    fn integer_types_classify_as_int() {
        assert_eq!(classify(&Type::INT2), CellKind::Int);
        assert_eq!(classify(&Type::INT4), CellKind::Int);
        assert_eq!(classify(&Type::INT8), CellKind::Int);
    }

    #[test]
    fn float_types_classify_as_float() {
        assert_eq!(classify(&Type::FLOAT4), CellKind::Float);
        assert_eq!(classify(&Type::FLOAT8), CellKind::Float);
    }

    #[test]
    fn bool_text_bytea_classify() {
        assert_eq!(classify(&Type::BOOL), CellKind::Bool);
        assert_eq!(classify(&Type::TEXT), CellKind::Text);
        assert_eq!(classify(&Type::VARCHAR), CellKind::Text);
        assert_eq!(classify(&Type::BYTEA), CellKind::Bytes);
    }

    #[test]
    fn unknown_type_falls_back_to_text() {
        // UUID has no dedicated branch -> string fallback bucket.
        assert_eq!(classify(&Type::UUID), CellKind::Text);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dbm-driver-postgres type_map`
Expected: FAIL — `classify` / `CellKind` not defined.

- [ ] **Step 3: Write minimal implementation (above the test module)**

```rust
use dbm_core::result::Cell;
use tokio_postgres::types::Type;
use tokio_postgres::Row;

/// Which `Cell` variant a pg column type maps to. Pragmatic, not exhaustive:
/// unknown types fall back to a string read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellKind {
    Int,
    Float,
    Bool,
    Text,
    Bytes,
}

/// Classify a pg `Type` into a `CellKind`. Unknown types -> `Text` (string
/// fallback), which is always safe to display.
pub fn classify(ty: &Type) -> CellKind {
    match *ty {
        Type::INT2 | Type::INT4 | Type::INT8 => CellKind::Int,
        Type::FLOAT4 | Type::FLOAT8 => CellKind::Float,
        Type::BOOL => CellKind::Bool,
        Type::BYTEA => CellKind::Bytes,
        Type::TEXT | Type::VARCHAR | Type::BPCHAR | Type::NAME => CellKind::Text,
        _ => CellKind::Text,
    }
}

/// Extract column `idx` of `row` into a `Cell`, honoring NULLs.
///
/// Reads via `try_get::<Option<T>>`: `Ok(None)` -> `Cell::Null`. If the typed
/// read fails (type we did not special-case, or a decode mismatch), fall back
/// to reading the value as `Option<String>`; if even that fails the value is
/// represented as `Cell::Null` so one odd column never aborts a whole result.
pub fn extract_cell(row: &Row, idx: usize) -> Cell {
    let ty = row.columns()[idx].type_();
    match classify(ty) {
        CellKind::Int => match row.try_get::<_, Option<i64>>(idx) {
            Ok(Some(v)) => Cell::Int(v),
            Ok(None) => Cell::Null,
            Err(_) => match row.try_get::<_, Option<i32>>(idx) {
                Ok(Some(v)) => Cell::Int(v as i64),
                Ok(None) => Cell::Null,
                Err(_) => string_fallback(row, idx),
            },
        },
        CellKind::Float => match row.try_get::<_, Option<f64>>(idx) {
            Ok(Some(v)) => Cell::Float(v),
            Ok(None) => Cell::Null,
            Err(_) => match row.try_get::<_, Option<f32>>(idx) {
                Ok(Some(v)) => Cell::Float(v as f64),
                Ok(None) => Cell::Null,
                Err(_) => string_fallback(row, idx),
            },
        },
        CellKind::Bool => match row.try_get::<_, Option<bool>>(idx) {
            Ok(Some(v)) => Cell::Bool(v),
            Ok(None) => Cell::Null,
            Err(_) => string_fallback(row, idx),
        },
        CellKind::Bytes => match row.try_get::<_, Option<Vec<u8>>>(idx) {
            Ok(Some(v)) => Cell::Bytes(v),
            Ok(None) => Cell::Null,
            Err(_) => string_fallback(row, idx),
        },
        CellKind::Text => string_fallback(row, idx),
    }
}

/// Read column `idx` as an optional string; failure or NULL -> `Cell::Null`,
/// otherwise `Cell::Text`.
fn string_fallback(row: &Row, idx: usize) -> Cell {
    match row.try_get::<_, Option<String>>(idx) {
        Ok(Some(s)) => Cell::Text(s),
        _ => Cell::Null,
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p dbm-driver-postgres type_map`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/driver-postgres/src/type_map.rs
git commit -m "feat(driver-postgres): pg type -> Cell mapping with string fallback"
```

---

### Task 4: PostgresDriver struct + connect/ping/close

Build the driver shell and the lifecycle methods. `query`/`schema` are stubbed with `unimplemented!()` only long enough to compile this task's test; they get real bodies (and tests) in Tasks 5–6. Connect/ping/close are exercised by the first integration test here.

**Files:**
- Create: `crates/driver-postgres/src/driver.rs`
- Create: `crates/driver-postgres/tests/integration.rs`

- [ ] **Step 1: Write the failing test (integration: connect + ping + close)**

```rust
// crates/driver-postgres/tests/integration.rs
//
// Integration tests run a REAL Postgres in Docker via testcontainers.
// They require a running Docker daemon. They are NOT #[ignore]-d: if Docker
// is present (CI, dev with Docker Desktop) they run; without Docker they fail
// fast at container start, which is the intended signal.

use dbm_core::conn::{ConnConfig, SslMode};
use dbm_core::driver::Driver;
use dbm_driver_postgres::PostgresDriver;
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;

/// Start a fresh Postgres container and return it plus a `ConnConfig` pointed
/// at it. The container is held by the caller; dropping it stops the container.
/// (Verify-on-build: confirm `get_host()` / `get_host_port_ipv4(5432)` accessor
/// names and the postgres/postgres/postgres defaults against the installed
/// testcontainers-modules version.)
async fn start_pg() -> (ContainerAsync<Postgres>, ConnConfig) {
    let container = Postgres::default()
        .start()
        .await
        .expect("start postgres container (is Docker running?)");
    let host = container.get_host().await.expect("host").to_string();
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("mapped port");
    let cfg = ConnConfig {
        host,
        port,
        user: "postgres".into(),
        database: Some("postgres".into()),
        password: Some("postgres".into()),
        sslmode: SslMode::Disable,
    };
    (container, cfg)
}

#[tokio::test]
async fn connect_ping_close() {
    let (_container, cfg) = start_pg().await;
    let driver = PostgresDriver::connect(&cfg).await.expect("connect");
    driver.ping().await.expect("ping");
    driver.close().await.expect("close");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dbm-driver-postgres --test integration connect_ping_close`
Expected: FAIL — `PostgresDriver` not defined / `driver` module empty (compile error).

- [ ] **Step 3: Write minimal implementation**

Create `crates/driver-postgres/src/driver.rs`:

```rust
use async_trait::async_trait;
use tokio::task::JoinHandle;
use tokio_postgres::{Client, NoTls};

use dbm_core::conn::ConnConfig;
use dbm_core::driver::Driver;
use dbm_core::error::{DbmError, Result};
use dbm_core::query::Query;
use dbm_core::result::ResultSet;
use dbm_core::schema::Schema;

use crate::conn_string::build_conn_string;

/// A `Driver` backed by tokio-postgres over a single connection.
///
/// TLS limitation (MVP): connections always use `NoTls`. `SslMode::Require`
/// is accepted and written to the conn string but NOT enforced at the
/// transport layer — real enforcement needs `tokio-postgres-rustls` and is a
/// documented follow-up. `Disable`/`Prefer` behave correctly against a plain
/// server.
pub struct PostgresDriver {
    client: Client,
    /// Handle to the spawned connection-driver task; aborted on `close`.
    conn_task: JoinHandle<()>,
}

#[async_trait]
impl Driver for PostgresDriver {
    async fn connect(cfg: &ConnConfig) -> Result<Self> {
        let conn_str = build_conn_string(cfg);
        let (client, connection) = tokio_postgres::connect(&conn_str, NoTls)
            .await
            .map_err(|e| DbmError::Connection(e.to_string()))?;
        // The connection object drives the protocol; it must be polled on its
        // own task for the client to make progress.
        let conn_task = tokio::spawn(async move {
            let _ = connection.await;
        });
        Ok(PostgresDriver { client, conn_task })
    }

    async fn ping(&self) -> Result<()> {
        self.client
            .query("SELECT 1", &[])
            .await
            .map_err(|e| DbmError::Connection(e.to_string()))?;
        Ok(())
    }

    async fn schema(&self) -> Result<Schema> {
        crate::driver::schema_impl(&self.client).await
    }

    async fn query(&self, q: &Query) -> Result<ResultSet> {
        crate::driver::query_impl(&self.client, q).await
    }

    async fn close(self) -> Result<()> {
        // Drop the client first so the connection future can complete, then
        // abort the connection task to release the spawned future promptly.
        let PostgresDriver { client, conn_task } = self;
        drop(client);
        conn_task.abort();
        Ok(())
    }
}

// Stubbed here; real bodies land in Tasks 5 (query) and 6 (schema).
async fn query_impl(_client: &Client, _q: &Query) -> Result<ResultSet> {
    unimplemented!("query_impl: implemented in Task 5")
}

async fn schema_impl(_client: &Client) -> Result<Schema> {
    unimplemented!("schema_impl: implemented in Task 6")
}
```

- [ ] **Step 4: Run test to verify it passes (Docker required)**

Run: `cargo test -p dbm-driver-postgres --test integration connect_ping_close`
Expected: PASS (1 test). If it errors at `start postgres container`, start Docker and re-run.

- [ ] **Step 5: Commit**

```bash
git add crates/driver-postgres/src/driver.rs crates/driver-postgres/tests/integration.rs
git commit -m "feat(driver-postgres): PostgresDriver connect/ping/close"
```

---

### Task 5: `query` — SQL execution and ResultSet mapping

Implement `query_impl`: `Query::Sql` runs against the client; a result with columns maps to `Tabular`, a result with no columns (writes/DDL) maps to `Affected`. Non-SQL variants return `UnsupportedQuery`.

**Files:**
- Modify: `crates/driver-postgres/src/driver.rs`
- Modify: `crates/driver-postgres/tests/integration.rs`

- [ ] **Step 1: Write the failing tests (append to `tests/integration.rs`)**

```rust
#[tokio::test]
async fn select_returns_tabular_rows() {
    use dbm_core::query::Query;
    use dbm_core::result::{Cell, ResultSet};

    let (_container, cfg) = start_pg().await;
    let driver = PostgresDriver::connect(&cfg).await.expect("connect");

    // DDL + write return Affected; select returns Tabular.
    driver
        .query(&Query::Sql(
            "CREATE TABLE t (id INT4 PRIMARY KEY, name TEXT, ok BOOL)".into(),
        ))
        .await
        .expect("create table");

    let affected = driver
        .query(&Query::Sql(
            "INSERT INTO t (id, name, ok) VALUES (1, 'alice', true), (2, NULL, false)".into(),
        ))
        .await
        .expect("insert");
    assert!(matches!(affected, ResultSet::Affected(2)));

    let rs = driver
        .query(&Query::Sql("SELECT id, name, ok FROM t ORDER BY id".into()))
        .await
        .expect("select");

    match rs {
        ResultSet::Tabular { cols, rows } => {
            assert_eq!(cols.len(), 3);
            assert_eq!(cols[0].name, "id");
            assert_eq!(rows.len(), 2);
            assert!(matches!(rows[0][0], Cell::Int(1)));
            assert!(matches!(&rows[0][1], Cell::Text(s) if s == "alice"));
            assert!(matches!(rows[0][2], Cell::Bool(true)));
            assert!(matches!(rows[1][1], Cell::Null)); // NULL name
        }
        other => panic!("expected Tabular, got {other:?}"),
    }

    driver.close().await.expect("close");
}

#[tokio::test]
async fn non_sql_queries_are_unsupported() {
    use dbm_core::error::DbmError;
    use dbm_core::query::{MongoKind, MongoOp, Query};

    let (_container, cfg) = start_pg().await;
    let driver = PostgresDriver::connect(&cfg).await.expect("connect");

    let cmd = driver
        .query(&Query::Command(vec!["GET".into(), "k".into()]))
        .await;
    assert!(matches!(cmd, Err(DbmError::UnsupportedQuery)));

    let mongo = driver
        .query(&Query::Mongo(MongoOp {
            collection: "c".into(),
            kind: MongoKind::Find(serde_json::json!({})),
        }))
        .await;
    assert!(matches!(mongo, Err(DbmError::UnsupportedQuery)));

    driver.close().await.expect("close");
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p dbm-driver-postgres --test integration select_returns_tabular_rows non_sql_queries_are_unsupported`
Expected: FAIL — `query_impl` panics with `unimplemented!`.

- [ ] **Step 3: Replace the `query_impl` stub in `src/driver.rs`**

Replace the stub:

```rust
async fn query_impl(_client: &Client, _q: &Query) -> Result<ResultSet> {
    unimplemented!("query_impl: implemented in Task 5")
}
```

with:

```rust
async fn query_impl(client: &Client, q: &Query) -> Result<ResultSet> {
    let sql = match q {
        Query::Sql(s) => s,
        Query::Command(_) | Query::Mongo(_) => return Err(DbmError::UnsupportedQuery),
    };

    let rows = client
        .query(sql, &[])
        .await
        .map_err(|e| DbmError::Query(e.to_string()))?;

    // No rows AND no column metadata => a write/DDL statement. Report the
    // affected-row count via a separate `execute` call so writes still return
    // `Affected`. (A SELECT that matches zero rows still has column metadata
    // and correctly returns an empty `Tabular`.)
    if rows.is_empty() {
        let affected = client
            .execute(sql, &[])
            .await
            .map_err(|e| DbmError::Query(e.to_string()))?;
        return Ok(ResultSet::Affected(affected));
    }

    let columns = rows[0].columns();
    let cols = columns
        .iter()
        .map(|c| Column {
            name: c.name().to_string(),
            type_name: c.type_().name().to_string(),
        })
        .collect::<Vec<_>>();

    let mut out_rows: Vec<Row> = Vec::with_capacity(rows.len());
    for row in &rows {
        let mut cells: Row = Vec::with_capacity(cols.len());
        for idx in 0..cols.len() {
            cells.push(crate::type_map::extract_cell(row, idx));
        }
        out_rows.push(cells);
    }

    Ok(ResultSet::Tabular { cols, rows: out_rows })
}
```

Add the imports these new types need to the top of `src/driver.rs`:

```rust
use dbm_core::result::{Column, Row};
```

> Note on writes: a write statement returns an empty `Vec<Row>`, so it is run twice (once via `query`, once via `execute` to get the count). For an `INSERT ... VALUES (...)` this re-inserts — to avoid double-execution we instead branch on the statement up front. **Adjust Step 3** so the body first inspects the SQL: if it begins (case-insensitive, trimmed) with `select` or `with` or `show` or `values` it is treated as a row-returning query and `execute` is never called as a fallback; otherwise it is a write and ONLY `execute` is called. Use this corrected body instead:

```rust
async fn query_impl(client: &Client, q: &Query) -> Result<ResultSet> {
    let sql = match q {
        Query::Sql(s) => s,
        Query::Command(_) | Query::Mongo(_) => return Err(DbmError::UnsupportedQuery),
    };

    if is_row_returning(sql) {
        let rows = client
            .query(sql, &[])
            .await
            .map_err(|e| DbmError::Query(e.to_string()))?;
        let cols = column_meta(&rows);
        let mut out_rows: Vec<Row> = Vec::with_capacity(rows.len());
        for row in &rows {
            let mut cells: Row = Vec::with_capacity(cols.len());
            for idx in 0..cols.len() {
                cells.push(crate::type_map::extract_cell(row, idx));
            }
            out_rows.push(cells);
        }
        Ok(ResultSet::Tabular { cols, rows: out_rows })
    } else {
        let affected = client
            .execute(sql, &[])
            .await
            .map_err(|e| DbmError::Query(e.to_string()))?;
        Ok(ResultSet::Affected(affected))
    }
}

/// Heuristic: does this statement return rows? Covers the common row-returning
/// leading keywords. Anything else (INSERT/UPDATE/DELETE/CREATE/DROP/...) is
/// treated as a write and routed to `execute` for an affected-row count.
fn is_row_returning(sql: &str) -> bool {
    let head = sql.trim_start().to_ascii_lowercase();
    head.starts_with("select")
        || head.starts_with("with")
        || head.starts_with("show")
        || head.starts_with("values")
        || head.starts_with("table")
        || head.starts_with("explain")
}

/// Build `Column` metadata from the first row (empty result -> no columns).
fn column_meta(rows: &[tokio_postgres::Row]) -> Vec<Column> {
    match rows.first() {
        Some(first) => first
            .columns()
            .iter()
            .map(|c| Column {
                name: c.name().to_string(),
                type_name: c.type_().name().to_string(),
            })
            .collect(),
        None => Vec::new(),
    }
}
```

> Limitation, documented intentionally: an empty-result `SELECT` yields a `Tabular` with **no column metadata** (we only have column info when at least one row is present, since `Row::columns()` is per-row). This is acceptable for MVP — the grid renders an empty result. A follow-up can use `Client::prepare` to read `Statement::columns()` for column names on zero-row selects. Keep it simple now.

- [ ] **Step 4: Run tests to verify they pass (Docker required)**

Run: `cargo test -p dbm-driver-postgres --test integration select_returns_tabular_rows non_sql_queries_are_unsupported`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/driver-postgres/src/driver.rs crates/driver-postgres/tests/integration.rs
git commit -m "feat(driver-postgres): query() SQL execution -> Tabular/Affected, reject non-SQL"
```

---

### Task 6: `schema` — information_schema introspection

Implement `schema_impl`: query `information_schema.columns` for user tables, group by table into `Container { kind: Table }` with `Field`s carrying `nullable` from `is_nullable`.

**Files:**
- Modify: `crates/driver-postgres/src/driver.rs`
- Modify: `crates/driver-postgres/tests/integration.rs`

- [ ] **Step 1: Write the failing test (append to `tests/integration.rs`)**

```rust
#[tokio::test]
async fn schema_lists_created_table_and_fields() {
    use dbm_core::query::Query;
    use dbm_core::schema::ContainerKind;

    let (_container, cfg) = start_pg().await;
    let driver = PostgresDriver::connect(&cfg).await.expect("connect");

    driver
        .query(&Query::Sql(
            "CREATE TABLE widget (id INT4 PRIMARY KEY, label TEXT NOT NULL, note TEXT)".into(),
        ))
        .await
        .expect("create table");

    let schema = driver.schema().await.expect("schema");

    // One logical database ("postgres"); find our table within it.
    let db = schema
        .databases
        .iter()
        .find(|d| d.name == "postgres")
        .expect("postgres database present");
    let widget = db
        .containers
        .iter()
        .find(|c| c.name == "widget")
        .expect("widget table present");

    assert_eq!(widget.kind, ContainerKind::Table);

    let id = widget.fields.iter().find(|f| f.name == "id").expect("id field");
    assert!(!id.nullable);
    let label = widget
        .fields
        .iter()
        .find(|f| f.name == "label")
        .expect("label field");
    assert!(!label.nullable);
    let note = widget
        .fields
        .iter()
        .find(|f| f.name == "note")
        .expect("note field");
    assert!(note.nullable);

    driver.close().await.expect("close");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dbm-driver-postgres --test integration schema_lists_created_table_and_fields`
Expected: FAIL — `schema_impl` panics with `unimplemented!`.

- [ ] **Step 3: Replace the `schema_impl` stub in `src/driver.rs`**

Replace the stub:

```rust
async fn schema_impl(_client: &Client) -> Result<Schema> {
    unimplemented!("schema_impl: implemented in Task 6")
}
```

with:

```rust
async fn schema_impl(client: &Client) -> Result<Schema> {
    // The current database name groups everything under one logical Database.
    let db_row = client
        .query_one("SELECT current_database()", &[])
        .await
        .map_err(|e| DbmError::Schema(e.to_string()))?;
    let db_name: String = db_row
        .try_get(0)
        .map_err(|e| DbmError::Schema(e.to_string()))?;

    // User tables + columns from information_schema, public schema only.
    // Ordered so columns of the same table are contiguous for grouping.
    let rows = client
        .query(
            "SELECT c.table_name, c.column_name, c.data_type, c.is_nullable \
             FROM information_schema.columns c \
             JOIN information_schema.tables t \
               ON t.table_schema = c.table_schema AND t.table_name = c.table_name \
             WHERE c.table_schema = 'public' AND t.table_type = 'BASE TABLE' \
             ORDER BY c.table_name, c.ordinal_position",
            &[],
        )
        .await
        .map_err(|e| DbmError::Schema(e.to_string()))?;

    let mut containers: Vec<Container> = Vec::new();
    for row in &rows {
        let table: String = row.get(0);
        let column: String = row.get(1);
        let data_type: String = row.get(2);
        let is_nullable: String = row.get(3); // 'YES' | 'NO'
        let field = Field {
            name: column,
            type_name: data_type,
            nullable: is_nullable.eq_ignore_ascii_case("YES"),
        };
        match containers.last_mut() {
            Some(last) if last.name == table => last.fields.push(field),
            _ => containers.push(Container {
                name: table,
                kind: ContainerKind::Table,
                fields: vec![field],
            }),
        }
    }

    Ok(Schema {
        databases: vec![Database {
            name: db_name,
            containers,
        }],
    })
}
```

Add the schema-type imports to the top of `src/driver.rs`:

```rust
use dbm_core::schema::{Container, ContainerKind, Database, Field};
```

(Replace the existing `use dbm_core::schema::Schema;` line with one that also brings in these — i.e. `use dbm_core::schema::{Container, ContainerKind, Database, Field, Schema};`.)

- [ ] **Step 4: Run test to verify it passes (Docker required)**

Run: `cargo test -p dbm-driver-postgres --test integration schema_lists_created_table_and_fields`
Expected: PASS (1 test).

- [ ] **Step 5: Commit**

```bash
git add crates/driver-postgres/src/driver.rs crates/driver-postgres/tests/integration.rs
git commit -m "feat(driver-postgres): schema() via information_schema introspection"
```

---

### Task 7: Full crate verification

**Files:** none (verification only)

- [ ] **Step 1: Run the whole crate (unit + integration) + clippy**

Run: `cargo test -p dbm-driver-postgres && cargo clippy -p dbm-driver-postgres -- -D warnings`
Expected: all tests PASS (4 unit conn_string/type_map + 4 integration), no clippy warnings. Docker must be running for the integration tests.

- [ ] **Step 2: Confirm the workspace still builds end-to-end**

Run: `cargo build --workspace`
Expected: clean build of `dbm-core` and `dbm-driver-postgres`.

- [ ] **Step 3: Commit any clippy fixups (if needed)**

```bash
git add -A
git commit -m "chore(driver-postgres): clippy clean + workspace build green"
```

(If no fixups were needed, skip this commit.)

---

## Self-Review

- **Spec coverage:** Implements the "driver-postgres / impl Driver via tokio-postgres" crate from the design spec's Architecture section, and the spec's Testing rule ("driver crates: integration tests against real engines using testcontainers, each driver spins its own container, no mocks"). Each integration test starts its own `Postgres::default()` container. Connection-pool strategy is single-connection (the spec lists this as a per-driver open question; single conn is the simplest correct MVP choice and is documented on the struct).
- **Type consistency vs frozen contract:** The crate `use`s and never redefines `DbmError`, `Result`, `ConnConfig`, `SslMode`, `Query`/`MongoOp`/`MongoKind`, `ResultSet`/`Column`/`Row`/`Cell`, `Schema`/`Database`/`Container`/`ContainerKind`/`Field`. `impl Driver for PostgresDriver` matches every signature in the frozen trait, including `where Self: Sized` on `connect`/`close` (inherited from the trait). Type mapping honors the contract: int2/int4/int8→`Int`, float4/float8→`Float`, bool→`Bool`, text/varchar/bpchar/name→`Text`, bytea→`Bytes`, NULL→`Null`, unknown→`Text` string fallback. Writes→`Affected(u64)`; non-SQL→`UnsupportedQuery`.
- **Placeholder scan:** No `TODO`s and no vague "add error handling" notes. Every step contains complete, real code. The only `unimplemented!()` calls are deliberate, short-lived stubs in Task 4 that are explicitly replaced with full bodies in Tasks 5 and 6 (TDD red→green) — they never survive to the final crate. Two documented MVP limitations are intentional design, not gaps: (1) `NoTls` only — `SslMode::Require` is not transport-enforced (follow-up: `tokio-postgres-rustls`); (2) zero-row `SELECT` returns `Tabular` with empty column metadata (follow-up: `Client::prepare` + `Statement::columns()`).
- **Verify-on-build flags:** testcontainers-modules `Postgres` accessor names (`get_host`/`get_host_port_ipv4`) and default credentials, plus latest compatible crate versions, are flagged to confirm on first build; none affect driver logic, only the test helper.
