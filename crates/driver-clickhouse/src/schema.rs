use rdb_core::schema::{Container, ContainerKind, Field, Schema};

/// `system.columns`, scoped to one database via a literal-embedded name (not
/// user input — always a database name this driver already listed itself via
/// `list_databases`/the connection's own `database`), ordered so a table's
/// columns stay contiguous for `fold_rows`. `Field::pk`/`fk` are always
/// false: ClickHouse's `ORDER BY` key is a physical sort order, not a row-
/// identity constraint, so `Driver::primary_key` deliberately returns empty
/// (no editable rows) rather than mislabeling it as a real PK here.
pub fn columns_query(database: &str) -> String {
    format!(
        "SELECT table, name, type FROM system.columns \
         WHERE database = '{}' ORDER BY table, position FORMAT JSON",
        database.replace('\'', "''")
    )
}

/// Every user database: `system.databases` minus ClickHouse's own internal
/// ones.
pub fn databases_query() -> &'static str {
    "SELECT name FROM system.databases \
     WHERE name NOT IN ('system', 'information_schema', 'INFORMATION_SCHEMA') \
     ORDER BY name FORMAT JSON"
}

/// One row of `columns_query`: (table, column, type_name).
pub type SchemaRow = (String, String, String);

/// Fold flat (table, column, type) rows — all from one database — into a
/// `Schema` wrapping a single `Database`. `type` carries `Nullable(...)`
/// verbatim in `Field::type_name`; `Field::nullable` is derived from that
/// prefix rather than a separate column.
pub fn fold_rows(database: &str, rows: Vec<SchemaRow>) -> Schema {
    let mut containers: Vec<Container> = Vec::new();

    for (table, col, type_name) in rows {
        let nullable = type_name.starts_with("Nullable(");
        let container = match containers.iter_mut().find(|c| c.name == table) {
            Some(c) => c,
            None => {
                containers.push(Container {
                    name: table,
                    kind: ContainerKind::Table,
                    fields: Vec::new(),
                });
                containers.last_mut().unwrap()
            }
        };
        container.fields.push(Field {
            name: col,
            type_name,
            nullable,
            pk: false,
            fk: false,
        });
    }

    Schema {
        databases: vec![rdb_core::schema::Database {
            name: database.to_string(),
            containers,
            functions: Vec::new(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_query_scopes_to_one_database_and_escapes_quotes() {
        let sql = columns_query("app's_db");
        assert!(sql.contains("system.columns"));
        assert!(sql.contains("database = 'app''s_db'"));
        assert!(sql.contains("FORMAT JSON"));
    }

    #[test]
    fn fold_rows_groups_columns_and_derives_nullable() {
        let rows = vec![
            ("users".to_string(), "id".to_string(), "UInt64".to_string()),
            (
                "users".to_string(),
                "name".to_string(),
                "Nullable(String)".to_string(),
            ),
            ("orders".to_string(), "id".to_string(), "UInt64".to_string()),
        ];
        let schema = fold_rows("app", rows);
        assert_eq!(schema.databases.len(), 1);
        let db = &schema.databases[0];
        assert_eq!(db.name, "app");
        assert_eq!(db.containers.len(), 2);
        let users = db.containers.iter().find(|c| c.name == "users").unwrap();
        assert_eq!(users.kind, ContainerKind::Table);
        assert!(!users.fields[0].nullable);
        assert!(users.fields[1].nullable);
    }
}
