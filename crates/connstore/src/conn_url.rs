//! Connection-URL import: parse a DB URL string (TablePlus-style paste) into
//! the optional fields the connection form expects. Pure, dependency-free
//! hand-parser — no `url` crate so we stay light.
//!
//! Grammar handled:
//!   scheme://[user[:password]@]host[:port][/database][?query]
//!
//! `sslmode` is pulled from the query string if present. Percent-encoding in
//! the user/password is decoded. Missing pieces are left `None`/empty so the
//! UI can fall back to its per-engine defaults.

use crate::model::Engine;
use rdb_core::conn::SslMode;
use thiserror::Error;

/// Error returned when a connection URL cannot be parsed.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ConnUrlError {
    #[error("missing scheme (expected e.g. postgresql://...)")]
    MissingScheme,
    #[error("unknown scheme: {0}")]
    UnknownScheme(String),
    #[error("missing host")]
    MissingHost,
    #[error("invalid port: {0}")]
    InvalidPort(String),
    #[error("malformed URL")]
    Malformed,
}

/// The parsed pieces of a connection URL. Every field is optional except the
/// engine (scheme is mandatory). The UI fills the form from whatever is set
/// and keeps its defaults for the rest.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedUrl {
    pub engine: Option<Engine>,
    pub host: Option<String>,
    pub port: Option<u16>,
    pub user: Option<String>,
    pub password: Option<String>,
    pub database: Option<String>,
    pub sslmode: Option<SslMode>,
}

/// Map a URL scheme (lowercased, without `://`) to an [`Engine`].
fn scheme_to_engine(scheme: &str) -> Option<Engine> {
    match scheme {
        "postgres" | "postgresql" => Some(Engine::Postgres),
        "mysql" => Some(Engine::MySql),
        "mariadb" => Some(Engine::MariaDb),
        "redis" | "rediss" => Some(Engine::Redis),
        "valkey" => Some(Engine::Valkey),
        "mongodb" | "mongodb+srv" => Some(Engine::Mongo),
        "sqlite" | "file" => Some(Engine::Sqlite),
        "cassandra" | "cql" | "scylla" => Some(Engine::Cassandra),
        "mssql" | "sqlserver" => Some(Engine::Mssql),
        "oracle" => Some(Engine::Oracle),
        "clickhouse" | "clickhouses" => Some(Engine::Clickhouse),
        _ => None,
    }
}

