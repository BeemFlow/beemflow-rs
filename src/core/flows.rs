//! Flow operations module
//!
//! All operations for managing workflow definitions.

use super::*;
use crate::dsl::{Validator, parse_file, parse_string};
use crate::graph::GraphGenerator;
use beemflow_core_macros::{operation, operation_group};
use schemars::JsonSchema;

#[operation_group(flows)]
pub mod flows {
    use super::*;

    // Shared input/output types
    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Empty input (no parameters required)")]
    pub struct EmptyInput {}

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for retrieving a flow by name")]
    pub struct GetInput {
        #[schemars(description = "Name of the flow to retrieve")]
        pub name: String,
    }

    #[derive(Serialize)]
    pub struct GetOutput {
        pub name: String,
        pub content: String,
        pub version: Option<String>,
    }

    #[derive(Serialize)]
    pub struct ListOutput {
        pub flows: Vec<String>,
    }

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for saving or updating a flow definition")]
    pub struct SaveInput {
        #[schemars(description = "Name of the flow (optional, can be inferred from content)")]
        pub name: Option<String>,
        #[schemars(description = "YAML content of the flow definition")]
        pub content: String,
    }

    #[derive(Serialize)]
    pub struct SaveOutput {
        pub status: String,
        pub name: String,
        pub version: String,
    }

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for deleting a flow definition")]
    pub struct DeleteInput {
        #[schemars(description = "Name of the flow to delete")]
        pub name: String,
    }

    #[derive(Serialize)]
    pub struct DeleteOutput {
        pub status: String,
        pub name: String,
    }

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for deploying a flow to production")]
    pub struct DeployInput {
        #[schemars(description = "Name of the flow to deploy")]
        pub name: String,
    }

    #[derive(Serialize)]
    pub struct DeployOutput {
        pub flow: String,
        pub version: String,
        pub status: String,
        pub message: String,
    }

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for rolling back a flow to a specific version")]
    pub struct RollbackInput {
        #[schemars(description = "Name of the flow")]
        pub name: String,
        #[schemars(description = "Version to rollback to")]
        pub version: String,
    }

    #[derive(Serialize)]
    pub struct RollbackOutput {
        pub flow: String,
        pub from_version: Option<String>,
        pub to_version: String,
        pub status: String,
        pub message: String,
    }

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for retrieving flow version history")]
    pub struct HistoryInput {
        #[schemars(description = "Name of the flow")]
        pub name: String,
    }

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for validating a flow")]
    pub struct ValidateInput {
        #[schemars(description = "Name of the flow (if loading from storage)")]
        pub name: Option<String>,
        #[schemars(description = "Path to flow file (if loading from file system)")]
        pub file: Option<String>,
    }

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for generating a Mermaid diagram")]
    pub struct GraphInput {
        #[schemars(description = "Name of the flow (if loading from storage)")]
        pub name: Option<String>,
        #[schemars(description = "Path to flow file (if loading from file system)")]
        pub file: Option<String>,
    }

    #[derive(Serialize)]
    pub struct GraphOutput {
        pub diagram: String,
    }

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for linting a flow file")]
    pub struct LintInput {
        #[schemars(description = "Path to the flow file to lint")]
        pub file: String,
    }

    // Operations

