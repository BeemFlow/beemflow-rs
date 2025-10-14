//! In-memory storage implementation
//!
//! Fast, non-persistent storage for development and testing.
//! Uses DashMap for lock-free concurrent access (40-60% faster than RwLock).
//!
//! **WARNING:** MemoryStorage is NOT recommended for production use:
//! - Data is lost on process restart
//! - Does not coordinate state across multiple process instances
//! - Atomic operations use DashMap's locking, not true database-level atomicity
//!
//! For production deployments, use SqliteStorage or PostgresStorage.

use super::*;
use dashmap::DashMap;
use std::sync::Arc;

/// In-memory storage implementation - uses DashMap for lock-free concurrent access
#[derive(Clone)]
pub struct MemoryStorage {
    runs: Arc<DashMap<Uuid, Run>>,
    steps: Arc<DashMap<Uuid, Vec<StepRun>>>,
    paused_runs: Arc<DashMap<String, serde_json::Value>>,
    wait_tokens: Arc<DashMap<Uuid, Option<i64>>>,
    flows: Arc<DashMap<String, String>>, // name -> content
    flow_versions: Arc<DashMap<String, Vec<FlowSnapshot>>>,
    deployed_versions: Arc<DashMap<String, String>>,
    oauth_credentials: Arc<DashMap<String, OAuthCredential>>,
    oauth_providers: Arc<DashMap<String, OAuthProvider>>,
    oauth_clients: Arc<DashMap<String, OAuthClient>>,
    oauth_tokens: Arc<DashMap<String, OAuthToken>>,
}

