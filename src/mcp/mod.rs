//! MCP (Model Context Protocol) server and client manager

pub mod manager;
mod server;

pub use manager::McpManager;
pub use server::{McpServer, McpServerState, create_mcp_metadata_routes, create_mcp_routes};
