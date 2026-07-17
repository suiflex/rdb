//! Postgres dialect for the shared literal write-builder.
//!
//! Values are emitted as escaped single-quoted literals, not binds: a quoted
//! literal is `unknown`-typed, so Postgres assignment-casts it to the column
//! type (int, uuid, timestamp, json, …) — typed binds would instead have to
//! match each column's wire type exactly. Escaping doubles single quotes and
//! strips NUL. Table names are schema-qualified (`public` default).

use rdb_core::result::Cell;
use rdb_core::write::TableRef;
use rdb_core::write_sql::{self as builder, Dialect};

pub use rdb_core::write_sql::quote_ident;

const DIALECT: Dialect = Dialect {
    table_name,
    literal,
};

/// Schema-qualified table name; `schema` defaults to `public`.
pub fn table_name(t: &TableRef) -> String {
    let schema = t.schema.as_deref().unwrap_or("public");
    format!("{}.{}", quote_ident(schema), quote_ident(&t.name))
}

/// A cell as a safe SQL literal (`unknown` type → assignment cast).
fn literal(c: &Cell) -> String {
    match c {
        Cell::Null => "NULL".into(),
        Cell::Int(i) => i.to_string(),
        Cell::Float(f) => f.to_string(),
        Cell::Bool(b) => b.to_string(),
        Cell::Text(s) => format!("'{}'", s.replace('\0', "").replace('\'', "''")),
        Cell::Bytes(b) => {
            let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
            format!("'\\x{hex}'")
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
            database: None,
            schema: Some("public".into()),
            name: "users".into(),
        }
    }

    #[test]
    fn bytes_use_pg_hex_escape() {
        let sql = insert_sql(&t(), &[("bin".into(), Cell::Bytes(vec![0xde, 0xad]))]);
        assert_eq!(
            sql,
            "INSERT INTO \"public\".\"users\" (\"bin\") VALUES ('\\xdead')"
        );
    }

    #[test]
    fn text_strips_nul_and_doubles_quotes() {
        let sql = update_sql(
            &t(),
            &[("id".into(), Cell::Int(7))],
            &[("name".into(), Cell::Text("a\0b'c".into()))],
        );
        assert_eq!(
            sql,
            "UPDATE \"public\".\"users\" SET \"name\" = 'ab''c' WHERE \"id\" = 7"
        );
    }
}
