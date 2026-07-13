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
    /// Structured Mongo operation. Boxed so the fat Mongo variant doesn't
    /// bloat every `Query` (the SQL path stays a bare `String`).
    Mongo(Box<MongoOp>),
}

#[derive(Debug, Clone)]
pub struct MongoOp {
    pub collection: String,
    /// Target database; `None` falls back to the connection's default database.
    pub database: Option<String>,
    /// Max rows for a `find`; `None` is unbounded. Browsing sets a default cap so
    /// a large collection never freezes the UI.
    pub limit: Option<i64>,
    /// Rows to skip before `limit` applies (pagination); `None` = 0.
    pub skip: Option<i64>,
    /// Sort document for a `find` (e.g. `{ "_id": -1 }`); `None` = natural order.
    pub sort: Option<Json>,
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
            limit: None,
            skip: None,
            sort: None,
            kind: MongoKind::Find(serde_json::json!({ "age": { "$gt": 18 } })),
        };
        assert_eq!(op.collection, "users");
        matches!(op.kind, MongoKind::Find(_));
    }
}
