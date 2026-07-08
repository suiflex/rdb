//! CQL-free SQL text builders for the buffered write path. Same shape as the
//! Postgres builder, minus schema qualification (SQLite has no schema
//! namespace) and with SQLite literal spellings (`X'..'` blobs, 0/1 bools).

use rdbs_core::result::Cell;
use rdbs_core::write::TableRef;

/// `"ident"` with embedded double quotes doubled.
pub fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// Bare quoted table name — SQLite has no schema namespace.
pub fn table_name(t: &TableRef) -> String {
    quote_ident(&t.name)
}

/// A cell as a safe SQLite literal.
pub fn literal(c: &Cell) -> String {
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
        TableRef::named("users")
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
            "UPDATE \"users\" SET \"name\" = 'bob' WHERE \"id\" = 7"
        );
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
    fn quotes_escaped_and_null_pk_is_null() {
        let sql = delete_sql(&t(), &[("k".into(), Cell::Null)]);
        assert_eq!(sql, "DELETE FROM \"users\" WHERE \"k\" IS NULL");
        let sql = delete_sql(&t(), &[("k".into(), Cell::Text("o'brien".into()))]);
        assert_eq!(sql, "DELETE FROM \"users\" WHERE \"k\" = 'o''brien'");
    }
}
