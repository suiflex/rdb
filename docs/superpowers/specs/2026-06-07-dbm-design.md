# DBM — Lightweight Cross-Platform Database Manager

**Date:** 2026-06-07
**Status:** Design approved, pending spec review

## Goal

A native, lightweight database management GUI in the spirit of TablePlus —
fast, small binary, low memory — but built from a single Rust codebase that
targets macOS, Windows, and Linux (TablePlus uses 4 separate native codebases).

End-goal vision: a connection picker supporting ~20 engines (Postgres, MySQL,
Redis, Mongo, ClickHouse, BigQuery, SQLite, Oracle, Cassandra, etc.).

MVP ships **4 engines**: PostgreSQL, MySQL/MariaDB, Redis, MongoDB — chosen so
the driver abstraction is forced to handle both SQL (tabular) and NoSQL
(document, key-value) paradigms from day one.

## Why Rust

- C-level performance, no GC, no bundled runtime → light binary, low RAM
  (the actual source of TablePlus-style lightness is native rendering, not a
  specific language).
- Memory safety without a garbage collector.
- Mature async drivers exist for all 4 MVP engines.
- One codebase cross-compiles to all 3 desktop platforms.

## UI: Slint

Native, GPU-rendered UI via Slint. No webview, no Chromium → smaller and
lighter than Electron/Tauri, more native feel. Declarative `.slint` markup
keeps UI logic thin. Single codebase across platforms.

## Architecture

Cargo workspace (monorepo):

```
dbm/
├── Cargo.toml              # workspace root
├── crates/
│   ├── core/               # Driver trait + result model (paradigm-agnostic)
│   ├── driver-postgres/    # impl Driver via tokio-postgres
│   ├── driver-mysql/       # impl Driver via mysql_async
│   ├── driver-redis/       # impl Driver via redis
│   ├── driver-mongo/       # impl Driver via mongodb
│   └── connstore/          # saved connections + secrets (OS keychain)
└── app/                    # Slint UI binary
    └── ui/*.slint          # connection picker, query editor, result grid
```

**Core rule:** the UI depends only on `core::Driver`. It never imports a
concrete driver crate. Adding a new engine = a new `driver-*` crate that
implements the trait; the UI is untouched. This is what makes the 20-engine
vision cheap.

**Data flow:** UI → `Driver::query()` → `ResultSet` enum → UI renders by
variant (grid for Tabular, tree/JSON for Documents, key-list for KeyValue).

## Driver Trait + Result Model

```rust
#[async_trait]
pub trait Driver: Send + Sync {
    async fn connect(cfg: &ConnConfig) -> Result<Self> where Self: Sized;
    async fn ping(&self) -> Result<()>;
    async fn schema(&self) -> Result<Schema>;   // databases > containers > fields
    async fn query(&self, q: &Query) -> Result<ResultSet>;
    async fn close(self) -> Result<()>;
}

pub enum Query {
    Sql(String),              // PG, MySQL
    Command(Vec<String>),     // Redis: ["GET","key"]
    Mongo(MongoOp),           // find/insert/aggregate as a structured op
}

pub enum ResultSet {
    Tabular  { cols: Vec<Column>, rows: Vec<Row> },  // SQL results
    Documents(Vec<Json>),                            // Mongo
    KeyValue(Vec<(String, RedisValue)>),             // Redis
    Affected(u64),                                   // writes
}
```

- `Query` is an enum, not a string, so non-SQL engines (Redis, Mongo) are
  first-class and SQL assumptions never leak into the abstraction. Each driver
  handles the variant it understands and errors on the rest.
- `Schema` is unified: every engine maps to `databases → containers → fields`
  even when the native names differ (table / collection / keyspace), so the UI
  tree renders one way.

## Connection Security

- Connection metadata (host, port, user, db, sslmode) stored in plain config:
  `~/.config/dbm/connections.json` (platform config dir).
- **Passwords are never written to that file.** They go to the OS keychain via
  the `keyring` crate (macOS Keychain, Windows Credential Manager, Linux Secret
  Service). Config stores only a keychain reference id.
- Per-connection TLS / `sslmode` support.
- SSH tunnel support is explicitly **post-MVP**.

## Testing

- **Driver crates:** integration tests against real engines using the
  `testcontainers` crate (each driver spins its own PG/MySQL/Redis/Mongo Docker
  container, runs connect → query → assert). No mocks for drivers — real
  protocol behavior is the point.
- **core:** unit tests on `ResultSet` / `Schema` mapping logic; pure, no
  network.
- **app/UI:** manual testing for MVP. Slint logic is kept thin to minimize
  untested surface.

## Scope (MVP)

In:
- 4 engines (PG, MySQL, Redis, Mongo) behind one `Driver` trait.
- Connection picker, save/edit connections, keychain-backed passwords.
- Query editor + result rendering for all 3 result paradigms.
- Schema browser tree.
- Builds for macOS, Windows, Linux.

Out (post-MVP, noted not built):
- SSH tunnels.
- The remaining ~16 engines (added later, one driver crate each).
- Automated UI tests.
- Data export, query history, multi-tab beyond basics (revisit after MVP).

## Open Questions

- Slint vs egui final call if Slint styling proves too limiting during build
  (fallback: egui, plainer look, same architecture).
- Connection-pool strategy per driver (single conn vs pool) — decide per driver
  during implementation.
