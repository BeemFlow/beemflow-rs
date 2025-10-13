//! HTTP adapter for making HTTP requests

use super::*;
use crate::constants::*;
use reqwest::{Client, Method};
use std::str::FromStr;

/// Type alias for HTTP request components (method, url, headers, body)
type HttpRequestComponents = (String, String, HashMap<String, String>, Option<Value>);

/// HTTP adapter for generic HTTP requests and registry tools
pub struct HttpAdapter {
    adapter_id: String,
    tool_manifest: Option<ToolManifest>,
    client: Client,
}

impl HttpAdapter {
    /// Create a new HTTP adapter
    pub fn new(adapter_id: String, tool_manifest: Option<ToolManifest>) -> Self {
        Self {
            adapter_id,
            tool_manifest,
            client: Client::new(),
        }
    }

    /// Execute HTTP request based on manifest or generic http call
    async fn execute_request(
        &self,
        mut inputs: HashMap<String, Value>,
    ) -> Result<HashMap<String, Value>> {
        // Extract storage from inputs if present (injected by engine via context)
        // This matches Go's pattern of injecting storage via context (engine.go line 1052)
        let storage: Option<Arc<dyn crate::storage::Storage>> =
            inputs.remove("__storage").and_then(|_v| {
                // Storage is passed as a type-erased value
                // For now, we'll skip this complex deserialization
                // and handle OAuth differently
                None
            });

        // Build request based on manifest or inputs
        let (url, method, mut headers, body) = if let Some(manifest) = &self.tool_manifest {
            // If manifest has endpoint, use it; otherwise fall back to inputs
            if manifest.endpoint.is_some() {
                self.build_from_manifest(manifest, &inputs)?
            } else {
                // Manifest exists but no endpoint - generic HTTP like http.fetch
                self.build_from_inputs(&inputs)?
            }
        } else {
            self.build_from_inputs(&inputs)?
        };

        // Expand OAuth tokens in headers if storage is available
        self.expand_oauth_in_headers(&mut headers, storage).await;

        // Create request
        let method_str = method.clone(); // Keep for error messages
        let method = Method::from_str(&method)
            .map_err(|e| crate::BeemFlowError::adapter(format!("invalid HTTP method: {}", e)))?;

        let mut request = self.client.request(method, &url);

        // Add headers
        for (k, v) in headers {
            request = request.header(k, v);
        }

        // Add body if present
        if let Some(body_val) = body {
            if body_val.is_object() || body_val.is_array() {
                request = request.json(&body_val);
            } else if let Some(s) = body_val.as_str() {
                request = request.body(s.to_string());
            }
        }

        // Execute request
        let response = request.send().await.map_err(|e| {
            crate::BeemFlowError::Network(crate::error::NetworkError::Http(e.to_string()))
        })?;

        // Check status code
        let status = response.status();

        // Extract response body
        let body_text = response.text().await.map_err(|e| {
            crate::BeemFlowError::Network(crate::error::NetworkError::Http(e.to_string()))
        })?;

        // Return error for non-2xx status codes
        if !status.is_success() {
            return Err(crate::BeemFlowError::Network(
                crate::error::NetworkError::Http(format!(
                    "HTTP {} {}: status {}: {}",
                    method_str,
                    url,
                    status.as_u16(),
                    body_text
                )),
            ));
        }

        // Try to parse as JSON
        if let Ok(json_value) = serde_json::from_str::<Value>(&body_text) {
            // For JSON objects, return the object directly (unwrapped)
            if let Some(obj) = json_value.as_object() {
                return Ok(obj.clone().into_iter().collect());
            }
            // For JSON arrays or primitives, wrap in body
            let mut result = HashMap::new();
            result.insert("body".to_string(), json_value);
            return Ok(result);
        }

        // For non-JSON responses, wrap in body
        let mut result = HashMap::new();
        result.insert("body".to_string(), Value::String(body_text));
        Ok(result)
    }

