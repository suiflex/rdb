//! rdb-driver-oracle: Oracle Database driver impl via Oracle's pure-Rust `oracledb` crate.

mod convert;
mod schema;
pub mod write_sql;

pub use driver::OracleDriver;

mod driver;
