//! ClickHouse dialect for the shared literal write-builder
//! (`rdb_core::write_sql`) — Insert only, mirroring `driver-mssql`'s reuse of
//! the same builder. No `update_sql`/`delete_sql` here at all (not stubs
//! returning an error — their absence is the intended shape): ClickHouse has
//! no row-level UPDATE/DELETE, see `driver.rs::commit`.

use rdb_core::result::Cell;
use rdb_core::write::TableRef;
use rdb_core::write_sql::{self as builder, Dialect};

pub use rdb_core::write_sql::quote_ident;

const DIALECT: Dialect = Dialect {
    table_name,
    literal,
};

/// Database-qualified table name (`TableRef.database`, not `.schema` — this
/// engine's namespace field is `database`, same convention as
/// `driver-mysql`, not `driver-postgres`/`driver-mssql`'s `.schema`).
pub fn table_name(t: &TableRef) -> String {
    match &t.database {
        Some(db) => format!("{}.{}", quote_ident(db), quote_ident(&t.name)),
        None => quote_ident(&t.name),
    }
}

/// A cell as a safe ClickHouse SQL literal. `Bool` is really `UInt8` under
/// the hood (no `TRUE`/`FALSE` literal) — spells as `1`/`0`. `Bytes` has no
/// native binary literal, so it goes through `unhex('...')` against
/// ClickHouse's `String` type. Text uses backslash-escaped quotes
/// (ClickHouse SQL, unlike Postgres/ANSI SQL, escapes `'` as `\'` not `''`).
fn literal(c: &Cell) -> String {
    match c {
        Cell::Null => "NULL".into(),
        Cell::Int(i) => i.to_string(),
        Cell::Float(f) => f.to_string(),
        Cell::Bool(b) => if *b { "1" } else { "0" }.to_string(),
        Cell::Text(s) => format!("'{}'", s.replace('\\', "\\\\").replace('\'', "\\'")),
        Cell::Bytes(b) => {
            let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
            format!("unhex('{hex}')")
        }
    }
}

pub fn insert_sql(t: &TableRef, values: &[(String, Cell)]) -> String {
    builder::insert_sql(t, values, &DIALECT)
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
    fn bytes_use_unhex() {
        let sql = insert_sql(&t(), &[("bin".into(), Cell::Bytes(vec![0xde, 0xad]))]);
        assert_eq!(
            sql,
            "INSERT INTO \"app\".\"users\" (\"bin\") VALUES (unhex('dead'))"
        );
    }

    #[test]
    fn bool_spells_as_one_or_zero() {
        let sql = insert_sql(&t(), &[("active".into(), Cell::Bool(true))]);
        assert_eq!(sql, "INSERT INTO \"app\".\"users\" (\"active\") VALUES (1)");
    }

    #[test]
    fn text_backslash_escapes_quotes() {
        let sql = insert_sql(&t(), &[("name".into(), Cell::Text("o'brien".into()))]);
        assert_eq!(
            sql,
            "INSERT INTO \"app\".\"users\" (\"name\") VALUES ('o\\'brien')"
        );
    }

    #[test]
    fn table_name_without_database_is_bare() {
        let t = TableRef {
            database: None,
            schema: None,
            name: "users".into(),
        };
        assert_eq!(table_name(&t), "\"users\"");
    }
}
