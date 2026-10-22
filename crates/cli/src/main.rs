use std::{
    collections::BTreeMap,
    error::Error,
    io::{self, IsTerminal},
    process::ExitCode,
};

use clap::{Parser, Subcommand};
use inquire::{Confirm, MultiSelect, Select, Text};
use kurir::{register, Harness, RegistrationOptions, Scope, ServerSpec};
use rdb_connstore::{ConnStore, Engine, SavedConnection};
use rdb_runtime::AnyDriver;

#[derive(Debug, Parser)]
#[command(name = "rdb-cli", version, about = "RDB command-line tools")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Configure an MCP server for an agent client.
    Mcp,
}

#[tokio::main]
async fn main() -> ExitCode {
    install_crypto_provider();
    rdb_core::conn::set_client_id(format!("RDB {}", env!("CARGO_PKG_VERSION")));
    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("rdb-cli: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn Error>> {
    match Cli::parse().command {
        Command::Mcp => manage_mcp().await,
    }
}

async fn manage_mcp() -> Result<(), Box<dyn Error>> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err("rdb-cli mcp requires an interactive terminal".into());
    }

    let store = open_store()?;
    let connections = store.list().to_vec();
    if connections.is_empty() {
        return Err(
            "No saved connections found. Add a connection in the RDB desktop app first.".into(),
        );
    }
    let connection_choices = connections
        .iter()
        .map(|connection| {
            format!(
                "{}  [{}]  {}",
                connection.name,
                connection.engine.display(),
                connection.id
            )
        })
        .collect::<Vec<_>>();
    let selected_connection =
        Select::new("Select the RDB connection to expose", connection_choices)
            .with_page_size(12)
            .prompt()?;
    let selected_id = selected_connection
        .rsplit_once("  ")
        .map(|(_, id)| id.to_string())
        .ok_or("Could not resolve the selected connection ID")?;
    let connection = connections
        .into_iter()
        .find(|connection| connection.id == selected_id)
        .ok_or("The selected connection no longer exists")?;
    let config = store.conn_config_for(&selected_id)?;
    drop(store);

    let database_choices = database_choices(&connection, &config).await?;
    let databases = if database_choices.len() == 1 {
        database_choices
    } else {
        MultiSelect::new("Select the databases to expose", database_choices)
            .with_page_size(12)
            .prompt()?
    };
    if databases.is_empty() {
        return Err("Select at least one database.".into());
    }
    let allow_writes = Confirm::new("Allow write queries on this connection?")
        .with_default(false)
        .prompt()?;
    let harnesses = Harness::ALL
        .iter()
        .copied()
        .filter(|harness| !harness.is_snippet_only() && harness.id() != "arsy-code")
        .map(|harness| format!("{} — {}", harness.id(), harness.summary()))
        .collect::<Vec<_>>();
    let selected_harness = Select::new("Select the agent client", harnesses)
        .with_page_size(12)
        .prompt()?;
    let harness_id = selected_harness
        .split_once(" — ")
        .map(|(id, _)| id)
        .ok_or("Could not resolve the selected agent client")?;
    let harness = Harness::ALL
        .iter()
        .copied()
        .find(|candidate| candidate.id() == harness_id)
        .ok_or("The selected agent client is not supported")?;
    let selected_scope = Select::new(
        "Where should Kurir register this MCP server?",
        vec!["Project", "User"],
    )
    .prompt()?;
    let scope = match selected_scope {
        "Project" => Scope::Project,
        _ => Scope::User,
    };
    let server_name = Text::new("MCP server name").with_default("rdb").prompt()?;

    println!("\nRDB MCP registration");
    println!("  Connection: {} ({})", connection.name, connection.id);
    println!("  Databases:  {}", databases.join(", "));
    println!(
        "  Writes:     {}",
        if allow_writes { "enabled" } else { "read-only" }
    );
    println!("  Client:     {}", harness.id());
    println!("  Scope:      {}", selected_scope.to_lowercase());
    if !Confirm::new("Register this scoped MCP server?")
        .with_default(true)
        .prompt()?
    {
        println!("Registration cancelled.");
        return Ok(());
    }

    let spec = build_server_spec(server_name, &connection.id, &databases, allow_writes);
    let options = RegistrationOptions {
        scope,
        cwd: std::env::current_dir()?,
        force: false,
        ..RegistrationOptions::default()
    };

    match register(harness, &spec, &options) {
        Ok(result) => print_registration(result),
        Err(kurir::Error::Conflict { .. }) => {
            if !Confirm::new("Replace the existing MCP entry? Kurir creates a backup.")
                .with_default(false)
                .prompt()?
            {
                println!("The existing registration was not changed.");
                return Ok(());
            }
            let options = RegistrationOptions {
                force: true,
                ..options
            };
            print_registration(register(harness, &spec, &options)?)
        }
        Err(error) => return Err(error.into()),
    }
    Ok(())
}
fn build_server_spec(
    server_name: String,
    connection_id: &str,
    databases: &[String],
    allow_writes: bool,
) -> ServerSpec {
    let mut env = BTreeMap::new();
    env.insert(
        "RDB_MCP_SCOPE_CONNECTION_IDS".to_string(),
        connection_id.to_string(),
    );
    env.insert("RDB_MCP_SCOPE_DATABASES".to_string(), databases.join(","));
    if allow_writes {
        env.insert("RDB_MCP_ALLOW_WRITES".to_string(), "1".to_string());
    }
    let mut spec = ServerSpec::stdio(server_name, "rdb-mcp", Vec::new());
    spec.env = env;
    spec
}

