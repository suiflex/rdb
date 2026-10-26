use std::{ops::ControlFlow, sync::Arc};

use rdb_connstore::{Engine, QueryLanguage};
use rdb_core::{
    query::{MongoKind, MongoOp, Query},
    result::ResultSet,
};
use rdb_runtime::AnyDriver;
use serde_json::{json, Value};
use sqlparser::{
    ast::{
        ObjectName, ObjectNamePart, ObjectType, Query as SqlQuery, SetExpr, Statement, Visit,
        Visitor,
    },
    dialect::GenericDialect,
    parser::Parser,
};

#[derive(Debug, Clone)]
pub struct AgentScope {
    pub connection_id: String,
    pub database: String,
    pub schema: Option<String>,
    pub engine: Engine,
    pub production: bool,
}

#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone)]
pub enum ToolResult {
    Data(String),
    WriteProposal(String),
}

pub struct ToolRunner {
    driver: Arc<AnyDriver>,
    scope: AgentScope,
}

impl ToolRunner {
    pub fn new(driver: Arc<AnyDriver>, scope: AgentScope) -> Self {
        Self { driver, scope }
    }

    pub fn scope(&self) -> &AgentScope {
        &self.scope
    }

    pub async fn execute(&self, call: &ToolCall) -> Result<ToolResult, String> {
        match call.name.as_str() {
            "list_databases" => {
                let names = self
                    .driver
                    .list_databases()
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(ToolResult::Data(json!(names).to_string()))
            }
            "describe_schema" => {
                let requested = requested_tables(&call.arguments)?;
                let schema = match self.scope.schema.as_deref() {
                    Some(name) => self.driver.schema_for(name).await,
                    None => self.driver.schema().await,
                }
                .map_err(|error| error.to_string())?;
                let databases = schema
                    .databases
                    .into_iter()
                    .filter(|database| {
                        self.scope.engine == Engine::Sqlite || database.name == self.scope.database
                    })
                    .map(|database| {
                        json!({"name": database.name,
                        "containers": database.containers.into_iter()
                            .filter(|container| requested.as_ref().is_none_or(|names| {
                                names.iter().any(|name| {
                                    mentioned_table_matches(&self.scope, name, &container.name)
                                })
                            }))
                            .map(|container| {
                            json!({"name": container.name, "fields": container.fields.into_iter()
                                .map(|field| json!({"name": field.name, "type": field.type_name,
                                    "nullable": field.nullable, "primary_key": field.pk,
                                    "foreign_key": field.fk})).collect::<Vec<_>>()})
                        }).collect::<Vec<_>>() })
                    })
                    .collect::<Vec<_>>();
                Ok(ToolResult::Data(json!(databases).to_string()))
            }
            "execute_query" => {
                let query = self.parse_query(&call.arguments, false)?;
                let result = self
                    .driver
                    .query(&query)
                    .await
                    .map_err(|error| error.to_string())?;
                Ok(ToolResult::Data(render_result(result)))
            }
            "propose_write" => {
                if self.scope.production {
                    return Err("Agent writes are unavailable on production connections".into());
                }
                let sql = call
                    .arguments
                    .get("query")
                    .and_then(Value::as_str)
                    .ok_or("Missing proposed query")?;
                self.parse_query(&call.arguments, true)?;
                Ok(ToolResult::WriteProposal(sql.to_owned()))
            }
            _ => Err(format!("Unknown agent tool: {}", call.name)),
        }
    }

    pub async fn execute_approved_write(&self, exact_query: &str) -> Result<String, String> {
        if self.scope.production {
            return Err("Agent writes are unavailable on production connections".into());
        }
        let input = json!({"query": exact_query});
        let query = self.parse_query(&input, true)?;
        let result = self
            .driver
            .query(&query)
            .await
            .map_err(|error| error.to_string())?;
        Ok(render_result(result))
    }

