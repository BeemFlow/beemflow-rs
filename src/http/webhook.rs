//! Webhook management system
//!
//! Handles dynamic webhook registration, signature verification, and event parsing.

use crate::Result;
use crate::engine::{Engine, PausedRun};
use crate::registry::{RegistryManager, WebhookConfig};
use crate::storage::Storage;
use axum::{
    Router,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::post,
};
use hmac::{Hmac, Mac};
use serde_json::Value;
use sha2::Sha256;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

type HmacSha256 = Hmac<Sha256>;

/// Webhook manager state
#[derive(Clone)]
pub struct WebhookManagerState {
    pub registry_manager: Arc<RegistryManager>,
    pub secrets_provider: Arc<dyn crate::secrets::SecretsProvider>,
    pub storage: Arc<dyn Storage>,
    pub engine: Arc<Engine>,
    pub config: Arc<crate::config::Config>,
}

/// Parsed webhook event
#[derive(Debug)]
pub(crate) struct ParsedEvent {
    pub(crate) topic: String,
    pub(crate) data: HashMap<String, Value>,
}

/// Create webhook routes
pub fn create_webhook_routes() -> Router<WebhookManagerState> {
    Router::new().route("/{provider}", post(handle_webhook))
}

/// Handle incoming webhook
async fn handle_webhook(
    State(state): State<WebhookManagerState>,
    Path(provider): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    tracing::debug!("Webhook received: provider={}", provider);

    // Find webhook configuration from registry
    let webhook_config = match find_webhook_config(&state.registry_manager, &provider).await {
        Some(config) => config,
        None => {
            tracing::warn!("Webhook not found: {}", provider);
            return (StatusCode::NOT_FOUND, "Webhook not configured").into_response();
        }
    };

    // Verify signature if configured
    if let Some(ref secret) = webhook_config.secret {
        // Expand $env: patterns in the secret value
        let secret_value = match crate::secrets::expand_value(secret, &state.secrets_provider).await
        {
            Ok(val) => val,
            Err(e) => {
                tracing::error!("Failed to expand webhook secret: {}", e);
                return (StatusCode::INTERNAL_SERVER_ERROR, "Configuration error").into_response();
            }
        };

        if !secret_value.is_empty()
            && !verify_webhook_signature(&webhook_config, &headers, &body, &secret_value)
        {
            tracing::error!("Invalid webhook signature for {}", provider);
            return (StatusCode::UNAUTHORIZED, "Invalid signature").into_response();
        }
    }

    // Parse webhook payload
    let payload: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Failed to parse webhook payload: {}", e);
            return (StatusCode::BAD_REQUEST, "Invalid JSON payload").into_response();
        }
    };

    // Extract events from payload
    let events = match parse_webhook_events(&webhook_config, &payload) {
        Ok(events) => events,
        Err(e) => {
            tracing::error!("Failed to parse webhook events: {}", e);
            return (StatusCode::BAD_REQUEST, "Failed to parse events").into_response();
        }
    };

    // Process webhook events - both trigger new flows AND resume paused runs
    let mut triggered_count = 0;
    let mut resumed_count = 0;

    for event in &events {
        tracing::info!("Processing webhook event: {}", event.topic);

        // Use Case 1: Trigger new workflow executions
        match trigger_flows_for_event(&state, event).await {
            Ok(count) => {
                triggered_count += count;
                tracing::info!("Event {} triggered {} new flow(s)", event.topic, count);
            }
            Err(e) => {
                tracing::error!("Failed to trigger flows for event {}: {}", event.topic, e);
                // Continue processing
            }
        }

        // Use Case 2: Resume paused workflow executions
        match resume_paused_runs_for_event(&state, event).await {
            Ok(count) => {
                resumed_count += count;
                tracing::info!("Event {} resumed {} paused run(s)", event.topic, count);
            }
            Err(e) => {
                tracing::error!("Failed to resume runs for event {}: {}", event.topic, e);
                // Continue processing
            }
        }
    }

    tracing::info!(
        "Webhook processed: {} events, {} new flows triggered, {} runs resumed",
        events.len(),
        triggered_count,
        resumed_count
    );

    (StatusCode::OK, "OK").into_response()
}

