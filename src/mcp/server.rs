//! MCP Server implementation
//!
//! Exposes BeemFlow operations as MCP tools for AI assistants (Claude Desktop, ChatGPT, etc.)
//! Uses the official `rmcp` SDK with auto-generation from operation metadata.

use crate::Result;
use crate::core::OperationRegistry;
use rmcp::{
    ErrorData as McpError,
    handler::server::ServerHandler,
    model::{
        CallToolRequestParam, CallToolResult, Content, ListToolsResult, PaginatedRequestParam,
        ServerCapabilities, ServerInfo, Tool, ToolsCapability,
    },
    service::{RequestContext, RoleServer, ServiceExt},
};
use serde_json::Value;
use std::sync::Arc;

/// MCP Server that exposes BeemFlow operations as tools
pub struct McpServer {
    operations: Arc<OperationRegistry>,
}

impl McpServer {
    /// Create a new MCP server
    pub fn new(operations: Arc<OperationRegistry>) -> Self {
        Self { operations }
    }

    /// Serve over stdio (for Claude Desktop, etc.)
    pub async fn serve_stdio(&self) -> Result<()> {
        tracing::info!("Starting MCP server on stdio using official rmcp SDK");

        // Use official SDK's stdio transport and serve
        let service = self
            .clone()
            .serve(rmcp::transport::io::stdio())
            .await
            .map_err(|e| {
                crate::BeemFlowError::internal(format!("Failed to start MCP server: {}", e))
            })?;

        // Wait for completion
        service
            .waiting()
            .await
            .map_err(|e| crate::BeemFlowError::internal(format!("MCP server error: {}", e)))?;

        tracing::info!("MCP server shutdown");
        Ok(())
    }

    /// Auto-generate MCP tools from operation metadata using generated registration functions
    fn get_tools_list(&self) -> Vec<Tool> {
        let deps = self.operations.get_dependencies();
        let mut tools = Vec::new();

        // Call generated registration functions from each operation group
        tools.extend(crate::core::flows::flows::register_mcp_tools(deps.clone()));
        tools.extend(crate::core::runs::runs::register_mcp_tools(deps.clone()));
        tools.extend(crate::core::tools::tools::register_mcp_tools(deps.clone()));
        tools.extend(crate::core::mcp::mcp::register_mcp_tools(deps.clone()));
        tools.extend(crate::core::events::events::register_mcp_tools(
            deps.clone(),
        ));
        tools.extend(crate::core::system::system::register_mcp_tools(
            deps.clone(),
        ));

        // Sort tools by name for consistent output
        tools.sort_by(|a, b| a.name.cmp(&b.name));

        tracing::info!(
            "Auto-generated {} MCP tools from operation metadata",
            tools.len()
        );
        tools
    }
}

impl Clone for McpServer {
    fn clone(&self) -> Self {
        Self {
            operations: Arc::clone(&self.operations),
        }
    }
}

impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            capabilities: ServerCapabilities {
                tools: Some(ToolsCapability::default()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParam>,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ListToolsResult, McpError> {
        let tools = self.get_tools_list();

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
        })
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParam,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<CallToolResult, McpError> {
        let tool_name = request.name.as_ref();
        let arguments_map = request.arguments.clone().unwrap_or_default();
        let arguments = Value::Object(arguments_map);

        tracing::debug!("Calling tool: {} with args: {:?}", tool_name, arguments);

        // Strip "beemflow_" prefix to get the actual operation name
        let operation_name = tool_name.strip_prefix("beemflow_").unwrap_or(tool_name);

        // Execute operation via registry
        match self.operations.execute(operation_name, arguments).await {
            Ok(result) => {
                let result_text =
                    serde_json::to_string_pretty(&result).unwrap_or_else(|_| "{}".to_string());

                Ok(CallToolResult::success(vec![Content::text(result_text)]))
            }
            Err(e) => {
                let error_msg = format!("Tool execution failed: {}", e);
                tracing::error!("{}", error_msg);

                Ok(CallToolResult::error(vec![Content::text(error_msg)]))
            }
        }
    }
}
