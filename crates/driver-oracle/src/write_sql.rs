//! Oracle SQL dialect for the shared literal write-builder
//! (`rdb_core::write_sql`) — the same pattern `driver-postgres` and
//! `driver-mssql` use rather than `driver-mysql`'s bind-param one, because
//! oracle-rs binds a typed `Value` and a `Cell` carries no Oracle type to
//! bind it as; a literal lets Oracle apply the column's own implicit
//! conversion.
//!
//! One Oracle behaviour worth knowing before editing a row: **Oracle stores
//! the empty string as NULL.** Writing `Cell::Text("")` emits the literal
//! `''`, which the server persists as NULL, and the cell reads back as NULL
//! rather than as an empty string. That is Oracle semantics, not a bug in
//! this layer, and it cannot be worked around from the SQL side.

use rdb_core::result::Cell;
use rdb_core::write::TableRef;
use rdb_core::write_sql::{self as builder, Dialect};

pub use rdb_core::write_sql::quote_ident;

const DIALECT: Dialect = Dialect {
    table_name,
    literal,
};

/// Schema-qualified table name. Oracle has no `public`/`dbo`-style default
/// schema name to fall back on — an unqualified name resolves against the
/// connected user's own schema, which is exactly the right default — so an
/// absent schema means "emit a bare table name", not "guess one".
pub fn table_name(t: &TableRef) -> String {
    match t.schema.as_deref() {
        Some(s) if !s.is_empty() => format!("{}.{}", quote_ident(s), quote_ident(&t.name)),
        _ => quote_ident(&t.name),
    }
}

/// A cell as a safe Oracle SQL literal.
///
/// Booleans spell as `1`/`0`: native `BOOLEAN` columns only exist in Oracle
/// 23c and later, and the overwhelmingly common encoding before that is
/// `NUMBER(1)` — which also accepts `1`/`0` on a real 23c `BOOLEAN` column,
/// so one spelling works on every supported version. Bytes use
/// `HEXTORAW('..')` (Oracle's `RAW` literal), not T-SQL's `0x..`. Text is
/// plain `'..'` with quotes doubled — no `N` prefix, since `VARCHAR2` in an
/// AL32UTF8 database is already Unicode.
fn literal(c: &Cell) -> String {
    match c {
        Cell::Null => "NULL".into(),
        Cell::Int(i) => i.to_string(),
        Cell::Float(f) => f.to_string(),
        Cell::Bool(b) => if *b { "1" } else { "0" }.to_string(),
        Cell::Text(s) => format!("'{}'", s.replace('\'', "''")),
        Cell::Bytes(b) => {
            let hex: String = b.iter().map(|x| format!("{x:02X}")).collect();
            format!("HEXTORAW('{hex}')")
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
            schema: None,
            name: "users".into(),
        }
    }

    #[test]
    fn bytes_use_hextoraw_literal() {
        let sql = insert_sql(&t(), &[("bin".into(), Cell::Bytes(vec![0xde, 0xad]))]);
        assert_eq!(
            sql,
            "INSERT INTO \"users\" (\"bin\") VALUES (HEXTORAW('DEAD'))"
        );
    }

    #[test]
    fn bool_spells_as_one_or_zero() {
        let sql = insert_sql(&t(), &[("active".into(), Cell::Bool(true))]);
        assert_eq!(sql, "INSERT INTO \"users\" (\"active\") VALUES (1)");
    }

    #[test]
    fn text_doubles_single_quotes_without_an_n_prefix() {
        let sql = update_sql(
            &t(),
            &[("id".into(), Cell::Int(7))],
            &[("name".into(), Cell::Text("o'brien".into()))],
        );
        assert_eq!(
            sql,
            "UPDATE \"users\" SET \"name\" = 'o''brien' WHERE \"id\" = 7"
        );
    }

    #[test]
    fn null_pk_uses_is_null() {
        let sql = delete_sql(&t(), &[("k".into(), Cell::Null)]);
        assert_eq!(sql, "DELETE FROM \"users\" WHERE \"k\" IS NULL");
    }

    #[test]
    fn table_name_without_a_schema_stays_unqualified() {
        assert_eq!(table_name(&t()), "\"users\"");
    }

    #[test]
    fn table_name_with_a_schema_quotes_both_halves() {
        let t = TableRef {
            database: None,
            schema: Some("APP_USER".into()),
            name: "users".into(),
        };
        assert_eq!(table_name(&t), "\"APP_USER\".\"users\"");
    }
}
