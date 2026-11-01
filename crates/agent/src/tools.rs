use std::{future::Future, ops::ControlFlow, sync::Arc, time::Duration};

use rdb_connstore::{Engine, QueryLanguage};
use rdb_core::{
    query::{MongoKind, MongoOp, Query},
    result::ResultSet,
};
use rdb_runtime::AnyDriver;
use serde_json::{json, Value};
use sqlparser::{
    ast::{
        Expr, ObjectName, ObjectNamePart, ObjectType, Query as SqlQuery, SetExpr, Statement,
        TableFactor, Visit, Visitor,
    },
    dialect::GenericDialect,
    parser::Parser,
};

/// Rows the model sees from one result. Reads fetch one more so the result can
/// say whether it was cut.
const MAX_ROWS: usize = 100;
// ponytail: one fixed budget for agent reads; make it a preference if users hit it.
const READ_TIMEOUT: Duration = Duration::from_secs(30);

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

/// How much an approved write can lose. `Destructive` covers DDL that drops or
/// rewrites objects, and DELETE/UPDATE with no WHERE clause.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteRisk {
    Write,
    Destructive,
}

#[derive(Debug, Clone)]
pub enum ToolResult {
    Data(String),
    WriteProposal { sql: String, risk: WriteRisk },
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

    /// Runs one database call under [`READ_TIMEOUT`]. On timeout the statement is
    /// cancelled server-side too: dropping the future only closes the client end.
    async fn bounded<T>(
        &self,
        work: impl Future<Output = rdb_core::error::Result<T>>,
    ) -> Result<T, String> {
        let cancel = || async {
            let _ = self.driver.cancel_running().await;
        };
        match within(READ_TIMEOUT, work, cancel).await {
            Some(result) => result.map_err(|error| error.to_string()),
            None => Err(format!(
                "Query exceeded {}s; cancellation was requested on the server",
                READ_TIMEOUT.as_secs()
            )),
        }
    }

