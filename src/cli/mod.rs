//! Command-line interface for BeemFlow
//!
//! Provides CLI access to all operations via auto-generated commands from metadata.
//! Uses the same DRY approach as HTTP routes and MCP tools.

use crate::Result;
use crate::config::Config;
use crate::core::{OperationMetadata, OperationRegistry};
use clap::{Arg, ArgAction, ArgMatches, Command};
use serde_json::Value;
use std::collections::HashMap;

/// Convert String to 'static str for CLI command building
///
/// This uses String::leak() which is appropriate for CLIs because:
/// 1. Commands are built once at program startup
/// 2. Program exits immediately after parsing (short-lived process)
/// 3. OS reclaims all memory on exit
/// 4. This is the standard Rust pattern for dynamic CLI generation with clap
///
/// Alternative approaches (and why they don't work here):
/// - Static strings: Can't be used for dynamic metadata-driven commands
/// - Owned storage: Clap's builder API requires 'static lifetime
/// - Runtime API: Would require rewriting all command logic
fn to_static_str(s: String) -> &'static str {
    s.leak()
}

/// Create operation registry with dependencies
async fn create_registry() -> Result<OperationRegistry> {
    let config = Config::load_and_inject(crate::constants::CONFIG_FILE_NAME)?;
    let deps = crate::core::create_dependencies(&config).await?;
    Ok(OperationRegistry::new(deps))
}

/// Main CLI entry point
pub async fn run() -> Result<()> {
    // Create registry for operation access
    let registry = create_registry().await?;

    // Build CLI from operation metadata (same pattern as HTTP/MCP use metadata)
    let app = build_cli(&registry);
    let matches = app.get_matches();

    // Handle special commands (not operations)
    match matches.subcommand() {
        Some(("serve", sub_matches)) => {
            let host = sub_matches.get_one::<String>("host").unwrap();
            let port = sub_matches
                .get_one::<String>("port")
                .and_then(|s| s.parse::<u16>().ok())
                .unwrap_or(crate::constants::DEFAULT_HTTP_PORT);
            return serve_http(host, port).await;
        }
        Some(("cron", _)) => {
            return run_cron_check().await;
        }
        Some(("mcp", sub_matches)) => {
            if let Some(("serve", mcp_serve_matches)) = sub_matches.subcommand() {
                let stdio = mcp_serve_matches.get_flag("stdio");
                return serve_mcp(stdio).await;
            }
        }
        _ => {}
    }

    // Try to dispatch to an operation (uses registry.execute() like MCP does)
    if let Some((op_name, input)) = dispatch_to_operation(&matches, &registry)? {
        let result = registry.execute(&op_name, input).await?;
        println!("{}", serde_json::to_string_pretty(&result)?);
        return Ok(());
    }

    // No command matched
    eprintln!("No command specified. Use --help for usage information.");
    std::process::exit(1);
}

// ============================================================================
// CLI Building (from operation metadata)
// ============================================================================

/// Build CLI from operations metadata (same DRY principle as HTTP/MCP)
fn build_cli(registry: &OperationRegistry) -> Command {
    let mut app = Command::new("flow")
        .about("BeemFlow - Workflow orchestration runtime")
        .version(env!("CARGO_PKG_VERSION"));

    // Add special commands that aren't operations
    app = app
        .subcommand(
            Command::new("serve")
                .about("Start the HTTP server")
                .arg(
                    Arg::new("host")
                        .long("host")
                        .default_value("127.0.0.1")
                        .help("Host to bind to"),
                )
                .arg(
                    Arg::new("port")
                        .long("port")
                        .short('p')
                        .default_value("3330")
                        .help("Port to bind to"),
                ),
        )
        .subcommand(Command::new("cron").about("Run cron checks"));

    // Build operation commands from metadata
    add_operation_commands(app, registry)
}

/// Build commands from operation metadata (mirrors HTTP route generation)
fn add_operation_commands(mut app: Command, registry: &OperationRegistry) -> Command {
    let metadata = registry.get_all_metadata();

    // Group operations by CLI structure
    let mut grouped: HashMap<String, Vec<(&String, &OperationMetadata)>> = HashMap::new();

    for (op_name, meta) in metadata {
        if let Some(cli_pattern) = meta.cli_pattern {
            let words: Vec<&str> = cli_pattern
                .split_whitespace()
                .take_while(|w| !w.starts_with('<') && !w.starts_with('['))
                .collect();

            let group = words.first().map(|s| s.to_string()).unwrap_or_default();
            grouped.entry(group).or_default().push((op_name, meta));
        }
    }

    // Build subcommands for each group
    for (group_name, ops) in grouped {
        if group_name.is_empty() {
            continue;
        }

        // Use to_static_str to satisfy clap's 'static lifetime requirement
        let group_name_static = to_static_str(group_name.clone());
        let group_about = to_static_str(format!("{} operations", group_name));
        let mut group_cmd = Command::new(group_name_static).about(group_about);

        for (op_name, meta) in ops {
            if let Some(cli_pattern) = meta.cli_pattern {
                // cli_pattern has 'static lifetime, so words do too
                let words: Vec<&'static str> = cli_pattern.split_whitespace().collect();
                let subcmd_name = words.get(1).copied().unwrap_or(words[0]);

                let cmd = build_operation_command(op_name, meta, subcmd_name);
                group_cmd = group_cmd.subcommand(cmd);
            }
        }

        // Add special "mcp serve" subcommand to the mcp group
        if group_name == "mcp" {
            group_cmd = group_cmd.subcommand(
                Command::new("serve")
                    .about("Start MCP server (expose BeemFlow as MCP tools)")
                    .arg(
                        Arg::new("stdio")
                            .long("stdio")
                            .action(ArgAction::SetTrue)
                            .help("Use stdio transport (default)"),
                    ),
            );
        }

        app = app.subcommand(group_cmd);
    }

    app
}

