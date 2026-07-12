//! Cassandra driver end-to-end. Needs a running cluster (like the Mongo
//! driver's tests), so it is `#[ignore]` by default. Run with:
//!
//!   docker run --rm -d -p 9042:9042 cassandra:5
//!   cargo test -p rdbs-driver-cassandra -- --ignored

use rdbs_core::conn::{ConnConfig, SslMode};
use rdbs_core::driver::Driver;
use rdbs_core::query::Query;
use rdbs_core::result::{Cell, ResultSet};
use rdbs_core::write::{TableRef, WriteOp};
use rdbs_driver_cassandra::CassandraDriver;

fn cfg() -> ConnConfig {
    ConnConfig {
        host: "127.0.0.1".into(),
        port: 9042,
        user: String::new(),
        database: None,
        password: None,
        sslmode: SslMode::Disable,
        params: None,
    }
}

fn table() -> TableRef {
    TableRef {
        database: Some("rdbs_it".into()),
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
        .query(&Query::Sql(
            "CREATE KEYSPACE IF NOT EXISTS rdbs_it WITH replication = \
             {'class':'SimpleStrategy','replication_factor':1}"
                .into(),
        ))
        .await
        .expect("create keyspace");
    driver
        .query(&Query::Sql(
            "CREATE TABLE IF NOT EXISTS rdbs_it.users (id int PRIMARY KEY, name text)".into(),
        ))
        .await
        .expect("create table");
    driver
        .query(&Query::Sql(
            "INSERT INTO rdbs_it.users (id, name) VALUES (1, 'ada')".into(),
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
        .any(|d| d.name == "rdbs_it"));
    assert!(driver
        .containers("rdbs_it")
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
        .query(&Query::Sql(
            "SELECT name FROM rdbs_it.users WHERE id = 1".into(),
        ))
        .await
        .unwrap()
    {
        ResultSet::Tabular { rows, .. } => assert_eq!(rows[0][0].render(), "grace"),
        other => panic!("expected Tabular, got {other:?}"),
    }

    driver
        .query(&Query::Sql("DROP KEYSPACE rdbs_it".into()))
        .await
        .expect("drop keyspace");
    driver.close().await.expect("close");
}
