//! Real-engine test: spins a SQL Server container, then exercises the
//! driver. Requires a running Docker daemon (and, on Apple silicon, Rosetta
//! emulation — SQL Server only ships amd64 images). Ignored by default so
//! plain `cargo test` stays offline; run with
//! `cargo test -p rdb-driver-mssql -- --ignored`.

use rdb_core::conn::{ConnConfig, SslMode};
use rdb_core::driver::Driver;
use rdb_core::query::Query;
use rdb_core::result::{Cell, ResultSet};
use rdb_driver_mssql::MssqlDriver;
use testcontainers::runners::AsyncRunner;
use testcontainers_modules::mssql_server::MssqlServer;

#[tokio::test]
#[ignore = "requires docker"]
async fn connect_query_schema_commit_against_real_mssql() {
    let container = MssqlServer::default()
        .with_accept_eula()
        .start()
        .await
        .unwrap();
    let host = container.get_host().await.unwrap().to_string();
    let port = container.get_host_port_ipv4(1433).await.unwrap();

    // No database created by the image beyond the system ones — connect to
    // the default `master` and work in `dbo` there, same as a fresh install.
    let cfg = ConnConfig {
        host,
        port,
        user: "sa".into(),
        database: None,
        password: Some(MssqlServer::DEFAULT_SA_PASSWORD.into()),
        sslmode: SslMode::Disable,
        params: None,
        ssh: None,
    };

    let driver = MssqlDriver::connect(&cfg).await.unwrap();
    driver.ping().await.unwrap();

    driver
        .query(&Query::Sql(
            "CREATE TABLE users (id INT PRIMARY KEY, name NVARCHAR(50), score FLOAT)".into(),
        ))
        .await
        .unwrap();
    driver
        .query(&Query::Sql(
            "INSERT INTO users (id, name, score) VALUES (1, 'alice', 9.5), (2, NULL, 1.0)".into(),
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
            assert_eq!(rows.len(), 2);
            assert!(matches!(rows[0][0], Cell::Int(1)));
            assert!(matches!(rows[0][1], Cell::Text(ref s) if s == "alice"));
            assert!(matches!(rows[0][2], Cell::Float(_)));
            assert!(matches!(rows[1][1], Cell::Null));
        }
        other => panic!("expected Tabular, got {other:?}"),
    }

    let schema = driver.schema_for("dbo").await.unwrap();
    let found = schema
        .databases
        .iter()
        .flat_map(|d| &d.containers)
        .any(|c| c.name == "users");
    assert!(found, "schema should contain the users table");

    let pk = driver
        .primary_key(&rdb_core::write::TableRef {
            database: None,
            schema: Some("dbo".into()),
            name: "users".into(),
        })
        .await
        .unwrap();
    assert_eq!(pk, vec!["id".to_string()]);

    let affected = driver
        .commit(&[rdb_core::write::WriteOp::Update {
            table: rdb_core::write::TableRef {
                database: None,
                schema: Some("dbo".into()),
                name: "users".into(),
            },
            pk: vec![("id".into(), Cell::Int(2))],
            changes: vec![("name".into(), Cell::Text("bob".into()))],
        }])
        .await
        .unwrap();
    assert_eq!(affected, 1);

    assert!(driver
        .query(&Query::Command(vec!["GET".into(), "k".into()]))
        .await
        .is_err());

    driver.close().await.unwrap();
}