async fn database_choices(
    connection: &SavedConnection,
    config: &rdb_core::conn::ConnConfig,
) -> Result<Vec<String>, Box<dyn Error>> {
    if connection.engine == Engine::Sqlite {
        return Ok(vec!["main".to_string()]);
    }
    let driver = AnyDriver::connect(connection.engine, config).await?;
    let mut databases = match driver.list_databases().await {
        Ok(databases) => databases,
        Err(error) => {
            eprintln!("rdb-cli: database discovery failed; use the saved database or enter one manually: {error}");
            Vec::new()
        }
    };
    if let Some(default) = connection
        .database
        .as_deref()
        .filter(|value| !value.is_empty())
    {
        if !databases.contains(&default.to_string()) {
            databases.push(default.to_string());
        }
    }
    if (connection.engine == Engine::Redis || connection.engine == Engine::Valkey)
        && databases.is_empty()
    {
        databases.push(
            connection
                .database
                .clone()
                .unwrap_or_else(|| "0".to_string()),
        );
    }
    if databases.is_empty() {
        let database =
            Text::new("Database name (the server returned no database list)").prompt()?;
        databases.push(database.trim().to_string());
    }
    databases.sort();
    databases.dedup();
    Ok(databases)
}

fn open_store() -> Result<ConnStore, Box<dyn Error>> {
    if let Some(dir) = std::env::var_os("RDB_STORE_DIR") {
        let dir = std::path::PathBuf::from(dir);
        let backend = Box::new(rdb_connstore::EncryptedFileBackend::new(&dir)?);
        return Ok(ConnStore::load(dir.join("connections.json"), backend)?);
    }
    Ok(ConnStore::open_default()?)
}

fn print_registration(result: kurir::RegistrationResult) {
    let target = result
        .target
        .as_deref()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|| "portable snippet".to_string());
    println!("{}: {} ({target})", result.action, result.name);
}

fn install_crypto_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_spec_defaults_to_scoped_read_only_stdio() {
        let spec = build_server_spec(
            "rdb-connection".into(),
            "connection-id",
            &["app".into()],
            false,
        );

        assert_eq!(spec.command.as_deref(), Some("rdb-mcp"));
        assert_eq!(spec.transport, kurir::Transport::Stdio);
        assert_eq!(
            spec.env
                .get("RDB_MCP_SCOPE_CONNECTION_IDS")
                .map(String::as_str),
            Some("connection-id")
        );
        assert_eq!(
            spec.env.get("RDB_MCP_SCOPE_DATABASES").map(String::as_str),
            Some("app")
        );
        assert!(!spec.env.contains_key("RDB_MCP_ALLOW_WRITES"));
    }

    #[test]
    fn server_spec_enables_writes_only_for_explicit_opt_in() {
        let spec = build_server_spec(
            "rdb-connection".into(),
            "connection-id",
            &["app".into()],
            true,
        );
        assert_eq!(
            spec.env.get("RDB_MCP_ALLOW_WRITES").map(String::as_str),
            Some("1")
        );
    }
}
