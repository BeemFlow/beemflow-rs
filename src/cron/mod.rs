//! Cron scheduling functionality for BeemFlow
//!
//! This module provides cron scheduling capabilities similar to the Go implementation,
//! allowing flows to be triggered based on cron expressions.

use crate::Result;
use crate::model::Flow;
// TODO: Refactor cron module to use OperationRegistry
// use crate::core::{list_flows, get_flow, start_run};
use chrono::{DateTime, Utc, Duration};
use cron::Schedule;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::str::FromStr;

/// Cron manager for handling cron-based flow execution
#[derive(Debug, Clone)]
pub struct CronManager {
    /// Server URL for cron callbacks (mainly for CLI usage)
    pub server_url: String,
    /// Cron secret for authentication
    pub cron_secret: Option<String>,
}

impl CronManager {
    /// Create a new cron manager
    pub fn new(server_url: String, cron_secret: Option<String>) -> Self {
        Self {
            server_url,
            cron_secret,
        }
    }

    /// Check all flows for cron schedules and execute those that are due
    /// This is stateless and relies only on the storage layer
    pub async fn check_and_execute_cron_flows(&self) -> Result<CronExecutionResult> {
        // List all flows
        let flows = list_flows().await?;

        let mut triggered = Vec::new();
        let mut errors = Vec::new();
        let mut checked = 0;

        // Get current time
        let now = Utc::now();

        for flow_name in &flows {
            // Get flow details
            if let Some(flow) = get_flow(flow_name).await? {
                // Check if flow has schedule.cron trigger
                if Self::has_schedule_cron_trigger(&flow) {
                    checked += 1;

                    // Parse cron expression
                    if let Some(cron_expr) = &flow.cron {
                        if let Ok(schedule) = Schedule::from_str(cron_expr) {
                            // Check if we should run now (within 5-minute window)
                            if Self::should_run_now(&schedule, &now, Duration::minutes(5)) {
                                // Execute the flow
                                match self.execute_flow_by_cron(&flow, flow_name).await {
                                    Ok(_) => triggered.push(flow_name.clone()),
                                    Err(e) => errors.push(format!("{}: {}", flow_name, e)),
                                }
                            }
                        } else {
                            errors.push(format!("{}: invalid cron expression", flow_name));
                        }
                    } else {
                        errors.push(format!("{}: missing cron expression", flow_name));
                    }
                }
            } else {
                errors.push(format!("{}: flow not found", flow_name));
            }
        }

        Ok(CronExecutionResult {
            status: "completed".to_string(),
            timestamp: now.to_rfc3339(),
            triggered: triggered.len(),
            workflows: triggered,
            errors,
            checked,
            total: flows.len(),
        })
    }

    /// Check if a flow has a schedule.cron trigger
    fn has_schedule_cron_trigger(flow: &Flow) -> bool {
        // For now, check if the flow has a cron field (simplified logic)
        // In a complete implementation, this would check the trigger configuration
        flow.cron.is_some()
    }

    /// Check if a cron schedule should run within the given time window
    fn should_run_now(schedule: &Schedule, now: &DateTime<Utc>, window: Duration) -> bool {
        // Check if the schedule matches within the time window
        let window_start = *now - window;
        let window_end = *now + Duration::minutes(1); // 1 minute buffer for early triggers

        // Get when it should next run after our check start time
        let next_run = schedule.upcoming(chrono::Utc).next();

        if let Some(scheduled_time) = next_run {
            // The scheduled time must be:
            // 1. After our check start time (window_start)
            // 2. Before or at the current time + buffer
            scheduled_time > window_start && scheduled_time <= window_end
        } else {
            false
        }
    }

    /// Execute a flow triggered by cron
    async fn execute_flow_by_cron(&self, _flow: &Flow, flow_name: &str) -> Result<()> {
        // Create event with the actual scheduled time for proper deduplication
        let mut event_data = HashMap::new();
        event_data.insert("trigger".to_string(), Value::String("schedule.cron".to_string()));
        event_data.insert("workflow".to_string(), Value::String(flow_name.to_string()));
        event_data.insert("timestamp".to_string(), Value::String(Utc::now().to_rfc3339()));

        // Start the run
        start_run(flow_name, event_data).await?;

        tracing::info!("Successfully triggered cron workflow: {} at {}", flow_name, Utc::now().to_rfc3339());
        Ok(())
    }
}

/// Result of cron execution check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronExecutionResult {
    /// Execution status
    pub status: String,
    /// Timestamp when check was performed
    pub timestamp: String,
    /// Number of workflows triggered
    pub triggered: usize,
    /// List of triggered workflow names
    pub workflows: Vec<String>,
    /// List of errors encountered
    pub errors: Vec<String>,
    /// Number of workflows checked
    pub checked: usize,
    /// Total number of workflows
    pub total: usize,
}

impl Default for CronExecutionResult {
    fn default() -> Self {
        Self {
            status: "completed".to_string(),
            timestamp: Utc::now().to_rfc3339(),
            triggered: 0,
            workflows: Vec::new(),
            errors: Vec::new(),
            checked: 0,
            total: 0,
        }
    }
}

/// Shell quote utility for safe shell command construction
pub fn shell_quote(s: &str) -> String {
    // Replace single quotes with '\'' (end quote, escaped quote, start quote)
    let escaped = s.replace('\'', "'\\''");
    // Wrap in single quotes
    format!("'{}'", escaped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shell_quote() {
        assert_eq!(shell_quote("simple"), "'simple'");
        assert_eq!(shell_quote("with'quote"), "'with'\\''quote'");
        assert_eq!(shell_quote("multiple'quotes'here"), "'multiple'\\''quotes'\\''here'");
    }

    #[test]
    fn test_has_schedule_cron_trigger() {
        let flow = Flow {
            cron: Some("0 * * * *".to_string()),
            ..Default::default()
        };

        assert!(CronManager::has_schedule_cron_trigger(&flow));

        let flow_no_cron = Flow {
            cron: None,
            ..Default::default()
        };
        assert!(!CronManager::has_schedule_cron_trigger(&flow_no_cron));
    }

    #[tokio::test]
    async fn test_cron_manager_creation() {
        let manager = CronManager::new("http://localhost:3000".to_string(), Some("secret".to_string()));
        assert_eq!(manager.server_url, "http://localhost:3000");
        assert_eq!(manager.cron_secret, Some("secret".to_string()));
    }
}
