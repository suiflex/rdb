use serde_json::Value as Json;

/// A request to a driver. An enum (not a string) so non-SQL engines are
/// first-class and SQL assumptions never leak into the abstraction. Each
/// driver handles the variant it understands and returns
/// `RdbsError::UnsupportedQuery` for the rest.
#[derive(Debug, Clone)]
pub enum Query {
    /// SQL text — Postgres, MySQL.
    Sql(String),
    /// Raw command tokens — Redis, e.g. `["GET", "key"]`.
    Command(Vec<String>),
    /// Structured Mongo operation.
    Mongo(MongoOp),
}

#[derive(Debug, Clone)]
pub struct MongoOp {
    pub collection: String,
    /// Target database; `None` falls back to the connection's default database.
    pub database: Option<String>,
    pub kind: MongoKind,
}

#[derive(Debug, Clone)]
pub enum MongoKind {
    Find(Json),
    Insert(Json),
    Aggregate(Vec<Json>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sql_variant_carries_text() {
        let q = Query::Sql("SELECT 1".into());
        match q {
            Query::Sql(s) => assert_eq!(s, "SELECT 1"),
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn mongo_find_op_constructs() {
        let op = MongoOp {
            collection: "users".into(),
            database: None,
            kind: MongoKind::Find(serde_json::json!({ "age": { "$gt": 18 } })),
        };
        assert_eq!(op.collection, "users");
        matches!(op.kind, MongoKind::Find(_));
    }
}
