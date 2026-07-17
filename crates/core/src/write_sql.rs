//! Shared SQL/CQL text builders for the literal-embedding write path
//! (UPDATE/INSERT/DELETE). Postgres, SQLite, and Cassandra all emit values as
//! inline escaped literals rather than binds; only two things vary per engine:
//! how a table is qualified and how a `Cell` is spelled as a literal. Those two
//! live in [`Dialect`]; everything else is shared here.
//!
//! MySQL is intentionally not a client of this module — it uses real `?` binds.

use crate::result::Cell;
use crate::write::TableRef;

/// The per-engine parts of literal SQL generation.
pub struct Dialect {
    /// Fully-qualified, quoted table name (schema/keyspace rules are engine-specific).
    pub table_name: fn(&TableRef) -> String,
    /// One `Cell` as a safe inline literal (quoting/blob/bool spelling vary).
    pub literal: fn(&Cell) -> String,
}

/// `"ident"` with embedded double quotes doubled. Same for every dialect here.
pub fn quote_ident(ident: &str) -> String {
    format!("\"{}\"", ident.replace('"', "\"\""))
}

/// `WHERE` clause from (column, value) identity pairs; NULL uses `IS NULL`.
fn where_clause(pk: &[(String, Cell)], d: &Dialect) -> String {
    pk.iter()
        .map(|(col, val)| match val {
            Cell::Null => format!("{} IS NULL", quote_ident(col)),
            v => format!("{} = {}", quote_ident(col), (d.literal)(v)),
        })
        .collect::<Vec<_>>()
        .join(" AND ")
}

pub fn update_sql(
    t: &TableRef,
    pk: &[(String, Cell)],
    changes: &[(String, Cell)],
    d: &Dialect,
) -> String {
    let sets: Vec<String> = changes
        .iter()
        .map(|(col, val)| format!("{} = {}", quote_ident(col), (d.literal)(val)))
        .collect();
    format!(
        "UPDATE {} SET {} WHERE {}",
        (d.table_name)(t),
        sets.join(", "),
        where_clause(pk, d)
    )
}

pub fn insert_sql(t: &TableRef, values: &[(String, Cell)], d: &Dialect) -> String {
    let cols: Vec<String> = values.iter().map(|(c, _)| quote_ident(c)).collect();
    let vals: Vec<String> = values.iter().map(|(_, v)| (d.literal)(v)).collect();
    format!(
        "INSERT INTO {} ({}) VALUES ({})",
        (d.table_name)(t),
        cols.join(", "),
        vals.join(", ")
    )
}

pub fn delete_sql(t: &TableRef, pk: &[(String, Cell)], d: &Dialect) -> String {
    format!(
        "DELETE FROM {} WHERE {}",
        (d.table_name)(t),
        where_clause(pk, d)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // A minimal Postgres-like dialect exercising the shared shape.
    fn table_name(t: &TableRef) -> String {
        let schema = t.schema.as_deref().unwrap_or("public");
        format!("{}.{}", quote_ident(schema), quote_ident(&t.name))
    }
    fn literal(c: &Cell) -> String {
        match c {
            Cell::Null => "NULL".into(),
            Cell::Int(i) => i.to_string(),
            Cell::Text(s) => format!("'{}'", s.replace('\'', "''")),
            _ => unreachable!(),
        }
    }
    const D: Dialect = Dialect {
        table_name,
        literal,
    };

    fn t() -> TableRef {
        TableRef {
            database: None,
            schema: Some("public".into()),
            name: "users".into(),
        }
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
            &D,
        );
        assert_eq!(
            sql,
            "UPDATE \"public\".\"users\" SET \"note\" = NULL WHERE \"a\" = 1 AND \"b\" = 'x'"
        );
    }

    #[test]
    fn insert_lists_columns_and_values_in_order() {
        let sql = insert_sql(
            &t(),
            &[
                ("id".into(), Cell::Int(1)),
                ("k".into(), Cell::Text("o'brien".into())),
            ],
            &D,
        );
        assert_eq!(
            sql,
            "INSERT INTO \"public\".\"users\" (\"id\", \"k\") VALUES (1, 'o''brien')"
        );
    }

    #[test]
    fn delete_null_pk_uses_is_null_and_idents_escape() {
        let sql = delete_sql(
            &TableRef {
                database: None,
                schema: None,
                name: "wei\"rd".into(),
            },
            &[("k".into(), Cell::Null)],
            &D,
        );
        assert_eq!(
            sql,
            "DELETE FROM \"public\".\"wei\"\"rd\" WHERE \"k\" IS NULL"
        );
    }
}
