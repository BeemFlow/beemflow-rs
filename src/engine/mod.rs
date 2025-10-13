//! Execution engine for BeemFlow workflows
//!
//! The engine handles step execution, parallel processing, loops, conditionals,
//! state management, and durable waits.

pub mod context;
pub mod executor;
pub mod runs_access;

use crate::adapter::AdapterRegistry;
use crate::dsl::Templater;
use crate::event::EventBus;
use crate::storage::Storage;
use crate::{BeemFlowError, Flow, Result};
pub use runs_access::RunsAccess;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

pub use context::StepContext;
pub use executor::Executor;

/// Result of a flow execution
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    pub run_id: Uuid,
    pub outputs: HashMap<String, serde_json::Value>,
}

/// Paused run information for await_event
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PausedRun {
    pub flow: Flow,
    pub step_idx: usize,
    pub context: StepContext,
    pub outputs: HashMap<String, serde_json::Value>,
    pub token: String,
    pub run_id: Uuid,
}

/// BeemFlow execution engine
pub struct Engine {
    adapters: Arc<AdapterRegistry>,
    mcp_adapter: Arc<crate::adapter::McpAdapter>,
    templater: Arc<Templater>,
    event_bus: Arc<dyn EventBus>,
    storage: Arc<dyn Storage>,
}

impl Default for Engine {
    fn default() -> Self {
        // Create base dependencies
        let adapters = Arc::new(AdapterRegistry::new());
        let templater = Arc::new(Templater::new());
        let event_bus: Arc<dyn EventBus> = Arc::new(crate::event::InProcEventBus::new());
        let storage: Arc<dyn Storage> = Arc::new(crate::storage::MemoryStorage::new());

        // Register core adapters
        adapters.register(Arc::new(crate::adapter::CoreAdapter::new()));
        adapters.register(Arc::new(crate::adapter::HttpAdapter::new(
            crate::constants::HTTP_ADAPTER_ID.to_string(),
            None,
        )));

        // Create and register MCP adapter
        let mcp_adapter = Arc::new(crate::adapter::McpAdapter::new());
        adapters.register(mcp_adapter.clone());

        // Load tools and MCP servers from default registry (synchronously from embedded JSON)
        Self::load_default_registry_tools(&adapters, &mcp_adapter);

        Self {
            adapters,
            mcp_adapter,
            templater,
            event_bus,
            storage,
        }
    }
}

impl Engine {
    /// Create a new engine with all dependencies
    pub fn new(
        adapters: Arc<AdapterRegistry>,
        mcp_adapter: Arc<crate::adapter::McpAdapter>,
        templater: Arc<Templater>,
        event_bus: Arc<dyn EventBus>,
        storage: Arc<dyn Storage>,
    ) -> Self {
        Self {
            adapters,
            mcp_adapter,
            templater,
            event_bus,
            storage,
        }
    }

    /// Load tools and MCP servers from default registry into adapter registry (sync)
    pub fn load_default_registry_tools(
        adapters: &Arc<AdapterRegistry>,
        mcp_adapter: &Arc<crate::adapter::McpAdapter>,
    ) {
        // Load embedded default.json directly
        let data = include_str!("../registry/default.json");
        match serde_json::from_str::<Vec<crate::registry::RegistryEntry>>(data) {
            Ok(entries) => {
                let mut tool_count = 0;
                let mut mcp_count = 0;

                for entry in entries {
                    match entry.entry_type.as_str() {
                        "tool" => {
                            tool_count += 1;

                            // Create tool manifest
                            let manifest = crate::adapter::ToolManifest {
                                name: entry.name.clone(),
                                description: entry.description.clone().unwrap_or_default(),
                                kind: entry.kind.unwrap_or_else(|| "task".to_string()),
                                version: entry.version,
                                parameters: entry.parameters.unwrap_or_default(),
                                endpoint: entry.endpoint,
                                method: entry.method,
                                headers: entry.headers,
                            };

                            // Register as HTTP adapter
                            adapters.register(Arc::new(crate::adapter::HttpAdapter::new(
                                entry.name.clone(),
                                Some(manifest),
                            )));

                            tracing::debug!("Registered tool: {}", entry.name);
                        }

                        "mcp_server" => {
                            mcp_count += 1;

                            // Expand environment variables in env map
                            let env = entry.env.map(|env_map| {
                                env_map
                                    .into_iter()
                                    .map(|(k, v)| (k, Self::expand_env_value(&v)))
                                    .collect()
                            });

                            // Create MCP server config
                            let config = crate::model::McpServerConfig {
                                command: entry.command.unwrap_or_default(),
                                args: entry.args,
                                env,
                                port: entry.port,
                                transport: entry.transport,
                                endpoint: entry.endpoint,
                            };

                            // Register with MCP adapter directly (no downcasting needed)
                            mcp_adapter.register_server(entry.name.clone(), config);
                            tracing::debug!("Registered MCP server: {}", entry.name);
                        }

                        _ => {
                            // Ignore other entry types (oauth_provider, etc.)
                        }
                    }
                }

                tracing::info!(
                    "Loaded {} tools and {} MCP servers from default registry",
                    tool_count,
                    mcp_count
                );
            }
            Err(e) => {
                tracing::error!("Failed to load default registry: {}", e);
            }
        }
    }

