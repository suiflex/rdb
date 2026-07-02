//! SQL builders for the buffered write path. Unlike Postgres, values bind as
//! `?` placeholders (mysql_async `Value`s are coerced server-side), so each
//! builder returns `(sql, params)`; identifiers are backtick-quoted.

use mysql_async::Value;
use rdbs_core::result::Cell;
use rdbs_core::write::TableRef;

/// `` `ident` `` with embedded backticks doubled.
pub fn quote_ident(ident: &str) -> String {
    format!("`{}`", ident.replace('`', "``"))
}

/// Database-qualified table name; bare name when no database is set.
pub fn table_name(t: &TableRef) -> String {
    match &t.database {
        Some(db) => format!("{}.{}", quote_ident(db), quote_ident(&t.name)),
        None => quote_ident(&t.name),
    }
}

pub fn cell_value(c: &Cell) -> Value {
    match c {
        Cell::Null => Value::NULL,
        Cell::Int(i) => Value::Int(*i),
        Cell::Float(f) => Value::Double(*f),
        Cell::Bool(b) => Value::Int(*b as i64),
        Cell::Text(s) => Value::Bytes(s.clone().into_bytes()),
        Cell::Bytes(b) => Value::Bytes(b.clone()),
    }
}

/// `WHERE` from identity pairs; NULL identities use `IS NULL` (no bind).
fn where_clause(pk: &[(String, Cell)], params: &mut Vec<Value>) -> String {
    let parts: Vec<String> = pk
        .iter()
        .map(|(col, val)| match val {
            Cell::Null => format!("{} IS NULL", quote_ident(col)),
            v => {
                params.push(cell_value(v));
                format!("{} = ?", quote_ident(col))
            }
        })
        .collect();
    parts.join(" AND ")
}

pub fn update_sql(
    t: &TableRef,
    pk: &[(String, Cell)],
    changes: &[(String, Cell)],
) -> (String, Vec<Value>) {
    let mut params: Vec<Value> = Vec::new();
    let sets: Vec<String> = changes
        .iter()
        .map(|(col, val)| {
            params.push(cell_value(val));
            format!("{} = ?", quote_ident(col))
        })
        .collect();
    let wher = where_clause(pk, &mut params);
    (
        format!(
            "UPDATE {} SET {} WHERE {}",
            table_name(t),
            sets.join(", "),
            wher
        ),
        params,
    )
}

pub fn insert_sql(t: &TableRef, values: &[(String, Cell)]) -> (String, Vec<Value>) {
    let cols: Vec<String> = values.iter().map(|(c, _)| quote_ident(c)).collect();
    let marks: Vec<&str> = values.iter().map(|_| "?").collect();
    let params: Vec<Value> = values.iter().map(|(_, v)| cell_value(v)).collect();
    (
        format!(
            "INSERT INTO {} ({}) VALUES ({})",
            table_name(t),
            cols.join(", "),
            marks.join(", ")
        ),
        params,
    )
}

pub fn delete_sql(t: &TableRef, pk: &[(String, Cell)]) -> (String, Vec<Value>) {
    let mut params: Vec<Value> = Vec::new();
    let wher = where_clause(pk, &mut params);
    (
        format!("DELETE FROM {} WHERE {}", table_name(t), wher),
        params,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t() -> TableRef {
        TableRef {
            database: Some("shop".into()),
            schema: None,
            name: "users".into(),
        }
    }

    #[test]
    fn update_binds_changes_then_pk() {
        let (sql, params) = update_sql(
            &t(),
            &[("id".into(), Cell::Int(7))],
            &[("name".into(), Cell::Text("bob".into()))],
        );
        assert_eq!(sql, "UPDATE `shop`.`users` SET `name` = ? WHERE `id` = ?");
        assert_eq!(params.len(), 2);
        assert!(matches!(params[0], Value::Bytes(_)));
        assert!(matches!(params[1], Value::Int(7)));
    }

    #[test]
    fn composite_pk_order_is_preserved() {
        let (sql, params) = delete_sql(
            &t(),
            &[
                ("a".into(), Cell::Int(1)),
                ("b".into(), Cell::Text("x".into())),
            ],
        );
        assert_eq!(sql, "DELETE FROM `shop`.`users` WHERE `a` = ? AND `b` = ?");
        assert_eq!(params.len(), 2);
    }

    #[test]
    fn null_pk_uses_is_null_without_bind() {
        let (sql, params) = delete_sql(&t(), &[("k".into(), Cell::Null)]);
        assert_eq!(sql, "DELETE FROM `shop`.`users` WHERE `k` IS NULL");
        assert!(params.is_empty());
    }

    #[test]
    fn insert_uses_placeholders_and_null_binds() {
        let (sql, params) = insert_sql(
            &t(),
            &[("id".into(), Cell::Int(1)), ("note".into(), Cell::Null)],
        );
        assert_eq!(
            sql,
            "INSERT INTO `shop`.`users` (`id`, `note`) VALUES (?, ?)"
        );
        assert!(matches!(params[1], Value::NULL));
    }

    #[test]
    fn backticks_in_idents_are_doubled() {
        assert_eq!(quote_ident("we`ird"), "`we``ird`");
    }
}
