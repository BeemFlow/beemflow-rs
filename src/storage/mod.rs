//! Storage backends for BeemFlow
//!
//! Provides multiple storage backends with a unified trait interface.

pub mod memory;
pub mod postgres;
pub mod sql_common;
pub mod sqlite;

use crate::{Result, model::*};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Storage trait for persisting flows, runs, and state
#[async_trait]
pub trait Storage: Send + Sync {
    // Run methods
    /// Save a run
    async fn save_run(&self, run: &Run) -> Result<()>;

    /// Get a run by ID
    async fn get_run(&self, id: Uuid) -> Result<Option<Run>>;

    /// List all runs
    async fn list_runs(&self) -> Result<Vec<Run>>;

    /// Delete a run and its steps
    async fn delete_run(&self, id: Uuid) -> Result<()>;

    /// Try to insert a run atomically
    /// Returns true if inserted, false if run already exists (based on ID)
    async fn try_insert_run(&self, run: &Run) -> Result<bool>;

    // Step methods
    /// Save a step execution
    async fn save_step(&self, step: &StepRun) -> Result<()>;

    /// Get steps for a run
    async fn get_steps(&self, run_id: Uuid) -> Result<Vec<StepRun>>;

    // Wait/timeout methods
    /// Register a wait token with optional wake time
    async fn register_wait(&self, token: Uuid, wake_at: Option<i64>) -> Result<()>;

    /// Resolve a wait token (returns run if found)
    async fn resolve_wait(&self, token: Uuid) -> Result<Option<Run>>;

    // Paused run methods
    /// Save a paused run (for await_event)
    async fn save_paused_run(&self, token: &str, data: serde_json::Value) -> Result<()>;

    /// Load all paused runs
    async fn load_paused_runs(&self) -> Result<HashMap<String, serde_json::Value>>;

    /// Delete a paused run
    async fn delete_paused_run(&self, token: &str) -> Result<()>;

    /// Atomically fetch and delete a paused run
    /// Returns None if not found, preventing double-resume
    async fn fetch_and_delete_paused_run(&self, token: &str) -> Result<Option<serde_json::Value>>;

    // Flow management methods (for operations layer)
    /// Save a flow definition
    async fn save_flow(&self, name: &str, content: &str, version: Option<&str>) -> Result<()>;

    /// Get a flow definition  
    async fn get_flow(&self, name: &str) -> Result<Option<String>>;

    /// List all flow names
    async fn list_flows(&self) -> Result<Vec<String>>;

    /// Delete a flow
    async fn delete_flow(&self, name: &str) -> Result<()>;

    // Flow versioning methods
    /// Deploy a flow version
    async fn deploy_flow_version(
        &self,
        flow_name: &str,
        version: &str,
        content: &str,
    ) -> Result<()>;

    /// Set the deployed version for a flow
    async fn set_deployed_version(&self, flow_name: &str, version: &str) -> Result<()>;

    /// Get the currently deployed version
    async fn get_deployed_version(&self, flow_name: &str) -> Result<Option<String>>;

    /// Get content for a specific flow version
    async fn get_flow_version_content(
        &self,
        flow_name: &str,
        version: &str,
    ) -> Result<Option<String>>;

    /// List all versions of a flow
    async fn list_flow_versions(&self, flow_name: &str) -> Result<Vec<FlowSnapshot>>;

    // OAuth credential methods
    /// Save OAuth credential
    async fn save_oauth_credential(&self, credential: &OAuthCredential) -> Result<()>;

    /// Get OAuth credential
    async fn get_oauth_credential(
        &self,
        provider: &str,
        integration: &str,
    ) -> Result<Option<OAuthCredential>>;

    /// List OAuth credentials
    async fn list_oauth_credentials(&self) -> Result<Vec<OAuthCredential>>;

    /// Delete OAuth credential by ID
    async fn delete_oauth_credential(&self, id: &str) -> Result<()>;

    /// Refresh OAuth credential token
    async fn refresh_oauth_credential(
        &self,
        id: &str,
        new_token: &str,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<()>;

    // OAuth provider methods
    /// Save OAuth provider
    async fn save_oauth_provider(&self, provider: &OAuthProvider) -> Result<()>;

    /// Get OAuth provider by ID
    async fn get_oauth_provider(&self, id: &str) -> Result<Option<OAuthProvider>>;

    /// List all OAuth providers
    async fn list_oauth_providers(&self) -> Result<Vec<OAuthProvider>>;

    /// Delete OAuth provider
    async fn delete_oauth_provider(&self, id: &str) -> Result<()>;

    // OAuth client methods (for dynamic client registration)
    /// Save OAuth client
    async fn save_oauth_client(&self, client: &OAuthClient) -> Result<()>;

    /// Get OAuth client by ID
    async fn get_oauth_client(&self, id: &str) -> Result<Option<OAuthClient>>;

    /// List all OAuth clients
    async fn list_oauth_clients(&self) -> Result<Vec<OAuthClient>>;

    /// Delete OAuth client
    async fn delete_oauth_client(&self, id: &str) -> Result<()>;

    // OAuth token methods (for token storage)
    /// Save OAuth token
    async fn save_oauth_token(&self, token: &OAuthToken) -> Result<()>;

    /// Get OAuth token by authorization code
    async fn get_oauth_token_by_code(&self, code: &str) -> Result<Option<OAuthToken>>;

    /// Get OAuth token by access token
    async fn get_oauth_token_by_access(&self, access: &str) -> Result<Option<OAuthToken>>;

    /// Get OAuth token by refresh token
    async fn get_oauth_token_by_refresh(&self, refresh: &str) -> Result<Option<OAuthToken>>;

    /// Delete OAuth token by authorization code
    async fn delete_oauth_token_by_code(&self, code: &str) -> Result<()>;

    /// Delete OAuth token by access token
    async fn delete_oauth_token_by_access(&self, access: &str) -> Result<()>;

    /// Delete OAuth token by refresh token
    async fn delete_oauth_token_by_refresh(&self, refresh: &str) -> Result<()>;
}

/// Flow snapshot represents a deployed flow version
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FlowSnapshot {
    pub flow_name: String,
    pub version: String,
    pub deployed_at: DateTime<Utc>,
    pub is_live: bool,
}

pub use memory::MemoryStorage;
pub use postgres::PostgresStorage;
pub use sqlite::SqliteStorage;

/// Create a storage backend from configuration
pub async fn create_storage_from_config(
    config: &crate::config::StorageConfig,
) -> crate::Result<Arc<dyn Storage>> {
    match config.driver.as_str() {
        "memory" => Ok(Arc::new(MemoryStorage::new())),
        "sqlite" => Ok(Arc::new(SqliteStorage::new(&config.dsn).await?)),
        "postgres" => Ok(Arc::new(PostgresStorage::new(&config.dsn).await?)),
        _ => Err(crate::BeemFlowError::config(format!(
            "Unknown storage driver: {}. Supported: memory, sqlite, postgres",
            config.driver
        ))),
    }
}

#[cfg(test)]
mod memory_test;
#[cfg(test)]
mod postgres_test;
#[cfg(test)]
mod sqlite_test;
#[cfg(test)]
mod storage_test;
