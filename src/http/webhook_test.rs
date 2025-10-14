//! Tests for webhook

use crate::http::webhook::{expand_env_value, extract_json_path, matches_event};
use serde_json::json;
use std::collections::HashMap;

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
