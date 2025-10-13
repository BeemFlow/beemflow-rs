//! Adapter system for tool execution
//!
//! Adapters provide a unified interface for executing different types of tools.

pub mod core;
pub mod http;
pub mod mcp;

use crate::Result;
use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Tool manifest information
#[derive(Debug, Clone)]
pub struct ToolManifest {
    pub name: String,
    pub description: String,
    pub kind: String,
    pub version: Option<String>,
    pub parameters: HashMap<String, Value>,
    pub endpoint: Option<String>,
    pub method: Option<String>,
    pub headers: Option<HashMap<String, String>>,
}

/// Adapter trait for tool execution
#[async_trait]
pub trait Adapter: Send + Sync {
    /// Get adapter ID
    fn id(&self) -> &str;

    /// Execute a tool with given inputs
    async fn execute(&self, inputs: HashMap<String, Value>) -> Result<HashMap<String, Value>>;

    /// Get tool manifest (if applicable)
    fn manifest(&self) -> Option<ToolManifest>;

    /// Get self as Any for downcasting
    fn as_any(&self) -> &dyn std::any::Any;
}

/// Registry of adapters - uses DashMap for lock-free concurrent access
pub struct AdapterRegistry {
    adapters: Arc<DashMap<String, Arc<dyn Adapter>>>,
}

impl AdapterRegistry {
    /// Create a new adapter registry
    pub fn new() -> Self {
        Self {
            adapters: Arc::new(DashMap::new()),
        }
    }

    /// Register an adapter
    pub fn register(&self, adapter: Arc<dyn Adapter>) {
        self.adapters.insert(adapter.id().to_string(), adapter);
    }

    /// Get an adapter by ID
    pub fn get(&self, id: &str) -> Option<Arc<dyn Adapter>> {
        self.adapters.get(id).map(|entry| Arc::clone(&*entry))
    }

    /// Get all adapters
    pub fn all(&self) -> Vec<Arc<dyn Adapter>> {
        self.adapters
            .iter()
            .map(|entry| Arc::clone(&*entry))
            .collect()
    }
}

impl Default for AdapterRegistry {
    fn default() -> Self {
        Self::new()
    }
}

pub use core::CoreAdapter;
pub use http::HttpAdapter;

pub use mcp::McpAdapter;

#[cfg(test)]
mod core_test;
#[cfg(test)]
mod mcp_test;