    fn build_from_manifest(
        &self,
        manifest: &ToolManifest,
        inputs: &HashMap<String, Value>,
    ) -> Result<HttpRequestComponents> {
        let mut url = manifest
            .endpoint
            .as_ref()
            .ok_or_else(|| crate::BeemFlowError::adapter("manifest missing endpoint"))?
            .clone();

        let method = manifest
            .method
            .as_ref()
            .unwrap_or(&HTTP_METHOD_GET.to_string())
            .clone();

        // Track which parameters are used in the URL path
        let mut path_params = std::collections::HashSet::new();

        // Render URL template with input parameters
        // Replace {param} with values from inputs
        for (key, value) in inputs {
            let placeholder = format!("{{{}}}", key);
            if url.contains(&placeholder) {
                path_params.insert(key.clone());
                let value_str = match value {
                    Value::String(s) => s.clone(),
                    Value::Number(n) => n.to_string(),
                    Value::Bool(b) => b.to_string(),
                    _ => serde_json::to_string(value)?,
                };
                url = url.replace(&placeholder, &value_str);
            }
        }

        // Process headers
        let mut headers = HashMap::new();
        if let Some(ref manifest_headers) = manifest.headers {
            for (k, v) in manifest_headers {
                let expanded = self.expand_header_value(v);
                headers.insert(k.clone(), expanded);
            }
        }

        // Build body from inputs for non-GET requests
        let body = if method.to_uppercase() != HTTP_METHOD_GET {
            // Filter out path parameters from the body
            let body_inputs: HashMap<String, Value> = inputs
                .iter()
                .filter(|(k, _)| !path_params.contains(*k))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();

            if !body_inputs.is_empty() {
                Some(serde_json::to_value(body_inputs)?)
            } else {
                None
            }
        } else {
            None
        };

        Ok((url, method, headers, body))
    }

    /// Expand $env: and $oauth: references in header values
    ///
    /// This is a synchronous helper - actual OAuth expansion must happen
    /// at execution time when we have access to storage context.
    fn expand_header_value(&self, value: &str) -> String {
        // Handle $env:VARNAME
        if value.starts_with("$env:") {
            let var_name = value.trim_start_matches("$env:");
            return std::env::var(var_name).unwrap_or_else(|_| value.to_string());
        }

        // Handle $oauth:provider:integration
        // Note: OAuth expansion is deferred to execution time
        // This just marks the value for expansion
        if value.starts_with("$oauth:") {
            return value.to_string();
        }

        // Handle Bearer $env: prefix
        if value.starts_with("Bearer $env:") {
            let var_name = value.trim_start_matches("Bearer $env:");
            if let Ok(token) = std::env::var(var_name) {
                return format!("Bearer {}", token);
            }
        }

        value.to_string()
    }

