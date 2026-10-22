use std::{collections::HashSet, ops::ControlFlow, path::PathBuf};

use rdb_connstore::{ConnStore, Engine};
use rdb_core::{
    query::{MongoKind, MongoOp, Query},
    result::{Cell, RedisValue, ResultSet},
    schema::{ContainerKind, Database, Schema},
};
use rmcp::{
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router, ServerHandler, ServiceExt,
};
use serde::Deserialize;
use serde_json::{json, Value};
use sqlparser::{
    ast::{
        ObjectName, ObjectNamePart, ObjectType, Query as SqlQuery, SetExpr, Statement, Visit,
        Visitor,
    },
    dialect::GenericDialect,
    parser::Parser,
};

use rdb_runtime::AnyDriver;

const MONGO_FIELD_SAMPLE_SIZE: u32 = 20;

#[derive(Clone)]
pub struct RdbMcpServer {
    tool_router: rmcp::handler::server::tool::ToolRouter<Self>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct ConnectionArgs {
    #[schemars(description = "ID returned by rdb_list_connections")]
    connection_id: String,
    #[schemars(
        description = "Optional database from RDB_MCP_SCOPE_DATABASES; use main for SQLite"
    )]
    database: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
struct QueryArgs {
    #[schemars(description = "ID returned by rdb_list_connections")]
    connection_id: String,
    #[schemars(description = "SQL, CQL, or MongoDB JSON; omit when supplying a Redis command")]
    query: Option<String>,
    #[schemars(description = "MongoDB operation: find, aggregate, or insert")]
    operation: Option<String>,
    #[schemars(description = "MongoDB collection name")]
    collection: Option<String>,
    #[schemars(description = "Database selected from RDB_MCP_SCOPE_DATABASES")]
    database: Option<String>,
    #[schemars(description = "Redis command arguments as tokens, for example [\"GET\", \"key\"]")]
    command: Option<Vec<String>>,
}
#[derive(Clone, Debug, Default)]
struct McpScope {
    connection_ids: HashSet<String>,
    databases: HashSet<String>,
}

impl McpScope {
    fn from_environment() -> Result<Self, String> {
        Ok(Self {
            connection_ids: parse_allowlist("RDB_MCP_SCOPE_CONNECTION_IDS")?,
            databases: parse_allowlist("RDB_MCP_SCOPE_DATABASES")?,
        })
    }

    fn allows_connection(&self, connection_id: &str) -> bool {
        self.connection_ids.contains(connection_id)
    }

    fn select_database(
        &self,
        engine: Engine,
        saved_database: Option<&str>,
        requested_database: Option<&str>,
    ) -> Result<String, String> {
        if self.databases.is_empty() {
            return Err("No database is allowed. Set RDB_MCP_SCOPE_DATABASES.".into());
        }

        if engine == Engine::Sqlite {
            let database = requested_database.unwrap_or("main");
            if database != "main" || !self.databases.contains("main") {
                return Err("SQLite MCP scope must explicitly allow database main.".into());
            }
            return Ok("main".into());
        }

        if let Some(database) = requested_database {
            if !self.databases.contains(database) {
                return Err(format!(
                    "Database is outside the RDB MCP allowlist: {database}"
                ));
            }
            return Ok(database.to_string());
        }
        saved_database
            .filter(|database| self.databases.contains(*database))
            .or_else(|| {
                if self.databases.len() == 1 {
                    self.databases.iter().next().map(String::as_str)
                } else {
                    None
                }
            })
            .map(str::to_string)
            .ok_or_else(|| "Choose a database from RDB_MCP_SCOPE_DATABASES.".to_string())
    }
}

fn parse_allowlist(name: &str) -> Result<HashSet<String>, String> {
    match std::env::var(name) {
        Ok(value) => Ok(value
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .collect()),
        Err(std::env::VarError::NotPresent) => Ok(HashSet::new()),
        Err(std::env::VarError::NotUnicode(_)) => Err(format!("{name} must contain valid UTF-8.")),
    }
}

impl Default for RdbMcpServer {
    fn default() -> Self {
        Self::new()
    }
}

