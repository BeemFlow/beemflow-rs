//! Template access to previous run outputs
//!
//! Provides the `runs.previous()` template function for accessing
//! outputs from the most recent previous run of the same workflow.

use crate::model::{RunStatus, StepStatus};
use crate::storage::Storage;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

/// Provides template access to previous run outputs
///
/// Matches Go's RunsAccess struct (engine.go lines 99-153)
#[derive(Clone)]
pub struct RunsAccess {
    storage: Arc<dyn Storage>,
    current_run_id: Option<Uuid>,
    flow_name: String,
}

impl RunsAccess {
    /// Create a new runs access helper
    pub fn new(storage: Arc<dyn Storage>, current_run_id: Option<Uuid>, flow_name: String) -> Self {
        Self {
            storage,
            current_run_id,
            flow_name,
        }
    }

    /// Get outputs from the most recent previous run of the same workflow
    ///
    /// Returns a map with:
    /// - id: Run ID as string
    /// - outputs: Map of step outputs
    /// - status: Run status
    /// - flow: Flow name
    ///
    /// Returns empty map if no previous run found.
    pub async fn previous(&self) -> HashMap<String, Value> {
        // Get all runs
        let runs = match self.storage.list_runs().await {
            Ok(runs) => runs,
            Err(_) => return HashMap::new(),
        };

        // Find the most recent successful run from the same workflow
        for run in runs {
            // Only consider runs from the same workflow
            if run.flow_name != self.flow_name {
                continue;
            }

            // Skip the current run
            if let Some(current_id) = self.current_run_id
                && run.id == current_id
            {
                continue;
            }

            // Only return successful runs
            if run.status != RunStatus::Succeeded {
                continue;
            }

            // Get step outputs for this run
            let steps = match self.storage.get_steps(run.id).await {
                Ok(steps) => steps,
                Err(_) => continue,
            };

            // Aggregate step outputs
            let mut outputs = HashMap::new();
            for step in steps {
                if step.status == StepStatus::Succeeded
                    && let Some(ref step_outputs) = step.outputs
                {
                    outputs.insert(
                        step.step_name.clone(),
                        serde_json::to_value(step_outputs).unwrap_or(Value::Null),
                    );
                }
            }

            // Return the first matching run (most recent due to ordering)
            return serde_json::json!({
                "id": run.id.to_string(),
                "outputs": outputs,
                "status": format!("{:?}", run.status),
                "flow": run.flow_name,
            })
            .as_object()
            .map(|obj| obj.clone().into_iter().collect())
            .unwrap_or_default();
        }

        // No previous run found
        HashMap::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Run, StepRun};
    use crate::storage::MemoryStorage;
    use chrono::Utc;

    #[tokio::test]
    async fn test_runs_access_previous() {
        let storage = Arc::new(MemoryStorage::new()) as Arc<dyn Storage>;

        // Create a previous run
        let prev_run = Run {
            id: Uuid::new_v4(),
            flow_name: "test_flow".to_string(),
            event: HashMap::new(),
            vars: HashMap::new(),
            status: RunStatus::Succeeded,
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            steps: None,
        };

        storage.save_run(&prev_run).await.unwrap();

        // Add step outputs
        let step = StepRun {
            id: Uuid::new_v4(),
            run_id: prev_run.id,
            step_name: "step1".to_string(),
            status: StepStatus::Succeeded,
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
            error: None,
            outputs: Some(
                serde_json::json!({"result": "hello"})
                    .as_object()
                    .unwrap()
                    .clone()
                    .into_iter()
                    .collect(),
            ),
        };

        storage.save_step(&step).await.unwrap();

        // Create RunsAccess
        let runs_access = RunsAccess::new(storage, None, "test_flow".to_string());

        // Get previous run
        let previous = runs_access.previous().await;

        // Verify outputs
        assert!(previous.contains_key("id"));
        assert!(previous.contains_key("outputs"));
    }
}
