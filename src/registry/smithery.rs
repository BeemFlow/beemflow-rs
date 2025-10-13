//! Smithery registry
//!
//! Integrates with Smithery.ai registry for community MCP servers.

use super::*;

/// Smithery registry integration
pub struct SmitheryRegistry {
    api_key: Option<String>,
    base_url: String,
}

impl SmitheryRegistry {
    /// Create a new Smithery registry
    pub fn new(api_key: Option<String>) -> Self {
        Self {
            api_key,
            base_url: "https://registry.smithery.ai/servers".to_string(),
        }
    }

    /// List all entries from Smithery
    pub async fn list_servers(&self) -> Result<Vec<RegistryEntry>> {
        // Build request with API key if available
        let client = reqwest::Client::new();
        let mut request = client.get(&self.base_url);

        if let Some(ref key) = self.api_key {
            request = request.header("Authorization", format!("Bearer {}", key));
        }

        let response = request.send().await.map_err(|e| {
            crate::BeemFlowError::Network(crate::error::NetworkError::Http(e.to_string()))
        })?;

        let mut entries: Vec<RegistryEntry> = response.json().await.map_err(|e| {
            crate::BeemFlowError::Network(crate::error::NetworkError::Http(e.to_string()))
        })?;

        // Label all entries with smithery registry
        for entry in &mut entries {
            entry.registry = Some("smithery".to_string());
        }

        Ok(entries)
    }

    /// Get a specific entry by name
    pub async fn get_server(&self, name: &str) -> Result<Option<RegistryEntry>> {
        let entries = self.list_servers().await?;
        Ok(entries.into_iter().find(|e| e.name == name))
    }
}

impl Default for SmitheryRegistry {
    fn default() -> Self {
        let api_key = std::env::var(crate::constants::ENV_SMITHERY_KEY).ok();
        Self::new(api_key)
    }
}
