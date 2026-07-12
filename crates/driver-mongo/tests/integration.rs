//! Real-engine test: spins a MongoDB container, then exercises the driver.
//! Requires Docker. Ignored by default; run with
//! `cargo test -p rdbs-driver-mongo -- --ignored`.

use rdbs_core::conn::{ConnConfig, SslMode};
use rdbs_core::driver::Driver;
use rdbs_core::query::{MongoKind, MongoOp, Query};
use rdbs_core::result::ResultSet;
use rdbs_driver_mongo::MongoDriver;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::mongo::Mongo;

#[tokio::test]
#[ignore = "requires docker"]
async fn connect_insert_find_aggregate_schema_against_real_mongo() {
    let container = Mongo::default().start().await.unwrap();
    let host = container.get_host().await.unwrap().to_string();
    let port = container.get_host_port_ipv4(27017).await.unwrap();

    let cfg = ConnConfig {
        host,
        port,
        user: String::new(),
        database: Some("appdb".into()),
        password: None,
        sslmode: SslMode::Disable,
        params: None,
    };

    let driver = MongoDriver::connect(&cfg).await.unwrap();
    driver.ping().await.unwrap();

    for (name, age) in [("alice", 30), ("bob", 17)] {
        let inserted = driver
            .query(&Query::Mongo(MongoOp {
                collection: "users".into(),
                database: None,
                limit: None,
                skip: None,
                kind: MongoKind::Insert(serde_json::json!({ "name": name, "age": age })),
            }))
            .await
            .unwrap();
        assert!(matches!(inserted, ResultSet::Affected(1)));
    }

    let found = driver
        .query(&Query::Mongo(MongoOp {
            collection: "users".into(),
            database: None,
            limit: None,
            skip: None,
            kind: MongoKind::Find(serde_json::json!({ "age": { "$gte": 18 } })),
        }))
        .await
        .unwrap();
    match found {
        ResultSet::Documents(docs) => {
            assert_eq!(docs.len(), 1);
            assert_eq!(docs[0]["name"], serde_json::json!("alice"));
        }
        other => panic!("expected Documents, got {other:?}"),
    }

    let agg = driver
        .query(&Query::Mongo(MongoOp {
            collection: "users".into(),
            database: None,
            limit: None,
            skip: None,
            kind: MongoKind::Aggregate(vec![
                serde_json::json!({ "$group": { "_id": null, "total": { "$sum": 1 } } }),
            ]),
        }))
        .await
        .unwrap();
    match agg {
        ResultSet::Documents(docs) => {
            assert_eq!(docs.len(), 1);
            assert_eq!(docs[0]["total"], serde_json::json!(2));
        }
        other => panic!("expected Documents, got {other:?}"),
    }

    // schema() is lazy: databases only, no eager collection listing.
    let schema = driver.schema().await.unwrap();
    assert!(
        schema.databases.iter().any(|d| d.name == "appdb"),
        "schema should list the appdb database"
    );
    // Collections are fetched per database on demand.
    let containers = driver.containers("appdb").await.unwrap();
    assert!(
        containers.iter().any(|c| c.name == "users"),
        "appdb should contain the users collection"
    );

    assert!(driver.query(&Query::Sql("SELECT 1".into())).await.is_err());

    driver.close().await.unwrap();
}
