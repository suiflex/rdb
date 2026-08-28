//! Seed a throwaway connection store for the UI harness.
//!
//! The suitest desktop suite drives a real connection, which means a store has
//! to exist before the app launches — and it must never be the developer's own.
//! `make ui-test` calls this to build one under `target/`.
//!
//! ```text
//! cargo run -p rdb-connstore --example seed-store -- \
//!     <dir> <name> <engine-key> <host> <port> <user> <database> <password>
//! ```
//!
//! The password arrives as an argument rather than being read from a file so
//! nothing has to be written to disk outside `<dir>`, which the caller owns.

use rdb_connstore::{ConnStore, Engine, SavedConnection};

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dir, name, engine, host, port, user, database, password] = args.as_slice() else {
        eprintln!(
            "usage: seed-store <dir> <name> <engine-key> <host> <port> <user> \
             <database> <password>"
        );
        std::process::exit(2);
    };

    let engine = Engine::from_key(engine).unwrap_or_else(|| {
        eprintln!("unknown engine key {engine:?}");
        std::process::exit(2);
    });
    let port: u16 = port.parse().unwrap_or_else(|_| {
        eprintln!("port {port:?} is not a number");
        std::process::exit(2);
    });

    let dir = std::path::PathBuf::from(dir);
    std::fs::create_dir_all(&dir).expect("create store dir");
    let secrets = rdb_connstore::secret::select_backend(&dir).expect("secret backend");
    let mut store = ConnStore::load(dir.join("connections.json"), secrets).expect("load store");

    let mut conn = SavedConnection::new(name, engine, host, port, user);
    conn.database = Some(database.clone());
    conn.sslmode = rdb_core::conn::SslMode::Disable;

    // save_connection, not add + set_password: the split version can leave
    // metadata written with the secret missing.
    store
        .save_connection(conn, Some(password))
        .expect("save connection");

    println!("seeded {:?} into {}", name, dir.display());
}