    /// Expand environment variable references ($env:VARNAME)
    fn expand_env_value(value: &str) -> String {
        if value.starts_with("$env:") {
            let var_name = value.trim_start_matches("$env:");
            std::env::var(var_name).unwrap_or_default()
        } else {
            value.to_string()
        }
    }

    /// Execute a flow with event data
    pub async fn execute(
        &self,
        flow: &Flow,
        event: HashMap<String, serde_json::Value>,
    ) -> Result<ExecutionResult> {
        if flow.steps.is_empty() {
            return Ok(ExecutionResult {
                run_id: Uuid::nil(),
                outputs: HashMap::new(),
            });
        }

        // Configure MCP servers if present in flow
        if let Some(ref mcp_servers) = flow.mcp_servers {
            for (name, config) in mcp_servers {
                self.mcp_adapter
                    .register_server(name.clone(), config.clone());
            }
        }

        // Setup execution context (returns error if duplicate run detected)
        let (step_ctx, run_id) = self.setup_execution_context(flow, event.clone()).await?;

        // Create executor
        let executor = Executor::new(
            self.adapters.clone(),
            self.templater.clone(),
            self.event_bus.clone(),
            self.storage.clone(),
        );

        // Execute steps
        let result = executor.execute_steps(flow, &step_ctx, 0, run_id).await;

        // Finalize execution and return result with run_id
        let outputs = self.finalize_execution(flow, event, result, run_id).await?;

        Ok(ExecutionResult { run_id, outputs })
    }

    /// Resume a paused run
    pub async fn resume(
        &self,
        token: &str,
        resume_event: HashMap<String, serde_json::Value>,
    ) -> Result<()> {
        tracing::debug!(
            "Resume called for token {} with event: {:?}",
            token,
            resume_event
        );

        // Atomically fetch and delete paused run from storage
        let paused_json = self
            .storage
            .fetch_and_delete_paused_run(token)
            .await?
            .ok_or_else(|| {
                crate::BeemFlowError::config(format!("No paused run found for token: {}", token))
            })?;

        // Deserialize paused run from JSON
        let paused: PausedRun = serde_json::from_value(paused_json)?;

        // Update context with resume event
        let updated_ctx = paused.context.clone();
        for (k, v) in resume_event {
            updated_ctx.set_event(k, v);
        }

        // Create executor
        let executor = Executor::new(
            self.adapters.clone(),
            self.templater.clone(),
            self.event_bus.clone(),
            self.storage.clone(),
        );

        // Continue execution
        let _outputs = executor
            .execute_steps(
                &paused.flow,
                &updated_ctx,
                paused.step_idx + 1,
                paused.run_id,
            )
            .await
            .unwrap_or_else(|_| HashMap::new());

        // Note: Outputs are tracked in storage via StepContext, not in-memory
        Ok(())
    }

    /// Handle resume events (called when resume events are received)
    pub async fn handle_resume_event(
        &self,
        token: &str,
        event_data: serde_json::Value,
    ) -> Result<()> {
        tracing::info!("Handling resume event for token: {}", token);

        // Extract event data into HashMap
        let resume_event = if let Some(obj) = event_data.as_object() {
            obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        } else {
            HashMap::new()
        };

        // Resume the run
        self.resume(token, resume_event).await
    }

    /// Setup execution context
    async fn setup_execution_context(
        &self,
        flow: &Flow,
        event: HashMap<String, serde_json::Value>,
    ) -> Result<(StepContext, Uuid)> {
        // Collect secrets from event
        let secrets = self.collect_secrets(&event);

        // Create step context
        let step_ctx = StepContext::new(
            event.clone(),
            flow.vars.clone().unwrap_or_default(),
            secrets,
        );

        // Generate deterministic run ID
        let run_id = self.generate_deterministic_run_id(&flow.name, &event);

        // Create run
        let run = crate::model::Run {
            id: run_id,
            flow_name: flow.name.clone(),
            event: event.clone(),
            vars: flow.vars.clone().unwrap_or_default(),
            status: crate::model::RunStatus::Running,
            started_at: chrono::Utc::now(),
            ended_at: None,
            steps: None,
        };

        // Try to atomically insert run - returns false if already exists
        // Note: Deterministic UUID includes time bucket, so duplicates within
        // the same minute window will have the same ID
        if !self.storage.try_insert_run(&run).await? {
            tracing::info!(
                "Duplicate run detected for {}, run_id: {}",
                flow.name,
                run_id
            );
            return Err(crate::BeemFlowError::validation(format!(
                "Duplicate run detected for flow '{}' (run_id: {}). A run with the same event data was already executed within the current time window.",
                flow.name, run_id
            )));
        }

        Ok((step_ctx, run_id))
    }

