use super::*;
use crate::storage::{MemoryStorage, Run, RunStatus};
use chrono::Utc;
use std::collections::HashMap;
use uuid::Uuid;

#[tokio::test]
async fn test_save_and_get_run() {
    let storage = MemoryStorage::new();
    let run = Run {
        id: Uuid::new_v4(),
        flow_name: "test".to_string(),
        event: HashMap::new(),
        vars: HashMap::new(),
        status: RunStatus::Running,
        started_at: Utc::now(),
        ended_at: None,
        steps: None,
    };

    storage.save_run(&run).await.unwrap();
    let retrieved = storage.get_run(run.id).await.unwrap();
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().flow_name, "test");
}

#[tokio::test]
async fn test_delete_run() {
    let storage = MemoryStorage::new();
    let run_id = Uuid::new_v4();
    let run = Run {
        id: run_id,
        flow_name: "test".to_string(),
        event: HashMap::new(),
        vars: HashMap::new(),
        status: RunStatus::Running,
        started_at: Utc::now(),
        ended_at: None,
        steps: None,
    };

    storage.save_run(&run).await.unwrap();
    storage.delete_run(run_id).await.unwrap();
    let retrieved = storage.get_run(run_id).await.unwrap();
    assert!(retrieved.is_none());
}

#[tokio::test]
async fn test_paused_runs() {
    let storage = MemoryStorage::new();
    let token = "test_token";
    let data = serde_json::json!({"foo": "bar"});

    storage.save_paused_run(token, data.clone()).await.unwrap();
    let loaded = storage.load_paused_runs().await.unwrap();
    assert_eq!(loaded.get(token), Some(&data));

    storage.delete_paused_run(token).await.unwrap();
    let loaded = storage.load_paused_runs().await.unwrap();
    assert!(!loaded.contains_key(token));
}

#[tokio::test]
async fn test_oauth_credentials() {
    let storage = MemoryStorage::new();
    let cred = OAuthCredential {
        id: "test_id".to_string(),
        provider: "google".to_string(),
        integration: "my_app".to_string(),
        access_token: "access".to_string(),
        refresh_token: Some("refresh".to_string()),
        expires_at: None,
        scope: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    storage.save_oauth_credential(&cred).await.unwrap();
    let retrieved = storage
        .get_oauth_credential("google", "my_app")
        .await
        .unwrap();
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().id, "test_id");

    storage.delete_oauth_credential("test_id").await.unwrap();
    let retrieved = storage
        .get_oauth_credential("google", "my_app")
        .await
        .unwrap();
    assert!(retrieved.is_none());
}

#[tokio::test]
async fn test_oauth_providers() {
    let storage = MemoryStorage::new();
    let provider = OAuthProvider {
        id: "google".to_string(),
        name: "Google".to_string(),
        client_id: "client".to_string(),
        client_secret: "secret".to_string(),
        auth_url: "https://auth".to_string(),
        token_url: "https://token".to_string(),
        scopes: Some(vec![]),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    storage.save_oauth_provider(&provider).await.unwrap();
    let retrieved = storage.get_oauth_provider("google").await.unwrap();
    assert!(retrieved.is_some());

    storage.delete_oauth_provider("google").await.unwrap();
    let retrieved = storage.get_oauth_provider("google").await.unwrap();
    assert!(retrieved.is_none());
}

#[tokio::test]
async fn test_flow_versioning() {
    let storage = MemoryStorage::new();

    storage
        .deploy_flow_version("my_flow", "v1", "content1")
        .await
        .unwrap();
    storage
        .deploy_flow_version("my_flow", "v2", "content2")
        .await
        .unwrap();
    storage.set_deployed_version("my_flow", "v2").await.unwrap();

    let deployed = storage.get_deployed_version("my_flow").await.unwrap();
    assert_eq!(deployed, Some("v2".to_string()));

    let versions = storage.list_flow_versions("my_flow").await.unwrap();
    assert_eq!(versions.len(), 2);
    assert!(versions.iter().any(|v| v.version == "v2" && v.is_live));
}
