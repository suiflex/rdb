# DBM Foundation & Core Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Scaffold the Cargo workspace and build the `core` crate — the paradigm-agnostic `Driver` trait plus the result/schema/query model that every driver and the UI depend on.

**Architecture:** A Cargo workspace (monorepo). `core` defines a `Driver` async trait and a set of enums (`Query`, `ResultSet`, `Schema`, `Cell`) that unify SQL, document, and key-value engines behind one interface. `core` has no network and no concrete driver deps — it is pure types + pure logic, so it is fully unit-testable.

**Tech Stack:** Rust (edition 2021), `tokio`, `async-trait`, `serde` / `serde_json`, `thiserror`.

> **Type contract:** The public types defined in this plan (`DbmError`, `Result`, `ConnConfig`, `SslMode`, `Query`, `MongoOp`, `MongoKind`, `ResultSet`, `Column`, `Row`, `Cell`, `RedisValue`, `Schema`, `Database`, `Container`, `ContainerKind`, `Field`, `Driver`) are the FROZEN contract. Driver plans, connstore plan, and UI plan all reference these exact names and signatures. Do not rename without updating dependent plans.

---

## File Structure

```
dbm/
├── Cargo.toml                 # workspace root (members + shared profile)
├── rust-toolchain.toml        # pin toolchain
├── .gitignore
└── crates/
    └── core/
        ├── Cargo.toml
        └── src/
            ├── lib.rs         # re-exports
            ├── error.rs       # DbmError, Result
            ├── conn.rs        # ConnConfig, SslMode
            ├── query.rs       # Query, MongoOp, MongoKind
            ├── result.rs      # ResultSet, Column, Row, Cell, RedisValue
            ├── schema.rs      # Schema, Database, Container, ContainerKind, Field
            └── driver.rs      # Driver trait
```

---

### Task 1: Workspace scaffold

**Files:**
- Create: `Cargo.toml`
- Create: `rust-toolchain.toml`
- Create: `.gitignore`

- [ ] **Step 1: Create workspace root `Cargo.toml`**

```toml
[workspace]
resolver = "2"
members = ["crates/core"]

# Size-optimized release profile (target: binary < 15 MB).
[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
strip = true
```

- [ ] **Step 2: Create `rust-toolchain.toml`**

```toml
[toolchain]
channel = "stable"
components = ["rustfmt", "clippy"]
```

- [ ] **Step 3: Create `.gitignore`**

```gitignore
/target
**/*.rs.bk
Cargo.lock.bak
.DS_Store
```

- [ ] **Step 4: Verify workspace parses**

Run: `cargo metadata --no-deps --format-version 1 > /dev/null && echo OK`
Expected: `OK` (no member yet exists, so this may warn — that is fine until Task 2 creates the crate; if it errors, proceed to Task 2 then re-run).

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml rust-toolchain.toml .gitignore
git commit -m "chore: scaffold cargo workspace with size-optimized release profile"
```

---

### Task 2: Create `core` crate skeleton

**Files:**
- Create: `crates/core/Cargo.toml`
- Create: `crates/core/src/lib.rs`

- [ ] **Step 1: Create `crates/core/Cargo.toml`**

```toml
[package]
name = "dbm-core"
version = "0.1.0"
edition = "2021"

[dependencies]
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt"] }
```

> Versions are minimums known good as of 2026-06; run `cargo update` and confirm latest compatible on first build.

- [ ] **Step 2: Create placeholder `crates/core/src/lib.rs`**

```rust
//! dbm-core: paradigm-agnostic Driver trait + unified result model.

pub mod conn;
pub mod driver;
pub mod error;
pub mod query;
pub mod result;
pub mod schema;
```

- [ ] **Step 3: Verify it fails to compile (modules not created yet)**

Run: `cargo build -p dbm-core`
Expected: FAIL — `file not found for module 'conn'` etc. This confirms `lib.rs` wiring is correct ahead of the module files.

- [ ] **Step 4: Commit**

```bash
git add crates/core/Cargo.toml crates/core/src/lib.rs
git commit -m "chore: add dbm-core crate skeleton"
```

---

### Task 3: Error type

**Files:**
- Create: `crates/core/src/error.rs`
- Test: inline `#[cfg(test)]` in `crates/core/src/error.rs`

