//! Local registry
//!
//! User-writable registry for custom tools (.beemflow/registry.json).

use super::*;
use std::path::PathBuf;

/// Local registry for user-installed tools
pub struct LocalRegistry {
    path: PathBuf,
}

impl LocalRegistry {
    /// Create a new local registry
    pub fn new(path: &str) -> Self {
        let path_buf = if path.is_empty() {
            crate::config::default_local_registry_path()
        } else {
            PathBuf::from(path)
        };

        Self { path: path_buf }
    }

    /// List all entries from local registry
    pub async fn list_servers(&self) -> Result<Vec<RegistryEntry>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }

        let content = tokio::fs::read_to_string(&self.path).await?;
        let mut entries: Vec<RegistryEntry> = serde_json::from_str(&content)?;

        // Label all entries with local registry
        for entry in &mut entries {
            entry.registry = Some("local".to_string());
        }

        Ok(entries)
    }

    /// Get a specific entry by name
    pub async fn get_server(&self, name: &str) -> Result<Option<RegistryEntry>> {
        let entries = self.list_servers().await?;
        Ok(entries.into_iter().find(|e| e.name == name))
    }

    /// Add or update an entry
    pub async fn upsert_entry(&self, entry: RegistryEntry) -> Result<()> {
        let mut entries = self.list_servers().await.unwrap_or_default();

        // Remove existing entry with same name
        entries.retain(|e| e.name != entry.name);

        // Add new entry
        entries.push(entry);

        // Save back to file
        let content = serde_json::to_string_pretty(&entries)?;

        // Create parent directory if needed
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        tokio::fs::write(&self.path, content).await?;
        Ok(())
    }

    /// Remove an entry by name
    pub async fn remove_entry(&self, name: &str) -> Result<bool> {
        let mut entries = self.list_servers().await.unwrap_or_default();
        let initial_len = entries.len();

        entries.retain(|e| e.name != name);

        if entries.len() == initial_len {
            return Ok(false); // Nothing removed
        }

        // Save back to file
        let content = serde_json::to_string_pretty(&entries)?;
        tokio::fs::write(&self.path, content).await?;
        Ok(true)
    }
}
