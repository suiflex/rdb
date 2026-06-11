//! Real-engine test: spins a Redis container, then exercises the driver.
//! Requires Docker. Ignored by default; run with
//! `cargo test -p dbm-driver-redis -- --ignored`.

use dbm_core::conn::{ConnConfig, SslMode};
use dbm_core::driver::Driver;
use dbm_core::query::Query;
use dbm_core::result::{RedisValue, ResultSet};
use dbm_core::schema::ContainerKind;
use dbm_driver_redis::RedisDriver;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::redis::Redis;

#[tokio::test]
#[ignore = "requires docker"]
async fn connect_command_schema_against_real_redis() {
    let container = Redis::default().start().await.unwrap();
    let host = container.get_host().await.unwrap().to_string();
    let port = container.get_host_port_ipv4(6379).await.unwrap();

    let cfg = ConnConfig {
        host,
        port,
        user: "default".into(),
        database: Some("0".into()),
        password: None,
        sslmode: SslMode::Disable,
    };

    let driver = RedisDriver::connect(&cfg).await.unwrap();
    driver.ping().await.unwrap();

    driver
        .query(&Query::Command(vec![
            "SET".into(),
            "greeting".into(),
            "hello".into(),
        ]))
        .await
        .unwrap();
    let got = driver
        .query(&Query::Command(vec!["GET".into(), "greeting".into()]))
        .await
        .unwrap();
    match got {
        ResultSet::KeyValue(pairs) => {
            assert_eq!(pairs.len(), 1);
            assert!(matches!(pairs[0].1, RedisValue::Str(ref s) if s == "hello"));
        }
        other => panic!("expected KeyValue, got {other:?}"),
    }

    let keys = driver
        .query(&Query::Command(vec!["KEYS".into(), "*".into()]))
        .await
        .unwrap();
    match keys {
        ResultSet::KeyValue(pairs) => {
            assert!(
                matches!(pairs[0].1, RedisValue::List(ref l) if l.contains(&"greeting".to_string()))
            );
        }
        other => panic!("expected KeyValue list, got {other:?}"),
    }

    let schema = driver.schema().await.unwrap();
    assert_eq!(schema.databases.len(), 1);
    assert_eq!(
        schema.databases[0].containers[0].kind,
        ContainerKind::Keyspace
    );

    assert!(driver.query(&Query::Sql("SELECT 1".into())).await.is_err());

    driver.close().await.unwrap();
}
