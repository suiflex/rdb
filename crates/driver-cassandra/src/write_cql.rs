//! CQL text builders for the buffered write path. Like the SQL builders but
//! keyspace-qualified (`"ks"."tbl"`) and with CQL literal spellings
//! (`0x..` blobs, lowercase `null`). No `IS NULL` — a CQL primary key is never
//! null, so identity comparisons are always `=`.

use rdbs_core::result::Cell;
use rdbs_core::write::TableRef;

/// `"ident"` with embedded double quotes doubled.
pub fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

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
pub fn literal(c: &Cell) -> String {
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

fn where_clause(pk: &[(String, Cell)]) -> String {
    pk.iter()
        .map(|(col, val)| format!("{} = {}", quote_ident(col), literal(val)))
        .collect::<Vec<_>>()
        .join(" AND ")
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
