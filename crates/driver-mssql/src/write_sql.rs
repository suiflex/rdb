//! T-SQL dialect for the shared literal write-builder (`rdb_core::write_sql`)
//! — same pattern `driver-postgres` uses, not `driver-mysql`'s bind-param
//! one: tiberius's `&[&dyn ToSql]` binding is awkward for a dynamically-typed
//! `Cell`, and a literal lets SQL Server implicitly convert to the column's
//! actual type same as Postgres's `unknown`-typed literal assignment-cast.

use rdb_core::result::Cell;
use rdb_core::write::TableRef;
use rdb_core::write_sql::{self as builder, Dialect};

pub use rdb_core::write_sql::quote_ident;

const DIALECT: Dialect = Dialect {
    table_name,
    literal,
};

/// Schema-qualified table name; `schema` defaults to `dbo` (SQL Server's
/// default schema, the rough equivalent of Postgres's `public`).
pub fn table_name(t: &TableRef) -> String {
    let schema = t.schema.as_deref().unwrap_or("dbo");
    format!("{}.{}", quote_ident(schema), quote_ident(&t.name))
}

/// A cell as a safe T-SQL literal. `BIT` has no `TRUE`/`FALSE` literal syntax
/// (unlike Postgres) — booleans spell as `1`/`0`. Bytes use `0x`-prefixed hex
/// (`VARBINARY` literal syntax), not Postgres's `\x` escape. Text is
/// `N'...'`-prefixed (Unicode string literal) so it converts cleanly into an
/// `NVARCHAR` column regardless of server collation.
fn literal(c: &Cell) -> String {
    match c {
        Cell::Null => "NULL".into(),
        Cell::Int(i) => i.to_string(),
        Cell::Float(f) => f.to_string(),
        Cell::Bool(b) => if *b { "1" } else { "0" }.to_string(),
        Cell::Text(s) => format!("N'{}'", s.replace('\'', "''")),
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
            database: None,
            schema: Some("dbo".into()),
            name: "users".into(),
        }
    }

    #[test]
    fn bytes_use_0x_hex_literal() {
        let sql = insert_sql(&t(), &[("bin".into(), Cell::Bytes(vec![0xde, 0xad]))]);
        assert_eq!(
            sql,
            "INSERT INTO \"dbo\".\"users\" (\"bin\") VALUES (0xdead)"
        );
    }

    #[test]
    fn bool_spells_as_one_or_zero() {
        let sql = insert_sql(&t(), &[("active".into(), Cell::Bool(true))]);
        assert_eq!(sql, "INSERT INTO \"dbo\".\"users\" (\"active\") VALUES (1)");
    }

    #[test]
    fn text_gets_n_prefix_and_doubles_quotes() {
        let sql = update_sql(
            &t(),
            &[("id".into(), Cell::Int(7))],
            &[("name".into(), Cell::Text("o'brien".into()))],
        );
        assert_eq!(
            sql,
            "UPDATE \"dbo\".\"users\" SET \"name\" = N'o''brien' WHERE \"id\" = 7"
        );
    }

    #[test]
    fn null_pk_uses_is_null() {
        let sql = delete_sql(&t(), &[("k".into(), Cell::Null)]);
        assert_eq!(sql, "DELETE FROM \"dbo\".\"users\" WHERE \"k\" IS NULL");
    }

    #[test]
    fn table_name_defaults_to_dbo_schema() {
        let t = TableRef {
            database: None,
            schema: None,
            name: "users".into(),
        };
        assert_eq!(table_name(&t), "\"dbo\".\"users\"");
    }
}
