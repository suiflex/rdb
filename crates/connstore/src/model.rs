use rdb_core::conn::{ConnConfig, SshAuthMode, SshTunnelConfig, SslMode};
use serde::{Deserialize, Serialize};

/// Database engine of a saved connection. The MVP ships four; the variant set
/// grows as new driver crates are added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Engine {
    Postgres,
    MySql,
    Redis,
    Mongo,
    /// Local file-based engine: no host/port/user; `database` holds the path.
    Sqlite,
    /// CQL engine (Cassandra / ScyllaDB); `database` holds the default keyspace.
    Cassandra,
    /// SQL Server (T-SQL); SQL auth only in v1, no Windows/AD auth.
    Mssql,
    /// ClickHouse (analytics/OLAP), HTTP interface; `database` holds the
    /// default database. Insert-only write-back (no row-level UPDATE/DELETE).
    Clickhouse,
    /// MariaDB: MySQL wire-protocol-compatible fork, dispatched through
    /// the same `MysqlDriver` as `Engine::MySql`.
    MariaDb,
    /// Valkey: RESP-compatible fork of Redis, dispatched through the same
    /// `RedisDriver` as `Engine::Redis`.
    Valkey,
}

/// The query dialect an engine's editor tab speaks. Drives completion, syntax
/// highlighting, and formatting — one variant per query paradigm, not per
/// engine, so SQL-family engines (Postgres/MySQL/SQLite) share a module
/// without dragging Redis or Mongo into SQL-shaped behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryLanguage {
    Sql,
    Cql,
    Command,
    Mongo,
}

/// Everything about an engine that is otherwise re-spelled as a string
/// somewhere in the app: the display label, the badge key, the URL scheme, the
/// default port, and the query dialect.
///
/// One row per engine here replaces the half-dozen independent `match`
/// tables these used to live in — none of which the compiler could
/// cross-check, and two of which silently fell back to Postgres on an
/// unrecognized label.
pub struct EngineMeta {
    pub engine: Engine,
    /// Shown in the UI (engine picker, sidebar, palette).
    pub display: &'static str,
    /// Stable lowercase key the UI's `DbBadge` and the export header use.
    pub key: &'static str,
    /// Canonical URI scheme for an exported connection string. Differs from
    /// `key` where the ecosystem spells it differently
    /// (`postgres`/`postgresql`, `mongo`/`mongodb`, `mssql`/`sqlserver`).
    pub scheme: &'static str,
    /// Port prefilled by the connection form. `"0"` for file-based engines.
    pub default_port: &'static str,
    pub language: QueryLanguage,
}

/// Every supported engine. Adding a driver means adding a row here — see
/// `Engine::meta`, which every string lookup goes through.
pub const ENGINES: &[EngineMeta] = &[
    EngineMeta {
        engine: Engine::Postgres,
        display: "PostgreSQL",
        key: "postgres",
        scheme: "postgresql",
        default_port: "5432",
        language: QueryLanguage::Sql,
    },
    EngineMeta {
        engine: Engine::MySql,
        display: "MySQL",
        key: "mysql",
        scheme: "mysql",
        default_port: "3306",
        language: QueryLanguage::Sql,
    },
    EngineMeta {
        engine: Engine::Redis,
        display: "Redis",
        key: "redis",
        scheme: "redis",
        default_port: "6379",
        language: QueryLanguage::Command,
    },
    EngineMeta {
        engine: Engine::Mongo,
        display: "MongoDB",
        key: "mongo",
        scheme: "mongodb",
        default_port: "27017",
        language: QueryLanguage::Mongo,
    },
    EngineMeta {
        engine: Engine::Sqlite,
        display: "SQLite",
        key: "sqlite",
        scheme: "sqlite",
        // file-based: port unused
        default_port: "0",
        language: QueryLanguage::Sql,
    },
    EngineMeta {
        engine: Engine::Cassandra,
        display: "Cassandra",
        key: "cassandra",
        scheme: "cassandra",
        default_port: "9042",
        language: QueryLanguage::Cql,
    },
    EngineMeta {
        engine: Engine::Mssql,
        display: "SQL Server",
        key: "mssql",
        scheme: "sqlserver",
        default_port: "1433",
        language: QueryLanguage::Sql,
    },
    EngineMeta {
        engine: Engine::Clickhouse,
        display: "ClickHouse",
        key: "clickhouse",
        scheme: "clickhouse",
        default_port: "8123",
        language: QueryLanguage::Sql,
    },
    EngineMeta {
        engine: Engine::MariaDb,
        display: "MariaDB",
        key: "mariadb",
        scheme: "mariadb",
        default_port: "3306",
        language: QueryLanguage::Sql,
    },
    EngineMeta {
        engine: Engine::Valkey,
        display: "Valkey",
        key: "valkey",
        scheme: "valkey",
        default_port: "6379",
        language: QueryLanguage::Command,
    },
];