    pub async fn execute(&self, call: &ToolCall) -> Result<ToolResult, String> {
        match call.name.as_str() {
            "list_databases" => {
                let names = self.bounded(self.driver.list_databases()).await?;
                Ok(ToolResult::Data(json!(names).to_string()))
            }
            "describe_schema" => {
                let requested = requested_tables(&call.arguments)?;
                let schema = match self.scope.schema.as_deref() {
                    Some(name) => self.bounded(self.driver.schema_for(name)).await,
                    None => self.bounded(self.driver.schema()).await,
                }?;
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
                let result = self.bounded(self.driver.query(&query)).await?;
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
                Ok(ToolResult::WriteProposal {
                    sql: sql.to_owned(),
                    risk: write_risk(sql),
                })
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
        // No time limit: the user approved this exact statement, and a long
        // migration should not be cut off. Stop cancels it instead.
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
                let statement =
                    validate_sql(text, allow_write, self.scope.engine, &self.scope.database)?;
                if allow_write {
                    return Ok(Query::Sql(text.to_owned()));
                }
                Ok(Query::Sql(cap_read(text, &statement, self.scope.engine)))
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
                    limit: Some(MAX_ROWS as i64 + 1),
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
) -> Result<Statement, String> {
    let mut statements = Parser::parse_sql(&GenericDialect {}, text)
        .map_err(|error| format!("Cannot verify query safety: {error}"))?;
    if statements.len() != 1 {
        return Err("Agent permits exactly one statement".into());
    }
    // sqlparser reads `/*! ... */` (MySQL and MariaDB) and `/*M! ... */`
    // (MariaDB only) as comments; the server executes what is inside.
    if matches!(engine, Engine::MySql | Engine::MariaDb)
        && (text.contains("/*!") || text.to_ascii_uppercase().contains("/*M!"))
    {
        return Err("Agent SQL cannot use MySQL or MariaDB executable comments".into());
    }
    if !allow_write
        && !matches!(statements.first(), Some(Statement::Query(query)) if is_read_query(query))
    {
        return Err("Agent query is not read-only".into());
    }
    let mut scope = DatabaseScope {
        engine,
        database,
        read_only: !allow_write,
        violation: None,
    };
    let _ = statements.visit(&mut scope);
    if let Some(reason) = scope.violation {
        return Err(reason);
    }
    Ok(statements.remove(0))
}

/// Functions that change state, sleep, or reach outside the database even
/// inside a `SELECT`. Matched on the unqualified, lowercased name; entries
/// ending in `*` match as a prefix.
// ponytail: a denylist only knows what it lists. The real boundary is a
// read-only database user; this catches the well-known ones.
const SIDE_EFFECT_FUNCTIONS: &[&str] = &[
    // Postgres
    "pg_sleep*",
    "pg_terminate_backend",
    "pg_cancel_backend",
    "pg_reload_conf",
    "pg_rotate_logfile",
    "set_config",
    "nextval",
    "setval",
    "pg_advisory_*",
    "pg_try_advisory_*",
    "lo_*",
    "pg_read_file",
    "pg_read_binary_file",
    "pg_ls_dir",
    "pg_stat_file",
    "dblink*",
    // MySQL / MariaDB
    "sleep",
    "benchmark",
    "get_lock",
    "release_lock",
    "release_all_locks",
    "load_file",
    // ClickHouse table functions
    "url",
    "s3*",
    "file",
    "remote",
    "remotesecure",
    "cluster",
    "mysql",
    "postgresql",
    "mongodb",
    "redis",
    "sqlite",
    "hdfs",
    "jdbc",
    "odbc",
    "executable",
    "input",
    // SQL Server
    "openrowset",
    "opendatasource",
    "openquery",
    "xp_cmdshell",
];

fn is_side_effect_function(name: &ObjectName) -> bool {
    let Some(ObjectNamePart::Identifier(last)) = name.0.last() else {
        return true;
    };
    let name = last.value.to_ascii_lowercase();
    SIDE_EFFECT_FUNCTIONS
        .iter()
        .any(|entry| match entry.strip_suffix('*') {
            Some(prefix) => name.starts_with(prefix),
            None => name == *entry,
        })
}

/// Awaits `work` for at most `limit`. When the limit passes, `cancel` runs
/// while `work` is still alive and only then is `work` dropped: the MySQL,
/// MariaDB, MongoDB and ClickHouse drivers forget a statement's id when its
/// future drops, so a cancel sent after the drop has nothing to cancel.
async fn within<T, C: Future<Output = ()>>(
    limit: Duration,
    work: impl Future<Output = T>,
    cancel: impl FnOnce() -> C,
) -> Option<T> {
    let mut work = std::pin::pin!(work);
    match tokio::time::timeout(limit, &mut work).await {
        Ok(output) => Some(output),
        Err(_) => {
            cancel().await;
            None
        }
    }
}

/// Adds a row cap to a validated read that has none, so a `SELECT *` on a huge
/// table never loads it whole. The cap goes on its own line so a trailing `--`
/// comment cannot swallow it, and the capped text must still parse as one
/// statement or the original is kept.
// ponytail: SQL Server, Oracle and Cassandra keep the cap after loading (no
// shared LIMIT syntax); the read timeout bounds them instead.
fn cap_read(text: &str, statement: &Statement, engine: Engine) -> String {
    let Statement::Query(query) = statement else {
        return text.to_owned();
    };
    let limits = matches!(
        engine,
        Engine::Postgres | Engine::MySql | Engine::MariaDb | Engine::Sqlite | Engine::Clickhouse
    );
    if !limits
        || query.limit_clause.is_some()
        || query.fetch.is_some()
        || query.settings.is_some()
        || query.format_clause.is_some()
    {
        return text.to_owned();
    }
    let capped = format!(
        "{}\nLIMIT {}",
        text.trim_end().trim_end_matches(';').trim_end(),
        MAX_ROWS + 1
    );
    match Parser::parse_sql(&GenericDialect {}, &capped) {
        Ok(parsed) if parsed.len() == 1 => capped,
        _ => text.to_owned(),
    }
}

struct DatabaseScope<'a> {
    engine: Engine,
    database: &'a str,
    read_only: bool,
    violation: Option<String>,
}

impl DatabaseScope<'_> {
    fn deny(&mut self, reason: String) -> ControlFlow<()> {
        self.violation = Some(reason);
        ControlFlow::Break(())
    }

    fn check_function(&mut self, name: &ObjectName) -> ControlFlow<()> {
        if self.read_only && is_side_effect_function(name) {
            return self.deny(format!("Agent reads cannot call {name}"));
        }
        ControlFlow::Continue(())
    }
}

impl Visitor for DatabaseScope<'_> {
    type Break = ();

