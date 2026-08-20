use serde::{Deserialize, Serialize};

/// TLS negotiation mode for a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SslMode {
    Disable,
    #[default]
    Prefer,
    Require,
}

/// Authentication mode for SSH tunnel bastion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum SshAuthMode {
    #[default]
    Agent,
    KeyFile,
    Password,
}

impl SshAuthMode {
    pub fn as_str(self) -> &'static str {
        match self {
            SshAuthMode::Agent => "Agent",
            SshAuthMode::KeyFile => "KeyFile",
            SshAuthMode::Password => "Password",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "KeyFile" | "key_file" | "key" => SshAuthMode::KeyFile,
            "Password" | "password" => SshAuthMode::Password,
            _ => SshAuthMode::Agent,
        }
    }
}

/// SSH tunnel configuration for routing a connection through a jump host / bastion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SshTunnelConfig {
    pub host: String,
    #[serde(default = "default_ssh_port")]
    pub port: u16,
    pub user: String,
    #[serde(default)]
    pub auth_mode: SshAuthMode,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub key_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub password: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub passphrase: Option<String>,
}

fn default_ssh_port() -> u16 {
    22
}

/// Process-wide client identity, e.g. `"RDB 0.40.0"`.
///
/// Drivers send this to the server on connect so RDB shows up by name in
/// `pg_stat_activity`, `SHOW PROCESSLIST`, `currentOp` and friends — the way
/// DBeaver and TablePlus do — instead of appearing as an anonymous client
/// nobody can attribute when a query misbehaves.
///
/// ponytail: a write-once global rather than a `ConnConfig` field. The identity
/// really is process-wide (one app, one name), and threading it through
/// `ConnConfig` would touch every construction site and every driver's test
/// fixtures to carry a value none of them vary. Make it a field if a
/// connection ever needs its own name.
static CLIENT_ID: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Register the client identity. Called once by the app at startup; later calls
/// are ignored, so a driver can never race a half-built name onto the wire.
pub fn set_client_id(id: impl Into<String>) {
    let _ = CLIENT_ID.set(id.into());
}

/// The registered client identity, or a bare `"RDB"` when nothing registered
/// one — which is the case for the driver crates' own tests.
pub fn client_id() -> &'static str {
    CLIENT_ID.get().map(String::as_str).unwrap_or("RDB")
}

/// How long a driver waits for a TCP connect / handshake before giving up.
///
/// Deliberately shorter than the app-level connect timeout that wraps it, so
/// the driver's own error (which names the engine and the failure) wins the
/// race against the generic "connection timed out" the UI would otherwise
/// report.
pub const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// TCP keepalive probe interval for long-lived connections.
///
/// This is what stops a query from parking forever when the network drops
/// silently — a NAT/VPN/firewall idle reap leaves the socket looking healthy to
/// the client, so without keepalive probes a read on a dead connection never
/// returns and the query task hangs with no way back except a hard abort.
pub const KEEPALIVE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(30);

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
    /// Optional SSH tunnel configuration to route through an SSH bastion.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ssh: Option<SshTunnelConfig>,
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
            ssh: None,
        };
        let json = serde_json::to_string(&cfg).unwrap();
        assert!(json.contains("\"host\":\"localhost\""));
        let back: ConnConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.port, 5432);
        assert_eq!(back.sslmode, SslMode::Require);
        assert!(back.ssh.is_none());
    }

    #[test]
    fn conn_config_roundtrips_with_ssh() {
        let cfg = ConnConfig {
            host: "db.internal".into(),
            port: 5432,
            user: "postgres".into(),
            database: Some("app".into()),
            password: None,
            sslmode: SslMode::Require,
            params: None,
            ssh: Some(SshTunnelConfig {
                host: "bastion.example.com".into(),
                port: 22,
                user: "jumpuser".into(),
                auth_mode: SshAuthMode::KeyFile,
                key_path: Some("~/.ssh/id_ed25519".into()),
                password: None,
                passphrase: None,
            }),
        };
        let json = serde_json::to_string(&cfg).unwrap();
        let back: ConnConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(back.host, "db.internal");
        let ssh = back.ssh.unwrap();
        assert_eq!(ssh.host, "bastion.example.com");
        assert_eq!(ssh.port, 22);
        assert_eq!(ssh.auth_mode, SshAuthMode::KeyFile);
        assert_eq!(ssh.key_path.as_deref(), Some("~/.ssh/id_ed25519"));
    }
}
