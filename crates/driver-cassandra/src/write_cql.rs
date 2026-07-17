//! Cassandra (CQL) dialect for the shared literal write-builder. Table names
//! are keyspace-qualified (`"ks"."tbl"`) and literals use CQL spellings
//! (`0x..` blobs, lowercase `null`). A CQL primary key is never null, so the
//! shared `IS NULL` identity path is unreachable in practice.

use rdb_core::result::Cell;
use rdb_core::write::TableRef;
use rdb_core::write_sql::{self as builder, Dialect};

pub use rdb_core::write_sql::quote_ident;

const DIALECT: Dialect = Dialect {
    table_name,
    literal,
};

/// Keyspace-qualified table name; `database` carries the keyspace.
pub fn table_name(t: &TableRef) -> String {
    match &t.database {
        Some(ks) if !ks.is_empty() => {
            format!("{}.{}", quote_ident(ks), quote_ident(&t.name))
        }
        _ => quote_ident(&t.name),
    }
}

/// A cell as a CQL literal.
fn literal(c: &Cell) -> String {
    match c {
        Cell::Null => "null".into(),
        Cell::Int(i) => i.to_string(),
        Cell::Float(f) => f.to_string(),
        Cell::Bool(b) => b.to_string(),
        Cell::Text(s) => format!("'{}'", s.replace('\'', "''")),
        Cell::Bytes(b) => {
            let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
            format!("0x{hex}")
        }
    }
}

pub fn update_sql(t: &TableRef, pk: &[(String, Cell)], changes: &[(String, Cell)]) -> String {
    builder::update_sql(t, pk, changes, &DIALECT)
}

pub fn insert_sql(t: &TableRef, values: &[(String, Cell)]) -> String {
    builder::insert_sql(t, values, &DIALECT)
}

pub fn delete_sql(t: &TableRef, pk: &[(String, Cell)]) -> String {
    builder::delete_sql(t, pk, &DIALECT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> TableRef {
        TableRef {
            database: Some("app".into()),
            schema: None,
            name: "users".into(),
        }
    }

    #[test]
    fn update_is_keyspace_qualified() {
        let sql = update_sql(
            &t(),
            &[("id".into(), Cell::Int(7))],
            &[("name".into(), Cell::Text("bob".into()))],
        );
        assert_eq!(
            sql,
            "UPDATE \"app\".\"users\" SET \"name\" = 'bob' WHERE \"id\" = 7"
        );
    }

    #[test]
    fn blob_and_quote_escaping() {
        let sql = insert_sql(
            &t(),
            &[
                ("k".into(), Cell::Text("o'brien".into())),
                ("bin".into(), Cell::Bytes(vec![0xde, 0xad])),
            ],
        );
        assert_eq!(
            sql,
            "INSERT INTO \"app\".\"users\" (\"k\", \"bin\") VALUES ('o''brien', 0xdead)"
        );
    }
}
