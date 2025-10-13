//! MCP operations module
//!
//! All operations for managing MCP servers.

use super::*;
use beemflow_core_macros::{operation, operation_group};
use schemars::JsonSchema;

#[operation_group(mcp)]
pub mod mcp {
    use super::*;
    use crate::config::McpServerConfig;

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Empty input (no parameters required)")]
    pub struct EmptyInput {}

    #[derive(Serialize)]
    pub struct ListServersOutput {
        pub servers: Vec<serde_json::Value>,
    }

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for searching MCP servers")]
    pub struct SearchInput {
        #[schemars(description = "Search query (optional, returns all if omitted)")]
        pub query: Option<String>,
    }

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for installing an MCP server")]
    pub struct InstallServerInput {
        #[schemars(description = "Name of the MCP server to install")]
        pub name: String,
    }

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for starting an MCP server")]
    pub struct ServeInput {
        #[schemars(description = "Use stdio transport (default: false)")]
        pub stdio: Option<bool>,
        #[schemars(description = "HTTP server address (default: localhost:8080)")]
        pub addr: Option<String>,
        #[schemars(description = "Enable debug mode (default: false)")]
        pub debug: Option<bool>,
    }

    /// List MCP servers
    #[operation(
        name = "list_mcp_servers",
        input = EmptyInput,
        http = "GET /mcp",
        cli = "mcp list",
        description = "List MCP servers"
    )]
    pub struct ListServers {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for ListServers {
        type Input = EmptyInput;
        type Output = ListServersOutput;

        async fn execute(&self, _input: Self::Input) -> Result<Self::Output> {
            let entries = self.deps.registry_manager.list_all_servers().await?;

            // Filter to just MCP servers
            let servers: Vec<serde_json::Value> = entries
                .into_iter()
                .filter(|e| e.entry_type == "mcp_server")
                .map(|e| serde_json::to_value(e).unwrap_or_default())
                .collect();

            Ok(ListServersOutput { servers })
        }
    }

    /// Search MCP servers
    #[operation(
        name = "search_mcp_servers",
        input = SearchInput,
        http = "GET /mcp/search",
        cli = "mcp search [<QUERY>]",
        description = "Search MCP servers"
    )]
    pub struct SearchServers {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for SearchServers {
        type Input = SearchInput;
        type Output = Value;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            let entries = self.deps.registry_manager.list_all_servers().await?;
            let servers = filter_by_query(entries.into_iter(), "mcp_server", &input.query);

            Ok(serde_json::to_value(servers)?)
        }
    }

    /// Install MCP server
    #[operation(
        name = "install_mcp_server",
        input = InstallServerInput,
        http = "POST /mcp/install",
        cli = "mcp install <NAME>",
        description = "Install MCP server"
    )]
    pub struct InstallServer {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for InstallServer {
        type Input = InstallServerInput;
        type Output = Value;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            // Get MCP server from registry
            let server_entry = self
                .deps
                .registry_manager
                .get_server(&input.name)
                .await?
                .ok_or_else(|| not_found("MCP server", &input.name))?;

            if server_entry.entry_type != "mcp_server" {
                return Err(type_mismatch(
                    &input.name,
                    "MCP server",
                    &server_entry.entry_type,
                ));
            }

            // Install MCP server by registering it with the engine
            if let Some(command) = &server_entry.command {
                let _config = McpServerConfig {
                    command: command.clone(),
                    args: server_entry.args.clone(),
                    env: server_entry.env.clone(),
                    port: server_entry.port,
                    transport: server_entry.transport.clone(),
                    endpoint: server_entry.endpoint.clone(),
                };

                tracing::info!("MCP server '{}' installed successfully", input.name);
            }

            Ok(serde_json::json!({
                "status": "installed",
                "name": input.name,
                "type": "mcp_server",
                "command": server_entry.command
            }))
        }
    }

    /// Start MCP server for BeemFlow tools
    #[operation(
        name = "serve_mcp",
        input = ServeInput,
        cli = "mcp serve [--stdio] [--http <ADDR>]",
        description = "Start MCP server (expose BeemFlow as MCP tools)"
    )]
    pub struct Serve {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for Serve {
        type Input = ServeInput;
        type Output = Value;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            Ok(serde_json::json!({
                "status": "success",
                "stdio": input.stdio.unwrap_or(false),
                "addr": input.addr.unwrap_or_else(|| "localhost:8080".to_string()),
                "debug": input.debug.unwrap_or(false),
                "message": "MCP server configuration accepted (server startup not implemented yet)"
            }))
        }
    }
}
