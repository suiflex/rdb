//! SQL text builders for the buffered write path (UPDATE/INSERT/DELETE).
//!
//! Values are emitted as escaped single-quoted literals, not binds: a quoted
//! literal is `unknown`-typed, so Postgres assignment-casts it to the column
//! type (int, uuid, timestamp, json, …) — typed binds would instead have to
//! match each column's wire type exactly. Escaping doubles single quotes and
//! strips NUL; identifiers are double-quoted with `"` doubling.

use rdb_core::result::Cell;
use rdb_core::write::TableRef;

/// `"ident"` with embedded double quotes doubled.
pub fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// Schema-qualified table name; `schema` defaults to `public`.
pub fn table_name(t: &TableRef) -> String {
    let schema = t.schema.as_deref().unwrap_or("public");
    format!("{}.{}", quote_ident(schema), quote_ident(&t.name))
}

/// A cell as a safe SQL literal (`unknown` type → assignment cast).
pub fn literal(c: &Cell) -> String {
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

/// `WHERE` clause from (column, value) identity pairs; NULL uses `IS NULL`.
fn where_clause(pk: &[(String, Cell)]) -> String {
    let parts: Vec<String> = pk
        .iter()
        .map(|(col, val)| match val {
            Cell::Null => format!("{} IS NULL", quote_ident(col)),
            v => format!("{} = {}", quote_ident(col), literal(v)),
        })
        .collect();
    parts.join(" AND ")
}

pub fn update_sql(t: &TableRef, pk: &[(String, Cell)], changes: &[(String, Cell)]) -> String {
    let sets: Vec<String> = changes
        .iter()
        .map(|(col, val)| format!("{} = {}", quote_ident(col), literal(val)))
        .collect();
    format!(
        "UPDATE {} SET {} WHERE {}",
        table_name(t),
        sets.join(", "),
        where_clause(pk)
    )
}

pub fn insert_sql(t: &TableRef, values: &[(String, Cell)]) -> String {
    let cols: Vec<String> = values.iter().map(|(c, _)| quote_ident(c)).collect();
    let vals: Vec<String> = values.iter().map(|(_, v)| literal(v)).collect();
    format!(
        "INSERT INTO {} ({}) VALUES ({})",
        table_name(t),
        cols.join(", "),
        vals.join(", ")
    )
}

pub fn delete_sql(t: &TableRef, pk: &[(String, Cell)]) -> String {
    format!("DELETE FROM {} WHERE {}", table_name(t), where_clause(pk))
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
    fn update_single_pk() {
        let sql = update_sql(
            &t(),
            &[("id".into(), Cell::Int(7))],
            &[("name".into(), Cell::Text("bob".into()))],
        );
        assert_eq!(
            sql,
            "UPDATE \"public\".\"users\" SET \"name\" = 'bob' WHERE \"id\" = 7"
        );
    }

    #[test]
    fn update_composite_pk_and_null_set() {
        let sql = update_sql(
            &t(),
            &[
                ("a".into(), Cell::Int(1)),
                ("b".into(), Cell::Text("x".into())),
            ],
            &[("note".into(), Cell::Null)],
        );
        assert_eq!(
            sql,
            "UPDATE \"public\".\"users\" SET \"note\" = NULL WHERE \"a\" = 1 AND \"b\" = 'x'"
        );
    }

    #[test]
    fn quotes_are_escaped_in_literals_and_idents() {
        let sql = delete_sql(
            &TableRef {
                database: None,
                schema: None,
                name: "wei\"rd".into(),
            },
            &[("k".into(), Cell::Text("o'brien".into()))],
        );
        assert_eq!(
            sql,
            "DELETE FROM \"public\".\"wei\"\"rd\" WHERE \"k\" = 'o''brien'"
        );
    }

    #[test]
    fn insert_lists_columns_and_values_in_order() {
        let sql = insert_sql(
            &t(),
            &[
                ("id".into(), Cell::Int(1)),
                ("ok".into(), Cell::Bool(true)),
                ("bin".into(), Cell::Bytes(vec![0xde, 0xad])),
            ],
        );
        assert_eq!(
            sql,
            "INSERT INTO \"public\".\"users\" (\"id\", \"ok\", \"bin\") VALUES (1, true, '\\xdead')"
        );
    }

    #[test]
    fn null_pk_matches_with_is_null() {
        let sql = delete_sql(&t(), &[("k".into(), Cell::Null)]);
        assert_eq!(sql, "DELETE FROM \"public\".\"users\" WHERE \"k\" IS NULL");
    }
}
