use rdbs_core::conn::{ConnConfig, SslMode};
use rdbs_core::result::{RedisValue, ResultSet};
use redis::Value;

/// Build a `redis://[:password@]host:port[/db]` URL from connection config.
/// Redis auth historically has no username, so only the password is included.
///
/// TLS: `Prefer`/`Require` use the `rediss://` scheme with a `#insecure`
/// fragment so redis-rs negotiates TLS without verifying the server cert
/// (matching the other drivers' "encrypt, don't validate" posture). `Disable`
/// uses plaintext `redis://`.
pub fn connection_url(cfg: &ConnConfig) -> String {
    let auth = match &cfg.password {
        Some(pw) if !pw.is_empty() => format!(":{pw}@"),
        _ => String::new(),
    };
    let db = match &cfg.database {
        Some(d) if !d.is_empty() => format!("/{d}"),
        _ => String::new(),
    };
    let (scheme, frag) = match cfg.sslmode {
        SslMode::Disable => ("redis", ""),
        SslMode::Prefer | SslMode::Require => ("rediss", "#insecure"),
    };
    format!("{scheme}://{auth}{}:{}{db}{frag}", cfg.host, cfg.port)
}

/// Render a single `redis::Value` (the reply to one command) into a
/// `KeyValue` result. `label` is the key shown for the reply (we use the
/// command name). Scalars become one entry; bulk arrays become one `List`
/// entry; nested/other shapes are flattened to their debug string.
pub fn value_to_resultset(label: String, value: Value) -> ResultSet {
    ResultSet::KeyValue(vec![(label, value_to_redis(value))])
}

fn value_to_redis(value: Value) -> RedisValue {
    match value {
        Value::Nil => RedisValue::Nil,
        Value::Int(i) => RedisValue::Int(i),
        Value::SimpleString(s) => RedisValue::Str(s),
        Value::BulkString(bytes) => RedisValue::Str(String::from_utf8_lossy(&bytes).into_owned()),
        Value::Array(items) => RedisValue::List(items.into_iter().map(scalar_to_string).collect()),
        Value::Map(pairs) => RedisValue::List(
            pairs
                .into_iter()
                .flat_map(|(k, v)| [scalar_to_string(k), scalar_to_string(v)])
                .collect(),
        ),
        Value::Okay => RedisValue::Str("OK".to_string()),
        other => RedisValue::Str(format!("{other:?}")),
    }
}

/// Rows for a list/set browse: index → member. Presented as key/value where the
/// key column is the position and the value is the member.
pub fn pairs_from_members(members: Vec<String>) -> Vec<(String, RedisValue)> {
    pairs_from_members_at(members, 0)
}

/// Like [`pairs_from_members`] but numbering from `start`, so a windowed
/// LRANGE page still shows (and edits by) absolute list indices.
pub fn pairs_from_members_at(members: Vec<String>, start: usize) -> Vec<(String, RedisValue)> {
    members
        .into_iter()
        .enumerate()
        .map(|(i, m)| ((start + i).to_string(), RedisValue::Str(m)))
        .collect()
}

/// Rows for a hash (HGETALL) or sorted-set (ZRANGE WITHSCORES) browse, both of
/// which reply as a flat `[k, v, k, v, …]` array: field/member → value/score.
pub fn pairs_from_flat(flat: Vec<String>) -> Vec<(String, RedisValue)> {
    flat.chunks(2)
        .map(|c| {
            (
                c[0].clone(),
                RedisValue::Str(c.get(1).cloned().unwrap_or_default()),
            )
        })
        .collect()
}

