//! Run operations module
//!
//! All operations for managing flow executions.

use super::*;
use crate::dsl::parse_string;
use beemflow_core_macros::{operation, operation_group};
use schemars::JsonSchema;
use uuid::Uuid;

#[operation_group(runs)]
pub mod runs {
    use super::*;

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Empty input (no parameters required)")]
    pub struct EmptyInput {}

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for starting a new flow run")]
    pub struct StartInput {
        #[schemars(description = "Name of the flow to execute")]
        pub flow_name: String,
        #[schemars(description = "Event data to pass to the flow")]
        pub event: Option<HashMap<String, Value>>,
        #[schemars(description = "Whether this is a draft run")]
        pub draft: Option<bool>,
    }

    #[derive(Serialize)]
    pub struct StartOutput {
        pub run_id: String,
        pub status: String,
    }

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for retrieving run details")]
    pub struct GetInput {
        #[schemars(description = "UUID of the run to retrieve")]
        pub run_id: String,
    }

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for resuming a paused run")]
    pub struct ResumeInput {
        #[schemars(description = "Resume token from the paused run")]
        pub token: String,
        #[schemars(description = "Event data for resuming")]
        pub event: Option<HashMap<String, Value>>,
    }

    /// Start a new flow run
    #[operation(
        name = "start_run",
        input = StartInput,
        http = "POST /runs",
        cli = "runs start <FLOW_NAME> [--event <JSON>] [--draft]",
        description = "Start a new flow run"
    )]
    pub struct Start {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for Start {
        type Input = StartInput;
        type Output = StartOutput;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            let event = input.event.unwrap_or_default();
            let is_draft = input.draft.unwrap_or(false);

            // Get flow content
            let content = if is_draft {
                // Draft mode: load from filesystem
                let flows_dir = crate::config::get_flows_dir(&self.deps.config);
                crate::storage::flows::get_flow(&flows_dir, &input.flow_name)
                    .await?
                    .ok_or_else(|| not_found("Flow", &input.flow_name))?
            } else {
                // Production mode: load from database (deployed version)
                let version = self
                    .deps
                    .storage
                    .get_deployed_version(&input.flow_name)
                    .await?
                    .ok_or_else(|| {
                        BeemFlowError::not_found(
                            "Deployed flow",
                            format!("{} (use --draft to run from filesystem)", input.flow_name),
                        )
                    })?;

                self.deps
                    .storage
                    .get_flow_version_content(&input.flow_name, &version)
                    .await?
                    .ok_or_else(|| not_found("Flow version", &version))?
            };

            // Parse and execute flow
            let flow = parse_string(&content, None)?;
            let result = self.deps.engine.execute(&flow, event).await?;

            Ok(StartOutput {
                run_id: result.run_id.to_string(),
                status: "completed".to_string(),
            })
        }
    }

    /// Get run details by ID
    #[operation(
        name = "get_run",
        input = GetInput,
        http = "GET /runs/{id}",
        cli = "runs get <RUN_ID>",
        description = "Get run details by ID"
    )]
    pub struct Get {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for Get {
        type Input = GetInput;
        type Output = Value;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            let run_id = Uuid::parse_str(&input.run_id)
                .map_err(|_| BeemFlowError::validation("Invalid run ID"))?;

            let mut run = self
                .deps
                .storage
                .get_run(run_id)
                .await?
                .ok_or_else(|| not_found("Run", &input.run_id))?;

            // Fetch step execution details
            let steps = self.deps.storage.get_steps(run_id).await?;
            run.steps = if steps.is_empty() { None } else { Some(steps) };

            Ok(serde_json::to_value(run)?)
        }
    }

    /// List all runs
    #[operation(
        name = "list_runs",
        input = EmptyInput,
        http = "GET /runs",
        cli = "runs list",
        description = "List all runs"
    )]
    pub struct List {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for List {
        type Input = EmptyInput;
        type Output = Value;

        async fn execute(&self, _input: Self::Input) -> Result<Self::Output> {
            let runs = self.deps.storage.list_runs().await?;
            Ok(serde_json::to_value(runs)?)
        }
    }

    /// Resume a paused run
    #[operation(
        name = "resume_run",
        input = ResumeInput,
        http = "POST /runs/resume/{token}",
        cli = "resume <TOKEN> [--event <JSON>]",
        description = "Resume a paused run"
    )]
    pub struct Resume {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for Resume {
        type Input = ResumeInput;
        type Output = Value;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            let event = input.event.unwrap_or_default();

            // Resume the run using the engine
            self.deps.engine.resume(&input.token, event).await?;

            Ok(serde_json::json!({
                "status": "resumed",
                "token": input.token,
                "message": "Run resumed successfully"
            }))
        }
    }
}
