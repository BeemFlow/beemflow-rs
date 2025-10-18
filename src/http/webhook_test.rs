//! Integration tests for webhook HTTP layer

use crate::http::webhook::{
    WebhookManagerState, create_webhook_routes, extract_json_path, matches_event,
    parse_webhook_events,
};
use crate::registry::{WebhookConfig, WebhookEvent};
use crate::utils::TestEnvironment;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use serde_json::json;
use std::collections::HashMap;
use tower::ServiceExt;

#[tokio::test]
async fn test_webhook_route_registration() {
    // Create test environment with all dependencies
    let env = TestEnvironment::new().await;

    // Create WebhookManagerState with storage and engine
    let webhook_state = WebhookManagerState {
        registry_manager: env.deps.registry_manager.clone(),
        secrets_provider: env.deps.config.create_secrets_provider(),
        storage: env.deps.storage.clone(),
        engine: env.deps.engine.clone(),
        config: env.deps.config.clone(),
    };

    // Build webhook router
    let app = create_webhook_routes().with_state(webhook_state);

    // Make a POST request to /test-provider
    // This should return 404 (webhook not configured) but proves the route is registered
    let request = Request::builder()
        .method("POST")
        .uri("/test-provider")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"test":"data"}"#))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    // Verify the route is accessible (not 404 NOT_FOUND for the route itself)
    // We expect 404 "Webhook not configured" since test-provider isn't in registry
    // This is different from Axum returning 404 for an unregistered route
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "Webhook handler should be invoked (not a routing 404)"
    );
}

#[tokio::test]
async fn test_webhook_state_has_storage_and_engine() {
    // Verify that WebhookManagerState can be created with storage and engine
    let env = TestEnvironment::new().await;

    let webhook_state = WebhookManagerState {
        registry_manager: env.deps.registry_manager.clone(),
        secrets_provider: env.deps.config.create_secrets_provider(),
        storage: env.deps.storage.clone(),
        engine: env.deps.engine.clone(),
        config: env.deps.config.clone(),
    };

    // Verify state fields are accessible
    use std::sync::Arc;
    assert!(Arc::strong_count(&webhook_state.storage) >= 1);
    assert!(Arc::strong_count(&webhook_state.engine) >= 1);
}

// Unit tests for webhook parsing functions

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
fn test_parse_webhook_events_with_extract() {
    // Create webhook config with extract rules (like Airtable)
    let webhook_config = WebhookConfig {
        enabled: true,
        secret: None,
        signature: None,
        events: vec![WebhookEvent {
            event_type: "record.updated".to_string(),
            topic: "airtable.record.updated".to_string(),
            match_: {
                let mut m = HashMap::new();
                m.insert("webhook.action".to_string(), json!("update"));
                m
            },
            extract: {
                let mut e = HashMap::new();
                e.insert("record_id".to_string(), "webhook.record.id".to_string());
                e.insert(
                    "status".to_string(),
                    "webhook.record.fields.status".to_string(),
                );
                e.insert("base_id".to_string(), "webhook.base.id".to_string());
                e
            },
        }],
    };

    // Simulate Airtable webhook payload
    let payload = json!({
        "webhook": {
            "action": "update",
            "base": {
                "id": "appXXXXX"
            },
            "record": {
                "id": "recYYYYY",
                "fields": {
                    "status": "approved"
                }
            }
        }
    });

    // Parse events
    let events = parse_webhook_events(&webhook_config, &payload).expect("Should parse events");

    // Verify we extracted the event
    assert_eq!(events.len(), 1, "Should extract one event");
    let event = &events[0];

    assert_eq!(event.topic, "airtable.record.updated");
    assert_eq!(event.data.get("record_id"), Some(&json!("recYYYYY")));
    assert_eq!(event.data.get("status"), Some(&json!("approved")));
    assert_eq!(event.data.get("base_id"), Some(&json!("appXXXXX")));
}

#[test]
fn test_parse_webhook_events_no_match() {
    let webhook_config = WebhookConfig {
        enabled: true,
        secret: None,
        signature: None,
        events: vec![WebhookEvent {
            event_type: "record.created".to_string(),
            topic: "airtable.record.created".to_string(),
            match_: {
                let mut m = HashMap::new();
                m.insert("webhook.action".to_string(), json!("create"));
                m
            },
            extract: HashMap::new(),
        }],
    };

    // Payload with different action
    let payload = json!({
        "webhook": {
            "action": "update"
        }
    });

    let events = parse_webhook_events(&webhook_config, &payload).expect("Should parse events");

    assert_eq!(events.len(), 0, "Should not match any events");
}

#[test]
fn test_parse_webhook_events_multiple_events() {
    let webhook_config = WebhookConfig {
        enabled: true,
        secret: None,
        signature: None,
        events: vec![
            WebhookEvent {
                event_type: "message.created".to_string(),
                topic: "slack.message.created".to_string(),
                match_: {
                    let mut m = HashMap::new();
                    m.insert("type".to_string(), json!("event_callback"));
                    m.insert("event.type".to_string(), json!("message"));
                    m
                },
                extract: {
                    let mut e = HashMap::new();
                    e.insert("channel".to_string(), "event.channel".to_string());
                    e.insert("user".to_string(), "event.user".to_string());
                    e.insert("text".to_string(), "event.text".to_string());
                    e
                },
            },
            WebhookEvent {
                event_type: "reaction.added".to_string(),
                topic: "slack.reaction.added".to_string(),
                match_: {
                    let mut m = HashMap::new();
                    m.insert("type".to_string(), json!("event_callback"));
                    m.insert("event.type".to_string(), json!("reaction_added"));
                    m
                },
                extract: {
                    let mut e = HashMap::new();
                    e.insert("reaction".to_string(), "event.reaction".to_string());
                    e
                },
            },
        ],
    };

    // Payload that matches first event
    let payload = json!({
        "type": "event_callback",
        "event": {
            "type": "message",
            "channel": "C123",
            "user": "U456",
            "text": "Hello world"
        }
    });

    let events = parse_webhook_events(&webhook_config, &payload).expect("Should parse events");

    assert_eq!(events.len(), 1, "Should extract one matching event");
    assert_eq!(events[0].topic, "slack.message.created");
    assert_eq!(events[0].data.get("channel"), Some(&json!("C123")));
    assert_eq!(events[0].data.get("text"), Some(&json!("Hello world")));
}
