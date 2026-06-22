// Integration tests run a REAL Postgres in Docker via testcontainers.
// They require a running Docker daemon. They are NOT #[ignore]-d: if Docker
// is present (CI, dev with Docker Desktop) they run; without Docker they fail
// fast at container start, which is the intended signal.

use rdbs_core::conn::{ConnConfig, SslMode};
use rdbs_core::driver::Driver;
use rdbs_driver_postgres::PostgresDriver;
use testcontainers::runners::AsyncRunner;
use testcontainers::ContainerAsync;
use testcontainers_modules::postgres::Postgres;

/// Start a fresh Postgres container and return it plus a `ConnConfig` pointed
/// at it. The container is held by the caller; dropping it stops the container.
async fn start_pg() -> (ContainerAsync<Postgres>, ConnConfig) {
    let container = Postgres::default()
        .start()
        .await
        .expect("start postgres container (is Docker running?)");
    let host = container.get_host().await.expect("host").to_string();
    let port = container
        .get_host_port_ipv4(5432)
        .await
        .expect("mapped port");
    let cfg = ConnConfig {
        host,
        port,
        user: "postgres".into(),
        database: Some("postgres".into()),
        password: Some("postgres".into()),
        sslmode: SslMode::Disable,
    };
    (container, cfg)
}

#[tokio::test]
async fn connect_ping_close() {
    let (_container, cfg) = start_pg().await;
    let driver = PostgresDriver::connect(&cfg).await.expect("connect");
    driver.ping().await.expect("ping");
    driver.close().await.expect("close");
}

#[tokio::test]
async fn select_returns_tabular_rows() {
    use rdbs_core::query::Query;
    use rdbs_core::result::{Cell, ResultSet};

    let (_container, cfg) = start_pg().await;
    let driver = PostgresDriver::connect(&cfg).await.expect("connect");

    // DDL + write return Affected; select returns Tabular.
    driver
        .query(&Query::Sql(
            "CREATE TABLE t (id INT4 PRIMARY KEY, name TEXT, ok BOOL)".into(),
        ))
        .await
        .expect("create table");

    let affected = driver
        .query(&Query::Sql(
            "INSERT INTO t (id, name, ok) VALUES (1, 'alice', true), (2, NULL, false)".into(),
        ))
        .await
        .expect("insert");
    assert!(matches!(affected, ResultSet::Affected(2)));

    let rs = driver
        .query(&Query::Sql("SELECT id, name, ok FROM t ORDER BY id".into()))
        .await
        .expect("select");

    match rs {
        ResultSet::Tabular { cols, rows } => {
            assert_eq!(cols.len(), 3);
            assert_eq!(cols[0].name, "id");
            assert_eq!(rows.len(), 2);
            assert!(matches!(rows[0][0], Cell::Int(1)));
            assert!(matches!(&rows[0][1], Cell::Text(s) if s == "alice"));
            assert!(matches!(rows[0][2], Cell::Bool(true)));
            assert!(matches!(rows[1][1], Cell::Null)); // NULL name
        }
        other => panic!("expected Tabular, got {other:?}"),
    }

    driver.close().await.expect("close");
}

#[tokio::test]
async fn non_sql_queries_are_unsupported() {
    use rdbs_core::error::RdbsError;
    use rdbs_core::query::{MongoKind, MongoOp, Query};

    let (_container, cfg) = start_pg().await;
    let driver = PostgresDriver::connect(&cfg).await.expect("connect");

    let cmd = driver
        .query(&Query::Command(vec!["GET".into(), "k".into()]))
        .await;
    assert!(matches!(cmd, Err(RdbsError::UnsupportedQuery)));

    let mongo = driver
        .query(&Query::Mongo(MongoOp {
            collection: "c".into(),
            kind: MongoKind::Find(serde_json::json!({})),
        }))
        .await;
    assert!(matches!(mongo, Err(RdbsError::UnsupportedQuery)));

    driver.close().await.expect("close");
}

#[tokio::test]
async fn schema_lists_created_table_and_fields() {
    use rdbs_core::query::Query;
    use rdbs_core::schema::ContainerKind;

    let (_container, cfg) = start_pg().await;
    let driver = PostgresDriver::connect(&cfg).await.expect("connect");

    driver
        .query(&Query::Sql(
            "CREATE TABLE widget (id INT4 PRIMARY KEY, label TEXT NOT NULL, note TEXT)".into(),
        ))
        .await
        .expect("create table");

    let schema = driver.schema().await.expect("schema");

    // One logical database ("postgres"); find our table within it.
    let db = schema
        .databases
        .iter()
        .find(|d| d.name == "postgres")
        .expect("postgres database present");
    let widget = db
        .containers
        .iter()
        .find(|c| c.name == "widget")
        .expect("widget table present");

    assert_eq!(widget.kind, ContainerKind::Table);

    let id = widget
        .fields
        .iter()
        .find(|f| f.name == "id")
        .expect("id field");
    assert!(!id.nullable);
    let label = widget
        .fields
        .iter()
        .find(|f| f.name == "label")
        .expect("label field");
    assert!(!label.nullable);
    let note = widget
        .fields
        .iter()
        .find(|f| f.name == "note")
        .expect("note field");
    assert!(note.nullable);

    driver.close().await.expect("close");
}
