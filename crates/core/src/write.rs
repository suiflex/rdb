use crate::result::Cell;

/// Fully-qualified reference to one editable container. `database` targets the
/// Mongo database / MySQL schema (None = the connection's default); `schema`
/// is the Postgres schema (e.g. "public"). For Redis, `name` is the key.
#[derive(Debug, Clone, PartialEq)]
pub struct TableRef {
    pub database: Option<String>,
    pub schema: Option<String>,
    pub name: String,
}

impl TableRef {
    /// Bare-name ref (connection defaults for database/schema).
    pub fn named(name: impl Into<String>) -> Self {
        TableRef {
            database: None,
            schema: None,
            name: name.into(),
        }
    }
}

/// One buffered mutation. `pk` carries (column, value) pairs identifying the
/// target row — the full primary key, or the engine's row identity (`_id`,
/// hash field, list index). Drivers reject ops whose `pk` is empty except
/// where the engine has a single-value identity (Redis string keys).
#[derive(Debug, Clone)]
pub enum WriteOp {
    Update {
        table: TableRef,
        pk: Vec<(String, Cell)>,
        changes: Vec<(String, Cell)>,
    },
    Insert {
        table: TableRef,
        values: Vec<(String, Cell)>,
    },
    Delete {
        table: TableRef,
        pk: Vec<(String, Cell)>,
    },
}

impl WriteOp {
    pub fn table(&self) -> &TableRef {
        match self {
            WriteOp::Update { table, .. }
            | WriteOp::Insert { table, .. }
            | WriteOp::Delete { table, .. } => table,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn named_ref_defaults_database_and_schema() {
        let t = TableRef::named("users");
        assert_eq!(t.name, "users");
        assert!(t.database.is_none() && t.schema.is_none());
    }

    #[test]
    fn ops_expose_their_table() {
        let t = TableRef::named("users");
        let up = WriteOp::Update {
            table: t.clone(),
            pk: vec![("id".into(), Cell::Int(1))],
            changes: vec![("name".into(), Cell::Text("x".into()))],
        };
        assert_eq!(up.table().name, "users");
        let del = WriteOp::Delete {
            table: t.clone(),
            pk: vec![("id".into(), Cell::Int(1))],
        };
        assert_eq!(del.table().name, "users");
        let ins = WriteOp::Insert {
            table: t,
            values: vec![("name".into(), Cell::Null)],
        };
        assert_eq!(ins.table().name, "users");
    }
}