/// Trigger new flow executions for matching deployed flows (Use Case 1)
async fn trigger_flows_for_event(
    state: &WebhookManagerState,
    event: &ParsedEvent,
) -> Result<usize> {
    // Fast O(log N) lookup: Query only flow names (not content)
    let flow_names = state.storage.find_flow_names_by_topic(&event.topic).await?;

    if flow_names.is_empty() {
        tracing::debug!("No flows registered for topic: {}", event.topic);
        return Ok(0);
    }

    let mut triggered = 0;

    // Use engine.start() - same code path as HTTP/CLI/MCP operations
    for flow_name in flow_names {
        tracing::info!(
            "Triggering flow '{}' for webhook topic '{}'",
            flow_name,
            event.topic
        );

        match state
            .engine
            .start(&flow_name, event.data.clone(), false)
            .await
        {
            Ok(_) => {
                triggered += 1;
                tracing::info!("Successfully triggered flow '{}'", flow_name);
            }
            Err(e) => {
                // Log but don't fail - flow execution errors shouldn't block webhook
                tracing::error!("Failed to trigger flow '{}': {}", flow_name, e);
            }
        }
    }

    Ok(triggered)
}

/// Resume paused runs for matching paused workflows (Use Case 2)
async fn resume_paused_runs_for_event(
    state: &WebhookManagerState,
    event: &ParsedEvent,
) -> Result<usize> {
    // Query paused runs by source (event topic)
    let paused_runs = state
        .storage
        .find_paused_runs_by_source(&event.topic)
        .await?;

    if paused_runs.is_empty() {
        tracing::debug!("No paused runs found for source: {}", event.topic);
        return Ok(0);
    }

    tracing::debug!(
        "Found {} paused run(s) for source: {}",
        paused_runs.len(),
        event.topic
    );

    let mut resumed = 0;

    for (token, paused_data) in paused_runs {
        // Deserialize paused run
        let paused: PausedRun = match serde_json::from_value(paused_data) {
            Ok(p) => p,
            Err(e) => {
                tracing::error!("Failed to deserialize paused run {}: {}", token, e);
                continue;
            }
        };

        // Get await_event spec from the current step
        let step = match paused.flow.steps.get(paused.step_idx) {
            Some(s) => s,
            None => {
                tracing::error!("Invalid step index {} for token {}", paused.step_idx, token);
                continue;
            }
        };

        let await_spec = match &step.await_event {
            Some(spec) => spec,
            None => {
                tracing::error!(
                    "Step {} has no await_event spec for token {}",
                    step.id,
                    token
                );
                continue;
            }
        };

        // Check if event matches the await criteria
        let event_value = serde_json::to_value(&event.data).unwrap_or_default();
        if !matches_criteria(&event_value, &await_spec.match_) {
            tracing::debug!(
                "Event does not match criteria for token {}, skipping",
                token
            );
            continue;
        }

        // Resume the paused run
        tracing::info!("Resuming paused run with token: {}", token);

        // Convert event data to HashMap for resume
        let resume_event = event.data.clone();

        match state.engine.resume(&token, resume_event).await {
            Ok(_) => {
                resumed += 1;
                tracing::info!("Successfully resumed run with token: {}", token);
            }
            Err(e) => {
                tracing::error!("Failed to resume run with token {}: {}", token, e);
            }
        }
    }

    Ok(resumed)
}

/// Find webhook configuration from registry
async fn find_webhook_config(
    registry: &Arc<RegistryManager>,
    provider: &str,
) -> Option<WebhookConfig> {
    // Query registry for OAuth provider which includes webhook config
    let provider_name = format!("oauth_{}", provider);

    match registry.get_server(&provider_name).await {
        Ok(Some(entry)) => {
            // Webhook config is embedded in the registry entry
            entry.webhook.clone()
        }
        Ok(None) | Err(_) => None,
    }
}

