use rdb_core::conn::{client_id, ConnConfig, SslMode, CONNECT_TIMEOUT, KEEPALIVE_INTERVAL};

/// Map our `SslMode` to a libpq `sslmode` token.
///
/// MVP TLS limitation: `connect()` always uses `NoTls`, so `Require` is
/// advisory only — it is written into the conn string but a `NoTls`
/// connection cannot actually negotiate TLS. Real `Require` enforcement needs
/// `tokio-postgres-rustls` and is a documented follow-up (see driver.rs).
fn sslmode_token(mode: SslMode) -> &'static str {
    match mode {
        SslMode::Disable => "disable",
        SslMode::Prefer => "prefer",
        SslMode::Require => "require",
    }
}

/// Build a libpq-style `key=value` connection string from `ConnConfig`.
/// `dbname` and `password` are only emitted when present.
pub fn build_conn_string(cfg: &ConnConfig) -> String {
    let mut parts = vec![
        format!("host={}", cfg.host),
        format!("port={}", cfg.port),
        format!("user={}", cfg.user),
    ];
    if let Some(db) = &cfg.database {
        parts.push(format!("dbname={db}"));
    }
    if let Some(pw) = &cfg.password {
        parts.push(format!("password={pw}"));
    }
    parts.push(format!("sslmode={}", sslmode_token(cfg.sslmode)));
    // Bound the handshake, and keep probing an idle socket so a silently
    // dropped link surfaces as an error instead of parking a query forever.
    // Names RDB in pg_stat_activity. Spaces are legal inside a libpq value
    // only when quoted, and the identity contains one ("RDB 0.40.0").
    parts.push(format!("application_name='{}'", client_id()));
    parts.push(format!("connect_timeout={}", CONNECT_TIMEOUT.as_secs()));
    parts.push("keepalives=1".to_string());
    parts.push(format!("keepalives_idle={}", KEEPALIVE_INTERVAL.as_secs()));
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdb_core::conn::{ConnConfig, SslMode};

    fn base() -> ConnConfig {
        ConnConfig {
            host: "localhost".into(),
            port: 5432,
            user: "postgres".into(),
            database: Some("app".into()),
            password: Some("secret".into()),
            sslmode: SslMode::Prefer,
            params: None,
            ssh: None,
        }
    }

    #[test]
    fn builds_full_conn_string() {
        let s = build_conn_string(&base());
        assert!(s.starts_with(
            "host=localhost port=5432 user=postgres dbname=app password=secret sslmode=prefer"
        ));
    }

    #[test]
    fn always_bounds_connect_and_enables_keepalive() {
        // Without these a silently dropped link parks a query forever.
        let s = build_conn_string(&base());
        assert!(s.contains(&format!("connect_timeout={}", CONNECT_TIMEOUT.as_secs())));
        assert!(s.contains("keepalives=1"));
        assert!(s.contains(&format!("keepalives_idle={}", KEEPALIVE_INTERVAL.as_secs())));
    }

    #[test]
    fn omits_dbname_when_absent() {
        let mut cfg = base();
        cfg.database = None;
        let s = build_conn_string(&cfg);
        assert!(!s.contains("dbname="));
        assert!(s.contains("host=localhost"));
    }

    #[test]
    fn omits_password_when_absent() {
        let mut cfg = base();
        cfg.password = None;
        let s = build_conn_string(&cfg);
        assert!(!s.contains("password="));
    }

    #[test]
    fn maps_sslmode_to_libpq_token() {
        let mut cfg = base();
        cfg.sslmode = SslMode::Disable;
        assert!(build_conn_string(&cfg).contains("sslmode=disable"));
        cfg.sslmode = SslMode::Require;
        assert!(build_conn_string(&cfg).contains("sslmode=require"));
    }
}
