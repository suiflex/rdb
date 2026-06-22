use rdbs_core::conn::{ConnConfig, SslMode};

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
    parts.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdbs_core::conn::{ConnConfig, SslMode};

    fn base() -> ConnConfig {
        ConnConfig {
            host: "localhost".into(),
            port: 5432,
            user: "postgres".into(),
            database: Some("app".into()),
            password: Some("secret".into()),
            sslmode: SslMode::Prefer,
        }
    }

    #[test]
    fn builds_full_conn_string() {
        let s = build_conn_string(&base());
        assert_eq!(
            s,
            "host=localhost port=5432 user=postgres dbname=app password=secret sslmode=prefer"
        );
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