- [ ] **Step 1: Write the failing test**

```rust
// at bottom of crates/core/src/error.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_query_has_stable_message() {
        let e = DbmError::UnsupportedQuery;
        assert_eq!(e.to_string(), "unsupported query for this driver");
    }

    #[test]
    fn connection_error_includes_detail() {
        let e = DbmError::Connection("refused".into());
        assert_eq!(e.to_string(), "connection failed: refused");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dbm-core error`
Expected: FAIL — `DbmError` not defined.

- [ ] **Step 3: Write minimal implementation (above the test module)**

```rust
use thiserror::Error;

/// All fallible operations in dbm return this error.
#[derive(Error, Debug)]
pub enum DbmError {
    #[error("connection failed: {0}")]
    Connection(String),
    #[error("query failed: {0}")]
    Query(String),
    #[error("unsupported query for this driver")]
    UnsupportedQuery,
    #[error("schema error: {0}")]
    Schema(String),
}

pub type Result<T> = std::result::Result<T, DbmError>;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p dbm-core error`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/error.rs
git commit -m "feat(core): add DbmError and Result alias"
```

---

### Task 4: Connection config

**Files:**
- Create: `crates/core/src/conn.rs`
- Test: inline `#[cfg(test)]` in `crates/core/src/conn.rs`

- [ ] **Step 1: Write the failing test**

```rust
// at bottom of crates/core/src/conn.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssl_mode_default_is_prefer() {
        assert_eq!(SslMode::default(), SslMode::Prefer);
    }

    #[test]
    fn conn_config_roundtrips_json_without_password() {
        let cfg = ConnConfig {
            host: "localhost".into(),
            port: 5432,
            user: "postgres".into(),
            database: Some("app".into()),
            password: None,
            sslmode: SslMode::Require,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("\"host\":\"localhost\""));
        let back: ConnConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.port, 5432);
        assert_eq!(back.sslmode, SslMode::Require);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dbm-core conn`
Expected: FAIL — `ConnConfig` / `SslMode` not defined.

- [ ] **Step 3: Write minimal implementation (above the test module)**

```rust
use serde::{Deserialize, Serialize};

/// TLS negotiation mode for a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SslMode {
    Disable,
    Prefer,
    Require,
}

impl Default for SslMode {
    fn default() -> Self {
        SslMode::Prefer
    }
}

/// Everything needed to open a connection. `password` is injected at connect
/// time from the keychain — it is never persisted as part of saved config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub database: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub password: Option<String>,
    #[serde(default)]
    pub sslmode: SslMode,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p dbm-core conn`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/conn.rs
git commit -m "feat(core): add ConnConfig and SslMode"
```

---

### Task 5: Query model

**Files:**
- Create: `crates/core/src/query.rs`
- Test: inline `#[cfg(test)]` in `crates/core/src/query.rs`

- [ ] **Step 1: Write the failing test**

```rust
// at bottom of crates/core/src/query.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_variant_carries_text() {
        let q = Query::Sql("SELECT 1".into());
        match q {
            Query::Sql(s) => assert_eq!(s, "SELECT 1"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn mongo_find_op_constructs() {
        let op = MongoOp {
            collection: "users".into(),
            kind: MongoKind::Find(serde_json::json!({ "age": { "$gt": 18 } })),
        };
        assert_eq!(op.collection, "users");
        matches!(op.kind, MongoKind::Find(_));
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dbm-core query`
Expected: FAIL — `Query` / `MongoOp` / `MongoKind` not defined.

- [ ] **Step 3: Write minimal implementation (above the test module)**