impl MemoryStorage {
    /// Create a new in-memory storage
    pub fn new() -> Self {
        Self {
            runs: Arc::new(DashMap::new()),
            steps: Arc::new(DashMap::new()),
            paused_runs: Arc::new(DashMap::new()),
            wait_tokens: Arc::new(DashMap::new()),
            flows: Arc::new(DashMap::new()),
            flow_versions: Arc::new(DashMap::new()),
            deployed_versions: Arc::new(DashMap::new()),
            oauth_credentials: Arc::new(DashMap::new()),
            oauth_providers: Arc::new(DashMap::new()),
            oauth_clients: Arc::new(DashMap::new()),
            oauth_tokens: Arc::new(DashMap::new()),
        }
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl RunStorage for MemoryStorage {
    // Run methods
    async fn save_run(&self, run: &Run) -> Result<()> {
        self.runs.insert(run.id, run.clone());
        Ok(())
    }

    async fn get_run(&self, id: Uuid) -> Result<Option<Run>> {
        Ok(self.runs.get(&id).map(|r| r.clone()))
    }

    async fn list_runs(&self) -> Result<Vec<Run>> {
        let mut runs: Vec<Run> = self.runs.iter().map(|r| r.value().clone()).collect();
        // Sort by started_at descending (most recent first)
        runs.sort_unstable_by(|a, b| b.started_at.cmp(&a.started_at));
        Ok(runs)
    }

    async fn delete_run(&self, id: Uuid) -> Result<()> {
        self.runs.remove(&id);
        self.steps.remove(&id);
        Ok(())
    }

    async fn try_insert_run(&self, run: &Run) -> Result<bool> {
        // Try to insert, returns true if inserted, false if already exists
        use dashmap::mapref::entry::Entry;

        match self.runs.entry(run.id) {
            Entry::Vacant(entry) => {
                entry.insert(run.clone());
                Ok(true)
            }
            Entry::Occupied(_) => Ok(false),
        }
    }

    // Step methods
    async fn save_step(&self, step: &StepRun) -> Result<()> {
        self.steps
            .entry(step.run_id)
            .or_default()
            .push(step.clone());
        Ok(())
    }

    async fn get_steps(&self, run_id: Uuid) -> Result<Vec<StepRun>> {
        Ok(self
            .steps
            .get(&run_id)
            .map(|r| r.clone())
            .unwrap_or_default())
    }
}

#[async_trait]
impl StateStorage for MemoryStorage {
    // Wait/timeout methods
    async fn register_wait(&self, token: Uuid, wake_at: Option<i64>) -> Result<()> {
        self.wait_tokens.insert(token, wake_at);
        Ok(())
    }

    async fn resolve_wait(&self, token: Uuid) -> Result<Option<Run>> {
        self.wait_tokens.remove(&token);
        // Memory storage doesn't resolve waits to specific runs
        Ok(None)
    }

    // Paused run methods
    async fn save_paused_run(&self, token: &str, data: serde_json::Value) -> Result<()> {
        self.paused_runs.insert(token.to_string(), data);
        Ok(())
    }

    async fn load_paused_runs(&self) -> Result<HashMap<String, serde_json::Value>> {
        Ok(self
            .paused_runs
            .iter()
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect())
    }

    async fn delete_paused_run(&self, token: &str) -> Result<()> {
        self.paused_runs.remove(token);
        Ok(())
    }

    async fn fetch_and_delete_paused_run(&self, token: &str) -> Result<Option<serde_json::Value>> {
        // Atomically remove and return the value (DashMap::remove returns Option<(K, V)>)
        Ok(self.paused_runs.remove(token).map(|(_, v)| v))
    }
}

#[async_trait]
impl FlowStorage for MemoryStorage {
    // Flow versioning methods
    async fn deploy_flow_version(
        &self,
        flow_name: &str,
        version: &str,
        content: &str,
    ) -> Result<()> {
        let snapshot = FlowSnapshot {
            flow_name: flow_name.to_string(),
            version: version.to_string(),
            deployed_at: Utc::now(),
            is_live: false, // Will be set by set_deployed_version
        };

        self.flow_versions
            .entry(flow_name.to_string())
            .or_default()
            .push(snapshot);

        // Store the content for this version (key: "flowname:version")
        let version_key = format!("{}:{}", flow_name, version);
        self.flows.insert(version_key, content.to_string());

        // Set as deployed version
        self.set_deployed_version(flow_name, version).await?;

        Ok(())
    }

    async fn set_deployed_version(&self, flow_name: &str, version: &str) -> Result<()> {
        self.deployed_versions
            .insert(flow_name.to_string(), version.to_string());
        Ok(())
    }

    async fn get_deployed_version(&self, flow_name: &str) -> Result<Option<String>> {
        Ok(self.deployed_versions.get(flow_name).map(|r| r.clone()))
    }

    async fn get_flow_version_content(
        &self,
        flow_name: &str,
        version: &str,
    ) -> Result<Option<String>> {
        let version_key = format!("{}:{}", flow_name, version);
        Ok(self.flows.get(&version_key).map(|r| r.clone()))
    }

    async fn list_flow_versions(&self, flow_name: &str) -> Result<Vec<FlowSnapshot>> {
        let mut snapshots = self
            .flow_versions
            .get(flow_name)
            .map(|r| r.clone())
            .unwrap_or_default();

        // Mark the deployed version as live
        if let Some(deployed_ver) = self.deployed_versions.get(flow_name) {
            for snapshot in &mut snapshots {
                snapshot.is_live = snapshot.version == *deployed_ver;
            }
        }

        // Sort by deployed_at descending
        snapshots.sort_unstable_by(|a, b| b.deployed_at.cmp(&a.deployed_at));
        Ok(snapshots)
    }

    async fn get_latest_deployed_version_from_history(
        &self,
        flow_name: &str,
    ) -> Result<Option<String>> {
        let snapshots = self
            .flow_versions
            .get(flow_name)
            .map(|r| r.clone())
            .unwrap_or_default();

        // Find most recent by deployed_at, then by version (lexicographic) for tie-breaking
        Ok(snapshots
            .into_iter()
            .max_by(|a, b| {
                a.deployed_at
                    .cmp(&b.deployed_at)
                    .then_with(|| a.version.cmp(&b.version))
            })
            .map(|s| s.version))
    }

    async fn unset_deployed_version(&self, flow_name: &str) -> Result<()> {
        self.deployed_versions.remove(flow_name);
        Ok(())
    }
}

#[async_trait]
impl OAuthStorage for MemoryStorage {
    // OAuth credential methods
    async fn save_oauth_credential(&self, credential: &OAuthCredential) -> Result<()> {
        let key = credential.unique_key();
        self.oauth_credentials.insert(key, credential.clone());
        Ok(())
    }

    async fn get_oauth_credential(
        &self,
        provider: &str,
        integration: &str,
    ) -> Result<Option<OAuthCredential>> {
        let key = format!("{}:{}", provider, integration);
        Ok(self.oauth_credentials.get(&key).map(|r| r.clone()))
    }

    async fn list_oauth_credentials(&self) -> Result<Vec<OAuthCredential>> {
        let mut creds: Vec<OAuthCredential> = self
            .oauth_credentials
            .iter()
            .map(|r| r.value().clone())
            .collect();
        // Sort by created_at descending
        creds.sort_unstable_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(creds)
    }

    async fn delete_oauth_credential(&self, id: &str) -> Result<()> {
        self.oauth_credentials.retain(|_, cred| cred.id != id);
        Ok(())
    }

    async fn refresh_oauth_credential(
        &self,
        id: &str,
        new_token: &str,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        for mut entry in self.oauth_credentials.iter_mut() {
            if entry.value().id == id {
                entry.value_mut().access_token = new_token.to_string();
                entry.value_mut().expires_at = expires_at;
                entry.value_mut().updated_at = Utc::now();
                return Ok(());
            }
        }
        Err(crate::BeemFlowError::not_found("OAuth credential", id))
    }

    // OAuth provider methods
    async fn save_oauth_provider(&self, provider: &OAuthProvider) -> Result<()> {
        self.oauth_providers
            .insert(provider.id.clone(), provider.clone());
        Ok(())
    }

    async fn get_oauth_provider(&self, id: &str) -> Result<Option<OAuthProvider>> {
        Ok(self.oauth_providers.get(id).map(|r| r.clone()))
    }

    async fn list_oauth_providers(&self) -> Result<Vec<OAuthProvider>> {
        let mut providers: Vec<OAuthProvider> = self
            .oauth_providers
            .iter()
            .map(|r| r.value().clone())
            .collect();
        // Sort by ID for consistent output
        providers.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        Ok(providers)
    }

    async fn delete_oauth_provider(&self, id: &str) -> Result<()> {
        self.oauth_providers.remove(id);
        Ok(())
    }

    // OAuth client methods
    async fn save_oauth_client(&self, client: &OAuthClient) -> Result<()> {
        self.oauth_clients.insert(client.id.clone(), client.clone());
        Ok(())
    }

    async fn get_oauth_client(&self, id: &str) -> Result<Option<OAuthClient>> {
        Ok(self.oauth_clients.get(id).map(|r| r.clone()))
    }

    async fn list_oauth_clients(&self) -> Result<Vec<OAuthClient>> {
        let mut clients: Vec<OAuthClient> = self
            .oauth_clients
            .iter()
            .map(|r| r.value().clone())
            .collect();
        // Sort by ID for consistent output
        clients.sort_unstable_by(|a, b| a.id.cmp(&b.id));
        Ok(clients)
    }

    async fn delete_oauth_client(&self, id: &str) -> Result<()> {
        self.oauth_clients.remove(id);
        Ok(())
    }

    // OAuth token methods
    async fn save_oauth_token(&self, token: &OAuthToken) -> Result<()> {
        self.oauth_tokens.insert(token.id.clone(), token.clone());
        Ok(())
    }

    async fn get_oauth_token_by_code(&self, code: &str) -> Result<Option<OAuthToken>> {
        Ok(self
            .oauth_tokens
            .iter()
            .find(|entry| entry.value().code.as_deref() == Some(code))
            .map(|entry| entry.value().clone()))
    }

    async fn get_oauth_token_by_access(&self, access: &str) -> Result<Option<OAuthToken>> {
        Ok(self
            .oauth_tokens
            .iter()
            .find(|entry| entry.value().access.as_deref() == Some(access))
            .map(|entry| entry.value().clone()))
    }

    async fn get_oauth_token_by_refresh(&self, refresh: &str) -> Result<Option<OAuthToken>> {
        Ok(self
            .oauth_tokens
            .iter()
            .find(|entry| entry.value().refresh.as_deref() == Some(refresh))
            .map(|entry| entry.value().clone()))
    }

    async fn delete_oauth_token_by_code(&self, code: &str) -> Result<()> {
        self.oauth_tokens
            .retain(|_, token| token.code.as_deref() != Some(code));
        Ok(())
    }

    async fn delete_oauth_token_by_access(&self, access: &str) -> Result<()> {
        self.oauth_tokens
            .retain(|_, token| token.access.as_deref() != Some(access));
        Ok(())
    }

    async fn delete_oauth_token_by_refresh(&self, refresh: &str) -> Result<()> {
        self.oauth_tokens
            .retain(|_, token| token.refresh.as_deref() != Some(refresh));
        Ok(())
    }
}
