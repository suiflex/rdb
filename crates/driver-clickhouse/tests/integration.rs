//! Real-engine test: spins a ClickHouse container, then exercises the
//! driver. Requires a running Docker daemon. Ignored by default so plain
//! `cargo test` stays offline; run with
//! `cargo test -p rdb-driver-clickhouse -- --ignored`.

use rdb_core::conn::{ConnConfig, SslMode};
use rdb_core::driver::Driver;
use rdb_core::query::Query;
use rdb_core::result::{Cell, ResultSet};
use rdb_core::write::{TableRef, WriteOp};
use rdb_driver_clickhouse::ClickhouseDriver;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::clickhouse::ClickHouse;

#[tokio::test]
#[ignore = "requires docker"]
async fn connect_query_schema_commit_against_real_clickhouse() {
    let container = ClickHouse::default().start().await.unwrap();
    let host = container.get_host().await.unwrap().to_string();
    let port = container.get_host_port_ipv4(8123).await.unwrap();

    // The image sets no auth env vars: default user, no password, default db.
    let cfg = ConnConfig {
        host,
        port,
        user: "default".into(),
        database: Some("default".into()),
        password: None,
        sslmode: SslMode::Disable,
        params: None,
        ssh: None,
    };

    let driver = ClickhouseDriver::connect(&cfg).await.unwrap();
    driver.ping().await.unwrap();

    driver
        .query(&Query::Sql(
            "CREATE TABLE users (id UInt32, name String, score Float64) \
             ENGINE = MergeTree() ORDER BY id"
                .into(),
        ))
        .await
        .unwrap();
    driver
        .query(&Query::Sql(
            "INSERT INTO users (id, name, score) VALUES (1, 'alice', 9.5)".into(),
        ))
        .await
        .unwrap();

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
            assert_eq!(rows.len(), 1);
            // UInt32 fits a JS-safe integer, so ClickHouse's JSON format
            // renders it as a number, not a string (unlike UInt64/Int64).
            assert!(matches!(rows[0][0], Cell::Int(1)));
            assert!(matches!(rows[0][1], Cell::Text(ref s) if s == "alice"));
            assert!(matches!(rows[0][2], Cell::Float(_)));
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

    let n = driver
        .count(&TableRef {
            database: Some("default".into()),
            schema: None,
            name: "users".into(),
        })
        .await
        .unwrap();
    assert_eq!(n, 1);

    let affected = driver
        .commit(&[WriteOp::Insert {
            table: TableRef {
                database: Some("default".into()),
                schema: None,
                name: "users".into(),
            },
            values: vec![
                ("id".into(), Cell::Int(2)),
                ("name".into(), Cell::Text("bob".into())),
                ("score".into(), Cell::Float(1.0)),
            ],
        }])
        .await
        .unwrap();
    assert_eq!(affected, 1);

    // Update/Delete are rejected outright — no row-level mutation support.
    assert!(driver
        .commit(&[WriteOp::Delete {
            table: TableRef {
                database: Some("default".into()),
                schema: None,
                name: "users".into(),
            },
            pk: vec![("id".into(), Cell::Int(1))],
        }])
        .await
        .is_err());

    assert!(driver
        .query(&Query::Command(vec!["GET".into(), "k".into()]))
        .await
        .is_err());

    driver.close().await.unwrap();
}
