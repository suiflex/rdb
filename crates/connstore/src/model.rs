use dbm_core::conn::{ConnConfig, SslMode};
use serde::{Deserialize, Serialize};

/// Database engine of a saved connection. The MVP ships four; the variant set
/// grows as new driver crates are added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Engine {
    Postgres,
    MySql,
    Redis,
    Mongo,
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
        }
    }

    /// Rebuild a `dbm-core::ConnConfig`, injecting the password fetched from the
    /// secret backend. The password is the only secret that ever lives in memory.
    pub fn to_conn_config(&self, password: Option<String>) -> ConnConfig {
        ConnConfig {
            host: self.host.clone(),
            port: self.port,
            user: self.user.clone(),
            database: self.database.clone(),
            password,
            sslmode: self.sslmode,
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
    }
}