    /// Expand OAuth tokens in headers at execution time
    ///
    /// This async version performs actual OAuth token lookups from storage.
    /// Matches Go's expandValue function (http_adapter.go lines 356-384).
    async fn expand_oauth_in_headers(
        &self,
        headers: &mut HashMap<String, String>,
        storage: Option<Arc<dyn crate::storage::Storage>>,
    ) {
        // Clone headers to avoid borrowing issues
        let headers_clone: Vec<(String, String)> = headers
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        for (key, value) in headers_clone {
            if value.starts_with("$oauth:") {
                if let Some(ref store) = storage {
                    // Parse $oauth:provider:integration
                    let oauth_ref = value.trim_start_matches("$oauth:");
                    let parts: Vec<&str> = oauth_ref.split(':').collect();

                    if parts.len() == 2 {
                        let provider = parts[0];
                        let integration = parts[1];

                        // Get OAuth credential directly from storage
                        // Note: We don't need OAuthClientManager here - just query storage
                        match store.get_oauth_credential(provider, integration).await {
                            Ok(Some(cred)) => {
                                // Check if token is expired (same logic as OAuthClientManager::needs_refresh)
                                let needs_refresh = if let Some(expires_at) = cred.expires_at {
                                    let buffer = chrono::Duration::minutes(5);
                                    chrono::Utc::now() + buffer >= expires_at
                                } else {
                                    false
                                };

                                if needs_refresh {
                                    tracing::warn!(
                                        "OAuth token for {}:{} is expired. Consider refreshing via the OAuth flow.",
                                        provider,
                                        integration
                                    );
                                }

                                // Use the token even if expired (provider may still accept it)
                                headers.insert(key, format!("Bearer {}", cred.access_token));
                            }
                            Ok(None) => {
                                tracing::warn!(
                                    "OAuth credential not found for {}:{}",
                                    provider,
                                    integration
                                );
                                // Keep original value if OAuth credential not found
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to get OAuth credential for {}:{} - {}",
                                    provider,
                                    integration,
                                    e
                                );
                                // Keep original value if OAuth fetch fails
                            }
                        }
                    }
                } else {
                    tracing::warn!("Storage not available for OAuth token expansion");
                }
            }
        }
    }

    fn build_from_inputs(&self, inputs: &HashMap<String, Value>) -> Result<HttpRequestComponents> {
        let url = inputs
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| crate::BeemFlowError::adapter("missing url for HTTP request"))?
            .to_string();

        let method = self.extract_method(inputs);
        let headers = self.extract_headers(inputs);
        let body = inputs.get("body").cloned();

        Ok((url, method, headers, body))
    }

    // ========================================
    // HELPER METHODS (used by tests)
    // ========================================

    /// Extract headers from inputs
    fn extract_headers(&self, inputs: &HashMap<String, Value>) -> HashMap<String, String> {
        inputs
            .get("headers")
            .and_then(|v| {
                if let Some(obj) = v.as_object() {
                    let mut headers = HashMap::new();
                    for (k, v) in obj {
                        if let Some(s) = v.as_str() {
                            headers.insert(k.clone(), s.to_string());
                        }
                    }
                    Some(headers)
                } else {
                    None
                }
            })
            .unwrap_or_default()
    }

    /// Extract method from inputs, default to GET
    fn extract_method(&self, inputs: &HashMap<String, Value>) -> String {
        inputs
            .get("method")
            .and_then(|v| v.as_str())
            .unwrap_or(HTTP_METHOD_GET)
            .to_string()
    }

    /// Enrich inputs with defaults from manifest parameters
    fn enrich_inputs_with_defaults(
        &self,
        mut inputs: HashMap<String, Value>,
    ) -> HashMap<String, Value> {
        if let Some(manifest) = &self.tool_manifest
            && let Some(properties) = manifest
                .parameters
                .get("properties")
                .and_then(|v| v.as_object())
        {
            for (key, prop_def) in properties {
                // Only set default if input doesn't already have this key
                if !inputs.contains_key(key)
                    && let Some(prop_obj) = prop_def.as_object()
                    && let Some(default) = prop_obj.get("default")
                {
                    // Handle environment variable expansion in defaults
                    if let Some(default_str) = default.as_str() {
                        if default_str.starts_with("$env:") {
                            let var_name = default_str.trim_start_matches("$env:");
                            if let Ok(env_val) = std::env::var(var_name) {
                                inputs.insert(key.clone(), Value::String(env_val));
                            }
                        } else {
                            inputs.insert(key.clone(), default.clone());
                        }
                    } else {
                        inputs.insert(key.clone(), default.clone());
                    }
                }
            }
        }
        inputs
    }
}

#[async_trait]
impl Adapter for HttpAdapter {
    fn id(&self) -> &str {
        &self.adapter_id
    }

    async fn execute(&self, inputs: HashMap<String, Value>) -> Result<HashMap<String, Value>> {
        // Enrich inputs with defaults from manifest if present
        let enriched_inputs = self.enrich_inputs_with_defaults(inputs);
        self.execute_request(enriched_inputs).await
    }

    fn manifest(&self) -> Option<ToolManifest> {
        self.tool_manifest.clone()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
