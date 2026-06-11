/// Unified schema tree. Every engine maps to `databases → containers → fields`
/// even when native names differ (table / collection / keyspace), so the UI
/// tree renders one way regardless of engine.
#[derive(Debug, Clone)]
pub struct Schema {
    pub databases: Vec<Database>,
}

#[derive(Debug, Clone)]
pub struct Database {
    pub name: String,
    pub containers: Vec<Container>,
}

#[derive(Debug, Clone)]
pub struct Container {
    pub name: String,
    pub kind: ContainerKind,
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerKind {
    Table,
    Collection,
    Keyspace,
}

#[derive(Debug, Clone)]
pub struct Field {
    pub name: String,
    pub type_name: String,
    pub nullable: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn schema_unifies_table_and_collection_under_container() {
        let schema = Schema {
            databases: vec![Database {
                name: "app".into(),
                containers: vec![
                    Container {
                        name: "users".into(),
                        kind: ContainerKind::Table,
                        fields: vec![Field {
                            name: "id".into(),
                            type_name: "int4".into(),
                            nullable: false,
                        }],
                    },
                    Container {
                        name: "events".into(),
                        kind: ContainerKind::Collection,
                        fields: vec![],
                    },
                ],
            }],
        };
        assert_eq!(schema.databases[0].containers.len(), 2);
        assert_eq!(schema.databases[0].containers[0].kind, ContainerKind::Table);
        assert!(!schema.databases[0].containers[0].fields[0].nullable);
    }
}
