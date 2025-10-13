//! MCP (Model Context Protocol) server and client manager

pub mod manager;
mod server;

pub use manager::McpManager;
pub use server::McpServer;
