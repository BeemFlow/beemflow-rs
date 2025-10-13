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
            .map(expand_oauth_provider_env_vars)
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
pub fn expand_oauth_provider_env_vars(mut entry: RegistryEntry) -> RegistryEntry {
    if let Some(ref client_id) = entry.client_id {
        entry.client_id = Some(expand_env_value(client_id));
    }
    if let Some(ref client_secret) = entry.client_secret {
        entry.client_secret = Some(expand_env_value(client_secret));
    }
    entry
}

/// Expand $env:VARNAME syntax
fn expand_env_value(value: &str) -> String {
    if value.starts_with("$env:") {
        let var_name = value.trim_start_matches("$env:");
        match std::env::var(var_name) {
            Ok(val) => {
                tracing::debug!("Expanded env var {} from {}", var_name, value);
                val
            }
            Err(_) => {
                tracing::warn!(
                    "Environment variable {} not found, keeping placeholder {}",
                    var_name,
                    value
                );
                value.to_string()
            }
        }
    } else {
        value.to_string()
    }
}
