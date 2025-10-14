//! Command-line interface for BeemFlow
//!
//! Provides the `flow` CLI tool that delegates all operations to the unified registry.

use crate::Result;
use crate::core::OperationRegistry;
use crate::dsl::{Validator, parse_file};
// TODO: Refactor cron
// use crate::cron::CronManager;
use crate::config::Config;
use clap::{Parser as ClapParser, Subcommand};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(ClapParser)]
#[command(name = "flow")]
#[command(about = "BeemFlow - Workflow orchestration runtime", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run a flow from a YAML file
    Run {
        /// Path to flow file
        file: String,

        /// Event data as JSON
        #[arg(long)]
        event: Option<String>,

        /// Run in draft mode
        #[arg(long)]
        draft: bool,
    },

    /// Validate a flow file
    Validate {
        /// Path to flow file
        file: String,
    },

    /// List all flows
    List,

    /// Get a specific flow
    Get {
        /// Flow name
        name: String,
    },

    /// Save a flow
    Save {
        /// Flow name
        name: String,

        /// Path to flow file
        #[arg(long, short)]
        file: Option<String>,
    },

    /// Delete a flow
    Delete {
        /// Flow name
        name: String,
    },

    /// Deploy a flow to production
    Deploy {
        /// Flow name
        name: String,
    },

    /// Rollback a flow to a specific version
    Rollback {
        /// Flow name
        name: String,

        /// Version to rollback to
        version: String,
    },

    /// Show flow version history
    History {
        /// Flow name
        name: String,
    },

    /// Generate Mermaid diagram for a flow
    Graph {
        /// Path to flow file
        file: String,

        /// Output file path (default: stdout)
        #[arg(long, short)]
        output: Option<String>,
    },

    /// Lint a flow file
    Lint {
        /// Path to flow file
        file: String,
    },

    /// Run commands
    #[command(subcommand)]
    Runs(RunsCommands),

    /// Tool commands
    #[command(subcommand)]
    Tools(ToolsCommands),

    /// MCP commands
    #[command(subcommand)]
    Mcp(McpCommands),

    /// Publish an event
    Publish {
        /// Topic name
        topic: String,

        /// Event payload as JSON
        #[arg(long)]
        payload: Option<String>,
    },

    /// Resume a paused run
    Resume {
        /// Resume token
        token: String,

        /// Event data as JSON
        #[arg(long)]
        event: Option<String>,
    },

    /// Show BeemFlow specification
    Spec,

    /// Run cron checks for scheduled workflows
    Cron,

    /// Start the HTTP server
    Serve {
        /// Host to bind to
        #[arg(long, default_value = "127.0.0.1")]
        host: String,

        /// Port to bind to
        #[arg(long, short, default_value_t = crate::constants::DEFAULT_HTTP_PORT)]
        port: u16,
    },
}

#[derive(Subcommand)]
enum RunsCommands {
    /// Start a new run
    Start {
        /// Flow name
        flow_name: String,

        /// Event data as JSON
        #[arg(long)]
        event: Option<String>,

        /// Run draft version
        #[arg(long)]
        draft: bool,
    },

    /// Get run details
    Get {
        /// Run ID (UUID)
        run_id: String,
    },

    /// List all runs
    List,
}

#[derive(Subcommand)]
enum ToolsCommands {
    /// List all tools
    List,

    /// Get tool manifest
    Get {
        /// Tool name
        name: String,
    },

    /// Search for tools
    Search {
        /// Search query
        query: Option<String>,
    },

    /// Install a tool
    Install {
        /// Tool name from registry or path to manifest file
        source: String,
    },

    /// Convert OpenAPI spec to BeemFlow tools
    Convert {
        /// Path to OpenAPI spec file
        file: String,

        /// API name prefix
        #[arg(long)]
        api_name: Option<String>,

        /// Base URL override
        #[arg(long)]
        base_url: Option<String>,

        /// Output file
        #[arg(long, short)]
        output: Option<String>,
    },
}

#[derive(Subcommand)]
enum McpCommands {
    /// List MCP servers
    List,

    /// Search MCP servers
    Search {
        /// Search query
        query: Option<String>,
    },

    /// Install MCP server
    Install {
        /// Server name
        name: String,
    },

    /// Start MCP server (expose BeemFlow as MCP tools)
    Serve {
        /// Use stdio transport (for Claude Desktop) - default
        #[arg(long, default_value_t = true)]
        stdio: bool,

        /// Use HTTP transport at specified address
        #[arg(long)]
        http: Option<String>,
    },
}

/// Create operation registry with dependencies
async fn create_registry() -> Result<OperationRegistry> {
    // Load configuration to get storage settings
    let config = Config::load_and_inject(crate::constants::CONFIG_FILE_NAME)?;

    // Use centralized dependency creation from core module
    let deps = crate::core::create_dependencies(&config).await?;

    Ok(OperationRegistry::new(deps))
}