impl RdbMcpServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl RdbMcpServer {
    #[tool(description = "List only connections allowed by RDB_MCP_SCOPE_CONNECTION_IDS.")]
    async fn rdb_list_connections(&self) -> CallToolResult {
        let scope = match McpScope::from_environment() {
            Ok(scope) => scope,
            Err(error) => return failure(error),
        };
        if scope.connection_ids.is_empty() || scope.databases.is_empty() {
            return success(json!([]));
        }
        match open_store() {
            Ok(store) => {
                let mut databases = scope.databases.iter().cloned().collect::<Vec<_>>();
                databases.sort_unstable();
                let connections: Vec<Value> = store
                    .list()
                    .iter()
                    .filter(|connection| {
                        scope.allows_connection(&connection.id)
                            && (connection.engine != Engine::Sqlite
                                || scope.databases.contains("main"))
                    })
                    .map(|connection| {
                        let allowed_databases = if connection.engine == Engine::Sqlite {
                            vec!["main".to_string()]
                        } else {
                            databases.clone()
                        };
                        json!({
                            "id": connection.id,
                            "name": connection.name,
                            "engine": connection.engine,
                            "allowed_databases": allowed_databases,
                            "environment": connection.env_tag,
                        })
                    })
                    .collect();
                success(json!(connections))
            }
            Err(error) => failure(error),
        }
    }

    #[tool(
        description = "Inspect a scoped RDB connection schema. Set database to a value from RDB_MCP_SCOPE_DATABASES."
    )]
    async fn rdb_describe_schema(
        &self,
        Parameters(args): Parameters<ConnectionArgs>,
    ) -> CallToolResult {
        let scope = match McpScope::from_environment() {
            Ok(scope) => scope,
            Err(error) => return failure(error),
        };
        match connect(&args.connection_id, args.database.as_deref(), &scope).await {
            Ok((engine, driver, database)) => {
                let schema = if engine == Engine::Mongo {
                    driver.schema_for(&database).await
                } else {
                    driver.schema().await
                };
                match schema {
                    Ok(schema) => match schema_to_json(&driver, schema).await {
                        Ok(schema) => success(schema),
                        Err(error) => failure(error),
                    },
                    Err(error) => failure(error.to_string()),
                }
            }
            Err(error) => failure(error),
        }
    }

    #[tool(
        description = "Run a query within the configured connection and database allowlists. Write operations also require RDB_MCP_ALLOW_WRITES=1."
    )]
    async fn rdb_execute_query(&self, Parameters(args): Parameters<QueryArgs>) -> CallToolResult {
        let scope = match McpScope::from_environment() {
            Ok(scope) => scope,
            Err(error) => return failure(error),
        };
        let (engine, driver, database) =
            match connect(&args.connection_id, args.database.as_deref(), &scope).await {
                Ok(connection) => connection,
                Err(error) => return failure(error),
            };
        let allow_writes = std::env::var("RDB_MCP_ALLOW_WRITES").is_ok_and(|value| value == "1");
        let query = match build_query(engine, &args, &database, allow_writes, &scope) {
            Ok(query) => query,
            Err(error) => return failure(error),
        };
        match driver.query(&query).await {
            Ok(result) => success(result_to_json(result)),
            Err(error) => failure(error.to_string()),
        }
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for RdbMcpServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions("Set RDB_MCP_SCOPE_CONNECTION_IDS and RDB_MCP_SCOPE_DATABASES to grant access. Missing or empty allowlists deny access. Write operations also require RDB_MCP_ALLOW_WRITES=1.")
    }
}

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let server = RdbMcpServer::new().serve(rmcp::transport::stdio()).await?;
    server.waiting().await?;
    Ok(())
}

fn open_store() -> Result<ConnStore, String> {
    if let Some(dir) = std::env::var_os("RDB_STORE_DIR") {
        let dir = PathBuf::from(dir);
        let backend = Box::new(
            rdb_connstore::EncryptedFileBackend::new(&dir).map_err(|error| error.to_string())?,
        );
        return ConnStore::load(dir.join("connections.json"), backend)
            .map_err(|error| error.to_string());
    }
    ConnStore::open_default()
        .or_else(|_| -> rdb_connstore::Result<ConnStore> {
            let dir = std::env::temp_dir().join("dbm");
            std::fs::create_dir_all(&dir)?;
            let backend = rdb_connstore::secret::select_backend(&dir)?;
            Ok(ConnStore::new(dir.join("connections.json"), backend))
        })
        .map_err(|error| error.to_string())
}

