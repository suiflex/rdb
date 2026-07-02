//! rdbs-driver-mysql: MySQL/MariaDB Driver impl via mysql_async.

mod convert;
mod schema;
mod write_sql;

pub use driver::MysqlDriver;

mod driver;
