// ! MCP Manager - Manages external MCP server processes and communication

use crate::{BeemFlowError, Result, model::McpServerConfig};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct McpTool {
    name: String,
    description: Option<String>,
    #[serde(rename = "inputSchema")]
    input_schema: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: i64,
    method: String,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[allow(dead_code)]
    jsonrpc: String,
    #[allow(dead_code)]
    id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[allow(dead_code)]
    #[serde(skip_serializing_if = "Option::is_none")]
    data: Option<Value>,
}

/// MCP server process instance
pub struct McpServer {
    #[allow(dead_code)]
    process: Arc<Mutex<Child>>,
    stdin: Arc<Mutex<ChildStdin>>,
    stdout: Arc<Mutex<BufReader<ChildStdout>>>,
    tools: Arc<RwLock<HashMap<String, McpTool>>>,
    next_id: Arc<Mutex<i64>>,
}

impl McpServer {
    async fn start(name: &str, config: &McpServerConfig) -> Result<Self> {
        let mut cmd = Command::new(&config.command);

        if let Some(ref args) = config.args {
            cmd.args(args);
        }

        if let Some(ref env) = config.env {
            for (k, v) in env {
                let expanded = crate::utils::expand_env_value(v);
                if !v.starts_with("$env:") || expanded != *v {
                    cmd.env(k, expanded);
                }
            }
        }

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());

        let mut process = cmd.spawn().map_err(|e| {
            BeemFlowError::adapter(format!("Failed to spawn MCP server '{}': {}", name, e))
        })?;

        let stdin = process
            .stdin
            .take()
            .ok_or_else(|| BeemFlowError::adapter("Failed to get stdin"))?;
        let stdout = process
            .stdout
            .take()
            .ok_or_else(|| BeemFlowError::adapter("Failed to get stdout"))?;

        let server = Self {
            process: Arc::new(Mutex::new(process)),
            stdin: Arc::new(Mutex::new(stdin)),
            stdout: Arc::new(Mutex::new(BufReader::new(stdout))),
            tools: Arc::new(RwLock::new(HashMap::new())),
            next_id: Arc::new(Mutex::new(1)),
        };

        server.initialize().await?;
        server.discover_tools().await?;

        tracing::info!(
            "Started MCP server '{}' with {} tools",
            name,
            server.tools.read().len()
        );

        Ok(server)
    }

    async fn initialize(&self) -> Result<()> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: self.next_id().await,
            method: "initialize".to_string(),
            params: json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "beemflow",
                    "version": env!("CARGO_PKG_VERSION"),
                }
            }),
        };

        self.send_request(request).await?;

        let notification = json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
        });

        self.send_notification(notification).await?;

        Ok(())
    }

    async fn discover_tools(&self) -> Result<()> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: self.next_id().await,
            method: "tools/list".to_string(),
            params: json!({}),
        };

        let response = self.send_request(request).await?;

        if let Some(result) = response.result
            && let Some(tools_array) = result.get("tools").and_then(|v| v.as_array())
        {
            let mut tools = self.tools.write();
            for tool_val in tools_array {
                if let Ok(tool) = serde_json::from_value::<McpTool>(tool_val.clone()) {
                    tools.insert(tool.name.clone(), tool);
                }
            }
        }

        Ok(())
    }

    pub async fn call_tool(&self, tool_name: &str, arguments: Value) -> Result<Value> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: self.next_id().await,
            method: "tools/call".to_string(),
            params: json!({
                "name": tool_name,
                "arguments": arguments,
            }),
        };

        let response = self.send_request(request).await?;

        if let Some(error) = response.error {
            return Err(BeemFlowError::adapter(format!(
                "MCP tool call failed: {} - {}",
                error.code, error.message
            )));
        }

        response
            .result
            .ok_or_else(|| BeemFlowError::adapter("MCP tool returned no result"))
    }

    async fn send_request(&self, request: JsonRpcRequest) -> Result<JsonRpcResponse> {
        let request_json = serde_json::to_string(&request)?;

        let mut stdin = self.stdin.lock().await;
        stdin.write_all(request_json.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;
        drop(stdin);

        let mut stdout = self.stdout.lock().await;
        let mut line = String::new();
        stdout.read_line(&mut line).await?;
        drop(stdout);

        let response: JsonRpcResponse = serde_json::from_str(&line)?;

        Ok(response)
    }

    async fn send_notification(&self, notification: Value) -> Result<()> {
        let notification_json = serde_json::to_string(&notification)?;

        let mut stdin = self.stdin.lock().await;
        stdin.write_all(notification_json.as_bytes()).await?;
        stdin.write_all(b"\n").await?;
        stdin.flush().await?;

        Ok(())
    }

    async fn next_id(&self) -> i64 {
        let mut id = self.next_id.lock().await;
        let current = *id;
        *id += 1;
        current
    }
}

/// Manages MCP server lifecycle and configuration
pub struct McpManager {
    servers: Arc<RwLock<HashMap<String, Arc<McpServer>>>>,
    configs: Arc<RwLock<HashMap<String, McpServerConfig>>>,
}

impl McpManager {
    pub fn new() -> Self {
        Self {
            servers: Arc::new(RwLock::new(HashMap::new())),
            configs: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub fn register_server(&self, name: String, config: McpServerConfig) {
        self.configs.write().insert(name, config);
    }

    pub async fn get_or_start_server(&self, server_name: &str) -> Result<Arc<McpServer>> {
        {
            let servers = self.servers.read();
            if let Some(server) = servers.get(server_name) {
                return Ok(server.clone());
            }
        }

        let config = self
            .configs
            .read()
            .get(server_name)
            .cloned()
            .ok_or_else(|| {
                BeemFlowError::adapter(format!("MCP server '{}' not configured", server_name))
            })?;

        let server = Arc::new(McpServer::start(server_name, &config).await?);

        self.servers
            .write()
            .insert(server_name.to_string(), server.clone());

        Ok(server)
    }

    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Value,
    ) -> Result<Value> {
        let server = self.get_or_start_server(server_name).await?;
        server.call_tool(tool_name, arguments).await
    }
}

impl Default for McpManager {
    fn default() -> Self {
        Self::new()
    }
}