async fn connect(
    connection_id: &str,
    requested_database: Option<&str>,
    scope: &McpScope,
) -> Result<(Engine, AnyDriver, String), String> {
    if !scope.allows_connection(connection_id) {
        return Err(format!(
            "Connection is outside the RDB MCP allowlist: {connection_id}"
        ));
    }
    let (engine, saved_database, mut config) = {
        let store = open_store()?;
        let connection = store
            .get(connection_id)
            .ok_or_else(|| format!("Connection not found: {connection_id}"))?;
        let engine = connection.engine;
        let saved_database = connection.database.clone();
        let config = store
            .conn_config_for(connection_id)
            .map_err(|error| error.to_string())?;
        (engine, saved_database, config)
    };
    let database = scope.select_database(engine, saved_database.as_deref(), requested_database)?;
    if engine != Engine::Sqlite {
        config.database = Some(database.clone());
    }
    let driver = AnyDriver::connect(engine, &config)
        .await
        .map_err(|error| error.to_string())?;
    Ok((engine, driver, database))
}
async fn schema_to_json(driver: &AnyDriver, schema: Schema) -> Result<Value, String> {
    let mut databases = Vec::with_capacity(schema.databases.len());
    for database in schema.databases {
        databases.push(database_to_json(driver, database).await?);
    }
    Ok(json!({"databases": databases}))
}

async fn database_to_json(driver: &AnyDriver, database: Database) -> Result<Value, String> {
    let mut containers = Vec::with_capacity(database.containers.len());
    for mut container in database.containers {
        if container.fields.is_empty() && matches!(&container.kind, ContainerKind::Collection) {
            container.fields = driver
                .sample_fields(&database.name, &container.name, MONGO_FIELD_SAMPLE_SIZE)
                .await
                .map_err(|error| error.to_string())?;
        }
        containers.push(json!({
            "name": container.name,
            "kind": format!("{:?}", container.kind),
            "fields": container.fields.into_iter().map(|field| json!({
                "name": field.name,
                "type": field.type_name,
                "nullable": field.nullable,
                "primary_key": field.pk,
                "foreign_key": field.fk,
            })).collect::<Vec<_>>(),
        }));
    }
    Ok(json!({
        "name": database.name,
        "functions": database.functions.into_iter().map(|function| json!({
            "name": function.name,
            "definition": function.definition,
        })).collect::<Vec<_>>(),
        "containers": containers,
    }))
}

fn build_query(
    engine: Engine,
    args: &QueryArgs,
    database: &str,
    allow_writes: bool,
    scope: &McpScope,
) -> Result<Query, String> {
    match engine {
        Engine::Mongo => {
            let operation = args.operation.as_deref().unwrap_or("find");
            let collection = args
                .collection
                .clone()
                .filter(|value| !value.trim().is_empty())
                .ok_or_else(|| "MongoDB query requires collection".to_string())?;
            let input: Value = serde_json::from_str(
                args.query
                    .as_deref()
                    .ok_or_else(|| "MongoDB query JSON is required".to_string())?,
            )
            .map_err(|error| format!("Invalid MongoDB JSON: {error}"))?;
            let kind = match operation {
                "find" => MongoKind::Find(input),
                "aggregate" => {
                    let stages = input.as_array().ok_or_else(|| {
                        "MongoDB aggregate query must be a JSON array".to_string()
                    })?;
                    ensure_mongo_database_scope(&input, scope)?;
                    if !allow_writes && contains_write_stage(&input) {
                        return Err(
                            "MongoDB aggregate write stages require RDB_MCP_ALLOW_WRITES=1".into(),
                        );
                    }
                    MongoKind::Aggregate(stages.clone())
                }
                "insert" if allow_writes => MongoKind::Insert(input),
                "insert" => return Err("MongoDB insert requires RDB_MCP_ALLOW_WRITES=1".into()),
                _ => return Err("MongoDB operation must be find, aggregate, or insert".into()),
            };
            Ok(Query::Mongo(Box::new(MongoOp {
                collection,
                database: Some(database.to_string()),
                limit: Some(500),
                skip: None,
                sort: None,
                kind,
            })))
        }
        Engine::Redis | Engine::Valkey => {
            let command = args.command.clone().unwrap_or_else(|| {
                args.query
                    .as_deref()
                    .unwrap_or_default()
                    .split_whitespace()
                    .map(str::to_string)
                    .collect()
            });
            let first = command
                .first()
                .map(|value| value.to_ascii_uppercase())
                .unwrap_or_default();
            let read_commands = [
                "GET",
                "MGET",
                "HGET",
                "HMGET",
                "HGETALL",
                "LRANGE",
                "LLEN",
                "SMEMBERS",
                "SCARD",
                "ZSCORE",
                "ZRANGE",
                "ZREVRANGE",
                "EXISTS",
                "TYPE",
                "TTL",
                "PTTL",
                "DBSIZE",
                "INFO",
            ];
            if !allow_writes && !read_commands.contains(&first.as_str()) {
                return Err("Redis command is not in the read-only allowlist; set RDB_MCP_ALLOW_WRITES=1 to allow other commands".into());
            }
            Ok(Query::Command(command))
        }
        Engine::Cassandra => {
            let query = args
                .query
                .as_deref()
                .ok_or_else(|| "CQL query is required".to_string())?;
            validate_sql_query(engine, query, allow_writes, scope)?;
            Ok(Query::Cql(query.to_string()))
        }
        _ => {
            let query = args
                .query
                .as_deref()
                .ok_or_else(|| "SQL query is required".to_string())?;
            validate_sql_query(engine, query, allow_writes, scope)?;
            Ok(Query::Sql(query.to_string()))
        }
    }
}

