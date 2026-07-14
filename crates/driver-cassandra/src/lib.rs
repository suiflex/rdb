//! rdbs-driver-cassandra: a `rdbs_core::driver::Driver` backed by scylla.
//!
//! Cassandra speaks CQL: results are tabular, but the namespace is
//! keyspace→table (a keyspace maps to a `Database`, its tables to `Container`s).
//! Schema is loaded lazily like Mongo — `schema()` lists keyspaces, and
//! `containers()` fills a keyspace's tables when the sidebar expands it.

mod driver;
mod type_map;
pub mod write_cql;

pub use driver::CassandraDriver;
