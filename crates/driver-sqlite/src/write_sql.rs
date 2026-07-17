//! SQLite dialect for the shared literal write-builder. Like Postgres minus
//! schema qualification (SQLite has no schema namespace) and with SQLite
//! literal spellings (`X'..'` blobs, 0/1 bools).

use rdb_core::result::Cell;
use rdb_core::write::TableRef;
use rdb_core::write_sql::{self as builder, Dialect};

pub use rdb_core::write_sql::quote_ident;

const DIALECT: Dialect = Dialect {
    table_name,
    literal,
};

/// Bare quoted table name — SQLite has no schema namespace.
pub fn table_name(t: &TableRef) -> String {
    quote_ident(&t.name)
}

/// A cell as a safe SQLite literal.
fn literal(c: &Cell) -> String {
    match c {
        Cell::Null => "NULL".into(),
        Cell::Int(i) => i.to_string(),
        Cell::Float(f) => f.to_string(),
        // SQLite has no boolean type; it stores 1/0.
        Cell::Bool(b) => if *b { "1" } else { "0" }.into(),
        Cell::Text(s) => format!("'{}'", s.replace('\0', "").replace('\'', "''")),
        Cell::Bytes(b) => {
            let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
            format!("X'{hex}'")
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
        TableRef::named("users")
    }

    #[test]
    fn insert_bool_and_blob_use_sqlite_spelling() {
        let sql = insert_sql(
            &t(),
            &[
                ("ok".into(), Cell::Bool(true)),
                ("bin".into(), Cell::Bytes(vec![0xde, 0xad])),
            ],
        );
        assert_eq!(
            sql,
            "INSERT INTO \"users\" (\"ok\", \"bin\") VALUES (1, X'dead')"
        );
    }

    #[test]
    fn table_name_is_not_schema_qualified() {
        let sql = delete_sql(&t(), &[("k".into(), Cell::Null)]);
        assert_eq!(sql, "DELETE FROM \"users\" WHERE \"k\" IS NULL");
    }
}