/// Decode percent-encoding (`%XX`) and `+`-as-space in a URL component.
/// Invalid escapes are passed through verbatim rather than erroring.
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                if let (Some(hi), Some(lo)) = (hi, lo) {
                    out.push((hi * 16 + lo) as u8);
                    i += 3;
                } else {
                    out.push(bytes[i]);
                    i += 1;
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Map a query-string `sslmode` value to [`SslMode`]. Recognizes the common
/// libpq spellings; `require`/`verify-*` collapse to `Require`.
fn parse_sslmode(value: &str) -> Option<SslMode> {
    match value.to_ascii_lowercase().as_str() {
        "disable" | "disabled" | "false" | "0" => Some(SslMode::Disable),
        "prefer" | "allow" => Some(SslMode::Prefer),
        "require" | "required" | "verify-ca" | "verify-full" | "true" | "1" => {
            Some(SslMode::Require)
        }
        _ => None,
    }
}

/// Parse a database connection URL into [`ParsedUrl`].
///
/// Hand-rolled; no allocation-heavy URL crate. Returns an error for an empty
/// input, a missing/unknown scheme, a missing host, or an unparseable port.
pub fn parse_conn_url(url: &str) -> Result<ParsedUrl, ConnUrlError> {
    let url = url.trim();
    if url.is_empty() {
        return Err(ConnUrlError::MissingScheme);
    }

    // 1. scheme://
    let (scheme, rest) = url.split_once("://").ok_or(ConnUrlError::MissingScheme)?;
    let scheme = scheme.trim().to_ascii_lowercase();
    if scheme.is_empty() {
        return Err(ConnUrlError::MissingScheme);
    }
    let engine =
        scheme_to_engine(&scheme).ok_or_else(|| ConnUrlError::UnknownScheme(scheme.clone()))?;

    // 2. split off ?query
    let (authority_path, query) = match rest.split_once('?') {
        Some((ap, q)) => (ap, Some(q)),
        None => (rest, None),
    };

    // 3. split off /path (database) — first '/' after the authority.
    let (authority, database_raw) = match authority_path.split_once('/') {
        Some((a, d)) => (a, Some(d)),
        None => (authority_path, None),
    };

    // 4. split userinfo@hostport on the LAST '@' (so '@' in a password is ok
    //    only if percent-encoded; an unencoded '@' in the password splits early,
    //    which matches browser/libpq behavior of taking the last '@').
    let (userinfo, hostport) = match authority.rsplit_once('@') {
        Some((ui, hp)) => (Some(ui), hp),
        None => (None, authority),
    };

    // 5. user[:password]
    let (mut user, mut password) = (None, None);
    if let Some(ui) = userinfo {
        match ui.split_once(':') {
            Some((u, p)) => {
                if !u.is_empty() {
                    user = Some(percent_decode(u));
                }
                password = Some(percent_decode(p));
            }
            None => {
                if !ui.is_empty() {
                    user = Some(percent_decode(ui));
                }
            }
        }
    }

    // 6. host[:port] — host must not be empty.
    if hostport.is_empty() {
        return Err(ConnUrlError::MissingHost);
    }
    let (host, port) = match hostport.rsplit_once(':') {
        Some((h, p)) => {
            let parsed = p
                .parse::<u16>()
                .map_err(|_| ConnUrlError::InvalidPort(p.to_string()))?;
            (h, Some(parsed))
        }
        None => (hostport, None),
    };
    if host.is_empty() {
        return Err(ConnUrlError::MissingHost);
    }
    let host = Some(host.to_string());

    // 7. database (strip a trailing slash; treat empty as None).
    let database = database_raw.and_then(|d| {
        let d = d.trim_end_matches('/');
        if d.is_empty() {
            None
        } else {
            Some(percent_decode(d))
        }
    });

    // 8. sslmode from query string.
    let mut sslmode = None;
    if let Some(q) = query {
        for pair in q.split('&') {
            if let Some((k, v)) = pair.split_once('=') {
                if k.eq_ignore_ascii_case("sslmode") || k.eq_ignore_ascii_case("ssl") {
                    sslmode = parse_sslmode(v);
                }
            }
        }
    }
    // `rediss://` / `clickhouses://` implies TLS even without a query flag.
    if sslmode.is_none() && (scheme == "rediss" || scheme == "clickhouses") {
        sslmode = Some(SslMode::Require);
    }

    Ok(ParsedUrl {
        engine: Some(engine),
        host,
        port,
        user,
        password,
        database,
        sslmode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_postgres_url() {
        let p =
            parse_conn_url("postgresql://admin:s3cret@db.example.com:5432/appdb?sslmode=require")
                .unwrap();
        assert_eq!(p.engine, Some(Engine::Postgres));
        assert_eq!(p.host.as_deref(), Some("db.example.com"));
        assert_eq!(p.port, Some(5432));
        assert_eq!(p.user.as_deref(), Some("admin"));
        assert_eq!(p.password.as_deref(), Some("s3cret"));
        assert_eq!(p.database.as_deref(), Some("appdb"));
        assert_eq!(p.sslmode, Some(SslMode::Require));
    }

    #[test]
    fn postgres_short_scheme() {
        let p = parse_conn_url("postgres://localhost/mydb").unwrap();
        assert_eq!(p.engine, Some(Engine::Postgres));
        assert_eq!(p.host.as_deref(), Some("localhost"));
        assert_eq!(p.port, None);
        assert_eq!(p.database.as_deref(), Some("mydb"));
    }

    #[test]
    fn mysql_minimal() {
        let p = parse_conn_url("mysql://localhost/db").unwrap();
        assert_eq!(p.engine, Some(Engine::MySql));
        assert_eq!(p.host.as_deref(), Some("localhost"));
        assert_eq!(p.port, None);
        assert_eq!(p.user, None);
        assert_eq!(p.password, None);
        assert_eq!(p.database.as_deref(), Some("db"));
        assert_eq!(p.sslmode, None);
    }

    #[test]
    fn mariadb_scheme_maps_to_its_own_engine() {
        let p = parse_conn_url("mariadb://root@127.0.0.1:3307").unwrap();
        assert_eq!(p.engine, Some(Engine::MariaDb));
        assert_eq!(p.host.as_deref(), Some("127.0.0.1"));
        assert_eq!(p.port, Some(3307));
        assert_eq!(p.user.as_deref(), Some("root"));
    }

    #[test]
    fn valkey_scheme_maps_to_its_own_engine() {
        let p = parse_conn_url("valkey://:authpass@valkey.host:6379").unwrap();
        assert_eq!(p.engine, Some(Engine::Valkey));
        assert_eq!(p.host.as_deref(), Some("valkey.host"));
        assert_eq!(p.port, Some(6379));
    }

    #[test]
    fn redis_secure() {
        let p = parse_conn_url("rediss://:authpass@redis.host:6380").unwrap();
        assert_eq!(p.engine, Some(Engine::Redis));
        assert_eq!(p.host.as_deref(), Some("redis.host"));
        assert_eq!(p.port, Some(6380));
        assert_eq!(p.user, None);
        assert_eq!(p.password.as_deref(), Some("authpass"));
        // rediss implies TLS.
        assert_eq!(p.sslmode, Some(SslMode::Require));
    }

    #[test]
    fn mongo_srv() {
        let p = parse_conn_url("mongodb+srv://user:pw@cluster0.mongodb.net/test").unwrap();
        assert_eq!(p.engine, Some(Engine::Mongo));
        assert_eq!(p.host.as_deref(), Some("cluster0.mongodb.net"));
        assert_eq!(p.port, None);
        assert_eq!(p.user.as_deref(), Some("user"));
        assert_eq!(p.password.as_deref(), Some("pw"));
        assert_eq!(p.database.as_deref(), Some("test"));
    }

    #[test]
    fn percent_encoded_password() {
        // p@ss:word/ -> p%40ss%3Aword%2F
        let p = parse_conn_url("postgres://user:p%40ss%3Aword%2F@host:5432/db").unwrap();
        assert_eq!(p.user.as_deref(), Some("user"));
        assert_eq!(p.password.as_deref(), Some("p@ss:word/"));
    }

    #[test]
    fn mssql_scheme() {
        let p = parse_conn_url("sqlserver://sa:pw@localhost:1433/app").unwrap();
        assert_eq!(p.engine, Some(Engine::Mssql));
        assert_eq!(p.host.as_deref(), Some("localhost"));
        assert_eq!(p.port, Some(1433));
        assert_eq!(p.user.as_deref(), Some("sa"));
        assert_eq!(p.database.as_deref(), Some("app"));
    }

    #[test]
    fn clickhouse_scheme() {
        let p = parse_conn_url("clickhouse://default@localhost:8123/app").unwrap();
        assert_eq!(p.engine, Some(Engine::Clickhouse));
        assert_eq!(p.host.as_deref(), Some("localhost"));
        assert_eq!(p.port, Some(8123));
        assert_eq!(p.database.as_deref(), Some("app"));
    }

    #[test]
    fn clickhouses_scheme_implies_tls() {
        let p = parse_conn_url("clickhouses://host:8443").unwrap();
        assert_eq!(p.engine, Some(Engine::Clickhouse));
        assert_eq!(p.sslmode, Some(SslMode::Require));
    }

    #[test]
    fn unknown_scheme_errors() {
        let err = parse_conn_url("ftp://host/path").unwrap_err();
        assert_eq!(err, ConnUrlError::UnknownScheme("ftp".into()));
    }

    #[test]
    fn garbage_string_errors() {
        assert_eq!(
            parse_conn_url("not a url at all").unwrap_err(),
            ConnUrlError::MissingScheme
        );
        assert_eq!(parse_conn_url("").unwrap_err(), ConnUrlError::MissingScheme);
    }

    #[test]
    fn invalid_port_errors() {
        let err = parse_conn_url("postgres://host:notaport/db").unwrap_err();
        assert_eq!(err, ConnUrlError::InvalidPort("notaport".into()));
    }

    #[test]
    fn missing_host_errors() {
        assert_eq!(
            parse_conn_url("postgres:///db").unwrap_err(),
            ConnUrlError::MissingHost
        );
    }
}
