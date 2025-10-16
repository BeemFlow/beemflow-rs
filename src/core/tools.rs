//! Tool operations module
//!
//! All operations for managing tools and adapters.

use super::*;
use beemflow_core_macros::{operation, operation_group};
use schemars::JsonSchema;

#[operation_group(tools)]
pub mod tools {
    use super::*;

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Empty input (no parameters required)")]
    pub struct EmptyInput {}

    #[derive(Serialize)]
    pub struct ListOutput {
        pub tools: Vec<serde_json::Value>,
    }

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for retrieving a tool manifest")]
    pub struct GetManifestInput {
        #[schemars(description = "Name of the tool to retrieve")]
        pub name: String,
    }

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for searching tools")]
    pub struct SearchInput {
        #[schemars(description = "Search query (optional, returns all if omitted)")]
        pub query: Option<String>,
    }

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for installing a tool")]
    pub struct InstallInput {
        #[schemars(description = "Name of the tool to install from registry")]
        pub name: Option<String>,
        #[schemars(description = "Tool manifest as JSON (alternative to name)")]
        pub manifest: Option<Value>,
    }

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for converting OpenAPI specification to tools")]
    pub struct ConvertOpenAPIInput {
        #[schemars(description = "OpenAPI specification as JSON string")]
        pub openapi: String,
        #[schemars(description = "Custom API name (defaults to spec title)")]
        pub api_name: Option<String>,
        #[schemars(description = "Base URL for API (defaults to first server in spec)")]
        pub base_url: Option<String>,
    }

