use rdb_core::schema::{Container, ContainerKind, Field, Schema};

/// One row of the schema query: (table, column, type_name, nullable, pk, fk).
pub type SchemaRow = (String, String, String, bool, bool, bool);

/// SQL pulling every user column of one schema (e.g. `dbo`), scoped via the
/// `@P1` bind parameter the caller supplies — not a formatted-in string, so
/// there's no manual escaping to get wrong. Scoped by schema the same way
/// `driver-postgres`'s `schema_impl` scopes by Postgres schema: SQL Server
/// has a real `sys.schemas` namespace layer, so a bare
/// `INFORMATION_SCHEMA.COLUMNS` scan without a `TABLE_SCHEMA` filter would
/// silently merge tables from unrelated schemas.
pub const COLUMNS_QUERY: &str = "SELECT c.TABLE_NAME, c.COLUMN_NAME, c.DATA_TYPE, \
     CASE WHEN c.IS_NULLABLE = 'YES' THEN 1 ELSE 0 END, \
     CASE WHEN EXISTS ( \
         SELECT 1 FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS tc \
         JOIN INFORMATION_SCHEMA.KEY_COLUMN_USAGE kcu \
           ON tc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME AND tc.TABLE_SCHEMA = kcu.TABLE_SCHEMA \
         WHERE tc.CONSTRAINT_TYPE = 'PRIMARY KEY' AND tc.TABLE_SCHEMA = c.TABLE_SCHEMA \
           AND tc.TABLE_NAME = c.TABLE_NAME AND kcu.COLUMN_NAME = c.COLUMN_NAME \
     ) THEN 1 ELSE 0 END, \
     CASE WHEN EXISTS ( \
         SELECT 1 FROM INFORMATION_SCHEMA.TABLE_CONSTRAINTS tc \
         JOIN INFORMATION_SCHEMA.KEY_COLUMN_USAGE kcu \
           ON tc.CONSTRAINT_NAME = kcu.CONSTRAINT_NAME AND tc.TABLE_SCHEMA = kcu.TABLE_SCHEMA \
         WHERE tc.CONSTRAINT_TYPE = 'FOREIGN KEY' AND tc.TABLE_SCHEMA = c.TABLE_SCHEMA \
           AND tc.TABLE_NAME = c.TABLE_NAME AND kcu.COLUMN_NAME = c.COLUMN_NAME \
     ) THEN 1 ELSE 0 END \
     FROM INFORMATION_SCHEMA.COLUMNS c \
     JOIN INFORMATION_SCHEMA.TABLES t \
       ON t.TABLE_SCHEMA = c.TABLE_SCHEMA AND t.TABLE_NAME = c.TABLE_NAME \
     WHERE c.TABLE_SCHEMA = @P1 AND t.TABLE_TYPE = 'BASE TABLE' \
     ORDER BY c.TABLE_NAME, c.ORDINAL_POSITION";

/// Fold flat (table, column, ...) rows — all from one schema — into a
/// `Schema` wrapping a single `Database` named after that schema. Mirrors
/// `driver-postgres::schema_impl`'s convention: the connection's actual
/// database is tracked separately via `list_databases`, and `Schema.databases`
/// here means "the one namespace level the sidebar renders", not the SQL
/// Server database.
pub fn fold_rows(schema_name: &str, rows: Vec<SchemaRow>) -> Schema {
    let mut containers: Vec<Container> = Vec::new();

    for (table, col, type_name, nullable, pk, fk) in rows {
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
            pk,
            fk,
        });
    }

    Schema {
        databases: vec![rdb_core::schema::Database {
            name: schema_name.to_string(),
            containers,
            functions: Vec::new(),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_query_targets_information_schema_via_bind_param() {
        assert!(COLUMNS_QUERY.contains("INFORMATION_SCHEMA.COLUMNS"));
        assert!(COLUMNS_QUERY.contains("c.TABLE_SCHEMA = @P1"));
    }

    #[test]
    fn fold_rows_groups_columns_under_one_schema_database() {
        let rows = vec![
            (
                "users".to_string(),
                "id".to_string(),
                "int".to_string(),
                false,
                true,
                false,
            ),
            (
                "users".to_string(),
                "name".to_string(),
                "nvarchar".to_string(),
                true,
                false,
                false,
            ),
            (
                "orders".to_string(),
                "id".to_string(),
                "int".to_string(),
                false,
                true,
                false,
            ),
        ];
        let schema = fold_rows("dbo", rows);
        assert_eq!(schema.databases.len(), 1);
        let db = &schema.databases[0];
        assert_eq!(db.name, "dbo");
        assert_eq!(db.containers.len(), 2);
        let users = db.containers.iter().find(|c| c.name == "users").unwrap();
        assert_eq!(users.kind, ContainerKind::Table);
        assert_eq!(users.fields.len(), 2);
        assert!(users.fields.iter().any(|f| f.name == "name" && f.nullable));
    }
}