```rust
use serde_json::Value as Json;

/// A request to a driver. An enum (not a string) so non-SQL engines are
/// first-class and SQL assumptions never leak into the abstraction. Each
/// driver handles the variant it understands and returns
/// `DbmError::UnsupportedQuery` for the rest.
#[derive(Debug, Clone)]
pub enum Query {
    /// SQL text — Postgres, MySQL.
    Sql(String),
    /// Raw command tokens — Redis, e.g. `["GET", "key"]`.
    Command(Vec<String>),
    /// Structured Mongo operation.
    Mongo(MongoOp),
}

#[derive(Debug, Clone)]
pub struct MongoOp {
    pub collection: String,
    pub kind: MongoKind,
}

#[derive(Debug, Clone)]
pub enum MongoKind {
    Find(Json),
    Insert(Json),
    Aggregate(Vec<Json>),
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p dbm-core query`
Expected: PASS (2 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/query.rs
git commit -m "feat(core): add Query/MongoOp/MongoKind model"
```

---

### Task 6: Result model + Cell rendering

**Files:**
- Create: `crates/core/src/result.rs`
- Test: inline `#[cfg(test)]` in `crates/core/src/result.rs`

This task includes real logic to test: `Cell::render()` produces the display string the grid shows.

- [ ] **Step 1: Write the failing test**

```rust
// at bottom of crates/core/src/result.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn null_renders_as_null_marker() {
        assert_eq!(Cell::Null.render(), "NULL");
    }

    #[test]
    fn text_renders_verbatim() {
        assert_eq!(Cell::Text("hello".into()).render(), "hello");
    }

    #[test]
    fn int_and_bool_render() {
        assert_eq!(Cell::Int(42).render(), "42");
        assert_eq!(Cell::Bool(true).render(), "true");
    }

    #[test]
    fn bytes_render_as_size_summary_not_raw() {
        assert_eq!(Cell::Bytes(vec![0u8; 3]).render(), "(3 bytes)");
    }

    #[test]
    fn tabular_result_holds_cols_and_rows() {
        let rs = ResultSet::Tabular {
            cols: vec![Column { name: "id".into(), type_name: "int4".into() }],
            rows: vec![vec![Cell::Int(1)]],
        };
        match rs {
            ResultSet::Tabular { cols, rows } => {
                assert_eq!(cols.len(), 1);
                assert_eq!(rows[0][0].render(), "1");
            }
            _ => panic!("wrong variant"),
        }
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dbm-core result`
Expected: FAIL — `Cell` / `ResultSet` / `Column` not defined.

- [ ] **Step 3: Write minimal implementation (above the test module)**

```rust
use serde_json::Value as Json;

/// What a driver returns. The UI renders by variant: grid for `Tabular`,
/// tree/JSON for `Documents`, key-list for `KeyValue`, toast for `Affected`.
#[derive(Debug, Clone)]
pub enum ResultSet {
    Tabular { cols: Vec<Column>, rows: Vec<Row> },
    Documents(Vec<Json>),
    KeyValue(Vec<(String, RedisValue)>),
    Affected(u64),
}

#[derive(Debug, Clone)]
pub struct Column {
    pub name: String,
    pub type_name: String,
}

pub type Row = Vec<Cell>;

/// One grid cell. Engine-native types are normalized into this set.
#[derive(Debug, Clone)]
pub enum Cell {
    Null,
    Int(i64),
    Float(f64),
    Text(String),
    Bool(bool),
    Bytes(Vec<u8>),
}

impl Cell {
    /// Display string for the result grid.
    pub fn render(&self) -> String {
        match self {
            Cell::Null => "NULL".to_string(),
            Cell::Int(i) => i.to_string(),
            Cell::Float(f) => f.to_string(),
            Cell::Text(s) => s.clone(),
            Cell::Bool(b) => b.to_string(),
            Cell::Bytes(b) => format!("({} bytes)", b.len()),
        }
    }
}

/// Redis values are their own shape, kept separate from SQL `Cell`.
#[derive(Debug, Clone)]
pub enum RedisValue {
    Str(String),
    Int(i64),
    List(Vec<String>),
    Nil,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p dbm-core result`
Expected: PASS (5 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/result.rs
git commit -m "feat(core): add ResultSet model with Cell rendering"
```

---

### Task 7: Schema model

**Files:**
- Create: `crates/core/src/schema.rs`
- Test: inline `#[cfg(test)]` in `crates/core/src/schema.rs`

- [ ] **Step 1: Write the failing test**

```rust
// at bottom of crates/core/src/schema.rs
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_unifies_table_and_collection_under_container() {
        let schema = Schema {
            databases: vec![Database {
                name: "app".into(),
                containers: vec![
                    Container {
                        name: "users".into(),
                        kind: ContainerKind::Table,
                        fields: vec![Field {
                            name: "id".into(),
                            type_name: "int4".into(),
                            nullable: false,
                        }],
                    },
                    Container {
                        name: "events".into(),
                        kind: ContainerKind::Collection,
                        fields: vec![],
                    },
                ],
            }],
        };
        assert_eq!(schema.databases[0].containers.len(), 2);
        assert_eq!(schema.databases[0].containers[0].kind, ContainerKind::Table);
        assert!(!schema.databases[0].containers[0].fields[0].nullable);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dbm-core schema`
Expected: FAIL — `Schema` / `Database` / `Container` not defined.

- [ ] **Step 3: Write minimal implementation (above the test module)**

```rust
/// Unified schema tree. Every engine maps to `databases → containers → fields`
/// even when native names differ (table / collection / keyspace), so the UI
/// tree renders one way regardless of engine.
#[derive(Debug, Clone)]
pub struct Schema {
    pub databases: Vec<Database>,
}

#[derive(Debug, Clone)]
pub struct Database {
    pub name: String,
    pub containers: Vec<Container>,
}

#[derive(Debug, Clone)]
pub struct Container {
    pub name: String,
    pub kind: ContainerKind,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerKind {
    Table,
    Collection,
    Keyspace,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub type_name: String,
    pub nullable: bool,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p dbm-core schema`
Expected: PASS (1 test).

- [ ] **Step 5: Commit**

```bash
git add crates/core/src/schema.rs
git commit -m "feat(core): add unified Schema model"
```

---

### Task 8: Driver trait

**Files:**
- Create: `crates/core/src/driver.rs`
- Test: inline `#[cfg(test)]` in `crates/core/src/driver.rs` (a fake in-memory driver proves the trait is object-safe-enough and usable)

- [ ] **Step 1: Write the failing test**

```rust
// at bottom of crates/core/src/driver.rs
#[cfg(test)]
mod tests {
    use super::*;
    use crate::conn::ConnConfig;
    use crate::query::Query;
    use crate::result::ResultSet;

    struct FakeDriver;

    #[async_trait::async_trait]
    impl Driver for FakeDriver {
        async fn connect(_cfg: &ConnConfig) -> crate::error::Result<Self> {
            Ok(FakeDriver)
        }
        async fn ping(&self) -> crate::error::Result<()> {
            Ok(())
        }
        async fn schema(&self) -> crate::error::Result<crate::schema::Schema> {
            Ok(crate::schema::Schema { databases: vec![] })
        }
        async fn query(&self, q: &Query) -> crate::error::Result<ResultSet> {
            match q {
                Query::Sql(_) => Ok(ResultSet::Affected(0)),
                _ => Err(crate::error::DbmError::UnsupportedQuery),
            }
        }
        async fn close(self) -> crate::error::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn fake_driver_satisfies_trait_and_rejects_unsupported() {
        let cfg = ConnConfig {
            host: "x".into(),
            port: 0,
            user: "x".into(),
            database: None,
            password: None,
            sslmode: Default::default(),
        };
        let d = FakeDriver::connect(&cfg).await.unwrap();
        d.ping().await.unwrap();
        assert!(matches!(
            d.query(&Query::Sql("SELECT 1".into())).await.unwrap(),
            ResultSet::Affected(0)
        ));
        assert!(d.query(&Query::Command(vec!["GET".into()])).await.is_err());
        d.close().await.unwrap();
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p dbm-core driver`
Expected: FAIL — `Driver` trait not defined.

- [ ] **Step 3: Write minimal implementation (above the test module)**

```rust
use async_trait::async_trait;

use crate::conn::ConnConfig;
use crate::error::Result;
use crate::query::Query;
use crate::result::ResultSet;
use crate::schema::Schema;

/// The single interface the UI depends on. The UI NEVER imports a concrete
/// driver crate — adding an engine is a new crate that implements this trait.
#[async_trait]
pub trait Driver: Send + Sync {
    /// Open a connection. `cfg.password` is expected to be populated by the
    /// caller (from the keychain) before this is called.
    async fn connect(cfg: &ConnConfig) -> Result<Self>
    where
        Self: Sized;

    /// Cheap liveness check.
    async fn ping(&self) -> Result<()>;

    /// Full schema tree (databases → containers → fields).
    async fn schema(&self) -> Result<Schema>;

    /// Run a query. Drivers handle the `Query` variant(s) they support and
    /// return `DbmError::UnsupportedQuery` for the rest.
    async fn query(&self, q: &Query) -> Result<ResultSet>;

    /// Close the connection, consuming the driver.
    async fn close(self) -> Result<()>
    where
        Self: Sized;
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p dbm-core driver`
Expected: PASS (1 test).

- [ ] **Step 5: Run the whole crate + clippy**

Run: `cargo test -p dbm-core && cargo clippy -p dbm-core -- -D warnings`
Expected: all tests PASS, no clippy warnings.

- [ ] **Step 6: Commit**

```bash
git add crates/core/src/driver.rs
git commit -m "feat(core): add Driver trait (the UI-facing contract)"
```

---

## Self-Review

- **Spec coverage:** Covers the "Driver Trait + Result Model" and "Architecture" sections of the design spec. Driver impls, connstore, and UI are deliberately out of scope — separate plans (see below).
- **Type consistency:** All type names match the frozen contract header. `Driver::connect` and `Driver::close` both carry `where Self: Sized` so the trait stays usable; UI will hold concrete driver types or a boxed enum (decided in UI plan).
- **No placeholders:** every step has real code and a real command.

## Dependent Plans (written next, reference this contract)

- `2026-06-11-02-driver-postgres.md`
- `2026-06-11-03-drivers-mysql-redis-mongo.md`
- `2026-06-11-04-connstore.md`
- `2026-06-11-05-app-ui-slint.md`

## Workspace Assembly (cross-plan — READ BEFORE EXECUTING ANY DEPENDENT PLAN)

Each dependent plan contains a "modify root `Cargo.toml`" step that shows the
`members` array. Those snippets are written as if each plan ran alone, so they
each list only `crates/core` + that plan's own crate. **When executing, APPEND
your crate to the existing `members` array — never replace it**, or earlier
crates get dropped from the workspace.

After all plans are executed, the root `Cargo.toml` `members` MUST be exactly:

```toml
members = [
    "crates/core",
    "crates/driver-postgres",
    "crates/driver-mysql",
    "crates/driver-redis",
    "crates/driver-mongo",
    "crates/connstore",
    "app",
]
```

Recommended execution order (vertical slice to a runnable app first, remaining
drivers after): foundation (this) → connstore → driver-postgres → app-ui →
drivers-mysql-redis-mongo. The app plan only wires `driver-postgres` initially;
the other three slot into the `AnyDriver` dispatch when their plan completes.

## Execution Handoff

After the dependent plans exist, implement in this order: foundation → postgres driver → connstore → UI (vertical slice to a working app) → remaining 3 drivers. Recommended: subagent-driven-development, fresh subagent per task, review between tasks.
