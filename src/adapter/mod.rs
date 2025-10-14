//! Adapter system for tool execution
//!
//! Adapters provide a unified interface for executing different types of tools.
//!
//! # ExecutionContext
//!
//! The `ExecutionContext` provides adapters with access to system-level resources
//! like storage, without polluting the tool's input parameters. This enables features like:
//!
//! - **OAuth Token Expansion**: HTTP adapters can look up OAuth credentials from storage
//!   to automatically inject access tokens into API requests
//! - **Future Security Features**: User context, permissions, rate limiting, audit logging
//! - **Multi-Tenancy**: Different users can have isolated OAuth credentials and permissions
//!
//! ## Design Rationale
//!
//! We pass ExecutionContext separately from tool inputs because:
//!
//! 1. **Separation of Concerns**: Tool inputs are user-provided data, while context
//!    contains system-level resources. Mixing them would blur this boundary.
//!
//! 2. **Type Safety**: Context uses proper Rust types (Arc<dyn Storage>) instead of
//!    trying to serialize trait objects into JSON, which is impossible.
//!
//! 3. **Extensibility**: We can add new context fields (user_id, permissions, etc.)
//!    without changing the Adapter trait again - just add fields to ExecutionContext.
//!
//! 4. **Security**: Makes it obvious what resources adapters can access, easier to audit.
//!
//! ## Example: OAuth Token Expansion
//!
//! ```yaml
//! # Registry tool definition
//! name: github.get_user
//! endpoint: https://api.github.com/user
//! headers:
//!   Authorization: $oauth:github:default  # Expanded at runtime!
//! ```
//!
//! The HttpAdapter receives ExecutionContext with Storage, looks up the OAuth
//! credential for "github:default", and replaces the placeholder with the actual
//! access token before making the HTTP request.

pub mod core;
pub mod http;
pub mod mcp;

use crate::Result;
use crate::storage::Storage;
use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Execution context providing system-level resources to adapters
///
/// This context is passed to adapters during execution, giving them access to
/// system resources like storage without mixing them with user-provided tool inputs.
///
/// # Design Notes
///
/// ## Why a separate context instead of adding to inputs?
///
/// Tool inputs (`HashMap<String, Value>`) are user-provided parameters that get
/// serialized to/from JSON. ExecutionContext contains system resources like storage
/// that cannot be serialized into JSON (trait objects, Arc pointers, etc.).
///
/// Keeping them separate maintains a clean boundary:
/// - **Inputs**: User data (serializable, part of the workflow definition)
/// - **Context**: System resources (non-serializable, provided by the runtime)
///
/// ## Future Extensibility
///
/// This struct is designed to be extended without breaking the Adapter trait:
///
/// ```rust,ignore
/// pub struct ExecutionContext {
///     pub storage: Arc<dyn Storage>,
///
///     // Future additions (no trait changes needed!):
///     pub user_id: Option<String>,          // Who triggered this execution?
///     pub permissions: Arc<Permissions>,    // What can they access?
///     pub rate_limiter: Arc<RateLimiter>,   // Prevent abuse
///     pub audit_log: Arc<AuditLogger>,      // Track all actions
///     pub request_id: String,               // For tracing/debugging
/// }
/// ```
///
/// Adapters that don't need these features can simply ignore them.
///
/// ## Security Considerations
///
/// Having an explicit context makes security audits easier:
/// - Clear what resources each adapter can access
/// - Can add permission checks before execution
/// - Easy to see data flow from storage to adapters
///
/// Example future usage:
/// ```rust,ignore
/// if !ctx.permissions.can_http_request(&url) {
///     return Err("Permission denied");
/// }
/// ctx.audit_log.log(ctx.user_id, "http.request", &url);
/// ```
#[derive(Clone)]
pub struct ExecutionContext {
    /// Storage backend for looking up OAuth credentials, secrets, etc.
    ///
    /// Currently used by HttpAdapter for OAuth token expansion:
    /// - Tool manifest specifies: `Authorization: $oauth:github:default`
    /// - HttpAdapter calls: `ctx.storage.get_oauth_credential("github", "default")`
    /// - Token is injected into request headers automatically
    pub storage: Arc<dyn Storage>,
    // Future fields will be added here as needed without breaking changes
}

impl ExecutionContext {
    /// Create a new execution context
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self { storage }
    }
}

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
///
/// Adapters provide a unified interface for executing different types of tools
/// (core tools, HTTP APIs, MCP servers, etc.).
///
/// # ExecutionContext Parameter
///
/// All adapters receive an `ExecutionContext` during execution, providing access to
/// system resources like storage. This enables features like OAuth token expansion
/// without mixing system resources with user-provided tool inputs.
///
/// See module-level documentation for detailed rationale and examples.
#[async_trait]
pub trait Adapter: Send + Sync {
    /// Get adapter ID
    fn id(&self) -> &str;

    /// Execute a tool with given inputs and execution context
    ///
    /// # Parameters
    ///
    /// - `inputs`: User-provided tool parameters from the workflow definition
    /// - `ctx`: System resources (storage, future: permissions, rate limits, etc.)
    ///
    /// # Returns
    ///
    /// A HashMap of output values that will be available to subsequent steps via
    /// template expressions like `{{ steps.step_name.output.field }}`.
    async fn execute(
        &self,
        inputs: HashMap<String, Value>,
        ctx: &ExecutionContext,
    ) -> Result<HashMap<String, Value>>;

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
