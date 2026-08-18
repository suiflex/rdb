//! Real-engine test: spins a MySQL container, then exercises the driver.
//! Requires a running Docker daemon. Ignored by default so plain `cargo test`
//! stays offline; run with `cargo test -p rdb-driver-mysql -- --ignored`.

use rdb_core::conn::{ConnConfig, SslMode};
use rdb_core::driver::Driver;
use rdb_core::query::Query;
use rdb_core::result::{Cell, ResultSet};
use rdb_driver_mysql::MysqlDriver;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::mysql::Mysql;

#[tokio::test]
#[ignore = "requires docker"]
async fn connect_query_schema_against_real_mysql() {
    let container = Mysql::default().start().await.unwrap();
    let host = container.get_host().await.unwrap().to_string();
    let port = container.get_host_port_ipv4(3306).await.unwrap();

    // testcontainers-modules `Mysql` default: user `root`, no password, db `test`.
    let cfg = ConnConfig {
        host,
        port,
        user: "root".into(),
        database: Some("test".into()),
        password: None,
        sslmode: SslMode::Disable,
        params: None,
        ssh: None,
    };

    let driver = MysqlDriver::connect(&cfg).await.unwrap();
    driver.ping().await.unwrap();

    driver
        .query(&Query::Sql(
            "CREATE TABLE users (id INT PRIMARY KEY, name VARCHAR(50), score DOUBLE)".into(),
        ))
        .await
        .unwrap();
    let inserted = driver
        .query(&Query::Sql(
            "INSERT INTO users (id, name, score) VALUES (1, 'alice', 9.5), (2, NULL, 1.0)".into(),
        ))
        .await
        .unwrap();
    assert!(matches!(inserted, ResultSet::Affected(2)));

    let rs = driver
        .query(&Query::Sql(
            "SELECT id, name, score FROM users ORDER BY id".into(),
        ))
        .await
        .unwrap();
    match rs {
        ResultSet::Tabular { cols, rows } => {
            assert_eq!(cols.len(), 3);
            assert_eq!(cols[0].name, "id");
            assert_eq!(rows.len(), 2);
            assert!(matches!(rows[0][0], Cell::Int(1)));
            assert!(matches!(rows[0][1], Cell::Text(ref s) if s == "alice"));
            assert!(matches!(rows[0][2], Cell::Float(_)));
            assert!(matches!(rows[1][1], Cell::Null));
        }
        other => panic!("expected Tabular, got {other:?}"),
    }

    let schema = driver.schema().await.unwrap();
    let found = schema
        .databases
        .iter()
        .flat_map(|d| &d.containers)
        .any(|c| c.name == "users");
    assert!(found, "schema should contain the users table");

    assert!(driver
        .query(&Query::Command(vec!["GET".into(), "k".into()]))
        .await
        .is_err());

    driver.close().await.unwrap();
}
