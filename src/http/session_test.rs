//! Tests for session

use crate::http::session::SessionStore;
use chrono::{Duration, Utc};

#[tokio::test]
async fn test_create_session() {
    let store = SessionStore::new();
    let session = store.create_session("user123", Duration::hours(1));

    assert_eq!(session.user_id, "user123");
    assert!(session.expires_at > Utc::now());
}

#[tokio::test]
async fn test_get_session() {
    let store = SessionStore::new();
    let session = store.create_session("user123", Duration::hours(1));

    let retrieved = store.get_session(&session.id);
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().user_id, "user123");
}

#[tokio::test]
async fn test_update_session() {
    let store = SessionStore::new();
    let session = store.create_session("user123", Duration::hours(1));

    let updated = store.update_session(&session.id, "key".to_string(), serde_json::json!("value"));
    assert!(updated);

    let retrieved = store.get_session(&session.id).unwrap();
    assert_eq!(
        retrieved.data.get("key").unwrap(),
        &serde_json::json!("value")
    );
}

#[tokio::test]
async fn test_csrf_token() {
    let store = SessionStore::new();
    let session = store.create_session("user123", Duration::hours(1));

    let token = store.generate_csrf_token(&session.id).unwrap();
    assert!(!token.is_empty());

    assert!(store.validate_csrf_token(&session.id, &token));
    assert!(!store.validate_csrf_token(&session.id, "invalid"));
}

#[tokio::test]
async fn test_delete_session() {
    let store = SessionStore::new();
    let session = store.create_session("user123", Duration::hours(1));

    store.delete_session(&session.id);

    let retrieved = store.get_session(&session.id);
    assert!(retrieved.is_none());
}
