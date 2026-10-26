# RDB — native database manager

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/logo-dark.svg">
    <img src="assets/brand/logo-light.svg" alt="RDB" width="360">
  </picture>
</p>

<p align="center">
  <strong>Native cross-platform database manager.<br>PostgreSQL · MySQL · MariaDB · Redis · Valkey · MongoDB · SQLite · Cassandra · Scylla · SQL Server · Oracle · ClickHouse — one desktop app, no Electron.</strong>
</p>

<p align="center">
  <a href="https://github.com/suiflex/rdb/actions/workflows/rdb-app.yml"><img src="https://img.shields.io/github/actions/workflow/status/suiflex/rdb/rdb-app.yml?branch=develop&style=for-the-badge" alt="Build status"></a>
  <a href="https://github.com/suiflex/rdb/releases"><img src="https://img.shields.io/github/v/tag/suiflex/rdb?include_prereleases&style=for-the-badge&label=release" alt="Release"></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/License-Apache_2.0-4ade80.svg?style=for-the-badge" alt="Apache 2.0 License"></a>
  <img src="https://img.shields.io/badge/platform-macOS_·_Linux_·_Windows-4ade80?style=for-the-badge" alt="Platforms">
  <img src="https://img.shields.io/badge/built_with-Rust-4ade80?style=for-the-badge" alt="Built with Rust">
</p>

A native, lightweight, cross-platform database manager built with Rust and Slint — in the spirit of TablePlus but compiled from a single codebase for macOS and Linux.

> Where RDB is headed and why — see [**VISION.md**](./VISION.md).

## Features

- **Multi-engine** — PostgreSQL, MySQL, MariaDB, Redis, Valkey, MongoDB, SQLite, Cassandra, SQL Server, Oracle, ClickHouse in one app
- **Native UI** — GPU-rendered via Slint (no webview, no Chromium, no Electron)
- **CLI and MCP** — manage scoped agent access with `rdb-cli mcp`; agent clients launch the separate `rdb-mcp` stdio server
- **In-app Assistant** — Ask suggests queries from schema and editor context; Agent can inspect metadata and run read queries when you request results. Writes require approval of the exact SQL and are disabled for production connections
- **Fast & light** — no GC or bundled browser runtime; aggressive release optimization (LTO, `opt-level=z`, `panic=abort`)
- **Secure connections** — passwords stored in OS keychain (macOS Keychain, libsecret) with AES-GCM encrypted-file fallback
- **Schema browser** — sidebar tree: databases → tables/collections/keys → columns/fields
- **Tabbed query editor** — multiple tabs per session, one per engine
- **Results grid** — resizable columns, client-side row filtering, copy support
- **Command palette** — `Cmd+K` to jump to any connection or table instantly
- **DSN import** — paste a connection URL, fields auto-fill
- **Connection test** — verify creds before saving
- **Light / dark mode** toggle

## Supported Engines