fn validate_sql_query(
    engine: Engine,
    query: &str,
    allow_writes: bool,
    scope: &McpScope,
) -> Result<(), String> {
    let dialect = GenericDialect {};
    let statements = Parser::parse_sql(&dialect, query)
        .map_err(|error| format!("Cannot verify query safety: {error}"))?;
    if statements.len() != 1 {
        return Err("MCP accepts one SQL statement per tool call.".into());
    }
    if !allow_writes
        && !matches!(
            statements.first(),
            Some(Statement::Query(statement)) if is_read_query(statement)
        )
    {
        return Err("Only one read query is allowed unless RDB_MCP_ALLOW_WRITES=1".into());
    }

    let mut visitor = DatabaseScopeVisitor {
        engine,
        allowed_databases: &scope.databases,
        violation: None,
    };
    let _ = statements.visit(&mut visitor);
    if let Some(database) = visitor.violation {
        return Err(format!(
            "SQL references a database outside the RDB MCP scope: {database}"
        ));
    }
    Ok(())
}

struct DatabaseScopeVisitor<'a> {
    engine: Engine,
    allowed_databases: &'a HashSet<String>,
    violation: Option<String>,
}

impl Visitor for DatabaseScopeVisitor<'_> {
    type Break = ();

    fn pre_visit_statement(&mut self, statement: &Statement) -> ControlFlow<Self::Break> {
        match statement {
            Statement::Use(_)
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
            | Statement::CreateMacro { .. } => {
                self.violation = Some("session, database, privilege, or procedural SQL".into());
                ControlFlow::Break(())
            }
            Statement::CreateView(view) => self.check_relation(&view.name),
            Statement::Drop {
                object_type, names, ..
            } => {
                if !matches!(
                    object_type,
                    ObjectType::Table
                        | ObjectType::View
                        | ObjectType::MaterializedView
                        | ObjectType::Index
                ) {
                    self.violation = Some("non-table DDL".into());
                    return ControlFlow::Break(());
                }
                for name in names {
                    self.check_relation(name)?;
                }
                ControlFlow::Continue(())
            }
            _ => ControlFlow::Continue(()),
        }
    }

    fn pre_visit_relation(&mut self, relation: &ObjectName) -> ControlFlow<Self::Break> {
        self.check_relation(relation)
    }
}

impl DatabaseScopeVisitor<'_> {
    fn check_relation(&mut self, relation: &ObjectName) -> ControlFlow<()> {
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

        let database_index = match self.engine {
            Engine::MySql | Engine::MariaDb | Engine::Clickhouse | Engine::Cassandra
                if parts.len() >= 2 =>
            {
                Some(0)
            }
            Engine::Mssql if parts.len() >= 3 => Some(parts.len() - 3),
            Engine::Sqlite if parts.len() >= 2 => Some(0),
            _ => None,
        };
        if let Some(index) = database_index {
            let database = parts[index];
            if !self.allowed_databases.contains(database) {
                self.violation = Some(database.to_string());
                return ControlFlow::Break(());
            }
        }
        ControlFlow::Continue(())
    }
}

