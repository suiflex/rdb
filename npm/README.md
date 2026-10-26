# @suiflex/rdb

Native cross-platform database manager, CLI manager, and MCP server for
PostgreSQL, MySQL, MariaDB, Redis, Valkey, MongoDB, SQLite, Cassandra, SQL
Server, Oracle, and ClickHouse. No Electron.

## Install

```bash
npm i -g @suiflex/rdb
```

The postinstall step downloads the prebuilt RDB desktop, `rdb-cli`, and
`rdb-mcp` binaries for your platform (macOS, Linux, Windows — x64 & arm64)
from the matching [GitHub Release](https://github.com/suiflex/rdb/releases).
Run the desktop app with:

```bash
rdb
```

Open the interactive MCP registration flow with:

```bash
rdb-cli mcp
```

Agent clients launch `rdb-mcp` through stdio after registration.

## Features

- **Multi-engine** — PostgreSQL, MySQL, MariaDB, Redis, Valkey, MongoDB, SQLite, Cassandra, SQL Server, Oracle, ClickHouse in one app
- **Native UI** — GPU-rendered via Slint (no webview, no Chromium, no Electron)
- **Fast & light** — no GC or bundled browser runtime; aggressive release optimization (LTO, `opt-level=z`, `panic=abort`)
- **Agent integration** — configure a scoped MCP server for supported agent clients with `rdb-cli mcp`
- **In-app Assistant** — Ask drafts queries from schema and editor context; Agent inspects metadata and runs read queries when you request results. SQL writes require approval and are disabled for production connections
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

`rdb-core` defines the driver contract. `rdb-runtime` owns concrete driver dispatch and tunnels; the desktop app uses it through `app/src/dispatch.rs`. The in-app Assistant lives in `rdb-agent`, while `rdb-cli` and `rdb-mcp` provide external agent access. Adding an engine requires a driver implementation, an `Engine` catalog entry in `rdb-connstore`, and a dispatch variant in `rdb-runtime`.

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

## License

Apache-2.0