| Engine | Protocol | Result type |
|--------|----------|-------------|
| PostgreSQL | `tokio-postgres` | Tabular |
| MySQL | `mysql_async` | Tabular |
| MariaDB | `mysql_async` (MySQL-protocol-compatible) | Tabular |
| Redis | `redis` crate | Key-value / raw |
| Valkey | `redis` crate (RESP-compatible) | Key-value / raw |
| MongoDB | `mongodb` crate | Documents (JSON) |
| SQLite | `rusqlite` | Tabular |
| Cassandra | `scylla` | Tabular |
| SQL Server | `tiberius` | Tabular |
| Oracle | `oracledb` (Oracle's thin driver) | Tabular |
| ClickHouse | `clickhouse` (HTTP) | Tabular |

## Design

### Core design rule

`rdb-core` defines the driver contract. `rdb-runtime` owns the concrete `AnyDriver` dispatch and tunnel integration. The UI uses that dispatch through `app/src/dispatch.rs`; it does not import individual driver crates. Adding an engine requires a driver implementation, an `Engine` catalog entry in `rdb-connstore`, and a dispatch variant in `rdb-runtime`.

### Async bridge

Slint's event loop runs on the main thread. All I/O (connect, query, schema fetch) spawns on a `tokio` multi-thread runtime. Results return to the UI thread via `invoke_from_event_loop`.

### Driver trait

The main methods are shown below; the trait also supports streaming, schema
discovery, writes, and cancellation.

```rust
#[async_trait]
pub trait Driver: Send + Sync {
    async fn connect(cfg: &ConnConfig) -> Result<Self> where Self: Sized;
    async fn ping(&self) -> Result<()>;
    async fn schema(&self) -> Result<Schema>;
    async fn query(&self, q: &Query) -> Result<ResultSet>;
    async fn close(self) -> Result<()> where Self: Sized;
}
```

### Query enum

```rust
pub enum Query {
    Sql(String),           // PostgreSQL, MySQL, MariaDB, SQLite, SQL Server, Oracle, ClickHouse
    Cql(String),           // Cassandra/ScyllaDB
    Command(Vec<String>),  // Redis/Valkey: ["GET", "key"]
    Mongo(Box<MongoOp>),   // find / insert / aggregate
}
```

### ResultSet enum

```rust
pub enum ResultSet {
    Tabular   { cols: Vec<Column>, rows: Vec<Row> },
    Documents(Vec<serde_json::Value>),
    KeyValue(Vec<(String, RedisValue)>),
    Affected(u64),
}
```

## In-app Assistant

Configure a model under **Settings → AI**. In the Assistant panel, choose the
connection, database, and model for the conversation.

- **Ask** uses schema and editor context to draft, explain, optimize, repair,
  or translate queries. It does not execute them.
- **Agent** can inspect database metadata and run a read query when you ask for
  actual data or results. A proposed SQL write runs only after you approve its
  exact text. Production connections cannot use Agent writes.

The in-app Assistant is implemented in `rdb-agent`. External agent clients use
the separate `rdb-mcp` server described below.

## MCP Server

Run the interactive TTY manager:

```sh
rdb-cli mcp
```

The manager reads saved RDB connections, lets the user select one connection and database, sets read-only or write-enabled access, and registers `rdb-mcp` with the selected agent through Kurir. The MCP client starts `rdb-mcp` over stdio when it needs the server.

The server exposes `rdb_list_connections`, `rdb_describe_schema`, and `rdb_execute_query`. Connection and database access default to deny. The manager writes the selected connection ID and database into the agent's MCP environment; each registration scopes one connection.

SQLite connections use the saved database file; the scope name must be `main`. SQL database qualifiers outside the selected database, session/database switches, privilege changes, and procedural SQL are rejected. Database credentials remain the final permission boundary because SQL functions can have side effects.

Query responses include at most 500 rows and report the number of omitted rows.

Kurir supports manual registration when the TTY manager is unavailable:

```sh
kurir register \
  --client claude-code \
  --name rdb \
  --command rdb-mcp \
  --env RDB_MCP_SCOPE_CONNECTION_IDS=connection-id \
  --env RDB_MCP_SCOPE_DATABASES=app
```

To enable writes, add `--env RDB_MCP_ALLOW_WRITES=1` only for a trusted connection. The permission applies to every connection in that server's allowlist.

## Install

Prebuilt installers are expected to be attached to GitHub Releases.

### npm (all platforms)

```bash
npm i -g @suiflex/rdb
```

The postinstall step downloads the prebuilt `rdb`, `rdb-cli`, and `rdb-mcp`
binaries from the matching GitHub Release. On macOS it also installs
`RDB.app` into `~/Applications` so it shows up in Launchpad.

### macOS / Linux

```bash
curl -fsSL https://raw.githubusercontent.com/suiflex/rdb/develop/scripts/install.sh | bash
```

Optional:

```bash
curl -fsSL https://raw.githubusercontent.com/suiflex/rdb/develop/scripts/install.sh | RDB_VERSION=v0.1.0 INSTALL_DIR="$HOME/.local/bin" bash
```

On macOS the script installs `RDB.app` and links `rdb`, `rdb-cli`, and `rdb-mcp`
into `~/.local/bin`. On Linux it installs all three commands into
`/usr/local/bin` when writable, otherwise `~/.local/bin`.

### Homebrew (macOS / Linux)

For the macOS app (Launchpad):

```bash
brew install --cask suiflex/tap/rdb
```

Linux package (desktop, CLI, and MCP server):

```bash
brew install suiflex/tap/rdb
```

The Linux formula installs `rdb`, `rdb-cli`, and `rdb-mcp`. The macOS cask
installs `RDB.app` and links the two terminal commands.
Upgrade later with `brew upgrade rdb`.

> **macOS Gatekeeper note:** the `.app` is ad-hoc signed (no paid Apple
> Developer notarization). The cask and `install.sh` both strip the
> quarantine flag automatically, so a normal install just works. If you
> downloaded the `.dmg` straight from a GitHub Release and macOS says the
> app is damaged / rejected, clear it yourself:
> ```bash
> xattr -dr com.apple.quarantine /Applications/RDB.app
> ```

### Windows

```powershell
irm https://raw.githubusercontent.com/suiflex/rdb/develop/scripts/install.ps1 | iex
```

Optional:

```powershell
$env:RDB_VERSION="v0.28.0"; $env:INSTALL_DIR="$env:LOCALAPPDATA\Programs\RDB\bin"; irm https://raw.githubusercontent.com/suiflex/rdb/develop/scripts/install.ps1 | iex
```

The script installs `rdb.exe` into
`%LOCALAPPDATA%\Programs\RDB\bin` by default and warns if that directory is not
on `PATH`.

### Scoop (Windows)

```powershell
scoop bucket add suiflex https://github.com/suiflex/scoop-bucket
scoop install rdb
```

Upgrade later with `scoop update rdb`.

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
cargo build -p rdb

# Optimized release (~smaller binary)
cargo build --release -p rdb

# Run directly
cargo run -p rdb
```

The release binary lands at `target/release/rdb`.

The root `Makefile` provides `make be-build` and `make be-test` for the backend
libraries, including `rdb-runtime` and `rdb-agent`. CLI and MCP have their own
package-scoped CI workflows.

Build the CLI manager and MCP server too:

```bash
cargo build --release -p rdb-cli -p rdb-mcp
```

## Usage

### Add a connection

1. Launch RDB — connection picker opens.
2. Click **+** → fill in host, port, credentials, database.
3. Or paste a DSN URL into the **Import URL** field — fields auto-fill.
4. Click **Test** to verify, then **Save**.

### Connect & query

1. Click a saved connection → connects, schema loads in sidebar.
2. Type a query for the connected engine — syntax differs per engine, see below.
3. `Cmd+Enter` (macOS) / `Ctrl+Enter` (Linux) to run.
4. Click a table/collection in the sidebar to auto-generate and run a `SELECT *` / `find` query.

### Query syntax per engine

| Engine | Language | Example |
| --- | --- | --- |
| PostgreSQL, MySQL, MariaDB, SQLite, SQL Server, Oracle, ClickHouse | SQL | `SELECT * FROM users` |
| Cassandra | CQL (no JOIN/subquery/HAVING) | `SELECT * FROM keyspace.table` |
| Redis, Valkey | command tokens | `GET user:1`, `SET user:1 value` |
| MongoDB | mongosh chain | `db.users.find({ age: { $gt: 20 } }).limit(5)`<br>`use('shop')`<br>`db.getCollection('fs.files').find({}).sort({ _id: -1 })` |

More detail (including MongoDB's supported methods/modifiers) is on the
[docs page](https://rdb.suiflex.dev/docs).

### Command palette

`Cmd+K` — fuzzy-search all connections and schema objects.

### Filter results

Type in the **Filter** box above the grid — filters rows client-side without re-querying.

## Project status

Active development. Ships 11 engines (PostgreSQL, MySQL, MariaDB, Redis, Valkey, MongoDB, SQLite, Cassandra, SQL Server, Oracle, ClickHouse); planned expansion to ~20 (BigQuery and more).

## Crate overview

| Crate | Description |
|-------|-------------|
| `rdb` | Desktop binary (app/) |
| `rdb-core` | `Driver` trait, `Query`, `ResultSet`, `Schema`, `RdbError` |
| `rdb-connstore` | Saved connections — JSON on disk + OS keychain / AES-GCM file |
| `rdb-tunnel` | SSH tunnel support for database connections |
| `rdb-runtime` | Concrete driver dispatch, connection runtime, and mock driver |
| `rdb-agent` | In-app Assistant models, sessions, tools, and local settings |
| `rdb-cli` | Terminal manager for scoped MCP registration |
| `rdb-mcp` | Stdio MCP server for external agent clients |
| `rdb-driver-postgres` | PostgreSQL driver via `tokio-postgres` |
| `rdb-driver-mysql` | MySQL/MariaDB driver via `mysql_async` |
| `rdb-driver-redis` | Redis/Valkey driver via `redis` crate |
| `rdb-driver-mongo` | MongoDB driver via `mongodb` crate |
| `rdb-driver-sqlite` | SQLite driver via `rusqlite` |
| `rdb-driver-cassandra` | Cassandra driver via `scylla` |
| `rdb-driver-mssql` | SQL Server driver via `tiberius` |
| `rdb-driver-oracle` | Oracle driver via `oracledb` |
| `rdb-driver-clickhouse` | ClickHouse driver via the `clickhouse` HTTP crate |

## License

Apache-2.0
