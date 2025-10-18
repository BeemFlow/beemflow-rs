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
    pub async fn list_oauth_providers(
        &self,
        secrets_provider: &std::sync::Arc<dyn crate::secrets::SecretsProvider>,
    ) -> Result<Vec<RegistryEntry>> {
        let entries = self.list_servers().await?;

        // Expand environment variables for each OAuth provider
        let mut providers = Vec::new();
        for entry in entries {
            if entry.entry_type == "oauth_provider"
                && let Some(expanded) =
                    expand_oauth_provider_env_vars(entry, secrets_provider).await
            {
                providers.push(expanded);
            }
        }

        Ok(providers)
    }

    /// Get OAuth provider by name
    pub async fn get_oauth_provider(
        &self,
        name: &str,
        secrets_provider: &std::sync::Arc<dyn crate::secrets::SecretsProvider>,
    ) -> Result<Option<RegistryEntry>> {
        let providers = self.list_oauth_providers(secrets_provider).await?;
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
pub async fn expand_oauth_provider_env_vars(
    mut entry: RegistryEntry,
    secrets_provider: &std::sync::Arc<dyn crate::secrets::SecretsProvider>,
) -> Option<RegistryEntry> {
    let mut missing_vars = Vec::new();

    // Expand client_id if present
    if let Some(ref client_id) = entry.client_id {
        match expand_env_value_checked(client_id, secrets_provider).await {
            Ok(val) => entry.client_id = Some(val),
            Err(var_name) => missing_vars.push(var_name),
        }
    }

    // Expand client_secret if present
    if let Some(ref client_secret) = entry.client_secret {
        match expand_env_value_checked(client_secret, secrets_provider).await {
            Ok(val) => entry.client_secret = Some(val),
            Err(var_name) => missing_vars.push(var_name),
        }
    }

    // If any required variables are missing, skip this provider
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

/// Expand $env:VARNAME syntax using secrets provider
/// Returns error if variable is not found
async fn expand_env_value_checked(
    value: &str,
    secrets_provider: &std::sync::Arc<dyn crate::secrets::SecretsProvider>,
) -> std::result::Result<String, String> {
    if value.starts_with("$env:") {
        let var_name = value.trim_start_matches("$env:");
        match secrets_provider.get_secret(var_name).await {
            Ok(Some(val)) => {
                tracing::debug!("Expanded env var {} from {}", var_name, value);
                Ok(val)
            }
            Ok(None) => Err(var_name.to_string()),
            Err(e) => {
                tracing::error!("Failed to get secret {}: {}", var_name, e);
                Err(var_name.to_string())
            }
        }
    } else {
        Ok(value.to_string())
    }
}
