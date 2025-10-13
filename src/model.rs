//! Core data models for BeemFlow
//!
//! This module contains all the data structures that define BeemFlow workflows,
//! runs, steps, and related types. These models provide complete workflow orchestration
//! capabilities with strong type safety and comprehensive serialization support.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// A complete workflow definition
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Flow {
    /// Unique workflow identifier (REQUIRED)
    pub name: String,

    /// Human-readable description (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,

    /// Semantic version (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Trigger type (optional for testing)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub on: Option<Trigger>,

    /// Cron expression (required if on: schedule.cron)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cron: Option<String>,

    /// Workflow-level variables (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vars: Option<HashMap<String, serde_json::Value>>,

    /// Array of execution steps (REQUIRED)
    pub steps: Vec<Step>,

    /// Error handling steps (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub catch: Option<Vec<Step>>,

    /// MCP server configurations (optional)
    #[serde(skip_serializing_if = "Option::is_none", rename = "mcpServers")]
    pub mcp_servers: Option<HashMap<String, McpServerConfig>>,
}

/// Trigger type for workflow execution
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Trigger {
    /// Single trigger type as string
    Single(String),
    /// Multiple trigger types as array of strings
    Multiple(Vec<String>),
    /// Complex trigger with additional data (for event: format)
    Complex(Vec<serde_json::Value>),
    /// Raw value for maximum flexibility (accepts any valid JSON)
    Raw(serde_json::Value),
}

impl Trigger {
    /// Check if this trigger includes a specific type
    pub fn includes(&self, trigger_type: &str) -> bool {
        match self {
            Trigger::Single(t) => t == trigger_type,
            Trigger::Multiple(triggers) => triggers.iter().any(|t| t == trigger_type),
            Trigger::Complex(_) => {
                // For complex triggers, we'd need to inspect the structure
                // For now, just return false
                false
            }
            Trigger::Raw(_) => {
                // For raw values, try to match against string or array
                false
            }
        }
    }
}

/// A single workflow step
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Step {
    /// Unique step identifier (REQUIRED)
    pub id: String,

    /// Tool to execute
    #[serde(skip_serializing_if = "Option::is_none", rename = "use")]
    pub use_: Option<String>,

    /// Tool input parameters
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with: Option<HashMap<String, serde_json::Value>>,

    /// Step dependencies
    #[serde(skip_serializing_if = "Option::is_none")]
    pub depends_on: Option<Vec<String>>,

    /// Run nested steps in parallel
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parallel: Option<bool>,

    /// Conditional execution
    #[serde(skip_serializing_if = "Option::is_none", rename = "if")]
    pub if_: Option<String>,

    /// Loop over array
    #[serde(skip_serializing_if = "Option::is_none")]
    pub foreach: Option<String>,

    /// Loop variable name
    #[serde(skip_serializing_if = "Option::is_none", rename = "as")]
    pub as_: Option<String>,

    /// Steps to execute in loop
    #[serde(skip_serializing_if = "Option::is_none", rename = "do")]
    pub do_: Option<Vec<Step>>,

    /// Steps for parallel block
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<Step>>,

    /// Retry configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retry: Option<RetrySpec>,

    /// Event wait configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub await_event: Option<AwaitEventSpec>,

    /// Time delay configuration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub wait: Option<WaitSpec>,
}

/// Retry configuration for a step
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetrySpec {
    /// Total attempts (including first)
    pub attempts: u32,

    /// Delay between attempts in seconds
    pub delay_sec: u64,
}

/// Event wait configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AwaitEventSpec {
    /// Event source
    pub source: String,

    /// Match criteria
    #[serde(rename = "match")]
    pub match_: HashMap<String, serde_json::Value>,

    /// Timeout duration
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<String>,
}

/// Time delay configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WaitSpec {
    /// Wait seconds
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds: Option<u64>,

    /// Wait until timestamp
    #[serde(skip_serializing_if = "Option::is_none")]
    pub until: Option<String>,
}

/// MCP server configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Command to execute
    pub command: String,

    /// Command arguments
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,

    /// Environment variables
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<HashMap<String, String>>,

    /// Server port
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,

    /// Transport protocol
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,

    /// Server endpoint
    #[serde(skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
}

/// A workflow run instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Run {
    /// Unique run identifier
    pub id: Uuid,

    /// Flow name
    pub flow_name: String,

    /// Event data that triggered this run
    pub event: HashMap<String, serde_json::Value>,

    /// Flow variables
    pub vars: HashMap<String, serde_json::Value>,

    /// Current run status
    pub status: RunStatus,

    /// Start timestamp
    pub started_at: DateTime<Utc>,

    /// End timestamp (if completed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<DateTime<Utc>>,

    /// Step execution records
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<Vec<StepRun>>,
}

/// Run execution status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RunStatus {
    /// Run is pending execution
    Pending,

    /// Run is currently executing
    Running,

    /// Run completed successfully
    Succeeded,

    /// Run failed with error
    Failed,

    /// Run is waiting for external event
    Waiting,

    /// Run was skipped (duplicate)
    Skipped,
}

