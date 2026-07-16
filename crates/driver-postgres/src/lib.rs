//! rdbs-driver-postgres: a `rdb_core::driver::Driver` backed by tokio-postgres.

mod conn_string;
mod driver;
mod type_map;
pub mod write_sql;

pub use driver::PostgresDriver;
