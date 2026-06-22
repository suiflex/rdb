//! rdbs-driver-postgres: a `rdbs_core::driver::Driver` backed by tokio-postgres.

mod conn_string;
mod driver;
mod type_map;

pub use driver::PostgresDriver;