fn ensure_mongo_database_scope(value: &Value, scope: &McpScope) -> Result<(), String> {
    match value {
        Value::Array(values) => {
            for value in values {
                ensure_mongo_database_scope(value, scope)?;
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                let target = match key.as_str() {
                    "$out" => Some(value),
                    "$merge" => Some(value.get("into").unwrap_or(value)),
                    "$lookup" => value.get("from"),
                    "$unionWith" => Some(value),
                    _ => None,
                };
                if let Some(database) = target
                    .and_then(Value::as_object)
                    .and_then(|target| target.get("db"))
                    .and_then(Value::as_str)
                {
                    if !scope.databases.contains(database) {
                        return Err(format!(
                            "MongoDB pipeline targets a database outside the RDB MCP scope: {database}"
                        ));
                    }
                }
                ensure_mongo_database_scope(value, scope)?;
            }
        }
        _ => {}
    }
    Ok(())
}
fn contains_write_stage(value: &Value) -> bool {
    match value {
        Value::Array(values) => values.iter().any(contains_write_stage),
        Value::Object(values) => values.iter().any(|(key, value)| {
            matches!(key.as_str(), "$out" | "$merge") || contains_write_stage(value)
        }),
        _ => false,
    }
}

#[cfg(test)]
fn require_read_query(query: &str, allow_writes: bool) -> Result<(), String> {
    validate_sql_query(Engine::Postgres, query, allow_writes, &McpScope::default())
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
    match query.body.as_ref() {
        SetExpr::Select(select) => select.into.is_none(),
        SetExpr::Query(nested) => is_read_query(nested),
        SetExpr::SetOperation { left, right, .. } => is_read_set(left) && is_read_set(right),
        SetExpr::Values(_) => true,
        _ => false,
    }
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

const MAX_TOOL_ROWS: usize = 500;

fn result_to_json(result: ResultSet) -> Value {
    match result {
        ResultSet::Tabular { cols, rows } => {
            let names: Vec<String> = cols.iter().map(|column| column.name.clone()).collect();
            let total = rows.len();
            let rows: Vec<Value> = rows
                .into_iter()
                .take(MAX_TOOL_ROWS)
                .map(|row| {
                    Value::Object(
                        names
                            .iter()
                            .zip(row)
                            .map(|(name, cell)| (name.clone(), cell_to_json(cell)))
                            .collect(),
                    )
                })
                .collect();
            json!({
                "columns": cols.into_iter().map(|column| json!({"name": column.name, "type": column.type_name})).collect::<Vec<_>>(),
                "rows": rows,
                "truncated": total.saturating_sub(MAX_TOOL_ROWS),
            })
        }
        ResultSet::Documents(documents) => {
            let total = documents.len();
            json!({"documents": documents.into_iter().take(MAX_TOOL_ROWS).collect::<Vec<_>>(), "truncated": total.saturating_sub(MAX_TOOL_ROWS)})
        }
        ResultSet::KeyValue(values) => {
            let total = values.len();
            let values = values
                .into_iter()
                .take(MAX_TOOL_ROWS)
                .map(|(key, value)| {
                    let (value, truncated) = match value {
                        RedisValue::Str(value) => (Value::String(value), 0),
                        RedisValue::Int(value) => (json!(value), 0),
                        RedisValue::List(values) => {
                            let truncated = values.len().saturating_sub(MAX_TOOL_ROWS);
                            (
                                json!(values.into_iter().take(MAX_TOOL_ROWS).collect::<Vec<_>>()),
                                truncated,
                            )
                        }
                        RedisValue::Nil => (Value::Null, 0),
                    };
                    json!({"key": key, "value": value, "truncated": truncated})
                })
                .collect::<Vec<_>>();
            json!({"values": values, "truncated": total.saturating_sub(MAX_TOOL_ROWS)})
        }
        ResultSet::Affected(count) => json!({"affected": count}),
    }
}

fn cell_to_json(cell: Cell) -> Value {
    match cell {
        Cell::Null => Value::Null,
        Cell::Int(value) => json!(value),
        Cell::Float(value) => json!(value),
        Cell::Text(value) => Value::String(value),
        Cell::Bool(value) => json!(value),
        Cell::Bytes(value) => json!(value),
    }
}

fn success(value: Value) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(value.to_string())])
}

