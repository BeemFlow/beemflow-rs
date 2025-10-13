//! Webhook management system
//!
//! Handles dynamic webhook registration, signature verification, and event parsing.

use crate::Result;
use crate::event::EventBus;
use crate::registry::{RegistryManager, WebhookConfig};
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
    pub event_bus: Arc<dyn EventBus>,
    pub registry_manager: Arc<RegistryManager>,
}

/// Parsed webhook event
#[derive(Debug)]
struct ParsedEvent {
    topic: String,
    data: HashMap<String, Value>,
}

/// Create webhook routes
pub fn create_webhook_routes() -> Router<WebhookManagerState> {
    Router::new().route("/{provider}/{*path}", post(handle_webhook))
}

/// Handle incoming webhook
async fn handle_webhook(
    State(state): State<WebhookManagerState>,
    Path((provider, path)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    tracing::debug!("Webhook received: provider={}, path={}", provider, path);

    // Find webhook configuration from registry
    let webhook_config = match find_webhook_config(&state.registry_manager, &provider, &path).await
    {
        Some(config) => config,
        None => {
            tracing::warn!("Webhook not found: {}/{}", provider, path);
            return (StatusCode::NOT_FOUND, "Webhook not configured").into_response();
        }
    };

    // Verify signature if configured
    if let Some(ref secret) = webhook_config.secret {
        let secret_value = expand_env_value(secret);
        if !secret_value.is_empty()
            && !verify_webhook_signature(&webhook_config, &headers, &body, &secret_value)
        {
            tracing::error!("Invalid webhook signature for {}/{}", provider, path);
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

    // Publish events to event bus
    for event in events {
        let event_data = serde_json::to_value(&event.data).unwrap_or_default();
        if let Err(e) = state.event_bus.publish(&event.topic, event_data).await {
            tracing::error!("Failed to publish event {}: {}", event.topic, e);
            return (StatusCode::INTERNAL_SERVER_ERROR, "Failed to publish event").into_response();
        }
        tracing::info!("Published webhook event: {}", event.topic);
    }

    (StatusCode::OK, "OK").into_response()
}

/// Find webhook configuration from registry
async fn find_webhook_config(
    registry: &Arc<RegistryManager>,
    provider: &str,
    _path: &str,
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
fn parse_webhook_events(config: &WebhookConfig, payload: &Value) -> Result<Vec<ParsedEvent>> {
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
fn matches_event(payload: &Value, match_conditions: &HashMap<String, Value>) -> bool {
    for (path, expected) in match_conditions {
        let actual = extract_json_path(payload, path);
        if actual.as_ref() != Some(expected) {
            return false;
        }
    }
    true
}

/// Extract value from JSON using dot notation path
fn extract_json_path(data: &Value, path: &str) -> Option<Value> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = data;

    for part in parts {
        current = current.get(part)?;
    }

    Some(current.clone())
}

/// Expand environment variable references ($env:VAR_NAME)
fn expand_env_value(value: &str) -> String {
    if value.starts_with("$env:") {
        let var_name = value.trim_start_matches("$env:");
        std::env::var(var_name).unwrap_or_default()
    } else {
        value.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_json_path() {
        let data = json!({
            "user": {
                "name": "Alice",
                "email": "alice@example.com"
            },
            "count": 42
        });

        assert_eq!(extract_json_path(&data, "user.name"), Some(json!("Alice")));
        assert_eq!(
            extract_json_path(&data, "user.email"),
            Some(json!("alice@example.com"))
        );
        assert_eq!(extract_json_path(&data, "count"), Some(json!(42)));
        assert_eq!(extract_json_path(&data, "nonexistent"), None);
    }

    #[test]
    fn test_matches_event() {
        let payload = json!({
            "type": "message.created",
            "user_id": "123"
        });

        let mut conditions = HashMap::new();
        conditions.insert("type".to_string(), json!("message.created"));

        assert!(matches_event(&payload, &conditions));

        conditions.insert("user_id".to_string(), json!("123"));
        assert!(matches_event(&payload, &conditions));

        conditions.insert("user_id".to_string(), json!("456"));
        assert!(!matches_event(&payload, &conditions));
    }

    #[test]
    fn test_expand_env_value() {
        unsafe {
            std::env::set_var("TEST_VAR", "test_value");
        }

        assert_eq!(expand_env_value("$env:TEST_VAR"), "test_value");
        assert_eq!(expand_env_value("literal_value"), "literal_value");
        assert_eq!(expand_env_value("$env:NONEXISTENT"), "");

        unsafe {
            std::env::remove_var("TEST_VAR");
        }
    }
}
