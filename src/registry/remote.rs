//! Remote registry
//!
//! Fetches tools from remote HTTP registries.

use super::*;

/// Remote registry (HTTP-based)
pub struct RemoteRegistry {
    url: String,
    name: String,
}

impl RemoteRegistry {
    /// Create a new remote registry
    pub fn new(url: &str, name: &str) -> Self {
        Self {
            url: url.to_string(),
            name: name.to_string(),
        }
    }

    /// List all entries from remote registry
    pub async fn list_servers(&self) -> Result<Vec<RegistryEntry>> {
        // Fetch from URL
        let response = reqwest::get(&self.url).await.map_err(|e| {
            crate::BeemFlowError::Network(crate::error::NetworkError::Http(e.to_string()))
        })?;

        let mut entries: Vec<RegistryEntry> = response.json().await.map_err(|e| {
            crate::BeemFlowError::Network(crate::error::NetworkError::Http(e.to_string()))
        })?;

        // Label all entries with registry name
        for entry in &mut entries {
            entry.registry = Some(self.name.clone());
        }

        Ok(entries)
    }

    /// Get a specific entry by name
    pub async fn get_server(&self, name: &str) -> Result<Option<RegistryEntry>> {
        let entries = self.list_servers().await?;
        Ok(entries.into_iter().find(|e| e.name == name))
    }
}
