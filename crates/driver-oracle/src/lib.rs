//! rdb-driver-oracle: Oracle Database driver impl via the `oracle` crate (ODPI-C).

mod convert;
mod schema;
pub mod write_sql;

pub use driver::OracleDriver;

mod driver;