/// A single step execution record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepRun {
    /// Unique step run identifier
    pub id: Uuid,

    /// Parent run identifier
    pub run_id: Uuid,

    /// Step name/ID
    pub step_name: String,

    /// Step execution status
    pub status: StepStatus,

    /// Start timestamp
    pub started_at: DateTime<Utc>,

    /// End timestamp (if completed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ended_at: Option<DateTime<Utc>>,

    /// Error message (if failed)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,

    /// Step outputs
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs: Option<HashMap<String, serde_json::Value>>,
}

/// Step execution status
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum StepStatus {
    /// Step is pending execution
    Pending,

    /// Step is currently executing
    Running,

    /// Step completed successfully
    Succeeded,

    /// Step failed with error
    Failed,

    /// Step is waiting for external event
    Waiting,

    /// Step was skipped (conditional)
    Skipped,
}

/// OAuth credential for managing OAuth2.0 credentials
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthCredential {
    /// Unique identifier
    pub id: String,

    /// Provider name (e.g., "google", "github")
    pub provider: String,

    /// Integration name (e.g., "sheets_default")
    pub integration: String,

    /// Access token (encrypted at storage layer)
    pub access_token: String,

    /// Refresh token (optional)
    pub refresh_token: Option<String>,

    /// Token expiration time (optional)
    pub expires_at: Option<DateTime<Utc>>,

    /// OAuth scope
    pub scope: Option<String>,

    /// Creation time
    pub created_at: DateTime<Utc>,

    /// Last update time
    pub updated_at: DateTime<Utc>,
}

// Validation macros for required fields
macro_rules! require_field {
    ($field:expr, $name:literal) => {
        if $field.is_empty() {
            return Err(concat!($name, " is required"));
        }
    };
}

macro_rules! require_field_err {
    ($field:expr, $name:literal) => {
        if $field.is_empty() {
            return Err(crate::BeemFlowError::validation(concat!(
                $name,
                " is required"
            )));
        }
    };
}

impl OAuthCredential {
    /// Validate the OAuth credential
    pub fn validate(&self) -> Result<(), &'static str> {
        require_field!(self.provider, "provider");
        require_field!(self.integration, "integration");
        require_field!(self.access_token, "access_token");
        Ok(())
    }

    /// Check if the credential is expired
    #[must_use]
    pub fn is_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|expires_at| Utc::now() >= expires_at)
    }

    /// Get unique key for the credential
    #[must_use]
    pub fn unique_key(&self) -> String {
        format!("{}:{}", self.provider, self.integration)
    }
}

/// OAuth provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthProvider {
    /// Unique identifier
    pub id: String,

    /// Provider name
    pub name: String,

    /// OAuth client ID
    pub client_id: String,

    /// OAuth client secret (encrypted at storage layer)
    pub client_secret: String,

    /// Authorization URL
    pub auth_url: String,

    /// Token URL
    pub token_url: String,

    /// Supported scopes
    pub scopes: Option<Vec<String>>,

    /// Creation time
    pub created_at: DateTime<Utc>,

    /// Last update time
    pub updated_at: DateTime<Utc>,
}

impl OAuthProvider {
    /// Validate the OAuth provider configuration
    pub fn validate(&self) -> crate::Result<()> {
        require_field_err!(self.client_id, "client_id");
        require_field_err!(self.client_secret, "client_secret");
        require_field_err!(self.auth_url, "auth_url");
        require_field_err!(self.token_url, "token_url");
        Ok(())
    }
}

/// Registered OAuth client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthClient {
    /// Unique identifier
    pub id: String,

    /// Client secret (encrypted at storage layer)
    pub secret: String,

    /// Client name
    pub name: String,

    /// Allowed redirect URIs
    pub redirect_uris: Vec<String>,

    /// Supported grant types
    pub grant_types: Vec<String>,

    /// Supported response types
    pub response_types: Vec<String>,

    /// OAuth scope
    pub scope: String,

    /// Client URI (homepage)
    pub client_uri: Option<String>,

    /// Logo URI
    pub logo_uri: Option<String>,

    /// Creation time
    pub created_at: DateTime<Utc>,

    /// Last update time
    pub updated_at: DateTime<Utc>,
}

/// OAuth token information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OAuthToken {
    /// Unique identifier
    pub id: String,

    /// Client ID
    pub client_id: String,

    /// User ID
    pub user_id: String,

    /// Redirect URI used
    pub redirect_uri: String,

    /// OAuth scope
    pub scope: String,

    /// Authorization code
    pub code: Option<String>,

    /// Code creation time
    pub code_create_at: Option<DateTime<Utc>>,

    /// Code expiration duration
    pub code_expires_in: Option<std::time::Duration>,

    /// PKCE code challenge (for OAuth 2.1 PKCE)
    pub code_challenge: Option<String>,

    /// PKCE code challenge method (S256)
    pub code_challenge_method: Option<String>,

    /// Access token
    pub access: Option<String>,

    /// Access token creation time
    pub access_create_at: Option<DateTime<Utc>>,

    /// Access token expiration duration
    pub access_expires_in: Option<std::time::Duration>,

    /// Refresh token
    pub refresh: Option<String>,

    /// Refresh token creation time
    pub refresh_create_at: Option<DateTime<Utc>>,

    /// Refresh token expiration duration
    pub refresh_expires_in: Option<std::time::Duration>,
}
