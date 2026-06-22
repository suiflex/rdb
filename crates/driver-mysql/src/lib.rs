//! rdbs-driver-mysql: MySQL/MariaDB Driver impl via mysql_async.

mod convert;
mod schema;

pub use driver::MysqlDriver;

mod driver;