/// Build a clap Command for an operation using its schema
fn build_operation_command(
    _op_name: &str,
    meta: &OperationMetadata,
    cmd_name: &'static str,
) -> Command {
    let mut cmd = Command::new(cmd_name).about(meta.description);

    // Extract field information from JSON schema
    if let Some(properties) = meta.schema.get("properties").and_then(|p| p.as_object()) {
        let required: Vec<&str> = meta
            .schema
            .get("required")
            .and_then(|r| r.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect())
            .unwrap_or_default();

        let cli_pattern = meta.cli_pattern.unwrap_or("");
        let mut positional_index = 1;

        for (field_name, field_schema) in properties {
            let is_required = required.contains(&field_name.as_str());
            let field_type = field_schema
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("string");
            let description = field_schema
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("");

            // Check if this is a positional arg in the pattern
            let is_positional = cli_pattern.contains(&format!("<{}>", field_name.to_uppercase()));

            // Use to_static_str for clap's 'static lifetime requirement
            let field_name_static = to_static_str(field_name.clone());
            let description_static = to_static_str(description.to_string());

            if is_positional {
                cmd = cmd.arg(
                    Arg::new(field_name_static)
                        .required(is_required)
                        .index(positional_index)
                        .help(description_static),
                );
                positional_index += 1;
            } else if field_type == "boolean" {
                cmd = cmd.arg(
                    Arg::new(field_name_static)
                        .long(field_name_static)
                        .action(ArgAction::SetTrue)
                        .help(description_static),
                );
            } else {
                cmd = cmd.arg(
                    Arg::new(field_name_static)
                        .long(field_name_static)
                        .required(is_required)
                        .help(description_static),
                );
            }
        }
    }

    cmd
}

/// Dispatch CLI matches to operation (uses registry.execute() like MCP)
fn dispatch_to_operation(
    matches: &ArgMatches,
    registry: &OperationRegistry,
) -> Result<Option<(String, Value)>> {
    let metadata = registry.get_all_metadata();

    // Check for two-level subcommands (group subcmd)
    if let Some((group, group_matches)) = matches.subcommand()
        && let Some((subcmd, subcmd_matches)) = group_matches.subcommand()
    {
        let prefix = format!("{} {}", group, subcmd);

        // Find matching operation
        for (op_name, meta) in metadata {
            if let Some(cli_pattern) = meta.cli_pattern
                && cli_pattern.starts_with(&prefix)
            {
                let input = extract_input_from_matches(subcmd_matches, meta)?;
                return Ok(Some((op_name.clone(), input)));
            }
        }
    }

    Ok(None)
}

/// Extract operation input from CLI arguments using schema
fn extract_input_from_matches(matches: &ArgMatches, meta: &OperationMetadata) -> Result<Value> {
    let mut input = serde_json::Map::new();

    if let Some(properties) = meta.schema.get("properties").and_then(|p| p.as_object()) {
        for (field_name, field_schema) in properties {
            let field_type = field_schema
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("string");

            if field_type == "boolean" {
                if matches.get_flag(field_name.as_str()) {
                    input.insert(field_name.clone(), serde_json::json!(true));
                }
            } else if let Some(value_str) = matches.get_one::<String>(field_name.as_str()) {
                let parsed = match field_type {
                    "integer" | "number" => value_str
                        .parse::<i64>()
                        .map(|n| serde_json::json!(n))
                        .unwrap_or_else(|_| serde_json::json!(value_str)),
                    "object" | "array" => serde_json::from_str(value_str)
                        .unwrap_or_else(|_| serde_json::json!(value_str)),
                    _ => serde_json::json!(value_str),
                };
                input.insert(field_name.clone(), parsed);
            }
        }
    }

    Ok(serde_json::json!(input))
}

// ============================================================================
// Special Commands (not operations)
// ============================================================================

/// Start HTTP API server (special command - not an operation)
async fn serve_http(host: &str, port: u16) -> Result<()> {
    println!("🚀 Starting BeemFlow HTTP server on {}:{}", host, port);
    println!("   Access API at http://{}:{}", host, port);
    println!("   Press Ctrl+C to stop\n");

    let mut config = Config::default();
    config.http = Some(crate::config::HttpConfig {
        host: host.to_string(),
        port,
        secure: false,
    });

    crate::http::start_server(config).await?;

    Ok(())
}

/// Start MCP server (special command - not an operation)
async fn serve_mcp(_stdio: bool) -> Result<()> {
    println!("🚀 Starting BeemFlow MCP server");
    println!("   Exposes BeemFlow operations as MCP tools");
    println!("   Press Ctrl+C to stop\n");

    // Create registry and MCP server
    let registry = create_registry().await?;
    let mcp_server = crate::mcp::McpServer::new(std::sync::Arc::new(registry));

    // Start MCP server on stdio
    mcp_server.serve_stdio().await?;

    Ok(())
}

/// Run cron check (special command - not an operation)
async fn run_cron_check() -> Result<()> {
    eprintln!("Cron functionality temporarily disabled during refactoring");
    Ok(())
}