fn failure(message: impl Into<String>) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message.into())])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_mode_accepts_one_query_and_rejects_mutation_or_batch() {
        assert!(require_read_query("SELECT 1", false).is_ok());
        assert!(require_read_query("WITH x AS (SELECT 1) SELECT * FROM x", false).is_ok());
        assert!(require_read_query("DELETE FROM users", false).is_err());
        assert!(require_read_query("SELECT 1; DELETE FROM users", false).is_err());
        assert!(require_read_query("SELECT * INTO copy FROM users", false).is_err());
        assert!(require_read_query("SELECT * FROM users FOR UPDATE", false).is_err());
        assert!(require_read_query("DELETE FROM users", true).is_ok());
        assert!(require_read_query("SELECT 1; DELETE FROM users", true).is_err());
    }
    #[test]
    fn environment_scope_denies_missing_or_unlisted_targets() {
        let empty = McpScope::default();
        assert!(!empty.allows_connection("conn-1"));
        assert!(empty
            .select_database(Engine::Postgres, Some("app"), None)
            .is_err());

        let scope = McpScope {
            connection_ids: ["conn-1".to_string()].into(),
            databases: ["app".to_string()].into(),
        };
        assert!(scope.allows_connection("conn-1"));
        assert!(!scope.allows_connection("conn-2"));
        assert_eq!(
            scope
                .select_database(Engine::Postgres, Some("app"), None)
                .unwrap(),
            "app"
        );
        assert!(scope
            .select_database(Engine::Postgres, None, Some("other"))
            .is_err());
    }

    #[test]
    fn sql_scope_rejects_cross_database_relations() {
        let scope = McpScope {
            connection_ids: ["conn-1".to_string()].into(),
            databases: ["app".to_string()].into(),
        };
        assert!(
            validate_sql_query(Engine::MySql, "SELECT * FROM app.users", false, &scope).is_ok()
        );
        assert!(
            validate_sql_query(Engine::MySql, "SELECT * FROM other.users", false, &scope).is_err()
        );
        assert!(validate_sql_query(
            Engine::Mssql,
            "SELECT * FROM other.dbo.users",
            false,
            &scope
        )
        .is_err());
        assert!(validate_sql_query(
            Engine::MySql,
            "INSERT INTO app.users (id) VALUES (1)",
            true,
            &scope
        )
        .is_ok());
        assert!(validate_sql_query(
            Engine::MySql,
            "INSERT INTO other.users (id) VALUES (1)",
            true,
            &scope
        )
        .is_err());
        assert!(validate_sql_query(Engine::MySql, "USE other", true, &scope).is_err());
        assert!(validate_sql_query(
            Engine::MySql,
            "CREATE TABLE other.users (id INT)",
            true,
            &scope
        )
        .is_err());
        assert!(validate_sql_query(
            Engine::MySql,
            "CREATE TABLE app.users (id INT)",
            true,
            &scope
        )
        .is_ok());
        assert!(validate_sql_query(
            Engine::MySql,
            "GRANT SELECT ON app.users TO analyst",
            true,
            &scope
        )
        .is_err());
    }

    #[test]
    fn mongo_pipeline_cannot_write_outside_database_scope() {
        let scope = McpScope {
            connection_ids: ["conn-1".to_string()].into(),
            databases: ["app".to_string()].into(),
        };
        assert!(ensure_mongo_database_scope(
            &json!([{"$merge": {"into": {"db": "other", "coll": "archive"}}}]),
            &scope
        )
        .is_err());
        assert!(ensure_mongo_database_scope(
            &json!([{"$merge": {"into": {"db": "app", "coll": "archive"}}}]),
            &scope
        )
        .is_ok());
    }

    #[test]
    fn mongo_pipeline_write_stages_are_detected_recursively() {
        let pipeline = json!([{"$facet": {"nested": [{"$merge": "archive"}]}}]);
        assert!(contains_write_stage(&pipeline));
        assert!(!contains_write_stage(
            &json!([{"$match": {"active": true}}])
        ));
    }
    #[test]
    fn result_payload_is_capped_at_five_hundred_rows() {
        let documents = (0..501).map(|value| json!({"value": value})).collect();
        let output = result_to_json(ResultSet::Documents(documents));
        assert_eq!(output["documents"].as_array().unwrap().len(), MAX_TOOL_ROWS);
        assert_eq!(output["truncated"], 1);
    }

    #[test]
    fn redis_read_only_mode_rejects_write_commands() {
        let scope = McpScope {
            connection_ids: ["c1".to_string()].into(),
            databases: ["0".to_string()].into(),
        };
        let args = QueryArgs {
            connection_id: "c1".into(),
            query: Some("SET key value".into()),
            operation: None,
            collection: None,
            database: None,
            command: None,
        };
        assert!(build_query(Engine::Redis, &args, "0", false, &scope).is_err());
        assert!(build_query(Engine::Redis, &args, "0", true, &scope).is_ok());
        let command_only = QueryArgs {
            connection_id: "c1".into(),
            query: None,
            operation: None,
            collection: None,
            database: None,
            command: Some(vec!["GET".into(), "key".into()]),
        };
        assert!(matches!(
            build_query(Engine::Redis, &command_only, "0", false, &scope),
            Ok(Query::Command(_))
        ));
    }
}