    fn pre_visit_expr(&mut self, expr: &Expr) -> ControlFlow<Self::Break> {
        match expr {
            Expr::Function(function) => self.check_function(&function.name),
            _ => ControlFlow::Continue(()),
        }
    }

    fn pre_visit_table_factor(&mut self, factor: &TableFactor) -> ControlFlow<Self::Break> {
        match factor {
            TableFactor::Table {
                name,
                args: Some(_),
                ..
            }
            | TableFactor::Function { name, .. } => self.check_function(name),
            _ => ControlFlow::Continue(()),
        }
    }

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
                // Transaction and session control would leave the shared
                // connection in a state the editor does not know about.
                | Statement::StartTransaction { .. }
                | Statement::Commit { .. }
                | Statement::Rollback { .. }
                | Statement::Savepoint { .. }
                | Statement::ReleaseSavepoint { .. }
                | Statement::Prepare { .. }
                | Statement::Deallocate { .. }
                | Statement::Discard { .. }
                | Statement::Lock(_)
                | Statement::LockTables { .. }
                | Statement::UnlockTables
                // Server-side file access and server administration.
                | Statement::Copy { .. }
                | Statement::Pragma { .. }
                | Statement::Kill { .. }
                | Statement::Flush { .. }
                | Statement::Install { .. }
                | Statement::Load { .. }
        ) {
            return self.deny(
                "Agent SQL is outside its database scope: \
                 session, database, privilege, or procedural SQL"
                    .into(),
            );
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
            return self.deny(format!(
                "Agent SQL is outside its database scope: {relation}"
            ));
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
                let name = parts[index].to_owned();
                return self.deny(format!("Agent SQL is outside its database scope: {name}"));
            }
        }
        ControlFlow::Continue(())
    }
}

/// Classifies SQL that already passed [`validate_sql`]. Anything that fails to
/// parse here is treated as destructive.
pub(crate) fn write_risk(sql: &str) -> WriteRisk {
    let destructive = match Parser::parse_sql(&GenericDialect {}, sql).ok().as_deref() {
        // Every statement in the tree, not only the outer one: a CTE such as
        // `WITH d AS (DELETE FROM t RETURNING *) SELECT ...` is a query on
        // the outside and an unfiltered delete inside.
        Some([statement]) => statement.visit(&mut DestructiveScan).is_break(),
        _ => true,
    };
    if destructive {
        WriteRisk::Destructive
    } else {
        WriteRisk::Write
    }
}

/// Breaks on the first statement that drops or rewrites data.
struct DestructiveScan;

impl Visitor for DestructiveScan {
    type Break = ();

