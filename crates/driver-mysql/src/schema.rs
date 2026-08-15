use rdb_core::schema::{Container, ContainerKind, Database, Field, Schema};

/// One row of the schema query:
/// (database, table, column, type_name, nullable, pk, fk).
pub type SchemaRow = (String, String, String, String, bool, bool, bool);

/// SQL pulling every user column. System schemas are excluded so the tree is
/// the user's data, not server internals.
pub fn columns_query() -> String {
    "SELECT c.TABLE_SCHEMA, c.TABLE_NAME, c.COLUMN_NAME, c.DATA_TYPE, c.IS_NULLABLE, \
     IF(c.COLUMN_KEY = 'PRI', 'YES', 'NO') AS IS_PK, \
     IF(EXISTS (SELECT 1 FROM INFORMATION_SCHEMA.KEY_COLUMN_USAGE k \
                WHERE k.TABLE_SCHEMA = c.TABLE_SCHEMA AND k.TABLE_NAME = c.TABLE_NAME \
                  AND k.COLUMN_NAME = c.COLUMN_NAME AND k.REFERENCED_TABLE_NAME IS NOT NULL), \
        'YES', 'NO') AS IS_FK \
     FROM INFORMATION_SCHEMA.COLUMNS c \
     WHERE c.TABLE_SCHEMA NOT IN ('mysql','information_schema','performance_schema','sys') \
     ORDER BY c.TABLE_SCHEMA, c.TABLE_NAME, c.ORDINAL_POSITION"
        .to_string()
}

/// Fold flat (db, table, column, ...) rows into the nested `Schema` tree.
/// Rows are assumed ordered by db, then table (the query's ORDER BY guarantees this).
pub fn fold_rows(rows: Vec<SchemaRow>) -> Schema {
    let mut databases: Vec<Database> = Vec::new();

    for (db_name, table, col, type_name, nullable, pk, fk) in rows {
        // `last_mut()` rather than a `find()` scan: the ORDER BY keeps every
        // database's (and every table's) rows contiguous, so the group being
        // filled is always the one at the end. The scan version was
        // O(rows * tables), which bites on servers with thousands of tables.
        let db = match databases.last_mut() {
            Some(d) if d.name == db_name => d,
            _ => {
                databases.push(Database {
                    functions: Vec::new(),
                    name: db_name,
                    containers: Vec::new(),
                });
                databases.last_mut().unwrap()
            }
        };

        let field = Field {
            name: col,
            type_name,
            nullable,
            pk,
            fk,
        };

        match db.containers.last_mut() {
            Some(c) if c.name == table => c.fields.push(field),
            _ => db.containers.push(Container {
                name: table,
                kind: ContainerKind::Table,
                fields: vec![field],
            }),
        }
    }

    Schema { databases }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rdb_core::schema::ContainerKind;

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
                true,
                false,
            ),
            (
                "app".to_string(),
                "users".to_string(),
                "name".to_string(),
                "varchar".to_string(),
                true,
                false,
                false,
            ),
            (
                "app".to_string(),
                "orders".to_string(),
                "id".to_string(),
                "int".to_string(),
                false,
                true,
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

    /// The fold walks the rows once and only ever appends to the last group,
    /// so the database boundary has to be picked up from the ORDER BY.
    #[test]
    fn fold_rows_separates_consecutive_databases() {
        let row = |db: &str, table: &str, col: &str| {
            (
                db.to_string(),
                table.to_string(),
                col.to_string(),
                "int".to_string(),
                false,
                false,
                false,
            )
        };
        let schema = fold_rows(vec![
            row("app", "users", "id"),
            row("app", "users", "email"),
            row("shop", "users", "id"),
        ]);
        assert_eq!(schema.databases.len(), 2);
        assert_eq!(schema.databases[0].name, "app");
        assert_eq!(schema.databases[0].containers.len(), 1);
        assert_eq!(schema.databases[0].containers[0].fields.len(), 2);
        assert_eq!(schema.databases[1].name, "shop");
        assert_eq!(schema.databases[1].containers.len(), 1);
        assert_eq!(schema.databases[1].containers[0].fields.len(), 1);
    }
}
