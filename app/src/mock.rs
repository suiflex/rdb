//! Demo seeding for design parity: `RDBS_MOCK=1` swaps the user's real
//! connection store for an in-memory temp store matching the reference
//! design, and (later) routes "connect" to the in-process MockDriver.

use rdbs_connstore::{ConnStore, Engine, SavedConnection};

fn conn(
    name: &str,
    engine: Engine,
    host: &str,
    port: u16,
    db: Option<&str>,
    group: &str,
    local: bool,
) -> SavedConnection {
    let mut c = SavedConnection::new(name, engine, host, port, "fintech_admin");
    c.database = db.map(str::to_string);
    c.group = Some(group.to_string());
    c.local = local;
    c
}

/// The connection list from design/1-connections.png: OSS 3 · LOCAL 4 ·
/// PROFIN 5 · SPMB 6 · UNGROUPED 9.
pub fn mock_store(dir: std::path::PathBuf) -> ConnStore {
    let _ = std::fs::create_dir_all(&dir);
    let backend = rdbs_connstore::secret::select_backend(&dir).expect("secret backend");
    let mut store = ConnStore::new(dir.join("connections.json"), backend);

    let host = "128.199.74.52";
    let mut add = |c: SavedConnection| {
        let _ = store.add(c);
    };

    for (name, db) in [
        ("oss rba", "oss_rba_master"),
        ("jdih bkpm", "jdih_bkpm_2025"),
        ("primbon", "primbon"),
    ] {
        add(conn(name, Engine::Postgres, host, 5432, Some(db), "OSS", false));
    }
    for (name, engine, port, db) in [
        ("pg local", Engine::Postgres, 5432, Some("postgres")),
        ("mysql local", Engine::MySql, 3306, Some("mysql")),
        ("redis local", Engine::Redis, 6379, None),
        ("mongo local", Engine::Mongo, 27017, Some("local")),
    ] {
        add(conn(name, engine, "127.0.0.1", port, db, "LOCAL", true));
    }
    // PROFIN — the expanded group in the reference.
    add(conn("portfolio", Engine::Postgres, host, 5432, Some("portfolio"), "PROFIN", false));
    {
        let mut c = conn(
            "bot ai tele",
            Engine::Postgres,
            host,
            5432,
            Some("ai_bot_fintech"),
            "PROFIN",
            true,
        );
        c.sslmode = rdbs_core::conn::SslMode::Require;
        c.tags = vec!["profin".into(), "fintech".into()];
        add(c);
    }
    add(conn("profin", Engine::Postgres, host, 5432, Some("profin"), "PROFIN", false));
    add(conn("POS", Engine::Postgres, host, 5432, Some("pos"), "PROFIN", false));
    add(conn("redis portfolio", Engine::Redis, host, 6379, None, "PROFIN", false));

    for (name, db) in [
        ("spmb pusat", "spmb"),
        ("spmb jabar", "spmb_jabar"),
        ("spmb jatim", "spmb_jatim"),
        ("spmb banten", "spmb_banten"),
        ("spmb dki", "spmb_dki"),
        ("spmb diy", "spmb_diy"),
    ] {
        add(conn(name, Engine::Postgres, host, 5432, Some(db), "SPMB", false));
    }
    for (name, engine, port, db) in [
        ("suitest", Engine::Postgres, 5432, Some("suitest")),
        ("suitest test", Engine::Postgres, 5432, Some("suitest_test")),
        ("rtmanagement", Engine::Postgres, 5432, Some("rtmanagement")),
        ("analytics", Engine::MySql, 3306, Some("analytics")),
        ("billing", Engine::MySql, 3306, Some("billing")),
        ("cache edge", Engine::Redis, 6379, None),
        ("queue", Engine::Redis, 6379, None),
        ("iot events", Engine::Mongo, 27017, Some("iot")),
        ("logs", Engine::Mongo, 27017, Some("logs")),
    ] {
        add(conn(name, engine, host, port, db, "", false));
    }
    store
}

/// True when the app runs in design-mock mode.
pub fn mock_mode() -> bool {
    std::env::var("RDBS_MOCK").is_ok_and(|v| v == "1")
}
