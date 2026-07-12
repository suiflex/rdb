//! SQLite driver end-to-end. No Docker: uses a temp file, so this runs in
//! normal `cargo test` (unlike the network-engine drivers).

use rdbs_core::conn::{ConnConfig, SslMode};
use rdbs_core::driver::Driver;
use rdbs_core::query::Query;
use rdbs_core::result::{Cell, ResultSet};
use rdbs_core::write::{TableRef, WriteOp};
use rdbs_driver_sqlite::SqliteDriver;

fn cfg(path: &str) -> ConnConfig {
    ConnConfig {
        host: String::new(),
        port: 0,
        user: String::new(),
        database: Some(path.to_string()),
        password: None,
        sslmode: SslMode::Disable,
        params: None,
    }
}

fn temp_db() -> String {
    let mut p = std::env::temp_dir();
    p.push(format!("rdbs-sqlite-test-{}.db", std::process::id()));
    let _ = std::fs::remove_file(&p);
    p.to_string_lossy().into_owned()
}

#[tokio::test]
async fn connect_query_schema_count_commit() {
    let path = temp_db();
    let driver = SqliteDriver::connect(&cfg(&path)).await.expect("connect");
    driver.ping().await.expect("ping");

    driver
        .query(&Query::Sql(
            "CREATE TABLE users (id INTEGER PRIMARY KEY, name TEXT, ok BOOLEAN)".into(),
        ))
        .await
        .expect("create");

    match driver
        .query(&Query::Sql(
            "INSERT INTO users (id, name, ok) VALUES (1, 'ada', 1)".into(),
        ))
        .await
        .expect("insert")
    {
        ResultSet::Affected(n) => assert_eq!(n, 1),
        other => panic!("expected Affected, got {other:?}"),
    }

    // schema exposes the table + its PK column
    let schema = driver.schema().await.expect("schema");
    let db = &schema.databases[0];
    let users = db
        .containers
        .iter()
        .find(|c| c.name == "users")
        .expect("users table");
    assert!(users.fields.iter().any(|f| f.name == "id"));
    assert_eq!(
        driver
            .primary_key(&TableRef::named("users"))
            .await
            .expect("pk"),
        vec!["id".to_string()]
    );

    // count
    assert_eq!(driver.count(&TableRef::named("users")).await.unwrap(), 1);

    // buffered write path: update the row, then read it back
    let applied = driver
        .commit(&[WriteOp::Update {
            table: TableRef::named("users"),
            pk: vec![("id".into(), Cell::Int(1))],
            changes: vec![("name".into(), Cell::Text("grace".into()))],
        }])
        .await
        .expect("commit");
    assert_eq!(applied, 1);

    match driver
        .query(&Query::Sql("SELECT name FROM users WHERE id = 1".into()))
        .await
        .unwrap()
    {
        ResultSet::Tabular { rows, .. } => assert_eq!(rows[0][0].render(), "grace"),
        other => panic!("expected Tabular, got {other:?}"),
    }

    driver.close().await.expect("close");
    let _ = std::fs::remove_file(&path);
}

#[tokio::test]
async fn rejects_non_sql_query() {
    let driver = SqliteDriver::connect(&cfg(":memory:"))
        .await
        .expect("connect");
    assert!(driver
        .query(&Query::Command(vec!["GET".into()]))
        .await
        .is_err());
}