    /// List all tools
    #[operation(
        name = "list_tools",
        input = EmptyInput,
        http = "GET /tools",
        cli = "tools list",
        description = "List all tools"
    )]
    pub struct List {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for List {
        type Input = EmptyInput;
        type Output = ListOutput;

        async fn execute(&self, _input: Self::Input) -> Result<Self::Output> {
            let entries = self.deps.registry_manager.list_all_servers().await?;

            // Filter to just tools
            let tools: Vec<serde_json::Value> = entries
                .into_iter()
                .filter(|e| e.entry_type == "tool")
                .map(|e| serde_json::to_value(e).unwrap_or_default())
                .collect();

            Ok(ListOutput { tools })
        }
    }

    /// Get tool manifest
    #[operation(
        name = "get_tool_manifest",
        input = GetManifestInput,
        http = "GET /tools/{name}",
        cli = "tools get <NAME>",
        description = "Get tool manifest"
    )]
    pub struct GetManifest {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for GetManifest {
        type Input = GetManifestInput;
        type Output = Value;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            let entry = self
                .deps
                .registry_manager
                .get_server(&input.name)
                .await?
                .ok_or_else(|| not_found("Tool", &input.name))?;

            Ok(serde_json::to_value(entry)?)
        }
    }

    /// Search for tools
    #[operation(
        name = "search_tools",
        input = SearchInput,
        http = "GET /tools/search",
        cli = "tools search [<QUERY>]",
        description = "Search for tools"
    )]
    pub struct Search {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for Search {
        type Input = SearchInput;
        type Output = Value;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            let entries = self.deps.registry_manager.list_all_servers().await?;
            let tools = filter_by_query(entries.into_iter(), "tool", &input.query);

            Ok(serde_json::to_value(tools)?)
        }
    }

    /// Install a tool
    #[operation(
        name = "install_tool",
        input = InstallInput,
        http = "POST /tools/install",
        cli = "tools install <SOURCE>",
        description = "Install a tool"
    )]
    pub struct Install {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for Install {
        type Input = InstallInput;
        type Output = Value;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            match (input.name, input.manifest) {
                (Some(name), None) => {
                    // Install from registry by name
                    let tool_entry = self
                        .deps
                        .registry_manager
                        .get_server(&name)
                        .await?
                        .ok_or_else(|| not_found("Tool", &name))?;

                    if tool_entry.entry_type != "tool" {
                        return Err(type_mismatch(&name, "tool", &tool_entry.entry_type));
                    }

                    Ok(serde_json::json!({
                        "status": "installed",
                        "name": name,
                        "type": "tool",
                        "endpoint": tool_entry.endpoint
                    }))
                }
                (None, Some(manifest)) => {
                    // Install from manifest
                    let tool_name = manifest
                        .get("name")
                        .and_then(|n| n.as_str())
                        .ok_or_else(|| {
                            BeemFlowError::validation("Tool manifest must have a 'name' field")
                        })?
                        .to_string();

                    // Register the tool in the local registry
                    self.deps
                        .registry_manager
                        .register_tool_from_manifest(manifest)
                        .await?;

                    Ok(serde_json::json!({
                        "status": "installed",
                        "name": tool_name,
                        "type": "tool",
                        "source": "manifest"
                    }))
                }
                (Some(_), Some(_)) => Err(BeemFlowError::validation(
                    "Provide either 'name' or 'manifest', not both",
                )),
                (None, None) => Err(BeemFlowError::validation(
                    "Either 'name' or 'manifest' must be provided",
                )),
            }
        }
    }

    /// Convert OpenAPI to tools
    #[operation(
        name = "convert_openapi",
        input = ConvertOpenAPIInput,
        http = "POST /tools/convert",
        description = "Convert OpenAPI to tools"
    )]
    pub struct ConvertOpenAPI {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for ConvertOpenAPI {
        type Input = ConvertOpenAPIInput;
        type Output = Value;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            // Parse OpenAPI spec
            let openapi_spec: serde_json::Value = serde_json::from_str(&input.openapi)?;

            // Extract basic info
            let api_name = input.api_name.unwrap_or_else(|| {
                openapi_spec
                    .get("info")
                    .and_then(|info| info.get("title"))
                    .and_then(|title| title.as_str())
                    .unwrap_or("api")
                    .to_string()
            });

            let base_url = input.base_url.unwrap_or_else(|| {
                openapi_spec
                    .get("servers")
                    .and_then(|servers| servers.as_array())
                    .and_then(|servers| servers.first())
                    .and_then(|server| server.get("url"))
                    .and_then(|url| url.as_str())
                    .unwrap_or("https://api.example.com")
                    .to_string()
            });

            // Convert to tool manifests (simplified implementation)
            let mut manifests = Vec::new();

            if let Some(paths) = openapi_spec.get("paths").and_then(|p| p.as_object()) {
                for (path, path_item) in paths {
                    if let Some(path_obj) = path_item.as_object() {
                        for (method, operation) in path_obj {
                            if method != "parameters" {
                                // Skip parameters key
                                if let Some(op_obj) = operation.as_object() {
                                    // Generate tool name
                                    let tool_name = format!(
                                        "{}.{}_{}",
                                        api_name,
                                        path.trim_start_matches('/').replace('/', "_"),
                                        method
                                    );

                                    // Extract description
                                    let description_str = op_obj
                                        .get("summary")
                                        .or_else(|| op_obj.get("description"))
                                        .and_then(|d| d.as_str())
                                        .unwrap_or("API endpoint");

                                    // Create basic manifest
                                    let mut manifest = serde_json::Map::new();
                                    manifest.insert(
                                        "name".to_string(),
                                        serde_json::Value::String(tool_name.clone()),
                                    );
                                    manifest.insert(
                                        "description".to_string(),
                                        serde_json::Value::String(description_str.to_string()),
                                    );
                                    manifest.insert(
                                        "kind".to_string(),
                                        serde_json::Value::String("task".to_string()),
                                    );
                                    manifest.insert(
                                        "endpoint".to_string(),
                                        serde_json::Value::String(format!("{}{}", base_url, path)),
                                    );
                                    manifest.insert(
                                        "method".to_string(),
                                        serde_json::Value::String(method.to_uppercase()),
                                    );

                                    // Add basic parameters schema
                                    let mut properties = serde_json::Map::new();
                                    let mut required = Vec::new();

                                    // Add path parameters
                                    if let Some(params) =
                                        path_obj.get("parameters").and_then(|p| p.as_array())
                                    {
                                        for param in params {
                                            if let Some(param_obj) = param.as_object()
                                                && let Some(param_name) =
                                                    param_obj.get("name").and_then(|n| n.as_str())
                                            {
                                                let mut param_schema = serde_json::Map::new();
                                                param_schema.insert(
                                                    "type".to_string(),
                                                    serde_json::Value::String("string".to_string()),
                                                );

                                                if let Some(param_desc) = param_obj
                                                    .get("description")
                                                    .and_then(|d| d.as_str())
                                                {
                                                    param_schema.insert(
                                                        "description".to_string(),
                                                        serde_json::Value::String(
                                                            param_desc.to_string(),
                                                        ),
                                                    );
                                                }

                                                if param_obj
                                                    .get("required")
                                                    .and_then(|r| r.as_bool())
                                                    .unwrap_or(false)
                                                {
                                                    required.push(serde_json::Value::String(
                                                        param_name.to_string(),
                                                    ));
                                                }

                                                properties.insert(
                                                    param_name.to_string(),
                                                    serde_json::Value::Object(param_schema),
                                                );
                                            }
                                        }
                                    }

                                    let mut parameters = serde_json::Map::new();
                                    parameters.insert(
                                        "type".to_string(),
                                        serde_json::Value::String("object".to_string()),
                                    );
                                    parameters.insert(
                                        "properties".to_string(),
                                        serde_json::Value::Object(properties),
                                    );
                                    parameters.insert(
                                        "required".to_string(),
                                        serde_json::Value::Array(required),
                                    );

                                    manifest.insert(
                                        "parameters".to_string(),
                                        serde_json::Value::Object(parameters),
                                    );

                                    manifests.push(serde_json::Value::Object(manifest));
                                }
                            }
                        }
                    }
                }
            }

            Ok(serde_json::json!({
                "status": "converted",
                "api_name": api_name,
                "base_url": base_url,
                "manifests": manifests,
                "count": manifests.len()
            }))
        }
    }
}
