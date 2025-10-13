//! Common SQL storage implementation for SQLite and PostgreSQL
//!
//! This module provides shared helpers and parsing logic for both SQL backends,
//! eliminating ~1360 lines of duplication.

use crate::{Result, model::*};
use chrono::{DateTime, Utc};
use std::collections::HashMap;

// ============================================================================
// Status Conversions (used by both backends)
// ============================================================================

#[inline]
pub fn parse_run_status(s: &str) -> RunStatus {
    match s {
        "PENDING" => RunStatus::Pending,
        "RUNNING" => RunStatus::Running,
        "SUCCEEDED" => RunStatus::Succeeded,
        "FAILED" => RunStatus::Failed,
        "WAITING" => RunStatus::Waiting,
        "SKIPPED" => RunStatus::Skipped,
        _ => RunStatus::Failed,
    }
}

#[inline]
pub fn run_status_to_str(status: RunStatus) -> &'static str {
    match status {
        RunStatus::Pending => "PENDING",
        RunStatus::Running => "RUNNING",
        RunStatus::Succeeded => "SUCCEEDED",
        RunStatus::Failed => "FAILED",
        RunStatus::Waiting => "WAITING",
        RunStatus::Skipped => "SKIPPED",
    }
}

#[inline]
pub fn parse_step_status(s: &str) -> StepStatus {
    match s {
        "PENDING" => StepStatus::Pending,
        "RUNNING" => StepStatus::Running,
        "SUCCEEDED" => StepStatus::Succeeded,
        "FAILED" => StepStatus::Failed,
        "SKIPPED" => StepStatus::Skipped,
        "WAITING" => StepStatus::Waiting,
        _ => StepStatus::Failed,
    }
}

#[inline]
pub fn step_status_to_str(status: StepStatus) -> &'static str {
    match status {
        StepStatus::Pending => "PENDING",
        StepStatus::Running => "RUNNING",
        StepStatus::Succeeded => "SUCCEEDED",
        StepStatus::Failed => "FAILED",
        StepStatus::Skipped => "SKIPPED",
        StepStatus::Waiting => "WAITING",
    }
}

// ============================================================================
// SQLite-specific Helpers
// ============================================================================

/// Parse JSON from SQLite TEXT field
#[inline]
pub fn parse_json_from_text(text: &str) -> Result<serde_json::Value> {
    Ok(serde_json::from_str(text)?)
}

/// Parse HashMap from SQLite JSON TEXT
#[inline]
pub fn parse_hashmap_from_text(text: &str) -> Result<HashMap<String, serde_json::Value>> {
    Ok(serde_json::from_str(text)?)
}

/// Convert DateTime to SQLite INTEGER (unix timestamp)
#[inline]
pub fn datetime_to_unix(dt: DateTime<Utc>) -> i64 {
    dt.timestamp()
}

/// Parse DateTime from SQLite INTEGER (unix timestamp)
#[inline]
pub fn datetime_from_unix(ts: i64) -> DateTime<Utc> {
    DateTime::from_timestamp(ts, 0).unwrap_or_else(Utc::now)
}

// ============================================================================
// PostgreSQL-specific Helpers
// ============================================================================

/// Parse HashMap from Postgres JSONB
#[inline]
pub fn parse_hashmap_from_jsonb(val: serde_json::Value) -> HashMap<String, serde_json::Value> {
    val.as_object()
        .map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}