    fn parse_query(&self, arguments: &Value, allow_write: bool) -> Result<Query, String> {
        match self.scope.engine.language() {
            QueryLanguage::Sql => {
                let text = query_text(arguments)?;
                validate_sql(text, allow_write, self.scope.engine, &self.scope.database)?;
                Ok(Query::Sql(text.to_owned()))
            }
            QueryLanguage::Cql => {
                let text = query_text(arguments)?;
                if allow_write
                    || !text.to_ascii_lowercase().starts_with("select ")
                    || text.trim_end_matches(';').contains(';')
                {
                    return Err("Agent permits only one CQL SELECT".into());
                }
                Ok(Query::Cql(text.to_owned()))
            }
            QueryLanguage::Command => {
                if allow_write {
                    return Err("Agent cannot write Redis or Valkey data".into());
                }
                let tokens = arguments
                    .get("command")
                    .and_then(Value::as_array)
                    .ok_or("Redis command must be an array of tokens")?
                    .iter()
                    .map(|value| value.as_str().map(str::to_owned))
                    .collect::<Option<Vec<_>>>()
                    .ok_or("Redis command tokens must be strings")?;
                let command = tokens
                    .first()
                    .map(|s| s.to_ascii_uppercase())
                    .unwrap_or_default();
                if !matches!(
                    command.as_str(),
                    "GET"
                        | "MGET"
                        | "TYPE"
                        | "TTL"
                        | "PTTL"
                        | "HGET"
                        | "HMGET"
                        | "HGETALL"
                        | "HLEN"
                        | "LRANGE"
                        | "LLEN"
                        | "SCARD"
                        | "SMEMBERS"
                        | "SISMEMBER"
                        | "ZRANGE"
                        | "ZCARD"
                        | "ZSCORE"
                        | "EXISTS"
                        | "SCAN"
                        | "STRLEN"
                ) {
                    return Err("Redis command is outside the agent read allowlist".into());
                }
                Ok(Query::Command(tokens))
            }
            QueryLanguage::Mongo => {
                if allow_write {
                    return Err("Agent cannot write MongoDB data".into());
                }
                let collection = arguments
                    .get("collection")
                    .and_then(Value::as_str)
                    .ok_or("Missing MongoDB collection")?;
                let filter = arguments
                    .get("filter")
                    .cloned()
                    .unwrap_or_else(|| json!({}));
                Ok(Query::Mongo(Box::new(MongoOp {
                    collection: collection.into(),
                    database: Some(self.scope.database.clone()),
                    limit: Some(100),
                    skip: None,
                    sort: None,
                    kind: MongoKind::Find(filter),
                })))
            }
        }
    }
}

fn requested_tables(arguments: &Value) -> Result<Option<Vec<String>>, String> {
    let Some(value) = arguments.get("tables") else {
        return Ok(None);
    };
    let Some(names) = value.as_array() else {
        return Err("Table references must be an array".into());
    };
    if names.is_empty() || names.len() > 8 {
        return Err("Provide one to eight table references".into());
    }
    names
        .iter()
        .map(|value| {
            let name = value.as_str().ok_or("Table references must be strings")?;
            if name.is_empty() || name.chars().count() > 128 {
                return Err("Invalid table reference".into());
            }
            Ok(name.to_string())
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Some)
}

fn mentioned_table_matches(scope: &AgentScope, mention: &str, table: &str) -> bool {
    let parts = mention.split('.').collect::<Vec<_>>();
    match parts.as_slice() {
        [name] => name.eq_ignore_ascii_case(table),
        [namespace, name] => {
            name.eq_ignore_ascii_case(table)
                && (namespace.eq_ignore_ascii_case(&scope.database)
                    || scope
                        .schema
                        .as_deref()
                        .is_some_and(|schema| namespace.eq_ignore_ascii_case(schema)))
        }
        [database, schema, name] => {
            database.eq_ignore_ascii_case(&scope.database)
                && scope
                    .schema
                    .as_deref()
                    .is_some_and(|current| schema.eq_ignore_ascii_case(current))
                && name.eq_ignore_ascii_case(table)
        }
        _ => false,
    }
}

fn query_text(arguments: &Value) -> Result<&str, String> {
    let text = arguments
        .get("query")
        .and_then(Value::as_str)
        .ok_or("Missing query text")?;
    if text.trim().is_empty() {
        Err("Empty query".into())
    } else {
        Ok(text)
    }
}

fn validate_sql(
    text: &str,
    allow_write: bool,
    engine: Engine,
    database: &str,
) -> Result<(), String> {
    let statements = Parser::parse_sql(&GenericDialect {}, text)
        .map_err(|error| format!("Cannot verify query safety: {error}"))?;
    if statements.len() != 1 {
        return Err("Agent permits exactly one statement".into());
    }
    if !allow_write
        && !matches!(statements.first(), Some(Statement::Query(query)) if is_read_query(query))
    {
        return Err("Agent query is not read-only".into());
    }
    let mut scope = DatabaseScope {
        engine,
        database,
        violation: None,
    };
    let _ = statements.visit(&mut scope);
    if let Some(name) = scope.violation {
        return Err(format!("Agent SQL is outside its database scope: {name}"));
    }
    Ok(())
}

struct DatabaseScope<'a> {
    engine: Engine,
    database: &'a str,
    violation: Option<String>,
}

