# @suiflex/rdb

Native cross-platform database manager — PostgreSQL, MySQL, MariaDB, Redis,
Valkey, MongoDB, SQLite, Cassandra, SQL Server, Oracle, ClickHouse in one binary. No
Electron.

## Install

```bash
npm i -g @suiflex/rdb
```

The postinstall step downloads the prebuilt `rdb` binary for your platform
(macOS, Linux, Windows — x64 & arm64) from the matching
[GitHub Release](https://github.com/suiflex/rdb/releases). Then run:

```bash
rdb
```

## Features

- **Multi-engine** — PostgreSQL, MySQL, MariaDB, Redis, Valkey, MongoDB, SQLite, Cassandra, SQL Server, Oracle, ClickHouse in one app
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
| MySQL | `mysql_async` | Tabular |
| MariaDB | `mysql_async` (MySQL-protocol-compatible) | Tabular |
| Redis | `redis` crate | Key-value / raw |
| Valkey | `redis` crate (RESP-compatible) | Key-value / raw |
| MongoDB | `mongodb` crate | Documents (JSON) |
| SQLite | `rusqlite` | Tabular |
| Cassandra | `scylla` | Tabular |
| SQL Server | `tiberius` | Tabular |
| Oracle | `oracle` (ODPI-C) | Tabular |
| ClickHouse | `clickhouse` (HTTP) | Tabular |

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
    Sql(String),           // PostgreSQL, MySQL, MariaDB, SQLite, SQL Server, Oracle, ClickHouse
    Cql(String),           // Cassandra/ScyllaDB
    Command(Vec<String>),  // Redis/Valkey: ["GET", "key"]
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

## License

Apache-2.0
