//! Registry manager
//!
//! Coordinates multiple registries with priority order.

use super::*;
use crate::{Result, config::Config};

/// Registry manager that coordinates multiple registries
pub struct RegistryManager {
    registries: Vec<Box<dyn RegistrySource>>,
}

/// Trait for registry sources
#[async_trait::async_trait]
pub trait RegistrySource: Send + Sync {
    async fn list_servers(&self) -> Result<Vec<RegistryEntry>>;
    async fn get_server(&self, name: &str) -> Result<Option<RegistryEntry>>;
}

#[async_trait::async_trait]
impl RegistrySource for DefaultRegistry {
    async fn list_servers(&self) -> Result<Vec<RegistryEntry>> {
        self.list_servers().await
    }

    async fn get_server(&self, name: &str) -> Result<Option<RegistryEntry>> {
        self.get_server(name).await
    }
}

#[async_trait::async_trait]
impl RegistrySource for LocalRegistry {
    async fn list_servers(&self) -> Result<Vec<RegistryEntry>> {
        self.list_servers().await
    }

    async fn get_server(&self, name: &str) -> Result<Option<RegistryEntry>> {
        self.get_server(name).await
    }
}

#[async_trait::async_trait]
impl RegistrySource for RemoteRegistry {
    async fn list_servers(&self) -> Result<Vec<RegistryEntry>> {
        self.list_servers().await
    }

    async fn get_server(&self, name: &str) -> Result<Option<RegistryEntry>> {
        self.get_server(name).await
    }
}

#[async_trait::async_trait]
impl RegistrySource for SmitheryRegistry {
    async fn list_servers(&self) -> Result<Vec<RegistryEntry>> {
        self.list_servers().await
    }

    async fn get_server(&self, name: &str) -> Result<Option<RegistryEntry>> {
        self.get_server(name).await
    }
}

impl RegistryManager {
    /// Create a new registry manager with given sources (in priority order)
    pub fn new(registries: Vec<Box<dyn RegistrySource>>) -> Self {
        Self { registries }
    }

    /// Create standard manager with all registry types
    /// Priority: local → remote → hub → default
    pub fn standard(config: Option<&Config>) -> Self {
        let mut registries: Vec<Box<dyn RegistrySource>> = Vec::new();

        // 1. Local registry (highest priority)
        let local_path = config
            .and_then(|c| c.registries.as_ref())
            .and_then(|regs| regs.iter().find(|r| r.registry_type == "local"))
            .and_then(|r| r.path.as_ref())
            .map(|p| p.as_str())
            .unwrap_or("");

        registries.push(Box::new(LocalRegistry::new(local_path)));

        // 2. Remote registries from config
        if let Some(cfg) = config
            && let Some(ref regs) = cfg.registries
        {
            for reg in regs {
                if reg.registry_type == "remote"
                    && let Some(ref url) = reg.url
                {
                    registries.push(Box::new(RemoteRegistry::new(url, "remote")));
                }
            }
        }

        // 3. Smithery if API key available (disabled by default)
        // if let Ok(api_key) = std::env::var(crate::constants::ENV_SMITHERY_KEY) {
        //     if !api_key.is_empty() {
        //         registries.push(Box::new(SmitheryRegistry::new(Some(api_key))));
        //     }
        // }

        // 4. Hub registry (community curated) - disabled by default
        // registries.push(Box::new(RemoteRegistry::new(
        //     "https://hub.beemflow.com/index.json",
        //     "hub"
        // )));

        // 5. Default registry (lowest priority)
        registries.push(Box::new(DefaultRegistry::new()));

        Self { registries }
    }

    /// List all servers from all registries (first wins for duplicates)
    pub async fn list_all_servers(&self) -> Result<Vec<RegistryEntry>> {
        let mut all_entries = Vec::new();
        let mut seen_names = std::collections::HashSet::new();

        // Iterate in priority order
        for registry in &self.registries {
            match registry.list_servers().await {
                Ok(entries) => {
                    for entry in entries {
                        // Only add if not already seen (first registry wins)
                        if seen_names.insert(entry.name.clone()) {
                            all_entries.push(entry);
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to load registry: {}", e);
                    // Continue with other registries
                }
            }
        }

        Ok(all_entries)
    }

    /// Get a specific server by name (first matching registry wins)
    pub async fn get_server(&self, name: &str) -> Result<Option<RegistryEntry>> {
        for registry in &self.registries {
            match registry.get_server(name).await {
                Ok(Some(entry)) => return Ok(Some(entry)),
                Ok(None) => continue,
                Err(e) => {
                    tracing::warn!("Error querying registry: {}", e);
                    continue;
                }
            }
        }

        Ok(None)
    }

    /// List OAuth provider definitions from all registries
    ///
    /// Returns OAuth provider entries with environment variables expanded.
    /// These define how to authenticate (client_id, auth_url, etc.)
    /// but don't contain user credentials (which are stored in Storage).
    pub async fn list_oauth_providers(&self) -> Result<Vec<RegistryEntry>> {
        let all_servers = self.list_all_servers().await?;
        let providers: Vec<RegistryEntry> = all_servers
            .into_iter()
            .filter(|e| e.entry_type == "oauth_provider")
            .map(super::default::expand_oauth_provider_env_vars)
            .collect();
        Ok(providers)
    }

    /// Get a specific OAuth provider by name
    ///
    /// Returns the OAuth provider definition with environment variables expanded.
    pub async fn get_oauth_provider(&self, name: &str) -> Result<Option<RegistryEntry>> {
        tracing::debug!("get_oauth_provider called for: {}", name);

        if let Some(entry) = self.get_server(name).await? {
            let has_client_id = entry.client_id.is_some();
            tracing::debug!(
                "Found server entry for '{}': type={}, has_client_id={}",
                name,
                entry.entry_type,
                has_client_id
            );

            if entry.entry_type == "oauth_provider" {
                tracing::debug!("Entry is oauth_provider, expanding env vars for '{}'", name);
                let expanded = super::default::expand_oauth_provider_env_vars(entry);
                let has_expanded_client_id = expanded.client_id.is_some();
                let has_expanded_client_secret = expanded.client_secret.is_some();
                tracing::debug!(
                    "After expansion: has_client_id={}, has_client_secret={}",
                    has_expanded_client_id,
                    has_expanded_client_secret
                );
                return Ok(Some(expanded));
            } else {
                tracing::warn!(
                    "Entry '{}' is not an oauth_provider (type={})",
                    name,
                    entry.entry_type
                );
            }
        } else {
            tracing::warn!("No server entry found for '{}'", name);
        }
        Ok(None)
    }
}