pub async fn run() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Run {
            file,
            event,
            draft: _,
        } => {
            run_flow(&file, event).await?;
        }
        Commands::Validate { file } => {
            validate_flow(&file)?;
        }
        Commands::List => {
            let registry = create_registry().await?;
            let result = registry
                .execute("list_flows", serde_json::json!({}))
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Commands::Get { name } => {
            let registry = create_registry().await?;
            let result = registry
                .execute("get_flow", serde_json::json!({ "name": name }))
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Commands::Save { name, file } => {
            let registry = create_registry().await?;
            let content = if let Some(path) = file {
                std::fs::read_to_string(path)?
            } else {
                return Err(crate::BeemFlowError::validation("--file is required"));
            };
            let result = registry
                .execute(
                    "save_flow",
                    serde_json::json!({
                        "name": name,
                        "content": content
                    }),
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Commands::Delete { name } => {
            let registry = create_registry().await?;
            let result = registry
                .execute("delete_flow", serde_json::json!({ "name": name }))
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Commands::Deploy { name } => {
            let registry = create_registry().await?;
            let result = registry
                .execute("deploy_flow", serde_json::json!({ "name": name }))
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Commands::Rollback { name, version } => {
            let registry = create_registry().await?;
            let result = registry
                .execute(
                    "rollback_flow",
                    serde_json::json!({
                        "name": name,
                        "version": version
                    }),
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Commands::History { name } => {
            let registry = create_registry().await?;
            let result = registry
                .execute("flow_history", serde_json::json!({ "name": name }))
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Commands::Graph { file, output } => {
            graph_flow(&file, output)?;
        }
        Commands::Lint { file } => {
            let registry = create_registry().await?;
            let result = registry
                .execute("lint_flow", serde_json::json!({ "file": file }))
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Commands::Runs(runs_cmd) => {
            let registry = create_registry().await?;
            match runs_cmd {
                RunsCommands::Start {
                    flow_name,
                    event,
                    draft,
                } => {
                    let event_data: Option<HashMap<String, serde_json::Value>> =
                        if let Some(json_str) = event {
                            Some(serde_json::from_str(&json_str)?)
                        } else {
                            None
                        };
                    let result = registry
                        .execute(
                            "start_run",
                            serde_json::json!({
                                "flow_name": flow_name,
                                "event": event_data,
                                "draft": draft
                            }),
                        )
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                RunsCommands::Get { run_id } => {
                    let result = registry
                        .execute("get_run", serde_json::json!({ "run_id": run_id }))
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                RunsCommands::List => {
                    let result = registry.execute("list_runs", serde_json::json!({})).await?;
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
            }
        }
        Commands::Tools(tools_cmd) => {
            let registry = create_registry().await?;
            match tools_cmd {
                ToolsCommands::List => {
                    let result = registry
                        .execute("list_tools", serde_json::json!({}))
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                ToolsCommands::Get { name } => {
                    let result = registry
                        .execute("get_tool_manifest", serde_json::json!({ "name": name }))
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                ToolsCommands::Search { query } => {
                    let result = registry
                        .execute("search_tools", serde_json::json!({ "query": query }))
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                ToolsCommands::Install { source } => {
                    let result = registry
                        .execute("install_tool", serde_json::json!({ "name": source }))
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                ToolsCommands::Convert {
                    file,
                    api_name,
                    base_url,
                    output: _,
                } => {
                    let openapi_content = std::fs::read_to_string(file)?;
                    let result = registry
                        .execute(
                            "convert_openapi",
                            serde_json::json!({
                                "openapi": openapi_content,
                                "api_name": api_name,
                                "base_url": base_url
                            }),
                        )
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
            }
        }
        Commands::Mcp(mcp_cmd) => {
            let registry = create_registry().await?;
            match mcp_cmd {
                McpCommands::List => {
                    let result = registry
                        .execute("list_mcp_servers", serde_json::json!({}))
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                McpCommands::Search { query } => {
                    let result = registry
                        .execute("search_mcp_servers", serde_json::json!({ "query": query }))
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                McpCommands::Install { name } => {
                    let result = registry
                        .execute("install_mcp_server", serde_json::json!({ "name": name }))
                        .await?;
                    println!("{}", serde_json::to_string_pretty(&result)?);
                }
                McpCommands::Serve { stdio, http: _ } => {
                    if stdio {
                        serve_mcp_stdio(registry).await?;
                    } else {
                        eprintln!("HTTP transport not yet implemented, use --stdio");
                        std::process::exit(1);
                    }
                }
            }
        }
        Commands::Publish { topic, payload } => {
            let registry = create_registry().await?;
            let payload_data: HashMap<String, serde_json::Value> = if let Some(json_str) = payload {
                serde_json::from_str(&json_str)?
            } else {
                HashMap::new()
            };
            let result = registry
                .execute(
                    "publish_event",
                    serde_json::json!({
                        "topic": topic,
                        "payload": payload_data
                    }),
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Commands::Resume { token, event } => {
            let registry = create_registry().await?;
            let event_data: Option<HashMap<String, serde_json::Value>> =
                if let Some(json_str) = event {
                    Some(serde_json::from_str(&json_str)?)
                } else {
                    None
                };
            let result = registry
                .execute(
                    "resume_run",
                    serde_json::json!({
                        "token": token,
                        "event": event_data
                    }),
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Commands::Spec => {
            let registry = create_registry().await?;
            let result = registry.execute("spec", serde_json::json!({})).await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        Commands::Cron => {
            run_cron_check().await?;
        }
        Commands::Serve { host, port } => {
            serve_http(&host, port).await?;
        }
    }

    Ok(())
}

/// Run a flow from a file
async fn run_flow(file: &str, event_json: Option<String>) -> Result<()> {
    tracing::info!("Running flow from: {}", file);

    // Parse flow
    let flow = parse_file(file, None)?;

    // Validate flow
    Validator::validate(&flow)?;

    // Parse event data
    let event: HashMap<String, serde_json::Value> = if let Some(json) = event_json {
        serde_json::from_str(&json)?
    } else {
        HashMap::new()
    };

    // Create registry with proper dependencies (including shared storage)
    let registry = create_registry().await?;
    let engine = registry.get_dependencies().engine.clone();
    let result = engine.execute(&flow, event).await?;

    // Print outputs
    println!("\n✅ Flow executed successfully");
    println!("\nRun ID: {}", result.run_id);
    println!("\nOutputs:");
    for (step_id, output) in result.outputs {
        println!("\n[{}]", step_id);
        if let Ok(pretty) = serde_json::to_string_pretty(&output) {
            println!("{}", pretty);
        } else {
            println!("{:?}", output);
        }
    }

    Ok(())
}

/// Validate a flow file
fn validate_flow(file: &str) -> Result<()> {
    tracing::info!("Validating flow: {}", file);

    // Parse flow
    let flow = parse_file(file, None)?;

    // Validate flow
    Validator::validate(&flow)?;

    println!("✅ Validation OK: flow is valid!");

    Ok(())
}

/// Generate Mermaid diagram for a flow
fn graph_flow(file: &str, output: Option<String>) -> Result<()> {
    tracing::info!("Generating graph for: {}", file);

    // Parse flow
    let flow = parse_file(file, None)?;

    // Generate diagram
    let diagram = crate::graph::GraphGenerator::generate(&flow)?;

    // Output
    if let Some(output_path) = output {
        std::fs::write(&output_path, diagram)?;
        println!("✅ Graph written to: {}", output_path);
    } else {
        println!("{}", diagram);
    }

    Ok(())
}

/// Serve HTTP API
async fn serve_http(host: &str, port: u16) -> Result<()> {
    println!("🚀 Starting BeemFlow HTTP server on {}:{}", host, port);
    println!("   Press Ctrl+C to stop");

    // Create config
    let mut config = Config::default();
    config.http = Some(crate::config::HttpConfig {
        host: host.to_string(),
        port,
        secure: false, // CLI defaults to insecure for local development
    });

    // Start server
    crate::http::start_server(config).await?;

    Ok(())
}

/// Run cron check for scheduled workflows
async fn run_cron_check() -> Result<()> {
    // TODO: Refactor cron to use OperationRegistry
    eprintln!("Cron functionality temporarily disabled during refactoring");
    Ok(())

    /* Temporarily disabled - needs refactoring to use OperationRegistry
    let _registry = create_registry().await?;

    // Create cron manager with default settings
    let server_url = "http://localhost:3000".to_string(); // Default for CLI usage
    let cron_secret = None; // No secret for CLI usage

    let cron_manager = CronManager::new(server_url, cron_secret);

    // Run cron check
    let result = cron_manager.check_and_execute_cron_flows().await?;

    // Print results
    println!("Cron check completed:");
    println!("- Status: {}", result.status);
    println!("- Timestamp: {}", result.timestamp);
    println!("- Workflows checked: {}", result.checked);
    println!("- Workflows triggered: {}", result.triggered);
    println!("- Errors: {}", result.errors.len());

    if !result.workflows.is_empty() {
        println!("\nTriggered workflows:");
        for workflow in &result.workflows {
            println!("  - {}", workflow);
        }
    }

    if !result.errors.is_empty() {
        println!("\nErrors:");
        for error in &result.errors {
            println!("  - {}", error);
        }
    }

    Ok(())
    */
}

/// Start MCP server on stdio
async fn serve_mcp_stdio(registry: OperationRegistry) -> Result<()> {
    use crate::mcp::McpServer;

    // MCP protocol requires ONLY JSON-RPC messages on stdout
    // All diagnostic output is handled by tracing

    let server = McpServer::new(Arc::new(registry));
    server.serve_stdio().await
}