impl Engine {
    /// This engine's row in [`ENGINES`]. Panics only if a variant was added
    /// without a row, which `every_engine_has_a_row` catches in CI.
    pub fn meta(self) -> &'static EngineMeta {
        ENGINES
            .iter()
            .find(|m| m.engine == self)
            .expect("every Engine variant needs a row in ENGINES")
    }

    /// The query dialect this engine's editor tab speaks. Single source of
    /// truth for completion/lexer/format dispatch — see `QueryLanguage`.
    pub fn language(self) -> QueryLanguage {
        self.meta().language
    }

    /// Human label shown in the UI.
    pub fn display(self) -> &'static str {
        self.meta().display
    }

    /// Stable lowercase key (badge icons, export header).
    pub fn key(self) -> &'static str {
        self.meta().key
    }

    /// Canonical URI scheme for an exported connection string.
    pub fn scheme(self) -> &'static str {
        self.meta().scheme
    }

    /// Port the connection form prefills.
    pub fn default_port(self) -> &'static str {
        self.meta().default_port
    }

    /// Resolve a UI display label back to its engine. `None` for an unknown
    /// label — the old lookup silently answered Postgres, which routed a
    /// mistyped label's connection to the wrong driver.
    pub fn from_display(label: &str) -> Option<Engine> {
        ENGINES
            .iter()
            .find(|m| m.display == label)
            .map(|m| m.engine)
    }

    /// Resolve a badge key back to its engine, for the places that only carry
    /// the key (a workspace tab stores one to draw its badge). `None` for an
    /// unknown or empty key rather than a guess.
    pub fn from_key(key: &str) -> Option<Engine> {
        ENGINES.iter().find(|m| m.key == key).map(|m| m.engine)
    }
}

/// Environment classification rendered as a colored pill next to the
/// connection name. Independent of `local` (tunnel indicator) and `tags`
/// (free-form chips) — picking `Local` here does not set `local = true`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum EnvTag {
    #[default]
    None,
    Local,
    Dev,
    Staging,
    Testing,
    Production,
}

impl EnvTag {
    pub fn as_str(self) -> &'static str {
        match self {
            EnvTag::None => "None",
            EnvTag::Local => "Local",
            EnvTag::Dev => "Dev",
            EnvTag::Staging => "Staging",
            EnvTag::Testing => "Testing",
            EnvTag::Production => "Production",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "Local" => EnvTag::Local,
            "Dev" => EnvTag::Dev,
            "Staging" => EnvTag::Staging,
            "Testing" => EnvTag::Testing,
            "Production" => EnvTag::Production,
            _ => EnvTag::None,
        }
    }
}

/// A persisted connection. Only non-secret fields are stored. The password is
/// NEVER a field here — it lives in the OS keychain (or encrypted file) and is
/// located via `keyref`, then injected at connect time by `to_conn_config`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedConnection {
    pub id: String,
    pub name: String,
    pub engine: Engine,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub database: Option<String>,
    #[serde(default)]
    pub sslmode: SslMode,
    /// Per-connection accent color as `#rrggbb` (the signature UI feature).
    /// `None` => UI uses the default accent.
    #[serde(default)]
    pub color: Option<String>,
    /// Optional sidebar group/folder label (TablePlus-style grouping).
    /// `None` => connection renders under the default "Ungrouped" header.
    #[serde(default)]
    pub group: Option<String>,
    /// Marks a connection reached over a local tunnel/socket; the UI shows a
    /// LOCAL chip next to it.
    #[serde(default)]
    pub local: bool,
    /// Free-form labels shown as chips in the connection detail panel.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Driver-specific connection options: a query string
    /// (`?replicaSet=rs0&authSource=admin&tls=true`) or a full URI override
    /// (`mongodb+srv://…`). Consumed by the Mongo driver; ignored by others.
    #[serde(default)]
    pub params: Option<String>,
    /// Starred connection: the UI shows a badge and floats it to the top.
    #[serde(default)]
    pub favorite: bool,
    /// Explicit sort key for the connection list. Default `0` falls back to
    /// insertion order; drag-reorder rewrites it.
    #[serde(default)]
    pub order: i64,
    /// Environment classification (local/dev/staging/testing/production),
    /// rendered as a colored pill. See `EnvTag`.
    #[serde(default)]
    pub env_tag: EnvTag,
    /// Whether to route this connection through an SSH tunnel.
    #[serde(default)]
    pub ssh_enabled: bool,
    /// Hostname or IP of the SSH bastion.
    #[serde(default)]
    pub ssh_host: Option<String>,
    /// Port of the SSH bastion (default 22).
    #[serde(default)]
    pub ssh_port: Option<u16>,
    /// Username on the SSH bastion.
    #[serde(default)]
    pub ssh_user: Option<String>,
    /// Authentication mode for the SSH bastion.
    #[serde(default)]
    pub ssh_auth_mode: SshAuthMode,
    /// Path to the SSH private key file (when auth_mode is KeyFile).
    #[serde(default)]
    pub ssh_key_path: Option<String>,
}