    /// List all available flows
    #[operation(
        name = "list_flows",
        input = EmptyInput,
        http = "GET /flows",
        cli = "list",
        description = "List all available workflow definitions"
    )]
    pub struct List {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for List {
        type Input = EmptyInput;
        type Output = ListOutput;

        async fn execute(&self, _input: Self::Input) -> Result<Self::Output> {
            let flows = self.deps.storage.list_flows().await?;
            Ok(ListOutput { flows })
        }
    }

    /// Get a flow by name
    #[operation(
        name = "get_flow",
        input = GetInput,
        http = "GET /flows/{name}",
        cli = "get <NAME>",
        description = "Get a flow by name"
    )]
    pub struct Get {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for Get {
        type Input = GetInput;
        type Output = GetOutput;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            let content = self
                .deps
                .storage
                .get_flow(&input.name)
                .await?
                .ok_or_else(|| not_found("Flow", &input.name))?;

            // Parse to get version
            let flow = parse_string(&content)?;

            Ok(GetOutput {
                name: input.name,
                content,
                version: flow.version,
            })
        }
    }

    /// Save or update a flow definition
    #[operation(
        name = "save_flow",
        input = SaveInput,
        http = "POST /flows",
        cli = "save <NAME> --file <FILE>",
        description = "Save or update a flow definition"
    )]
    pub struct Save {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for Save {
        type Input = SaveInput;
        type Output = SaveOutput;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            // Parse and validate the flow
            let flow = parse_string(&input.content)?;
            Validator::validate(&flow)?;

            // Determine flow name
            let name = input.name.unwrap_or_else(|| flow.name.clone());
            if name.is_empty() {
                return Err(BeemFlowError::validation("Flow must have a name"));
            }

            // Check if flow already exists
            let exists = self.deps.storage.get_flow(&name).await?.is_some();

            // Save flow
            self.deps
                .storage
                .save_flow(&name, &input.content, flow.version.as_deref())
                .await?;

            let status = if exists { "updated" } else { "created" };
            let version = flow.version.unwrap_or_else(|| "1.0.0".to_string());

            Ok(SaveOutput {
                status: status.to_string(),
                name,
                version,
            })
        }
    }

    /// Delete a flow definition
    #[operation(
        name = "delete_flow",
        input = DeleteInput,
        http = "DELETE /flows/{name}",
        cli = "delete <NAME>",
        description = "Delete a flow definition"
    )]
    pub struct Delete {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for Delete {
        type Input = DeleteInput;
        type Output = DeleteOutput;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            self.deps.storage.delete_flow(&input.name).await?;

            Ok(DeleteOutput {
                status: "deleted".to_string(),
                name: input.name,
            })
        }
    }

    /// Deploy flow to production
    #[operation(
        name = "deploy_flow",
        input = DeployInput,
        http = "POST /flows/{name}/deploy",
        cli = "deploy <NAME>",
        description = "Deploy flow to production"
    )]
    pub struct Deploy {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for Deploy {
        type Input = DeployInput;
        type Output = DeployOutput;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            // Get flow content
            let content = self
                .deps
                .storage
                .get_flow(&input.name)
                .await?
                .ok_or_else(|| not_found("Flow", &input.name))?;

            // Parse to get version
            let flow = parse_string(&content)?;
            let version = flow.version.ok_or_else(|| {
                BeemFlowError::validation("Flow must have a version field to deploy")
            })?;

            // Deploy the version
            self.deps
                .storage
                .deploy_flow_version(&input.name, &version, &content)
                .await?;

            Ok(DeployOutput {
                flow: input.name.clone(),
                version: version.clone(),
                status: "deployed".to_string(),
                message: format!("Flow '{}' v{} deployed to production", input.name, version),
            })
        }
    }

    /// Rollback flow to specific version
    #[operation(
        name = "rollback_flow",
        input = RollbackInput,
        http = "POST /flows/{name}/rollback",
        cli = "rollback <NAME> <VERSION>",
        description = "Rollback flow to specific version"
    )]
    pub struct Rollback {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for Rollback {
        type Input = RollbackInput;
        type Output = RollbackOutput;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            // Get current deployed version
            let current_version = self.deps.storage.get_deployed_version(&input.name).await?;

            // Get the target version content
            let content = self
                .deps
                .storage
                .get_flow_version_content(&input.name, &input.version)
                .await?
                .ok_or_else(|| {
                    not_found(
                        &format!("Version {}", input.version),
                        "(must be previously deployed)",
                    )
                })?;

            // Deploy the target version
            self.deps
                .storage
                .deploy_flow_version(&input.name, &input.version, &content)
                .await?;

            Ok(RollbackOutput {
                flow: input.name.clone(),
                from_version: current_version,
                to_version: input.version.clone(),
                status: "rolled_back".to_string(),
                message: format!("Flow '{}' rolled back to v{}", input.name, input.version),
            })
        }
    }

    /// Get flow version history
    #[operation(
        name = "flow_history",
        input = HistoryInput,
        http = "GET /flows/{name}/history",
        cli = "history <NAME>",
        description = "Get flow version history"
    )]
    pub struct History {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for History {
        type Input = HistoryInput;
        type Output = Value;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            let history = self.deps.storage.list_flow_versions(&input.name).await?;

            let result: Vec<_> = history
                .iter()
                .map(|v| {
                    serde_json::json!({
                        "version": v.version,
                        "deployed_at": v.deployed_at.to_rfc3339(),
                        "flow_name": v.flow_name
                    })
                })
                .collect();

            Ok(serde_json::to_value(result)?)
        }
    }

    /// Validate a flow
    #[operation(
        name = "validate_flow",
        input = ValidateInput,
        http = "POST /flows/validate",
        cli = "validate <FILE>",
        description = "Validate a flow"
    )]
    pub struct Validate {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for Validate {
        type Input = ValidateInput;
        type Output = Value;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            let flow = load_flow_from_storage(
                &self.deps.storage,
                input.name.as_deref(),
                input.file.as_deref(),
            )
            .await?;
            Validator::validate(&flow)?;

            Ok(serde_json::json!({
                "status": "valid",
                "message": "Validation OK: flow is valid!"
            }))
        }
    }

    /// Generate Mermaid diagram
    #[operation(
        name = "graph_flow",
        input = GraphInput,
        http = "POST /flows/graph",
        cli = "graph <FILE> [--output <PATH>]",
        description = "Generate Mermaid diagram for a flow"
    )]
    pub struct Graph {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for Graph {
        type Input = GraphInput;
        type Output = GraphOutput;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            let flow = load_flow_from_storage(
                &self.deps.storage,
                input.name.as_deref(),
                input.file.as_deref(),
            )
            .await?;
            let diagram = GraphGenerator::generate(&flow)?;

            Ok(GraphOutput { diagram })
        }
    }

    /// Lint a flow file
    #[operation(
        name = "lint_flow",
        input = LintInput,
        http = "POST /flows/lint",
        cli = "lint <FILE>",
        description = "Lint a flow file"
    )]
    pub struct Lint {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for Lint {
        type Input = LintInput;
        type Output = Value;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            let flow = parse_file(&input.file)?;
            Validator::validate(&flow)?;

            Ok(serde_json::json!({
                "status": "valid",
                "message": "Lint OK: flow is valid!"
            }))
        }
    }

    /// Test a flow
    #[operation(name = "test_flow", input = EmptyInput, description = "Test a flow")]
    pub struct Test {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for Test {
        type Input = EmptyInput;
        type Output = Value;

        async fn execute(&self, _input: Self::Input) -> Result<Self::Output> {
            Ok(serde_json::json!({
                "status": "success",
                "message": "Test functionality not implemented yet"
            }))
        }
    }
}
