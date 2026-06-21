# RDBS

A native, lightweight, cross-platform database manager built with Rust and Slint — in the spirit of TablePlus but compiled from a single codebase for macOS, Windows, and Linux.

## Features

- **Multi-engine** — PostgreSQL, MySQL/MariaDB, Redis, MongoDB in one app
- **Native UI** — GPU-rendered via Slint (no webview, no Chromium, no Electron)
- **Fast & light** — no GC, no runtime; aggressive release optimization (LTO, `opt-level=z`, `panic=abort`)
- **Secure connections** — passwords stored in OS keychain (macOS Keychain, libsecret, Windows Credential Manager) with AES-GCM encrypted-file fallback
- **Schema browser** — sidebar tree: databases → tables/collections/keys → columns/fields
- **Tabbed query editor** — multiple tabs per session, one per engine
- **Results grid** — resizable columns, client-side row filtering, copy support
- **Command palette** — `Cmd+K` to jump to any connection or table instantly
- **DSN import** — paste a connection URL, fields auto-fill
- **Connection test** — verify creds before saving
- **Light / dark mode** toggle

## Supported Engines (MVP)

| Engine | Protocol | Result type |
|--------|----------|-------------|
| PostgreSQL | `tokio-postgres` | Tabular |
| MySQL / MariaDB | `mysql_async` | Tabular |
| Redis | `redis` crate | Key-value / raw |
| MongoDB | `mongodb` crate | Documents (JSON) |

## Architecture

Cargo workspace (monorepo):

```
storix/
├── Cargo.toml                  # workspace root
├── app/                        # binary: Slint UI + event handlers
│   ├── build.rs                # slint-build codegen
│   └── src/
│       ├── main.rs             # Slint event loop + Tokio async bridge
│       ├── dispatch.rs         # AnyDriver enum (erases concrete driver types)
│       ├── model.rs            # ResultSet / Schema → Slint view-models
│       ├── query_parse.rs      # text → Query enum per engine
│       ├── theme.rs            # color / accent helpers
│       └── ui/                 # .slint markup files
│           ├── app-window.slint
│           ├── conn-form.slint
│           ├── picker.slint
│           ├── workarea.slint
│           ├── sidebar.slint
│           ├── palette.slint
│           ├── structs.slint
│           ├── theme.slint
│           └── icons.slint
├── crates/
│   ├── core/                   # Driver trait + ResultSet + Schema + Error
│   ├── connstore/              # saved connections (JSON) + secret backend
│   ├── driver-postgres/
│   ├── driver-mysql/
│   ├── driver-redis/
│   └── driver-mongo/
└── docs/
    └── superpowers/            # design specs + implementation plans
```

### Core design rule

The UI (`app/`) depends only on `dbm-core`. It never imports a concrete driver crate. Adding a new engine = a new `driver-*` crate that implements the `Driver` trait; the UI is untouched.

### Async bridge

Slint's event loop runs on the main thread. All I/O (connect, query, schema fetch) spawns on a `tokio` multi-thread runtime. Results return to the UI thread via `invoke_from_event_loop`.

### Driver trait

```rust
#[async_trait]
pub trait Driver: Send + Sync {
    async fn connect(cfg: &ConnConfig) -> Result<Self> where Self: Sized;
    async fn ping(&self) -> Result<()>;
    async fn schema(&self) -> Result<Schema>;
    async fn query(&self, q: &Query) -> Result<ResultSet>;
    async fn close(self) -> Result<()>;
}
```

### Query enum

```rust
pub enum Query {
    Sql(String),           // PostgreSQL, MySQL
    Command(Vec<String>),  // Redis: ["GET", "key"]
    Mongo(MongoOp),        // find / insert / aggregate
}
```

### ResultSet enum

```rust
pub enum ResultSet {
    Tabular   { cols: Vec<Column>, rows: Vec<Row> },
    Documents(Vec<serde_json::Value>),
    Affected  { count: u64 },
    RedisValue(String),
}
```

## Install

Prebuilt installers are expected to be attached to GitHub Releases.

### macOS / Linux

```bash
curl -fsSL https://raw.githubusercontent.com/suiflex/rdb/develop/scripts/install.sh | bash
```

Optional:

```bash
curl -fsSL https://raw.githubusercontent.com/suiflex/rdb/develop/scripts/install.sh | STORIX_VERSION=v0.1.0 INSTALL_DIR="$HOME/.local/bin" bash
```

The script installs `storix` into `/usr/local/bin` when writable, otherwise it falls back to `~/.local/bin`.

## Build from source

| Tool | Version |
|------|---------|
| Rust | stable (see `rust-toolchain.toml`) |
| Cargo | bundled with Rust |

No additional native dependencies needed on macOS. Linux requires a few system packages for Slint rendering (see below).

### Linux system packages

```bash
# Debian / Ubuntu
sudo apt install libxkbcommon-dev libfontconfig1-dev libgl1-mesa-dev

# Fedora / RHEL
sudo dnf install libxkbcommon-devel fontconfig-devel mesa-libGL-devel
```

## Build

```bash
# Development
cargo build -p storix

# Optimized release (~smaller binary)
cargo build --release -p storix

# Run directly
cargo run -p storix
```

The release binary lands at `target/release/storix`.

## Usage

### Add a connection

1. Launch Storix — connection picker opens.
2. Click **+** → fill in host, port, credentials, database.
3. Or paste a DSN URL into the **Import URL** field — fields auto-fill.
4. Click **Test** to verify, then **Save**.

### Connect & query

1. Click a saved connection → connects, schema loads in sidebar.
2. Type SQL (PostgreSQL/MySQL), a Redis command (e.g. `GET key`), or a MongoDB JSON operation in the query editor.
3. `Cmd+Enter` (macOS) / `Ctrl+Enter` (Windows/Linux) to run.
4. Click a table/collection in the sidebar to auto-generate and run a `SELECT *` / `find` query.

### Command palette

`Cmd+K` — fuzzy-search all connections and schema objects.

### Filter results

Type in the **Filter** box above the grid — filters rows client-side without re-querying.

## Project status

Active development. MVP covers 4 engines; planned expansion to ~20 (SQLite, ClickHouse, BigQuery, Oracle, Cassandra, and more).

## Crate overview

| Crate | Description |
|-------|-------------|
| `storix` | Desktop binary (app/) |
| `dbm-core` | `Driver` trait, `Query`, `ResultSet`, `Schema`, `DbmError` |
| `dbm-connstore` | Saved connections — JSON on disk + OS keychain / AES-GCM file |
| `dbm-driver-postgres` | PostgreSQL driver via `tokio-postgres` |
| `dbm-driver-mysql` | MySQL/MariaDB driver via `mysql_async` |
| `dbm-driver-redis` | Redis driver via `redis` crate |
| `dbm-driver-mongo` | MongoDB driver via `mongodb` crate |

## License

MIT