impl Visitor for DatabaseScope<'_> {
    type Break = ();

    fn pre_visit_statement(&mut self, statement: &Statement) -> ControlFlow<Self::Break> {
        if matches!(
            statement,
            Statement::Use(_)
                | Statement::Drop {
                    object_type: ObjectType::Database
                        | ObjectType::Schema
                        | ObjectType::Role
                        | ObjectType::User,
                    ..
                }
                | Statement::CreateDatabase { .. }
                | Statement::CreateSchema { .. }
                | Statement::AlterSchema(_)
                | Statement::AttachDatabase { .. }
                | Statement::AttachDuckDBDatabase { .. }
                | Statement::DetachDuckDBDatabase { .. }
                | Statement::AlterSession { .. }
                | Statement::Set(_)
                | Statement::Reset(_)
                | Statement::Call(_)
                | Statement::Execute { .. }
                | Statement::Grant(_)
                | Statement::Deny(_)
                | Statement::Revoke(_)
                | Statement::CreateRole(_)
                | Statement::AlterRole { .. }
                | Statement::CreateUser(_)
                | Statement::AlterUser(_)
                | Statement::CreateFunction(_)
                | Statement::CreateProcedure { .. }
                | Statement::CreateTrigger(_)
                | Statement::DropTrigger(_)
                | Statement::CreateMacro { .. }
        ) {
            self.violation = Some("session, database, privilege, or procedural SQL".into());
            return ControlFlow::Break(());
        }
        if let Statement::Drop { names, .. } = statement {
            for name in names {
                self.pre_visit_relation(name)?;
            }
        }
        ControlFlow::Continue(())
    }

    fn pre_visit_relation(&mut self, relation: &ObjectName) -> ControlFlow<Self::Break> {
        let Some(parts) = relation
            .0
            .iter()
            .map(|part| match part {
                ObjectNamePart::Identifier(identifier) => Some(identifier.value.as_str()),
                ObjectNamePart::Function(_) => None,
            })
            .collect::<Option<Vec<_>>>()
        else {
            self.violation = Some(relation.to_string());
            return ControlFlow::Break(());
        };
        let index = match self.engine {
            Engine::MySql | Engine::MariaDb | Engine::Clickhouse | Engine::Sqlite
                if parts.len() >= 2 =>
            {
                Some(0)
            }
            Engine::Mssql if parts.len() >= 3 => Some(parts.len() - 3),
            _ => None,
        };
        if let Some(index) = index {
            if parts[index] != self.database {
                self.violation = Some(parts[index].to_owned());
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(())
    }
}

fn is_read_query(query: &SqlQuery) -> bool {
    if !query.locks.is_empty() || query.for_clause.is_some() {
        return false;
    }
    if query
        .with
        .as_ref()
        .is_some_and(|with| with.cte_tables.iter().any(|cte| !is_read_query(&cte.query)))
    {
        return false;
    }
    is_read_set(&query.body)
}

fn is_read_set(set: &SetExpr) -> bool {
    match set {
        SetExpr::Select(select) => select.into.is_none(),
        SetExpr::Query(query) => is_read_query(query),
        SetExpr::SetOperation { left, right, .. } => is_read_set(left) && is_read_set(right),
        SetExpr::Values(_) => true,
        _ => false,
    }
}

fn render_result(result: ResultSet) -> String {
    match result {
        ResultSet::Tabular { cols, rows } => json!({
            "columns": cols.into_iter().map(|col| col.name).collect::<Vec<_>>(),
            "rows": rows.into_iter().take(100).map(|row| row.into_iter().map(|cell| cell.render())
                .collect::<Vec<_>>()).collect::<Vec<_>>()
        })
        .to_string(),
        ResultSet::Documents(rows) => {
            json!(rows.into_iter().take(100).collect::<Vec<_>>()).to_string()
        }
        ResultSet::KeyValue(rows) => {
            format!("{:?}", rows.into_iter().take(100).collect::<Vec<_>>())
        }
        ResultSet::Affected(count) => json!({"affected": count}).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        mentioned_table_matches, requested_tables, validate_sql, AgentScope, ToolCall, ToolResult,
        ToolRunner,
    };
    use rdb_connstore::Engine;
    use rdb_core::{
        conn::{ConnConfig, SslMode},
        query::Query,
        result::{Cell, ResultSet},
    };
    use rdb_runtime::AnyDriver;
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn rejects_mutating_read_and_multiple_statements() {
        let check = |sql| validate_sql(sql, false, Engine::Postgres, "main");
        assert!(check("SELECT * FROM users").is_ok());
        assert!(check("DELETE FROM users").is_err());
        assert!(check("SELECT 1; DELETE FROM users").is_err());
        assert!(check("SELECT * INTO archive FROM users").is_err());
        assert!(check("SELECT * FROM users FOR UPDATE").is_err());
    }

    #[test]
    fn binds_qualified_sql_to_conversation_database() {
        assert!(validate_sql("SELECT * FROM main.users", false, Engine::MySql, "main").is_ok());
        assert!(validate_sql("SELECT * FROM other.users", false, Engine::MySql, "main").is_err());
        assert!(validate_sql(
            "UPDATE other.users SET name = 'x'",
            true,
            Engine::MySql,
            "main"
        )
        .is_err());
        assert!(validate_sql("USE other", true, Engine::MySql, "main").is_err());
        assert!(validate_sql("DROP DATABASE other", true, Engine::MySql, "main").is_err());
        assert!(validate_sql("DROP TABLE other.users", true, Engine::MySql, "main").is_err());
        assert!(validate_sql(
            "CREATE TABLE other.users (id INT)",
            true,
            Engine::MySql,
            "main"
        )
        .is_err());
    }

    #[test]
    fn table_mentions_stay_in_the_conversation_namespace() {
        let scope = AgentScope {
            connection_id: "connection".into(),
            database: "app".into(),
            schema: Some("public".into()),
            engine: Engine::Postgres,
            production: false,
        };
        assert!(mentioned_table_matches(&scope, "orders", "orders"));
        assert!(mentioned_table_matches(&scope, "public.orders", "orders"));
        assert!(mentioned_table_matches(
            &scope,
            "app.public.orders",
            "orders"
        ));
        assert!(!mentioned_table_matches(&scope, "other.orders", "orders"));
        assert!(!mentioned_table_matches(
            &scope,
            "other.public.orders",
            "orders"
        ));
    }

    #[test]
    fn table_filter_rejects_unbounded_or_invalid_arguments() {
        assert!(requested_tables(&json!({})).unwrap().is_none());
        assert!(requested_tables(&json!({"tables": []})).is_err());
        assert!(requested_tables(&json!({"tables": ["orders", 3]})).is_err());
        assert!(requested_tables(&json!({"tables": vec!["x"; 9]})).is_err());
    }

    #[tokio::test]
    async fn described_schema_contains_only_requested_sqlite_table() {
        let config = ConnConfig {
            host: String::new(),
            port: 0,
            user: String::new(),
            database: Some(":memory:".into()),
            password: None,
            sslmode: SslMode::Disable,
            params: None,
            ssh: None,
        };
        let driver = Arc::new(AnyDriver::connect(Engine::Sqlite, &config).await.unwrap());
        driver
            .query(&Query::Sql("CREATE TABLE orders (id INTEGER)".into()))
            .await
            .unwrap();
        driver
            .query(&Query::Sql("CREATE TABLE secrets (id INTEGER)".into()))
            .await
            .unwrap();
        let runner = ToolRunner::new(
            driver,
            AgentScope {
                connection_id: "connection".into(),
                database: "main".into(),
                schema: None,
                engine: Engine::Sqlite,
                production: false,
            },
        );
        let described = runner
            .execute(&ToolCall {
                name: "describe_schema".into(),
                arguments: json!({"tables": ["orders"]}),
            })
            .await
            .unwrap();
        let ToolResult::Data(schema) = described else {
            panic!("expected schema data")
        };
        assert!(schema.contains("orders"));
        assert!(!schema.contains("secrets"));
    }

    #[tokio::test]
    async fn only_approved_sqlite_write_runs_once() {
        let config = ConnConfig {
            host: String::new(),
            port: 0,
            user: String::new(),
            database: Some(":memory:".into()),
            password: None,
            sslmode: SslMode::Disable,
            params: None,
            ssh: None,
        };
        let driver = Arc::new(AnyDriver::connect(Engine::Sqlite, &config).await.unwrap());
        driver
            .query(&Query::Sql("CREATE TABLE sample (id INTEGER)".into()))
            .await
            .unwrap();
        let scope = AgentScope {
            connection_id: "sample-connection".into(),
            database: "main".into(),
            schema: None,
            engine: Engine::Sqlite,
            production: false,
        };
        let runner = ToolRunner::new(driver.clone(), scope.clone());
        let sql = " INSERT INTO sample (id) VALUES (1) ";
        let read_call = ToolCall {
            name: "execute_query".into(),
            arguments: json!({"query": sql}),
        };
        assert!(runner.execute(&read_call).await.is_err());
        let proposal = ToolCall {
            name: "propose_write".into(),
            arguments: json!({"query": sql}),
        };
        assert!(matches!(runner.execute(&proposal).await.unwrap(),
            ToolResult::WriteProposal(text) if text == sql));
        let production = ToolRunner::new(
            driver.clone(),
            AgentScope {
                production: true,
                ..scope
            },
        );
        assert!(production.execute(&proposal).await.is_err());
        assert!(production.execute_approved_write(sql).await.is_err());
        runner.execute_approved_write(sql).await.unwrap();
        let result = driver
            .query(&Query::Sql("SELECT COUNT(*) FROM sample".into()))
            .await
            .unwrap();
        assert!(matches!(result, ResultSet::Tabular { rows, .. }
            if matches!(rows[0][0], Cell::Int(1))));
    }
}