impl SavedConnection {
    /// Build a fresh record with a random v4 uuid id and no keyref yet.
    pub fn new(
        name: impl Into<String>,
        engine: Engine,
        host: impl Into<String>,
        port: u16,
        user: impl Into<String>,
    ) -> Self {
        SavedConnection {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.into(),
            engine,
            host: host.into(),
            port,
            user: user.into(),
            database: None,
            sslmode: SslMode::default(),
            color: None,
            group: None,
            local: false,
            tags: Vec::new(),
            params: None,
            favorite: false,
            order: 0,
            env_tag: EnvTag::None,
            ssh_enabled: false,
            ssh_host: None,
            ssh_port: None,
            ssh_user: None,
            ssh_auth_mode: SshAuthMode::default(),
            ssh_key_path: None,
        }
    }

    /// Rebuild a `rdb-core::ConnConfig`, injecting the password fetched from the
    /// secret backend. The password is the only secret that ever lives in memory.
    pub fn to_conn_config(&self, password: Option<String>) -> ConnConfig {
        self.to_conn_config_with_ssh(password, None)
    }

    /// Rebuild a `rdb-core::ConnConfig`, injecting both the DB password and SSH secret
    /// (SSH password or key passphrase) fetched from the secret backend.
    pub fn to_conn_config_with_ssh(
        &self,
        password: Option<String>,
        ssh_secret: Option<String>,
    ) -> ConnConfig {
        let ssh = if self.ssh_enabled {
            if let (Some(host), Some(user)) = (&self.ssh_host, &self.ssh_user) {
                let (ssh_pw, ssh_passphrase) = match self.ssh_auth_mode {
                    SshAuthMode::Password => (ssh_secret, None),
                    SshAuthMode::KeyFile => (None, ssh_secret),
                    SshAuthMode::Agent => (None, None),
                };
                Some(SshTunnelConfig {
                    host: host.clone(),
                    port: self.ssh_port.unwrap_or(22),
                    user: user.clone(),
                    auth_mode: self.ssh_auth_mode,
                    key_path: self.ssh_key_path.clone(),
                    password: ssh_pw,
                    passphrase: ssh_passphrase,
                })
            } else {
                None
            }
        } else {
            None
        };

        ConnConfig {
            host: self.host.clone(),
            port: self.port,
            user: self.user.clone(),
            database: self.database.clone(),
            password,
            sslmode: self.sslmode,
            params: self.params.clone(),
            ssh,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> SavedConnection {
        SavedConnection {
            id: "fixed-id".into(),
            name: "Local PG".into(),
            engine: Engine::Postgres,
            host: "localhost".into(),
            port: 5432,
            user: "postgres".into(),
            database: Some("app".into()),
            sslmode: SslMode::Require,
            color: Some("#3b82f6".into()),
            group: Some("Local".into()),
            local: false,
            tags: Vec::new(),
            params: None,
            favorite: false,
            order: 0,
            env_tag: EnvTag::Production,
            ssh_enabled: false,
            ssh_host: None,
            ssh_port: None,
            ssh_user: None,
            ssh_auth_mode: SshAuthMode::Agent,
            ssh_key_path: None,
        }
    }

    #[test]
    fn json_roundtrips() {
        let conn = sample();
        let json = serde_json::to_string(&conn).unwrap();
        let back: SavedConnection = serde_json::from_str(&json).unwrap();
        assert_eq!(back.id, "fixed-id");
        assert_eq!(back.engine, Engine::Postgres);
        assert_eq!(back.port, 5432);
        assert_eq!(back.sslmode, SslMode::Require);
        assert_eq!(back.env_tag, EnvTag::Production);
    }

    #[test]
    fn missing_env_tag_key_defaults_to_none() {
        // Pre-existing connections.json files predate this field entirely.
        let json = r#"{
            "id": "old-id", "name": "Old", "engine": "Postgres",
            "host": "localhost", "port": 5432, "user": "postgres",
            "database": null
        }"#;
        let conn: SavedConnection = serde_json::from_str(json).unwrap();
        assert_eq!(conn.env_tag, EnvTag::None);
    }

    #[test]
    fn json_never_contains_a_password_field() {
        // SavedConnection has no password field at all; assert it cannot leak.
        let json = serde_json::to_string(&sample()).unwrap();
        assert!(!json.contains("password"));
    }

    #[test]
    fn to_conn_config_injects_password() {
        let cfg = sample().to_conn_config(Some("s3cret".into()));
        assert_eq!(cfg.host, "localhost");
        assert_eq!(cfg.port, 5432);
        assert_eq!(cfg.user, "postgres");
        assert_eq!(cfg.database.as_deref(), Some("app"));
        assert_eq!(cfg.sslmode, SslMode::Require);
        assert_eq!(cfg.password.as_deref(), Some("s3cret"));
    }

    #[test]
    fn to_conn_config_without_password_is_none() {
        let cfg = sample().to_conn_config(None);
        assert!(cfg.password.is_none());
        assert!(cfg.ssh.is_none());
    }

    #[test]
    fn to_conn_config_with_ssh_injects_ssh_config_and_secrets() {
        let mut conn = sample();
        conn.ssh_enabled = true;
        conn.ssh_host = Some("ssh.bastion.net".into());
        conn.ssh_port = Some(2222);
        conn.ssh_user = Some("jumpadmin".into());
        conn.ssh_auth_mode = SshAuthMode::KeyFile;
        conn.ssh_key_path = Some("~/.ssh/id_rsa".into());

        let cfg = conn.to_conn_config_with_ssh(Some("dbpass".into()), Some("keypass".into()));
        assert_eq!(cfg.password.as_deref(), Some("dbpass"));
        let ssh = cfg.ssh.expect("ssh config should be present");
        assert_eq!(ssh.host, "ssh.bastion.net");
        assert_eq!(ssh.port, 2222);
        assert_eq!(ssh.user, "jumpadmin");
        assert_eq!(ssh.auth_mode, SshAuthMode::KeyFile);
        assert_eq!(ssh.key_path.as_deref(), Some("~/.ssh/id_rsa"));
        assert_eq!(ssh.passphrase.as_deref(), Some("keypass"));
        assert!(ssh.password.is_none());
    }

    #[test]
    fn engine_language_covers_every_variant() {
        assert_eq!(Engine::Postgres.language(), QueryLanguage::Sql);
        assert_eq!(Engine::MySql.language(), QueryLanguage::Sql);
        assert_eq!(Engine::Sqlite.language(), QueryLanguage::Sql);
        assert_eq!(Engine::Cassandra.language(), QueryLanguage::Cql);
        assert_eq!(Engine::Redis.language(), QueryLanguage::Command);
        assert_eq!(Engine::Mongo.language(), QueryLanguage::Mongo);
        assert_eq!(Engine::Mssql.language(), QueryLanguage::Sql);
        assert_eq!(Engine::Clickhouse.language(), QueryLanguage::Sql);
    }

    /// The lookup panics if a variant has no row, so this is the guard that
    /// makes adding an `Engine` without an `ENGINES` entry a test failure
    /// rather than a runtime panic. The `match` is exhaustive on purpose: a
    /// new variant stops compiling here until it is listed.
    #[test]
    fn every_engine_has_a_row() {
        let all = [
            Engine::Postgres,
            Engine::MySql,
            Engine::Redis,
            Engine::Mongo,
            Engine::Sqlite,
            Engine::Cassandra,
            Engine::Mssql,
            Engine::Clickhouse,
            Engine::MariaDb,
            Engine::Valkey,
        ];
        for e in all {
            let m = e.meta();
            assert_eq!(m.engine, e);
            assert!(!m.display.is_empty());
            assert!(!m.key.is_empty());
            assert!(!m.scheme.is_empty());
            assert_eq!(Engine::from_display(m.display), Some(e));
            assert_eq!(Engine::from_key(m.key), Some(e));
        }
        assert_eq!(all.len(), ENGINES.len(), "ENGINES has an unlisted row");
        assert_eq!(Engine::from_key(""), None);
        assert_eq!(Engine::from_key("nope"), None);
        for e in all {
            // Keeps the array above honest against the enum: a new variant
            // fails to compile here until it is added to `all`.
            let _: () = match e {
                Engine::Postgres
                | Engine::MySql
                | Engine::Redis
                | Engine::Mongo
                | Engine::Sqlite
                | Engine::Cassandra
                | Engine::Mssql
                | Engine::Clickhouse
                | Engine::MariaDb
                | Engine::Valkey => (),
            };
        }
    }

    #[test]
    fn unknown_display_label_resolves_to_none() {
        assert_eq!(Engine::from_display("Postgres"), None);
        assert_eq!(Engine::from_display(""), None);
        assert_eq!(Engine::from_display("SQL Server"), Some(Engine::Mssql));
    }
}
