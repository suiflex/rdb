use serde::{Deserialize, Serialize};

/// TLS negotiation mode for a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SslMode {
    Disable,
    #[default]
    Prefer,
    Require,
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
    /// Driver-specific connection options: either a query string
    /// (`?replicaSet=rs0&authSource=admin&tls=true`) or a full URI override
    /// (`mongodb+srv://…`). Currently consumed by the Mongo driver; other
    /// drivers ignore it.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub params: Option<String>,
}

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
            params: None,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("\"host\":\"localhost\""));
        let back: ConnConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.port, 5432);
        assert_eq!(back.sslmode, SslMode::Require);
    }
}
