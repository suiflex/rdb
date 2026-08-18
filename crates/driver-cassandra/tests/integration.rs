//! Cassandra driver end-to-end. Needs a running cluster (like the Mongo
//! driver's tests), so it is `#[ignore]` by default. Run with:
//!
//!   docker run --rm -d -p 9042:9042 cassandra:5
//!   cargo test -p rdb-driver-cassandra -- --ignored

use rdb_core::conn::{ConnConfig, SslMode};
use rdb_core::driver::Driver;
use rdb_core::query::Query;
use rdb_core::result::{Cell, ResultSet};
use rdb_core::write::{TableRef, WriteOp};
use rdb_driver_cassandra::CassandraDriver;

fn cfg() -> ConnConfig {
    ConnConfig {
        host: "127.0.0.1".into(),
        port: 9042,
        user: String::new(),
        database: None,
        password: None,
        sslmode: SslMode::Disable,
        params: None,
        ssh: None,
    }
}

fn table() -> TableRef {
    TableRef {
        database: Some("rdb_it".into()),
        schema: None,
        name: "users".into(),
    }
}

#[tokio::test]
#[ignore = "requires a running Cassandra on 127.0.0.1:9042"]
async fn connect_schema_query_count_commit() {
    let driver = CassandraDriver::connect(&cfg()).await.expect("connect");
    driver.ping().await.expect("ping");

    driver
        .query(&Query::Cql(
            "CREATE KEYSPACE IF NOT EXISTS rdb_it WITH replication = \
             {'class':'SimpleStrategy','replication_factor':1}"
                .into(),
        ))
        .await
        .expect("create keyspace");
    driver
        .query(&Query::Cql(
            "CREATE TABLE IF NOT EXISTS rdb_it.users (id int PRIMARY KEY, name text)".into(),
        ))
        .await
        .expect("create table");
    driver
        .query(&Query::Cql(
            "INSERT INTO rdb_it.users (id, name) VALUES (1, 'ada')".into(),
        ))
        .await
        .expect("insert");

    // keyspace shows up + its table lists via lazy containers()
    assert!(driver
        .schema()
        .await
        .unwrap()
        .databases
        .iter()
        .any(|d| d.name == "rdb_it"));
    assert!(driver
        .containers("rdb_it")
        .await
        .unwrap()
        .iter()
        .any(|c| c.name == "users"));

    assert_eq!(driver.primary_key(&table()).await.unwrap(), vec!["id"]);
    assert_eq!(driver.count(&table()).await.unwrap(), 1);

    // buffered write path
    driver
        .commit(&[WriteOp::Update {
            table: table(),
            pk: vec![("id".into(), Cell::Int(1))],
            changes: vec![("name".into(), Cell::Text("grace".into()))],
        }])
        .await
        .expect("commit");

    match driver
        .query(&Query::Cql(
            "SELECT name FROM rdb_it.users WHERE id = 1".into(),
        ))
        .await
        .unwrap()
    {
        ResultSet::Tabular { rows, .. } => assert_eq!(rows[0][0].render(), "grace"),
        other => panic!("expected Tabular, got {other:?}"),
    }

    driver
        .query(&Query::Cql("DROP KEYSPACE rdb_it".into()))
        .await
        .expect("drop keyspace");
    driver.close().await.expect("close");
}
