//! Default (embedded) registry
//!
//! Provides founder-curated tools embedded in the binary.

use super::*;
use crate::Result;

/// Default registry with embedded tools
pub struct DefaultRegistry {
    registry_name: String,
}

impl DefaultRegistry {
    /// Create a new default registry
    pub fn new() -> Self {
        Self {
            registry_name: "default".to_string(),
        }
    }

    /// List all servers from default registry
    pub async fn list_servers(&self) -> Result<Vec<RegistryEntry>> {
        // Load embedded default.json
        let data = include_str!("default.json");
        let mut entries: Vec<RegistryEntry> = serde_json::from_str(data)?;

        // Label all entries with default registry
        for entry in &mut entries {
            entry.registry = Some(self.registry_name.clone());
        }

        Ok(entries)
    }

    /// Get a specific server by name
    pub async fn get_server(&self, name: &str) -> Result<Option<RegistryEntry>> {
        let entries = self.list_servers().await?;
        Ok(entries.into_iter().find(|e| e.name == name))
    }

    /// List OAuth providers from default registry
    pub async fn list_oauth_providers(&self) -> Result<Vec<RegistryEntry>> {
        let entries = self.list_servers().await?;
        let providers: Vec<RegistryEntry> = entries
            .into_iter()
            .filter(|e| e.entry_type == "oauth_provider")
            .filter_map(expand_oauth_provider_env_vars)
            .collect();
        Ok(providers)
    }

    /// Get OAuth provider by name
    pub async fn get_oauth_provider(&self, name: &str) -> Result<Option<RegistryEntry>> {
        let providers = self.list_oauth_providers().await?;
        Ok(providers.into_iter().find(|p| p.name == name))
    }
}

impl Default for DefaultRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Expand environment variables in OAuth provider configuration
/// Returns None if any required environment variables are missing
pub fn expand_oauth_provider_env_vars(mut entry: RegistryEntry) -> Option<RegistryEntry> {
    let mut missing_vars = Vec::new();

    if let Some(ref client_id) = entry.client_id {
        match expand_env_value_checked(client_id) {
            Ok(val) => entry.client_id = Some(val),
            Err(var_name) => missing_vars.push(var_name),
        }
    }

    if let Some(ref client_secret) = entry.client_secret {
        match expand_env_value_checked(client_secret) {
            Ok(val) => entry.client_secret = Some(val),
            Err(var_name) => missing_vars.push(var_name),
        }
    }

    if !missing_vars.is_empty() {
        tracing::info!(
            "Skipping OAuth provider '{}' - missing environment variables: {}. Set these variables to enable this provider.",
            entry.name,
            missing_vars.join(", ")
        );
        return None;
    }

    Some(entry)
}

/// Expand $env:VARNAME syntax, returning error if variable is not found
fn expand_env_value_checked(value: &str) -> std::result::Result<String, String> {
    if value.starts_with("$env:") {
        let var_name = value.trim_start_matches("$env:");
        match std::env::var(var_name) {
            Ok(val) => {
                tracing::debug!("Expanded env var {} from {}", var_name, value);
                Ok(val)
            }
            Err(_) => Err(var_name.to_string()),
        }
    } else {
        Ok(value.to_string())
    }
}
