//! Real-engine test for the Oracle driver.
//!
//! Ignored by default; run with
//! `cargo test -p rdb-driver-oracle --test integration -- --ignored`.
//!
//! By default this spins up `gvenzl/oracle-free` via testcontainers and needs
//! a running Docker/Podman daemon. An Oracle Free container takes several
//! minutes to initialise on a cold volume, so `RDB_ORACLE_TEST_URL` points
//! the test at an already-running server instead:
//!
//! ```text
//! RDB_ORACLE_TEST_URL=oracle://user:pass@127.0.0.1:1521/FREEPDB1
//! ```
//!
//! The test creates and drops its own tables, so it is safe against a
//! scratch database either way.

use rdb_core::conn::{ConnConfig, SslMode};
use rdb_core::driver::Driver;
use rdb_core::query::Query;
use rdb_core::result::{Cell, ResultSet};
use rdb_core::write::{TableRef, WriteOp};
use rdb_driver_oracle::OracleDriver;
use testcontainers::core::{ContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{GenericImage, ImageExt};

const SERVICE: &str = "FREEPDB1";

/// `oracle://user:pass@host:port/service` -> `ConnConfig`.
fn parse_url(url: &str) -> ConnConfig {
    let rest = url.strip_prefix("oracle://").expect("oracle:// URL");
    let (creds, hostpart) = rest.split_once('@').expect("user:pass@host");
    let (user, password) = creds.split_once(':').unwrap_or((creds, ""));
    let (hostport, database) = hostpart.split_once('/').unwrap_or((hostpart, SERVICE));
    let (host, port) = hostport.split_once(':').unwrap_or((hostport, "1521"));
    ConnConfig {
        host: host.to_string(),
        port: port.parse().expect("port"),
        user: user.to_string(),
        database: Some(database.to_string()),
        password: Some(password.to_string()),
        sslmode: SslMode::Disable,
        params: None,
        ssh: None,
    }
}

#[tokio::test]
#[ignore = "requires docker, or RDB_ORACLE_TEST_URL pointing at a live server"]
async fn connect_query_schema_commit_against_real_oracle() {
    // Keep the container alive for the whole test: dropping the handle stops
    // it. `_container` is None when running against an external server.
    let (_container, cfg) = match std::env::var("RDB_ORACLE_TEST_URL") {
        Ok(url) => (None, parse_url(&url)),
        Err(_) => {
            let container = GenericImage::new("gvenzl/oracle-free", "23-slim-faststart")
                .with_exposed_port(ContainerPort::Tcp(1521))
                .with_wait_for(WaitFor::message_on_stdout("DATABASE IS READY TO USE!"))
                .with_env_var("ORACLE_PASSWORD", "testsys")
                .with_env_var("APP_USER", "test")
                .with_env_var("APP_USER_PASSWORD", "test")
                .start()
                .await
                .expect("start oracle container");
            let host = container.get_host().await.unwrap().to_string();
            let port = container.get_host_port_ipv4(1521).await.unwrap();
            let cfg = parse_url(&format!("oracle://test:test@{host}:{port}/{SERVICE}"));
            (Some(container), cfg)
        }
    };

    let driver = OracleDriver::connect(&cfg).await.expect("connect");
    driver.ping().await.expect("ping");

    let sql = |s: &str| Query::Sql(s.to_string());
    // Left over from an earlier failed run, if any.
    let _ = driver.query(&sql("DROP TABLE rdb_it_users")).await;

    driver
        .query(&sql("CREATE TABLE rdb_it_users (\
               id NUMBER PRIMARY KEY, \
               name VARCHAR2(50), \
               score BINARY_DOUBLE, \
               big NUMBER(38,0), \
               payload RAW(8), \
               note CLOB, \
               when_ts TIMESTAMP, \
               bf BINARY_FLOAT, \
               tstz TIMESTAMP WITH TIME ZONE, \
               iv_ym INTERVAL YEAR TO MONTH, \
               iv_ds INTERVAL DAY TO SECOND)"))
        .await
        .expect("create table");

    // Oracle has no multi-row VALUES before 23c: one statement per row.
    driver
        .query(&sql(
            "INSERT INTO rdb_it_users VALUES (1, 'alice', 9.5, \
             99999999999999999999999999999999999999, HEXTORAW('DEAD'), \
             'a note', TIMESTAMP '2024-03-07 09:05:01', \
             2.5, TIMESTAMP '2024-03-07 09:05:01 +07:00', \
             INTERVAL '2-3' YEAR TO MONTH, INTERVAL '4 05:06:07' DAY TO SECOND)",
        ))
        .await
        .expect("insert 1");
    driver
        .query(&sql(
            "INSERT INTO rdb_it_users VALUES \
             (2, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL, NULL)",
        ))
        .await
        .expect("insert 2");

    let rs = driver
        .query(&sql(
            "SELECT id, name, score, big, payload, note, when_ts, \
             bf, tstz, iv_ym, iv_ds \
             FROM rdb_it_users ORDER BY id",
        ))
        .await
        .expect("select");
    let ResultSet::Tabular { cols, rows } = rs else {
        panic!("expected a tabular result");
    };
    assert_eq!(cols.len(), 11);
    assert_eq!(rows.len(), 2);
    assert!(matches!(rows[0][0], Cell::Int(1)));
    assert!(matches!(&rows[0][1], Cell::Text(s) if s == "alice"));
    assert!(matches!(rows[0][2], Cell::Float(f) if (f - 9.5).abs() < f64::EPSILON));
    // A NUMBER(38) exceeds i64 and is not exactly representable as f64, so it
    // has to survive as text rather than come back rounded.
    assert!(
        matches!(&rows[0][3], Cell::Text(s) if s == "99999999999999999999999999999999999999"),
        "wide NUMBER lost precision: {:?}",
        rows[0][3]
    );
    assert!(matches!(&rows[0][4], Cell::Bytes(b) if b == &[0xde, 0xad]));
    assert!(matches!(&rows[0][5], Cell::Text(s) if s == "a note"));
    assert!(matches!(&rows[0][6], Cell::Text(s) if s.starts_with("2024-03-07 09:05:01")));

    // These four are the regression guard for the driver this one replaced:
    // it returned raw wire bytes lossily decoded as UTF-8 for every one of
    // them, which reached the grid as mojibake and could not be recovered.
    assert!(
        matches!(rows[0][7], Cell::Float(f) if (f - 2.5).abs() < 1e-6),
        "BINARY_FLOAT not decoded: {:?}",
        rows[0][7]
    );
    // A zoned timestamp must keep the offset it was written with, not be
    // silently shifted to UTC while still displaying the original offset.
    assert!(
        matches!(&rows[0][8], Cell::Text(s) if s == "2024-03-07 09:05:01 +07:00"),
        "TIMESTAMP WITH TIME ZONE wrong: {:?}",
        rows[0][8]
    );
    assert!(
        matches!(&rows[0][9], Cell::Text(s) if s.contains('2') && s.contains('3')),
        "INTERVAL YEAR TO MONTH not decoded: {:?}",
        rows[0][9]
    );
    assert!(
        matches!(&rows[0][10], Cell::Text(s) if s.contains('4') && s.contains('5')),
        "INTERVAL DAY TO SECOND not decoded: {:?}",
        rows[0][10]
    );
    // The all-NULL row: every column NULL, and none of them a stray marker.
    for (i, c) in rows[1].iter().enumerate().skip(1) {
        assert!(matches!(c, Cell::Null), "column {i} should be NULL: {c:?}");
    }

    let table = TableRef {
        database: None,
        schema: None,
        name: "RDB_IT_USERS".into(),
    };

    let schema = driver.schema().await.expect("schema");
    assert!(
        schema
            .databases
            .iter()
            .flat_map(|d| &d.containers)
            .any(|c| c.name == "RDB_IT_USERS"),
        "created table missing from the schema"
    );

    let pk = driver.primary_key(&table).await.expect("primary key");
    assert_eq!(pk, vec!["ID".to_string()]);

    assert_eq!(driver.count(&table).await.expect("count"), 2);

    let affected = driver
        .commit(&[WriteOp::Update {
            table: table.clone(),
            pk: vec![("ID".into(), Cell::Int(1))],
            changes: vec![("NAME".into(), Cell::Text("o'brien".into()))],
        }])
        .await
        .expect("commit update");
    assert_eq!(affected, 1);

    let rs = driver
        .query(&sql("SELECT name FROM rdb_it_users WHERE id = 1"))
        .await
        .expect("verify update");
    let ResultSet::Tabular { rows, .. } = rs else {
        panic!("expected a tabular result");
    };
    assert!(matches!(&rows[0][0], Cell::Text(s) if s == "o'brien"));

    // A result set wider than the server's ~100-row first batch: the driver
    // has to keep fetching, or rows silently stop at the batch boundary.
    let rs = driver
        .query(&sql(
            "SELECT level FROM dual CONNECT BY level <= 250 ORDER BY level",
        ))
        .await
        .expect("multi-batch select");
    let ResultSet::Tabular { rows, .. } = rs else {
        panic!("expected a tabular result");
    };
    assert_eq!(rows.len(), 250, "result truncated at the first fetch batch");

    // Non-SQL queries belong to other engines and must be refused.
    assert!(driver
        .query(&Query::Command(vec!["GET".into(), "k".into()]))
        .await
        .is_err());

    driver
        .query(&sql("DROP TABLE rdb_it_users"))
        .await
        .expect("drop table");
    driver.close().await.expect("close");
}