    fn pre_visit_statement(&mut self, statement: &Statement) -> ControlFlow<Self::Break> {
        let destructive = matches!(
            statement,
            Statement::Drop { .. }
                | Statement::Truncate(_)
                | Statement::AlterTable(_)
                | Statement::AlterIndex { .. }
                | Statement::AlterView { .. }
        ) || matches!(statement, Statement::Delete(delete) if delete.selection.is_none())
            || matches!(statement, Statement::Update(update) if update.selection.is_none());
        if destructive {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
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
            "truncated": rows.len() > MAX_ROWS,
            "rows": rows.into_iter().take(MAX_ROWS).map(|row| row.into_iter()
                .map(|cell| cell.render()).collect::<Vec<_>>()).collect::<Vec<_>>()
        })
        .to_string(),
        ResultSet::Documents(rows) => json!({
            "truncated": rows.len() > MAX_ROWS,
            "documents": rows.into_iter().take(MAX_ROWS).collect::<Vec<_>>()
        })
        .to_string(),
        ResultSet::KeyValue(rows) => {
            format!("{:?}", rows.into_iter().take(MAX_ROWS).collect::<Vec<_>>())
        }
        ResultSet::Affected(count) => json!({"affected": count}).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        cap_read, mentioned_table_matches, requested_tables, validate_sql, within, write_risk,
        AgentScope, ToolCall, ToolResult, ToolRunner, WriteRisk,
    };
    use rdb_connstore::Engine;
    use rdb_core::{
        conn::{ConnConfig, SslMode},
        query::{MongoKind, Query},
        result::{Cell, ResultSet},
    };
    use rdb_runtime::AnyDriver;
    use serde_json::json;
    use std::{sync::Arc, time::Duration};

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
            ToolResult::WriteProposal { sql: text, .. } if text == sql));
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

    async fn sqlite_runner(setup: &[&str]) -> ToolRunner {
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
        for sql in setup {
            driver.query(&Query::Sql((*sql).into())).await.unwrap();
        }
        ToolRunner::new(
            driver,
            AgentScope {
                connection_id: "sample-connection".into(),
                database: "main".into(),
                schema: None,
                engine: Engine::Sqlite,
                production: false,
            },
        )
    }

    #[test]
    fn reads_reject_side_effect_functions() {
        let read = |engine, sql| validate_sql(sql, false, engine, "main");
        for sql in [
            "SELECT pg_sleep(10)",
            "SELECT pg_catalog.pg_terminate_backend(42)",
            "SELECT nextval('orders_id_seq')",
            "SELECT set_config('role', 'admin', false)",
            "SELECT pg_advisory_lock(1)",
            "SELECT dblink_exec('host=203.0.113.5', 'DROP TABLE t')",
        ] {
            assert!(read(Engine::Postgres, sql).is_err(), "{sql}");
        }
        for sql in [
            "SELECT SLEEP(5)",
            "SELECT LOAD_FILE('/etc/hosts')",
            "SELECT 1 /*!, SLEEP(5) */",
        ] {
            assert!(read(Engine::MySql, sql).is_err(), "{sql}");
        }
        for sql in [
            "SELECT * FROM url('http://203.0.113.5/x', CSV)",
            "SELECT * FROM s3('http://203.0.113.5/bucket/key')",
            "SELECT * FROM remote('203.0.113.5', db.t)",
        ] {
            assert!(read(Engine::Clickhouse, sql).is_err(), "{sql}");
        }
        assert!(read(
            Engine::Mssql,
            "SELECT * FROM OPENROWSET('SQLNCLI', 'Server=203.0.113.5;', 'SELECT 1')"
        )
        .is_err());
        // Ordinary functions, and columns that share a listed name, stay readable.
        for sql in [
            "SELECT lower(name), count(*) FROM users GROUP BY 1",
            "SELECT url, file FROM uploads",
            "SELECT * FROM generate_series(1, 3)",
        ] {
            assert!(read(Engine::Postgres, sql).is_ok(), "{sql}");
        }
        // An approved write may still draw from a sequence.
        assert!(validate_sql(
            "INSERT INTO orders (id) VALUES (nextval('orders_id_seq'))",
            true,
            Engine::Postgres,
            "main"
        )
        .is_ok());
    }

    #[test]
    fn reads_without_a_limit_get_a_row_cap() {
        let cap = |engine, sql: &str| {
            let statement = validate_sql(sql, false, engine, "main").unwrap();
            cap_read(sql, &statement, engine)
        };
        assert_eq!(
            cap(Engine::Postgres, "SELECT * FROM users;"),
            "SELECT * FROM users\nLIMIT 101"
        );
        assert_eq!(
            cap(Engine::Postgres, "SELECT * FROM users -- every row"),
            "SELECT * FROM users -- every row\nLIMIT 101"
        );
        for sql in [
            "SELECT * FROM users LIMIT 5",
            "SELECT * FROM users FETCH FIRST 5 ROWS ONLY",
        ] {
            assert_eq!(cap(Engine::Postgres, sql), sql);
        }
        for engine in [Engine::Mssql, Engine::Oracle] {
            assert_eq!(cap(engine, "SELECT * FROM users"), "SELECT * FROM users");
        }
        // A comment after the semicolon would split the capped text in two.
        assert_eq!(cap(Engine::MySql, "SELECT 1; -- done"), "SELECT 1; -- done");
    }

    #[tokio::test]
    async fn capped_sqlite_read_reports_truncation() {
        let runner = sqlite_runner(&[
            "CREATE TABLE numbers (n INTEGER)",
            "INSERT INTO numbers WITH RECURSIVE s(n) AS \
             (SELECT 1 UNION ALL SELECT n + 1 FROM s WHERE n < 150) SELECT n FROM s",
        ])
        .await;
        let read = |query: &str| ToolCall {
            name: "execute_query".into(),
            arguments: json!({ "query": query }),
        };
        let ToolResult::Data(data) = runner
            .execute(&read("SELECT n FROM numbers ORDER BY n"))
            .await
            .unwrap()
        else {
            panic!("expected rows")
        };
        let value: serde_json::Value = serde_json::from_str(&data).unwrap();
        assert_eq!(value["rows"].as_array().unwrap().len(), 100);
        assert_eq!(value["truncated"], true);
        let ToolResult::Data(data) = runner
            .execute(&read("SELECT n FROM numbers LIMIT 3"))
            .await
            .unwrap()
        else {
            panic!("expected rows")
        };
        let value: serde_json::Value = serde_json::from_str(&data).unwrap();
        assert_eq!(value["rows"].as_array().unwrap().len(), 3);
        assert_eq!(value["truncated"], false);
    }

    #[test]
    fn approved_writes_refuse_transaction_session_and_file_control() {
        let write = |engine, sql| validate_sql(sql, true, engine, "main");
        for sql in [
            "BEGIN",
            "START TRANSACTION",
            "COMMIT",
            "ROLLBACK",
            "SAVEPOINT before_change",
            "LOCK TABLE orders IN ACCESS EXCLUSIVE MODE",
            "COPY orders TO '/tmp/orders.csv'",
            "PREPARE fetch_one AS SELECT 1",
            "DISCARD ALL",
        ] {
            assert!(write(Engine::Postgres, sql).is_err(), "{sql}");
        }
        for sql in ["LOCK TABLES orders WRITE", "UNLOCK TABLES", "KILL 42"] {
            assert!(write(Engine::MySql, sql).is_err(), "{sql}");
        }
        assert!(write(Engine::Sqlite, "PRAGMA journal_mode = DELETE").is_err());
    }

    #[test]
    fn write_risk_flags_ddl_and_unfiltered_changes() {
        for sql in [
            "DROP TABLE orders",
            "TRUNCATE TABLE orders",
            "ALTER TABLE orders DROP COLUMN note",
            "DELETE FROM orders",
            "UPDATE orders SET status = 'void'",
        ] {
            assert!(
                validate_sql(sql, true, Engine::Postgres, "main").is_ok(),
                "{sql}"
            );
            assert_eq!(write_risk(sql), WriteRisk::Destructive, "{sql}");
        }
        for sql in [
            "INSERT INTO orders (id) VALUES (1)",
            "DELETE FROM orders WHERE id = 1",
            "UPDATE orders SET status = 'void' WHERE id = 1",
            "CREATE TABLE notes (id INT)",
        ] {
            assert_eq!(write_risk(sql), WriteRisk::Write, "{sql}");
        }
    }

    async fn runner_for(engine: Engine) -> ToolRunner {
        let runner = sqlite_runner(&[]).await;
        ToolRunner::new(
            runner.driver,
            AgentScope {
                connection_id: "sample-connection".into(),
                database: "app".into(),
                schema: None,
                engine,
                production: false,
            },
        )
    }

    #[tokio::test]
    async fn redis_reads_stay_on_the_allowlist() {
        let runner = runner_for(Engine::Redis).await;
        let command = |tokens: &[&str]| json!({ "command": tokens });
        for tokens in [
            &["GET", "session:1"][..],
            &["hgetall", "user:1"],
            &["SCAN", "0", "MATCH", "user:*"],
        ] {
            assert!(
                runner.parse_query(&command(tokens), false).is_ok(),
                "{tokens:?}"
            );
        }
        for tokens in [
            &["FLUSHALL"][..],
            &["KEYS", "*"],
            &["EVAL", "return 1", "0"],
            &["SET", "user:1", "x"],
            &["CONFIG", "SET", "dir", "/tmp"],
        ] {
            assert!(
                runner.parse_query(&command(tokens), false).is_err(),
                "{tokens:?}"
            );
        }
        assert!(runner
            .parse_query(&json!({"command": "GET a"}), false)
            .is_err());
        assert!(runner
            .parse_query(&command(&["GET", "session:1"]), true)
            .is_err());
        let valkey = runner_for(Engine::Valkey).await;
        assert!(valkey.parse_query(&command(&["DEL", "a"]), false).is_err());
    }

    #[tokio::test]
    async fn mongo_reads_are_bounded_finds_in_the_conversation_database() {
        let runner = runner_for(Engine::Mongo).await;
        let Ok(Query::Mongo(op)) = runner.parse_query(
            &json!({"collection": "orders", "filter": {"status": "paid"}}),
            false,
        ) else {
            panic!("expected a Mongo find")
        };
        assert_eq!(op.database.as_deref(), Some("app"));
        assert_eq!(op.limit, Some(101));
        assert!(matches!(op.kind, MongoKind::Find(_)));
        assert!(runner
            .parse_query(&json!({"collection": "orders"}), true)
            .is_err());
        assert!(runner.parse_query(&json!({}), false).is_err());
    }

    #[tokio::test]
    async fn cql_allows_one_select_only() {
        let runner = runner_for(Engine::Cassandra).await;
        let cql = |query: &str| json!({ "query": query });
        assert!(runner
            .parse_query(&cql("SELECT * FROM app.events LIMIT 10"), false)
            .is_ok());
        for query in [
            "DELETE FROM app.events WHERE id = 1",
            "SELECT * FROM app.events; DROP TABLE app.events",
            "TRUNCATE app.events",
        ] {
            assert!(runner.parse_query(&cql(query), false).is_err(), "{query}");
        }
        assert!(runner
            .parse_query(&cql("SELECT * FROM app.events"), true)
            .is_err());
    }
    #[test]
    fn mariadb_executable_comments_are_refused() {
        for (engine, sql) in [
            (Engine::MySql, "SELECT 1 /*!, SLEEP(5) */"),
            (Engine::MySql, "SELECT 1 /*M!, SLEEP(5) */"),
            (Engine::MariaDb, "SELECT 1 /*!, SLEEP(5) */"),
            (Engine::MariaDb, "SELECT 1 /*M!, SLEEP(5) */"),
        ] {
            assert!(validate_sql(sql, false, engine, "main").is_err(), "{sql}");
        }
        assert!(validate_sql("SELECT 1 /* plain note */", false, Engine::MariaDb, "main").is_ok());
    }

    #[test]
    fn write_risk_sees_changes_nested_in_ctes() {
        for sql in [
            "WITH removed AS (DELETE FROM orders RETURNING *) SELECT count(*) FROM removed",
            "WITH changed AS (UPDATE orders SET status = 'void' RETURNING *) SELECT * FROM changed",
        ] {
            assert!(
                validate_sql(sql, true, Engine::Postgres, "main").is_ok(),
                "{sql}"
            );
            assert_eq!(write_risk(sql), WriteRisk::Destructive, "{sql}");
        }
        let filtered =
            "WITH removed AS (DELETE FROM orders WHERE id = 1 RETURNING *) SELECT * FROM removed";
        assert_eq!(write_risk(filtered), WriteRisk::Write);
    }

    #[tokio::test]
    async fn cte_delete_proposal_is_destructive() {
        let runner = sqlite_runner(&["CREATE TABLE orders (id INTEGER)"]).await;
        let sql = "WITH removed AS (DELETE FROM orders RETURNING *) SELECT count(*) FROM removed";
        let proposal = ToolCall {
            name: "propose_write".into(),
            arguments: json!({ "query": sql }),
        };
        assert!(matches!(
            runner.execute(&proposal).await.unwrap(),
            ToolResult::WriteProposal {
                risk: WriteRisk::Destructive,
                ..
            }
        ));
    }
    /// Review reproduction: the timeout used to drop the query before asking
    /// the server to cancel it, and drivers forget a statement's id on drop.
    #[tokio::test]
    async fn timed_out_work_is_alive_when_cancel_starts() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct DropFlag(Arc<AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let flag = DropFlag(dropped.clone());
        let work = async move {
            let _flag = flag;
            std::future::pending::<()>().await
        };
        let alive_at_cancel = Arc::new(AtomicBool::new(false));
        let cancel = || {
            let (dropped, alive_at_cancel) = (dropped.clone(), alive_at_cancel.clone());
            async move { alive_at_cancel.store(!dropped.load(Ordering::SeqCst), Ordering::SeqCst) }
        };
        assert!(within(Duration::from_millis(20), work, cancel)
            .await
            .is_none());
        assert!(
            alive_at_cancel.load(Ordering::SeqCst),
            "work dropped before cancel"
        );
        assert!(dropped.load(Ordering::SeqCst), "work outlived the timeout");
        let done = within(Duration::from_secs(5), async { 7 }, || async {}).await;
        assert_eq!(done, Some(7));
    }
}
