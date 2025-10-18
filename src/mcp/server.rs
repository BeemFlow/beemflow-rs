//! MCP Server implementation
//!
//! Exposes BeemFlow operations as MCP tools for AI assistants (Claude Desktop, ChatGPT, etc.)
//! Uses the official `rmcp` SDK with auto-generation from operation metadata.

use crate::Result;
use crate::auth::middleware::validate_token;
use crate::core::OperationRegistry;
use crate::storage::Storage;
use axum::{
    Json, Router,
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
    routing::{any, get},
};
use rmcp::{
    ErrorData as McpError,
    handler::server::ServerHandler,
    model::{
        CallToolRequestParam, CallToolResult, Content, ListToolsResult, PaginatedRequestParam,
        ServerCapabilities, ServerInfo, Tool, ToolsCapability,
    },
    service::{RequestContext, RoleServer, ServiceExt},
    transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    },
};
use serde_json::{Value, json};
use std::sync::Arc;
use std::time::Duration;

/// State required for MCP routes
#[derive(Clone)]
pub struct McpServerState {
    pub operations: Arc<OperationRegistry>,
    pub oauth_issuer: Option<String>, // None = no auth
    pub storage: Arc<dyn Storage>,
}

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

    /// Serves the MCP server over Streamable HTTP with OAuth authentication.
    ///
    /// Uses the MCP 2025-03-26 Streamable HTTP transport specification, which replaces
    /// the deprecated HTTP+SSE transport from protocol version 2024-11-05.
    ///
    /// # Arguments
    /// * `host` - Host address to bind to (e.g., "127.0.0.1")
    /// * `port` - Port number to listen on
    /// * `oauth_issuer` - OAuth authorization server URL for token validation
    /// * `storage` - Storage backend for OAuth token validation
    ///
    /// # Security
    /// Requires Bearer token authentication with `mcp` scope prefix.
    /// Tokens are validated via the OAuth issuer and must have one of:
    /// - `mcp` (full access)
    /// - `mcp:read` (read-only)
    /// - `mcp:write` (write access)
    ///
    /// # Endpoints
    /// - `POST/GET/DELETE /mcp` - Unified MCP endpoint (Streamable HTTP)
    /// - `GET /.well-known/oauth-protected-resource/mcp` - RFC 9728 metadata
    /// - `GET /.well-known/oauth-protected-resource` - Root metadata
    ///
    /// # Streamable HTTP Transport
    /// The single `/mcp` endpoint handles:
    /// - POST: Send JSON-RPC messages, receive JSON responses or event streams
    /// - GET: Open event stream for server-initiated messages
    /// - DELETE: Close session and clean up resources
    ///
    /// # Example
    /// ```no_run
    /// # use beemflow::mcp::McpServer;
    /// # use beemflow::core::OperationRegistry;
    /// # use beemflow::utils::TestEnvironment;
    /// # use std::sync::Arc;
    /// # #[tokio::main]
    /// # async fn main() -> beemflow::Result<()> {
    /// # let env = TestEnvironment::new().await;
    /// # let ops = Arc::new(OperationRegistry::new(env.deps.clone()));
    /// # let server = McpServer::new(ops);
    /// server.serve_http(
    ///     "127.0.0.1",
    ///     3001,
    ///     "http://localhost:3000".to_string(),
    ///     env.deps.storage,
    /// ).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn serve_http(
        &self,
        host: &str,
        port: u16,
        oauth_issuer: String,
        storage: Arc<dyn Storage>,
    ) -> Result<()> {
        use rmcp::transport::streamable_http_server::{
            StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
        };
        use std::time::Duration;

        tracing::info!("Starting MCP server (Streamable HTTP) on {}:{}", host, port);

        let auth_state = Arc::new(McpAuthState {
            storage,
            oauth_issuer: oauth_issuer.clone(),
        });
        let metadata_state = Arc::new(McpMetadataState {
            base_url: format!("http://{}:{}", host, port),
            oauth_issuer,
        });

        let addr: std::net::SocketAddr = format!("{}:{}", host, port)
            .parse()
            .map_err(|e| crate::BeemFlowError::config(format!("Invalid address: {}", e)))?;

        // Create StreamableHttpService with default local session manager
        let streamable_config = StreamableHttpServerConfig {
            sse_keep_alive: Some(Duration::from_secs(15)),
            stateful_mode: true, // Enable sessions for persistent connections
        };

        let mcp_handler = self.clone();
        let streamable_service = StreamableHttpService::new(
            move || Ok(mcp_handler.clone()),
            Arc::new(LocalSessionManager::default()),
            streamable_config,
        );

        // Wrap MCP service with OAuth middleware
        let mcp_route = Router::new()
            .route(
                "/mcp",
                axum::routing::any(move |req| async move {
                    streamable_service.clone().handle(req).await
                }),
            )
            .layer(axum::middleware::from_fn_with_state(
                auth_state,
                mcp_oauth_middleware,
            ));

        // Add metadata routes
        let metadata_routes = Router::new()
            .route(
                "/.well-known/oauth-protected-resource/mcp",
                get(mcp_resource_metadata),
            )
            .route(
                "/.well-known/oauth-protected-resource",
                get(root_resource_metadata),
            )
            .with_state(metadata_state);

        let app = Router::new().merge(mcp_route).merge(metadata_routes);

        let listener = tokio::net::TcpListener::bind(addr)
            .await
            .map_err(|e| crate::BeemFlowError::config(format!("Failed to bind {}: {}", addr, e)))?;

        tracing::info!("✅ MCP Streamable HTTP server running on http://{}", addr);
        tracing::info!("   Unified endpoint: http://{}/mcp (POST/GET/DELETE)", addr);
        tracing::info!(
            "   OAuth metadata: http://{}/.well-known/oauth-protected-resource/mcp",
            addr
        );
        tracing::info!("   Authorization: Bearer token with 'mcp' scope required");
        tracing::info!("   Transport: MCP 2025-03-26 Streamable HTTP (replaces deprecated SSE)");

        axum::serve(listener, app)
            .await
            .map_err(|e| crate::BeemFlowError::internal(format!("Server error: {}", e)))?;

        Ok(())
    }

    /// Auto-generate MCP tools from operation metadata using generated registration functions
    fn get_tools_list(&self) -> Vec<Tool> {
        let deps = self.operations.get_dependencies();

        // Call generated registration functions from each operation group
        let mut tools: Vec<Tool> = [
            crate::core::flows::flows::register_mcp_tools,
            crate::core::runs::runs::register_mcp_tools,
            crate::core::tools::tools::register_mcp_tools,
            crate::core::mcp::mcp::register_mcp_tools,
            crate::core::system::system::register_mcp_tools,
        ]
        .into_iter()
        .flat_map(|register_fn| register_fn(deps.clone()))
        .collect();

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

// OAuth middleware state for MCP
#[derive(Clone)]
pub struct McpAuthState {
    pub storage: Arc<dyn Storage>,
    pub oauth_issuer: String,
}

// OAuth middleware for MCP
pub async fn mcp_oauth_middleware(
    State(state): State<Arc<McpAuthState>>,
    request: Request,
    next: Next,
) -> Response {
    let token = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "));

    let Some(token) = token else {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            [(
                axum::http::header::WWW_AUTHENTICATE,
                format!(
                    "Bearer realm=\"BeemFlow MCP\", \
                         resource_metadata=\"{}/.well-known/oauth-protected-resource/mcp\", \
                         scope=\"mcp\"",
                    state.oauth_issuer
                ),
            )],
            "Unauthorized",
        )
            .into_response();
    };

    match validate_token(&state.storage, token).await {
        Ok(user) if user.scopes.iter().any(|s| s.starts_with("mcp")) => next.run(request).await,
        Ok(_) => (axum::http::StatusCode::FORBIDDEN, "Insufficient scopes").into_response(),
        Err(e) => {
            tracing::warn!("MCP OAuth failed: {}", e);
            (axum::http::StatusCode::UNAUTHORIZED, "Invalid token").into_response()
        }
    }
}