    /// Finalize execution and update run status
    async fn finalize_execution(
        &self,
        flow: &Flow,
        event: HashMap<String, serde_json::Value>,
        result: std::result::Result<HashMap<String, serde_json::Value>, BeemFlowError>,
        run_id: Uuid,
    ) -> Result<HashMap<String, serde_json::Value>> {
        let (_outputs, status) = match &result {
            Ok(outputs) => (outputs.clone(), crate::model::RunStatus::Succeeded),
            Err(e)
                if e.to_string()
                    .contains(crate::constants::ERR_AWAIT_EVENT_PAUSE) =>
            {
                (HashMap::new(), crate::model::RunStatus::Waiting)
            }
            Err(_) => (HashMap::new(), crate::model::RunStatus::Failed),
        };

        // Clone event before moving
        let event_clone = event.clone();

        // Update run with final status
        let run = crate::model::Run {
            id: run_id,
            flow_name: flow.name.clone(),
            event,
            vars: flow.vars.clone().unwrap_or_default(),
            status,
            started_at: chrono::Utc::now(),
            ended_at: Some(chrono::Utc::now()),
            steps: None,
        };

        self.storage.save_run(&run).await?;

        // Handle catch blocks if there was an error
        if result.is_err() && flow.catch.is_some() {
            self.execute_catch_blocks(flow, &event_clone).await?;
        }

        result
    }

    /// Execute catch blocks on error
    async fn execute_catch_blocks(
        &self,
        flow: &Flow,
        event: &HashMap<String, serde_json::Value>,
    ) -> Result<HashMap<String, serde_json::Value>> {
        let catch_steps = flow
            .catch
            .as_ref()
            .ok_or_else(|| crate::BeemFlowError::validation("no catch blocks defined"))?;

        let secrets = self.collect_secrets(event);
        let step_ctx = StepContext::new(
            event.clone(),
            flow.vars.clone().unwrap_or_default(),
            secrets,
        );

        let executor = Executor::new(
            self.adapters.clone(),
            self.templater.clone(),
            self.event_bus.clone(),
            self.storage.clone(),
        );

        // Execute catch steps
        let mut catch_outputs = HashMap::new();
        for step in catch_steps {
            match executor.execute_step(step, &step_ctx, &step.id).await {
                Ok(_) => {
                    if let Some(output) = step_ctx.get_output(&step.id) {
                        catch_outputs.insert(step.id.clone(), output);
                    }
                }
                Err(e) => {
                    tracing::error!("Catch block step {} failed: {}", step.id, e);
                }
            }
        }

        Ok(catch_outputs)
    }

    /// Collect secrets from event data
    fn collect_secrets(
        &self,
        event: &HashMap<String, serde_json::Value>,
    ) -> HashMap<String, serde_json::Value> {
        let mut secrets = HashMap::new();

        // Extract secrets from event
        if let Some(event_secrets) = event
            .get(crate::constants::SECRETS_KEY)
            .and_then(|v| v.as_object())
        {
            for (k, v) in event_secrets {
                secrets.insert(k.clone(), v.clone());
            }
        }

        // Collect environment variables with $env prefix
        for (k, v) in event {
            if k.starts_with(crate::constants::ENV_VAR_PREFIX) {
                let env_var = k.trim_start_matches(crate::constants::ENV_VAR_PREFIX);
                secrets.insert(env_var.to_string(), v.clone());
            }
        }

        secrets
    }

    /// Generate deterministic run ID for deduplication
    fn generate_deterministic_run_id(
        &self,
        flow_name: &str,
        event: &HashMap<String, serde_json::Value>,
    ) -> Uuid {
        use sha2::Digest;
        use sha2::Sha256;

        let mut hasher = Sha256::new();

        // Add flow name
        hasher.update(flow_name.as_bytes());

        // Add time bucket (1 minute windows)
        let now = chrono::Utc::now();
        let time_bucket = now.timestamp() / 60 * 60; // truncate to minute
        hasher.update(time_bucket.to_string().as_bytes());

        // Add event data in sorted order for determinism
        let mut keys: Vec<&String> = event.keys().collect();
        keys.sort();
        for key in keys {
            hasher.update(key.as_bytes());
            if let Ok(json) = serde_json::to_string(&event[key]) {
                hasher.update(json.as_bytes());
            }
        }

        let hash = hasher.finalize();
        Uuid::new_v5(&Uuid::NAMESPACE_DNS, &hash)
    }
}

#[cfg(test)]
mod context_test;
#[cfg(test)]
mod engine_test;
#[cfg(test)]
mod error_test;
#[cfg(test)]
mod executor_test;
