use rdb_core::schema::{Container, ContainerKind, Field, Schema};

/// One row of the schema query: (table, column, type_name, nullable, pk, fk).
pub type SchemaRow = (String, String, String, bool, bool, bool);

/// SQL pulling every column of one Oracle schema, scoped via the `:1` bind
/// parameter the caller supplies.
///
/// Uses `all_tab_columns`/`all_tables` rather than `user_*` so the sidebar can
/// browse a schema other than the connected user's own. Oracle's data
/// dictionary stores unquoted identifiers folded to upper case, so the caller
/// must upper-case the owner before binding — see `driver::schema_impl`.
pub const COLUMNS_QUERY: &str = "SELECT t.table_name, c.column_name, c.data_type, \
     CASE WHEN c.nullable = 'Y' THEN 1 ELSE 0 END, \
     CASE WHEN EXISTS ( \
         SELECT 1 FROM all_constraints ac \
         JOIN all_cons_columns acc \
           ON acc.owner = ac.owner AND acc.constraint_name = ac.constraint_name \
         WHERE ac.constraint_type = 'P' AND ac.owner = c.owner \
           AND ac.table_name = c.table_name AND acc.column_name = c.column_name \
     ) THEN 1 ELSE 0 END, \
     CASE WHEN EXISTS ( \
         SELECT 1 FROM all_constraints ac \
         JOIN all_cons_columns acc \
           ON acc.owner = ac.owner AND acc.constraint_name = ac.constraint_name \
         WHERE ac.constraint_type = 'R' AND ac.owner = c.owner \
           AND ac.table_name = c.table_name AND acc.column_name = c.column_name \
     ) THEN 1 ELSE 0 END \
     FROM all_tab_columns c \
     JOIN all_tables t ON t.owner = c.owner AND t.table_name = c.table_name \
     WHERE c.owner = :1 \
     ORDER BY c.table_name, c.column_id";

/// Fold flat (table, column, ...) rows — all from one schema — into a
/// `Schema` wrapping a single `Database` named after that schema, the same
/// convention `driver-mssql` and `driver-postgres` use: `Schema.databases`
/// means "the one namespace level the sidebar renders", and for Oracle that
/// level is the schema (which is also a user), not the PDB.
pub fn fold_rows(schema_name: &str, rows: Vec<SchemaRow>) -> Schema {
    let mut containers: Vec<Container> = Vec::new();

    for (table, col, type_name, nullable, pk, fk) in rows {
        let field = Field {
            name: col,
            type_name,
            nullable,
            pk,
            fk,
        };
        // ORDER BY keeps a table's columns contiguous, so the container being
        // filled is always the last one — no O(rows * tables) scan.
        match containers.last_mut() {
            Some(c) if c.name == table => c.fields.push(field),
            _ => containers.push(Container {
                name: table,
                kind: ContainerKind::Table,
                fields: vec![field],
            }),
        }
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

    fn row(t: &str, c: &str, ty: &str, null: bool, pk: bool, fk: bool) -> SchemaRow {
        (t.into(), c.into(), ty.into(), null, pk, fk)
    }

    #[test]
    fn columns_query_scopes_owner_via_bind_param() {
        assert!(COLUMNS_QUERY.contains("all_tab_columns"));
        assert!(COLUMNS_QUERY.contains("c.owner = :1"));
    }

    #[test]
    fn empty_rows_still_yield_one_named_database() {
        let s = fold_rows("APP_USER", Vec::new());
        assert_eq!(s.databases.len(), 1);
        assert_eq!(s.databases[0].name, "APP_USER");
        assert!(s.databases[0].containers.is_empty());
    }

    #[test]
    fn fold_rows_groups_columns_under_one_schema_database() {
        let rows = vec![
            row("USERS", "ID", "NUMBER", false, true, false),
            row("USERS", "NAME", "VARCHAR2", true, false, false),
            row("ORDERS", "ID", "NUMBER", false, true, false),
            row("ORDERS", "USER_ID", "NUMBER", false, false, true),
        ];
        let schema = fold_rows("APP_USER", rows);
        let db = &schema.databases[0];
        assert_eq!(db.containers.len(), 2);
        let users = db.containers.iter().find(|c| c.name == "USERS").unwrap();
        assert_eq!(users.kind, ContainerKind::Table);
        assert_eq!(users.fields.len(), 2);
        assert!(users.fields.iter().any(|f| f.name == "NAME" && f.nullable));
        assert!(users.fields.iter().any(|f| f.name == "ID" && f.pk));
        let orders = db.containers.iter().find(|c| c.name == "ORDERS").unwrap();
        assert!(orders.fields.iter().any(|f| f.name == "USER_ID" && f.fk));
    }

    #[test]
    fn a_table_without_a_primary_key_is_still_listed() {
        let rows = vec![row("LOGS", "MSG", "VARCHAR2", true, false, false)];
        let db = &fold_rows("APP_USER", rows).databases[0];
        assert_eq!(db.containers.len(), 1);
        assert!(!db.containers[0].fields[0].pk);
    }
}