// Protected Resource Metadata (RFC 9728)
pub struct McpMetadataState {
    pub base_url: String,
    pub oauth_issuer: String,
}

async fn mcp_resource_metadata(State(state): State<Arc<McpMetadataState>>) -> Json<Value> {
    Json(json!({
        "resource": format!("{}/mcp", state.base_url),
        "authorization_servers": [state.oauth_issuer],
        "scopes_supported": ["mcp", "mcp:read", "mcp:write"],
        "bearer_methods_supported": ["header"],
    }))
}

async fn root_resource_metadata(State(state): State<Arc<McpMetadataState>>) -> Json<Value> {
    Json(json!({
        "resource": state.base_url,
        "authorization_servers": [state.oauth_issuer],
    }))
}

// ============================================================================
// Route Builders (for integration into main HTTP server)
// ============================================================================

/// Create MCP routes with optional OAuth authentication
///
/// This creates the unified /mcp endpoint (POST/GET/DELETE) with optional
/// Bearer token authentication. This function is exported for use by the
/// main HTTP server to integrate MCP endpoints.
///
/// Uses the operations framework to auto-generate MCP tools from operation metadata.
/// The single /mcp endpoint multiplexes all operations via JSON-RPC protocol, unlike
/// HTTP which creates separate routes per operation.
///
/// # Arguments
/// * `state` - MCP server state with optional OAuth configuration
///
/// # Returns
/// Router with /mcp endpoint, optionally wrapped in OAuth middleware
pub fn create_mcp_routes(state: Arc<McpServerState>) -> Router {
    let mcp_server = McpServer::new(state.operations.clone());
    let streamable_service = create_streamable_service(mcp_server);

    let mut router = Router::new().route(
        "/mcp",
        any(move |req| async move { streamable_service.clone().handle(req).await }),
    );

    // Conditionally apply OAuth middleware if issuer is configured
    if let Some(oauth_issuer) = state.oauth_issuer.clone() {
        let auth_state = Arc::new(McpAuthState {
            storage: state.storage.clone(),
            oauth_issuer,
        });

        router = router.layer(axum::middleware::from_fn_with_state(
            auth_state,
            mcp_oauth_middleware,
        ));
    }

    router
}

/// Create StreamableHttpService from McpServer
fn create_streamable_service(
    mcp_server: McpServer,
) -> StreamableHttpService<McpServer, LocalSessionManager> {
    let config = StreamableHttpServerConfig {
        sse_keep_alive: Some(Duration::from_secs(15)),
        stateful_mode: true,
    };

    StreamableHttpService::new(
        move || Ok(mcp_server.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    )
}

/// Create MCP metadata routes (RFC 9728)
///
/// Returns routes for OAuth protected resource metadata.
/// These routes are PUBLIC (no auth required).
///
/// # Arguments
/// * `oauth_issuer` - OAuth authorization server URL
/// * `base_url` - Base URL of this server
///
/// # Returns
/// Router with RFC 9728 metadata endpoints
pub fn create_mcp_metadata_routes(oauth_issuer: String, base_url: String) -> Router {
    let metadata_state = Arc::new(McpMetadataState {
        base_url,
        oauth_issuer,
    });

    Router::new()
        .route(
            "/.well-known/oauth-protected-resource/mcp",
            get(mcp_resource_metadata),
        )
        .route(
            "/.well-known/oauth-protected-resource",
            get(root_resource_metadata),
        )
        .with_state(metadata_state)
}
