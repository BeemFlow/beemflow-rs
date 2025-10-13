//! Step execution context
//!
//! Manages event data, variables, outputs, and secrets during workflow execution.

use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Context for step execution
#[derive(Debug, Clone)]
pub struct StepContext {
    event: Arc<RwLock<HashMap<String, Value>>>,
    vars: Arc<RwLock<HashMap<String, Value>>>,
    outputs: Arc<RwLock<HashMap<String, Value>>>,
    secrets: Arc<RwLock<HashMap<String, Value>>>,
}

// Custom Serialize implementation for StepContext
impl serde::Serialize for StepContext {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        // Serialize as a snapshot
        self.snapshot().serialize(serializer)
    }
}

// Custom Deserialize implementation for StepContext
impl<'de> serde::Deserialize<'de> for StepContext {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Deserialize from snapshot
        let snapshot = ContextSnapshot::deserialize(deserializer)?;
        Ok(Self::new(snapshot.event, snapshot.vars, snapshot.secrets))
    }
}

impl StepContext {
    /// Create a new step context
    pub fn new(
        event: HashMap<String, Value>,
        vars: HashMap<String, Value>,
        secrets: HashMap<String, Value>,
    ) -> Self {
        Self {
            event: Arc::new(RwLock::new(event)),
            vars: Arc::new(RwLock::new(vars)),
            outputs: Arc::new(RwLock::new(HashMap::new())),
            secrets: Arc::new(RwLock::new(secrets)),
        }
    }

    /// Get an output value
    pub fn get_output(&self, key: &str) -> Option<Value> {
        self.outputs.read().get(key).cloned()
    }

    /// Set an output value
    pub fn set_output(&self, key: String, value: Value) {
        self.outputs.write().insert(key, value);
    }

    /// Set an event value
    pub fn set_event(&self, key: String, value: Value) {
        self.event.write().insert(key, value);
    }

    /// Set a variable value
    pub fn set_var(&self, key: String, value: Value) {
        self.vars.write().insert(key, value);
    }

    /// Set a secret value
    #[allow(dead_code)]
    pub fn set_secret(&self, key: String, value: Value) {
        self.secrets.write().insert(key, value);
    }

    /// Get a snapshot of the context (cloned data)
    pub fn snapshot(&self) -> ContextSnapshot {
        ContextSnapshot {
            event: self.event.read().clone(),
            vars: self.vars.read().clone(),
            outputs: self.outputs.read().clone(),
            secrets: self.secrets.read().clone(),
        }
    }

    /// Get template data for rendering
    pub fn template_data(&self) -> HashMap<String, Value> {
        self.template_data_with_runs(None)
    }

    /// Get template data with runs access for history
    pub fn template_data_with_runs(
        &self,
        runs_data: Option<HashMap<String, Value>>,
    ) -> HashMap<String, Value> {
        let snapshot = self.snapshot();
        let mut data = HashMap::new();

        // Add structured fields using a helper closure to avoid repetition
        let add_field =
            |data: &mut HashMap<String, Value>, key: &str, value: &HashMap<String, Value>| {
                // Safe: HashMap<String, Value> serialization to JSON Value should never fail
                // as it's already a valid JSON-compatible structure
                if let Ok(json_value) = serde_json::to_value(value) {
                    data.insert(key.to_string(), json_value);
                } else {
                    tracing::warn!(
                        "Failed to serialize field '{}' to JSON, using empty object",
                        key
                    );
                    data.insert(key.to_string(), Value::Object(serde_json::Map::new()));
                }
            };

        add_field(
            &mut data,
            crate::constants::TEMPLATE_FIELD_EVENT,
            &snapshot.event,
        );
        add_field(
            &mut data,
            crate::constants::TEMPLATE_FIELD_VARS,
            &snapshot.vars,
        );
        add_field(
            &mut data,
            crate::constants::TEMPLATE_FIELD_OUTPUTS,
            &snapshot.outputs,
        );
        add_field(
            &mut data,
            crate::constants::TEMPLATE_FIELD_SECRETS,
            &snapshot.secrets,
        );
        add_field(
            &mut data,
            crate::constants::TEMPLATE_FIELD_STEPS,
            &snapshot.outputs,
        );

        // Add environment variables
        let env: HashMap<String, String> = std::env::vars().collect();
        // Safe: HashMap<String, String> serialization to JSON should never fail
        if let Ok(env_value) = serde_json::to_value(&env) {
            data.insert(crate::constants::TEMPLATE_FIELD_ENV.to_string(), env_value);
        } else {
            tracing::warn!("Failed to serialize environment variables, using empty object");
            data.insert(
                crate::constants::TEMPLATE_FIELD_ENV.to_string(),
                Value::Object(serde_json::Map::new()),
            );
        }

        // Add runs access if provided
        if let Some(runs) = runs_data {
            // Safe: HashMap<String, Value> serialization should never fail
            if let Ok(runs_value) = serde_json::to_value(&runs) {
                data.insert("runs".to_string(), runs_value);
            } else {
                tracing::warn!("Failed to serialize runs data, using empty object");
                data.insert("runs".to_string(), Value::Object(serde_json::Map::new()));
            }
        }

        // Add auto-generated variables
        let now = chrono::Utc::now();
        data.insert("now".to_string(), Value::String(now.to_rfc3339()));
        data.insert(
            "timestamp".to_string(),
            Value::Number(now.timestamp().into()),
        );

        // Flatten vars and event for easier access using extend
        data.extend(snapshot.vars);
        data.extend(snapshot.event);

        // Flatten outputs (only valid identifiers)
        data.extend(
            snapshot
                .outputs
                .into_iter()
                .filter(|(k, _)| is_valid_identifier(k)),
        );

        data
    }
}

/// Immutable snapshot of context data
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ContextSnapshot {
    pub event: HashMap<String, Value>,
    pub vars: HashMap<String, Value>,
    pub outputs: HashMap<String, Value>,
    pub secrets: HashMap<String, Value>,
}

/// Check if a string is a valid identifier (no template syntax)
pub fn is_valid_identifier(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }

    // Check for template syntax
    if s.contains("{{") || s.contains("}}") || s.contains("{%") || s.contains("%}") {
        return false;
    }

    // Check for valid Go-style identifier
    // Safe: We already checked that s is not empty above
    let first = s
        .chars()
        .next()
        .expect("string is not empty after length check");
    if !first.is_alphabetic() && first != '_' {
        return false;
    }

    s.chars().all(|c| c.is_alphanumeric() || c == '_')
}