/// Verify webhook signature
fn verify_webhook_signature(
    config: &WebhookConfig,
    headers: &HeaderMap,
    body: &[u8],
    secret: &str,
) -> bool {
    let signature_config = match &config.signature {
        Some(sig) => sig,
        None => return true, // No signature verification configured
    };

    // Get signature header
    let signature = match headers.get(&signature_config.header) {
        Some(header_value) => match header_value.to_str() {
            Ok(s) => s,
            Err(_) => return false,
        },
        None => return false,
    };

    // Get timestamp if required
    if let Some(ref timestamp_header) = signature_config.timestamp_header {
        let timestamp = match headers.get(timestamp_header) {
            Some(ts) => match ts.to_str() {
                Ok(s) => s,
                Err(_) => return false,
            },
            None => return false,
        };

        // Verify timestamp age
        if let Ok(ts) = timestamp.parse::<i64>() {
            let max_age = signature_config.max_age.unwrap_or(300); // Default 5 minutes
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64;

            if now - ts > max_age {
                return false;
            }
        } else {
            return false;
        }
    }

    // Build signature base string
    let base_string = if let Some(ref timestamp_header) = signature_config.timestamp_header {
        let timestamp = headers
            .get(timestamp_header)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("");
        format!("v0:{}:{}", timestamp, String::from_utf8_lossy(body))
    } else {
        String::from_utf8_lossy(body).to_string()
    };

    // Calculate expected signature
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
    mac.update(base_string.as_bytes());
    let expected_sig = hex::encode(mac.finalize().into_bytes());

    // Apply format template if provided
    let expected_formatted = if let Some(ref format) = signature_config.format {
        format.replace("{signature}", &expected_sig)
    } else {
        expected_sig
    };

    // Constant-time comparison
    use subtle::ConstantTimeEq;
    expected_formatted
        .as_bytes()
        .ct_eq(signature.as_bytes())
        .into()
}

/// Parse events from webhook payload
pub(crate) fn parse_webhook_events(
    config: &WebhookConfig,
    payload: &Value,
) -> Result<Vec<ParsedEvent>> {
    let mut events = Vec::new();

    for event_config in &config.events {
        // Check if payload matches this event
        if matches_event(payload, &event_config.match_) {
            // Extract data using JSON paths
            let mut event_data = HashMap::new();

            for (key, json_path) in &event_config.extract {
                if let Some(value) = extract_json_path(payload, json_path) {
                    event_data.insert(key.clone(), value);
                }
            }

            events.push(ParsedEvent {
                topic: event_config.topic.clone(),
                data: event_data,
            });
        }
    }

    Ok(events)
}

/// Check if payload matches event match conditions
pub(crate) fn matches_event(payload: &Value, match_conditions: &HashMap<String, Value>) -> bool {
    for (path, expected) in match_conditions {
        let actual = extract_json_path(payload, path);
        if actual.as_ref() != Some(expected) {
            return false;
        }
    }
    true
}

/// Check if event matches await_event criteria (excluding token field)
///
/// This is used by webhook handlers to determine if an incoming event
/// should resume a paused workflow. The "token" field is excluded from matching
/// as it's used for identification, not matching.
fn matches_criteria(payload: &Value, criteria: &HashMap<String, Value>) -> bool {
    criteria
        .iter()
        .filter(|(key, _)| *key != crate::constants::MATCH_KEY_TOKEN)
        .all(|(key, expected)| {
            let actual = extract_json_path(payload, key);
            actual.as_ref() == Some(expected)
        })
}

/// Extract value from JSON using dot notation path
pub(crate) fn extract_json_path(data: &Value, path: &str) -> Option<Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = data;

    for part in parts {
        current = current.get(part)?;
    }

    Some(current.clone())
}
