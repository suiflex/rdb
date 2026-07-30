# RDB — native database manager

<p align="center">
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="assets/brand/logo-dark.svg">
    <img src="assets/brand/logo-light.svg" alt="RDB" width="360">
  </picture>
</p>

<p align="center">
  <strong>Native cross-platform database manager.<br>PostgreSQL · MySQL · Redis · MongoDB · SQLite · Cassandra — one binary, no Electron.</strong>
</p>

<p align="center">
  <a href="https://github.com/suiflex/rdb/actions/workflows/rdb-app.yml"><img src="https://img.shields.io/github/actions/workflow/status/suiflex/rdb/rdb-app.yml?branch=develop&style=for-the-badge" alt="Build status"></a>
  <a href="https://github.com/suiflex/rdb/releases"><img src="https://img.shields.io/github/v/tag/suiflex/rdb?include_prereleases&style=for-the-badge&label=release" alt="Release"></a>
  <a href="./LICENSE"><img src="https://img.shields.io/badge/License-MIT-4ade80.svg?style=for-the-badge" alt="MIT License"></a>
  <img src="https://img.shields.io/badge/platform-macOS_·_Linux_·_Windows-4ade80?style=for-the-badge" alt="Platforms">
  <img src="https://img.shields.io/badge/built_with-Rust-4ade80?style=for-the-badge" alt="Built with Rust">
</p>

A native, lightweight, cross-platform database manager built with Rust and Slint — in the spirit of TablePlus but compiled from a single codebase for macOS and Linux.

> Where RDB is headed and why — see [**VISION.md**](./VISION.md).

## Features

- **Multi-engine** — PostgreSQL, MySQL/MariaDB, Redis, MongoDB, SQLite, Cassandra in one app
- **Native UI** — GPU-rendered via Slint (no webview, no Chromium, no Electron)
- **Fast & light** — no GC, no runtime; aggressive release optimization (LTO, `opt-level=z`, `panic=abort`)
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
| MySQL / MariaDB | `mysql_async` | Tabular |
| Redis | `redis` crate | Key-value / raw |
| MongoDB | `mongodb` crate | Documents (JSON) |
| SQLite | `rusqlite` | Tabular |
| Cassandra | `scylla` | Tabular |

## Design

### Core design rule

The UI (`app/`) depends only on `rdb-core`. It never imports a concrete driver crate. Adding a new engine = a new `driver-*` crate that implements the `Driver` trait; the UI is untouched.

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
    Sql(String),           // PostgreSQL, MySQL, SQLite, Cassandra
    Command(Vec<String>),  // Redis: ["GET", "key"]
    Mongo(MongoOp),        // find / insert / aggregate
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

## Install

Prebuilt installers are expected to be attached to GitHub Releases.

### npm (all platforms)

```bash
npm i -g @suiflex/rdb
```

The postinstall step downloads the prebuilt `rdb` binary for your platform from
the matching GitHub Release, then `rdb` launches it. On macOS it also installs
`RDB.app` into `~/Applications` so it shows up in Launchpad.

### macOS / Linux

```bash
curl -fsSL https://raw.githubusercontent.com/suiflex/rdb/develop/scripts/install.sh | bash
```

Optional:

```bash
curl -fsSL https://raw.githubusercontent.com/suiflex/rdb/develop/scripts/install.sh | RDB_VERSION=v0.1.0 INSTALL_DIR="$HOME/.local/bin" bash
```

On macOS the script installs `RDB.app` into Applications (Launchpad) and symlinks
the `rdb` command into `~/.local/bin`. On Linux it installs the `rdb` binary into
`/usr/local/bin` when writable, otherwise `~/.local/bin`.

### Homebrew (macOS / Linux)

For the macOS app (Launchpad):

```bash
brew install --cask suiflex/tap/rdb
```

For the CLI binary only:

```bash
brew install suiflex/tap/rdb
```

Upgrade later with `brew upgrade rdb`.

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

## Usage

### Add a connection

1. Launch RDB — connection picker opens.
2. Click **+** → fill in host, port, credentials, database.
3. Or paste a DSN URL into the **Import URL** field — fields auto-fill.
4. Click **Test** to verify, then **Save**.

### Connect & query

1. Click a saved connection → connects, schema loads in sidebar.
2. Type SQL (PostgreSQL/MySQL), a Redis command (e.g. `GET key`), or a MongoDB JSON operation in the query editor.
3. `Cmd+Enter` (macOS) / `Ctrl+Enter` (Linux) to run.
4. Click a table/collection in the sidebar to auto-generate and run a `SELECT *` / `find` query.

### Command palette

`Cmd+K` — fuzzy-search all connections and schema objects.

### Filter results

Type in the **Filter** box above the grid — filters rows client-side without re-querying.

## Project status

Active development. Ships 6 engines (PostgreSQL, MySQL, Redis, MongoDB, SQLite, Cassandra); planned expansion to ~20 (ClickHouse, BigQuery, Oracle, and more).

## Crate overview

| Crate | Description |
|-------|-------------|
| `rdb` | Desktop binary (app/) |
| `rdb-core` | `Driver` trait, `Query`, `ResultSet`, `Schema`, `RdbError` |
| `rdb-connstore` | Saved connections — JSON on disk + OS keychain / AES-GCM file |
| `rdb-driver-postgres` | PostgreSQL driver via `tokio-postgres` |
| `rdb-driver-mysql` | MySQL/MariaDB driver via `mysql_async` |
| `rdb-driver-redis` | Redis driver via `redis` crate |
| `rdb-driver-mongo` | MongoDB driver via `mongodb` crate |
| `rdb-driver-sqlite` | SQLite driver via `rusqlite` |
| `rdb-driver-cassandra` | Cassandra driver via `scylla` |

## License

MIT
