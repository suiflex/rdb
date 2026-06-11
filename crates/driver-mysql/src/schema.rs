use dbm_core::schema::{Container, ContainerKind, Database, Field, Schema};

/// One row of the schema query: (database, table, column, type_name, nullable).
pub type SchemaRow = (String, String, String, String, bool);

/// SQL pulling every user column. System schemas are excluded so the tree is
/// the user's data, not server internals.
pub fn columns_query() -> String {
    "SELECT TABLE_SCHEMA, TABLE_NAME, COLUMN_NAME, DATA_TYPE, IS_NULLABLE \
     FROM INFORMATION_SCHEMA.COLUMNS \
     WHERE TABLE_SCHEMA NOT IN ('mysql','information_schema','performance_schema','sys') \
     ORDER BY TABLE_SCHEMA, TABLE_NAME, ORDINAL_POSITION"
        .to_string()
}

/// Fold flat (db, table, column, ...) rows into the nested `Schema` tree.
/// Rows are assumed ordered by db, then table (the query's ORDER BY guarantees this).
pub fn fold_rows(rows: Vec<SchemaRow>) -> Schema {
    let mut databases: Vec<Database> = Vec::new();

    for (db_name, table, col, type_name, nullable) in rows {
        let db = match databases.iter_mut().find(|d| d.name == db_name) {
            Some(d) => d,
            None => {
                databases.push(Database {
                    name: db_name.clone(),
                    containers: Vec::new(),
                });
                databases.last_mut().unwrap()
            }
        };

        let container = match db.containers.iter_mut().find(|c| c.name == table) {
            Some(c) => c,
            None => {
                db.containers.push(Container {
                    name: table.clone(),
                    kind: ContainerKind::Table,
                    fields: Vec::new(),
                });
                db.containers.last_mut().unwrap()
            }
        };

        container.fields.push(Field {
            name: col,
            type_name,
            nullable,
        });
    }

    Schema { databases }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dbm_core::schema::ContainerKind;

    #[test]
    fn columns_query_targets_information_schema() {
        let sql = columns_query();
        assert!(sql.to_lowercase().contains("information_schema.columns"));
        assert!(sql.to_lowercase().contains("table_schema not in"));
    }

    #[test]
    fn fold_rows_groups_columns_under_tables_and_databases() {
        let rows = vec![
            (
                "app".to_string(),
                "users".to_string(),
                "id".to_string(),
                "int".to_string(),
                false,
            ),
            (
                "app".to_string(),
                "users".to_string(),
                "name".to_string(),
                "varchar".to_string(),
                true,
            ),
            (
                "app".to_string(),
                "orders".to_string(),
                "id".to_string(),
                "int".to_string(),
                false,
            ),
        ];
        let schema = fold_rows(rows);
        assert_eq!(schema.databases.len(), 1);
        let db = &schema.databases[0];
        assert_eq!(db.name, "app");
        assert_eq!(db.containers.len(), 2);
        let users = db.containers.iter().find(|c| c.name == "users").unwrap();
        assert_eq!(users.kind, ContainerKind::Table);
        assert_eq!(users.fields.len(), 2);
        assert!(users.fields.iter().any(|f| f.name == "name" && f.nullable));
    }
}