/// Flatten one element of a bulk reply to a display string.
fn scalar_to_string(value: Value) -> String {
    match value {
        Value::Nil => "(nil)".to_string(),
        Value::Int(i) => i.to_string(),
        Value::SimpleString(s) => s,
        Value::BulkString(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Value::Okay => "OK".to_string(),
        other => format!("{other:?}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdbs_core::conn::{ConnConfig, SslMode};
    use rdbs_core::result::{RedisValue, ResultSet};
    use redis::Value;

    fn cfg(pw: Option<&str>, db: Option<&str>) -> ConnConfig {
        ConnConfig {
            host: "localhost".into(),
            port: 6379,
            user: "default".into(),
            database: db.map(|s| s.to_string()),
            password: pw.map(|s| s.to_string()),
            sslmode: SslMode::Disable,
            params: None,
        }
    }

    #[test]
    fn url_without_password_or_db() {
        assert_eq!(connection_url(&cfg(None, None)), "redis://localhost:6379");
    }

    #[test]
    fn url_with_password_and_numeric_db() {
        assert_eq!(
            connection_url(&cfg(Some("s3cr3t"), Some("2"))),
            "redis://:s3cr3t@localhost:6379/2"
        );
    }

    #[test]
    fn simple_string_becomes_single_keyvalue_entry() {
        let label = "PING".to_string();
        let rs = value_to_resultset(label.clone(), Value::SimpleString("PONG".into()));
        match rs {
            ResultSet::KeyValue(pairs) => {
                assert_eq!(pairs.len(), 1);
                assert_eq!(pairs[0].0, "PING");
                assert!(matches!(pairs[0].1, RedisValue::Str(ref s) if s == "PONG"));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn integer_becomes_int_redis_value() {
        let rs = value_to_resultset("DBSIZE".into(), Value::Int(7));
        match rs {
            ResultSet::KeyValue(pairs) => {
                assert!(matches!(pairs[0].1, RedisValue::Int(7)));
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn nil_becomes_nil_redis_value() {
        let rs = value_to_resultset("GET".into(), Value::Nil);
        match rs {
            ResultSet::KeyValue(pairs) => assert!(matches!(pairs[0].1, RedisValue::Nil)),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn members_map_to_index_keyed_rows() {
        let rows = pairs_from_members(vec!["x".into(), "y".into()]);
        assert_eq!(rows[0].0, "0");
        assert!(matches!(rows[0].1, RedisValue::Str(ref s) if s == "x"));
        assert_eq!(rows[1].0, "1");
        assert!(matches!(rows[1].1, RedisValue::Str(ref s) if s == "y"));
    }

    #[test]
    fn members_at_offset_use_absolute_indices() {
        let rows = pairs_from_members_at(vec!["x".into(), "y".into()], 300);
        assert_eq!(rows[0].0, "300");
        assert_eq!(rows[1].0, "301");
    }

    #[test]
    fn flat_pairs_zip_field_and_value() {
        let rows = pairs_from_flat(vec!["f1".into(), "v1".into(), "f2".into(), "v2".into()]);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].0, "f1");
        assert!(matches!(rows[0].1, RedisValue::Str(ref s) if s == "v1"));
        assert_eq!(rows[1].0, "f2");
        assert!(matches!(rows[1].1, RedisValue::Str(ref s) if s == "v2"));
    }

    #[test]
    fn flat_pairs_tolerates_odd_trailing_element() {
        let rows = pairs_from_flat(vec!["only".into()]);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "only");
        assert!(matches!(rows[0].1, RedisValue::Str(ref s) if s.is_empty()));
    }

    #[test]
    fn bulk_array_becomes_single_list_entry() {
        let arr = Value::Array(vec![
            Value::BulkString(b"a".to_vec()),
            Value::BulkString(b"b".to_vec()),
        ]);
        let rs = value_to_resultset("KEYS".into(), arr);
        match rs {
            ResultSet::KeyValue(pairs) => {
                assert_eq!(pairs.len(), 1);
                assert!(
                    matches!(pairs[0].1, RedisValue::List(ref l) if l == &vec!["a".to_string(), "b".to_string()])
                );
            }
            _ => panic!("wrong variant"),
        }
    }
}
