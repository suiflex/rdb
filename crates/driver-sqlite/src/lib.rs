//! rdbs-driver-sqlite: a `rdbs_core::driver::Driver` backed by rusqlite.
//!
//! SQLite is a single local file, so `ConnConfig.database` carries the file
//! path (host/port/user are unused). rusqlite is synchronous; every call runs
//! inside `spawn_blocking` over an `Arc<Mutex<Connection>>`.

mod driver;
pub mod write_sql;

pub use driver::SqliteDriver;
