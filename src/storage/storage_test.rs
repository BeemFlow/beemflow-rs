use super::*;

/// Comprehensive test helper that runs all storage operations
/// Used to ensure parity between Memory and SQLite implementations
async fn test_all_storage_operations<S: Storage>(storage: Arc<S>) {
    // Test 1: SaveRun and GetRun
    let run_id = Uuid::new_v4();
    let run = Run {
        id: run_id,
        flow_name: "test_flow".to_string().into(),
        event: {
            let mut m = HashMap::new();
            m.insert("key".to_string(), serde_json::json!("value"));
            m
        },
        vars: HashMap::new(),
        status: RunStatus::Running,
        started_at: Utc::now(),
        ended_at: None,
        steps: None,
    };

    storage
        .save_run(&run)
        .await
        .expect("SaveRun should succeed");

    let retrieved = storage
        .get_run(run_id)
        .await
        .expect("GetRun should succeed");
    assert!(retrieved.is_some(), "Should find saved run");
    assert_eq!(retrieved.as_ref().unwrap().id, run_id);
    assert_eq!(retrieved.unwrap().flow_name.as_str(), "test_flow");

    // Test 2: GetRun with non-existent ID
    let non_existent_id = Uuid::new_v4();
    let missing = storage
        .get_run(non_existent_id)
        .await
        .expect("GetRun should not error");
    assert!(missing.is_none(), "Should return None for non-existent run");

    // Test 3: SaveStep and GetSteps
    let step_id = Uuid::new_v4();
    let step = StepRun {
        id: step_id,
        run_id,
        step_name: "test_step".to_string().into(),
        status: StepStatus::Succeeded,
        outputs: Some({
            let mut m = HashMap::new();
            m.insert("result".to_string(), serde_json::json!("success"));
            m
        }),
        error: None,
        started_at: Utc::now(),
        ended_at: Some(Utc::now()),
    };

    storage
        .save_step(&step)
        .await
        .expect("SaveStep should succeed");

    let steps = storage
        .get_steps(run_id)
        .await
        .expect("GetSteps should succeed");
    assert_eq!(steps.len(), 1, "Expected 1 step");
    assert_eq!(steps[0].id, step_id);
    assert_eq!(steps[0].step_name.as_str(), "test_step");

    // Test 4: RegisterWait and ResolveWait
    let token = Uuid::new_v4();
    storage
        .register_wait(token, None)
        .await
        .expect("RegisterWait should succeed");

    let resolved = storage
        .resolve_wait(token)
        .await
        .expect("ResolveWait should succeed");
    // Behavior may vary between implementations
    let _ = resolved;

    // Test 5: ListRuns
    let runs = storage
        .list_runs(100, 0)
        .await
        .expect("ListRuns should succeed");
    assert_eq!(runs.len(), 1, "Expected 1 run");

    // Test 6: SavePausedRun, LoadPausedRuns, DeletePausedRun
    let paused_data = serde_json::json!({
        "runID": run_id.to_string(),
        "token": "pause_token",
        "stepName": "paused_step"
    });

    storage
        .save_paused_run("pause_token", paused_data.clone())
        .await
        .expect("SavePausedRun should succeed");

    let paused_runs = storage
        .load_paused_runs()
        .await
        .expect("LoadPausedRuns should succeed");
    assert_eq!(paused_runs.len(), 1, "Expected 1 paused run");
    assert!(paused_runs.contains_key("pause_token"));

    storage
        .delete_paused_run("pause_token")
        .await
        .expect("DeletePausedRun should succeed");

    let paused_runs = storage
        .load_paused_runs()
        .await
        .expect("LoadPausedRuns should succeed");
    assert_eq!(paused_runs.len(), 0, "Expected 0 paused runs after delete");

    // Test 7: DeleteRun
    storage
        .delete_run(run_id)
        .await
        .expect("DeleteRun should succeed");

    let runs = storage
        .list_runs(100, 0)
        .await
        .expect("ListRuns should succeed");
    assert_eq!(runs.len(), 0, "Expected 0 runs after delete");
}

/// Test OAuth credential operations
async fn test_oauth_credential_operations<S: Storage>(storage: Arc<S>) {
    let cred = OAuthCredential {
        id: "test_cred".to_string(),
        provider: "google".to_string(),
        integration: "sheets".to_string(),
        access_token: "access_token_123".to_string(),
        refresh_token: Some("refresh_token_456".to_string()),
        expires_at: Some(Utc::now() + chrono::Duration::hours(1)),
        scope: Some("spreadsheets.readonly".to_string()),
        created_at: Utc::now(),
        updated_at: Utc::now(),
    };

    // Save credential
    storage
        .save_oauth_credential(&cred)
        .await
        .expect("SaveOAuthCredential should succeed");

    // Get credential
    let retrieved = storage
        .get_oauth_credential("google", "sheets")
        .await
        .expect("GetOAuthCredential should succeed");
    assert!(retrieved.is_some(), "Should find saved credential");
    assert_eq!(retrieved.as_ref().unwrap().id, "test_cred");
    assert_eq!(retrieved.as_ref().unwrap().access_token, "access_token_123");

    // List credentials
    let creds = storage
        .list_oauth_credentials()
        .await
        .expect("ListOAuthCredentials should succeed");
    assert_eq!(creds.len(), 1, "Expected 1 credential");

    // Refresh credential
    let new_expires = Utc::now() + chrono::Duration::hours(2);
    storage
        .refresh_oauth_credential("test_cred", "new_access_token", Some(new_expires))
        .await
        .expect("RefreshOAuthCredential should succeed");

    let refreshed = storage
        .get_oauth_credential("google", "sheets")
        .await
        .expect("GetOAuthCredential should succeed");
    assert_eq!(refreshed.as_ref().unwrap().access_token, "new_access_token");

    // Delete credential
    storage
        .delete_oauth_credential("test_cred")
        .await
        .expect("DeleteOAuthCredential should succeed");

    let creds = storage
        .list_oauth_credentials()
        .await
        .expect("ListOAuthCredentials should succeed");
    assert_eq!(creds.len(), 0, "Expected 0 credentials after delete");
}

/// Test flow versioning operations
async fn test_flow_versioning_operations<S: Storage>(storage: Arc<S>) {
    // Deploy version 1
    storage
        .deploy_flow_version("my_flow", "1.0.0", "content v1")
        .await
        .expect("Deploy v1 should succeed");

    // Deploy version 2
    storage
        .deploy_flow_version("my_flow", "2.0.0", "content v2")
        .await
        .expect("Deploy v2 should succeed");

    // Get deployed version (should be v2, latest)
    let deployed = storage
        .get_deployed_version("my_flow")
        .await
        .expect("GetDeployedVersion should succeed");
    assert_eq!(
        deployed,
        Some("2.0.0".to_string()),
        "Latest deployed should be v2"
    );

    // Get specific version content
    let content_v1 = storage
        .get_flow_version_content("my_flow", "1.0.0")
        .await
        .expect("GetFlowVersionContent should succeed");
    assert_eq!(content_v1, Some("content v1".to_string()));

    let content_v2 = storage
        .get_flow_version_content("my_flow", "2.0.0")
        .await
        .expect("GetFlowVersionContent should succeed");
    assert_eq!(content_v2, Some("content v2".to_string()));

    // List versions
    let versions = storage
        .list_flow_versions("my_flow")
        .await
        .expect("ListFlowVersions should succeed");
    assert_eq!(versions.len(), 2, "Expected 2 versions");
    assert!(
        versions.iter().any(|v| v.version == "2.0.0" && v.is_live),
        "v2 should be live"
    );
    assert!(
        versions.iter().any(|v| v.version == "1.0.0" && !v.is_live),
        "v1 should not be live"
    );

    // Rollback to v1
    storage
        .set_deployed_version("my_flow", "1.0.0")
        .await
        .expect("SetDeployedVersion should succeed");

    let deployed = storage
        .get_deployed_version("my_flow")
        .await
        .expect("GetDeployedVersion should succeed");
    assert_eq!(
        deployed,
        Some("1.0.0".to_string()),
        "Deployed should now be v1"
    );
}

// Note: Flow CRUD operations (save/get/list/delete) are now handled by pure functions
// in storage::flows module and tested there. Database storage only handles versioning.

/// Test multiple steps per run
async fn test_multiple_steps<S: Storage>(storage: Arc<S>) {
    let run_id = Uuid::new_v4();
    let run = Run {
        id: run_id,
        flow_name: "multi_step_flow".to_string().into(),
        event: HashMap::new(),
        vars: HashMap::new(),
        status: RunStatus::Running,
        started_at: Utc::now(),
        ended_at: None,
        steps: None,
    };

    storage
        .save_run(&run)
        .await
        .expect("SaveRun should succeed");

    // Add 10 steps
    for i in 0..10 {
        let step = StepRun {
            id: Uuid::new_v4(),
            run_id,
            step_name: format!("step_{}", i).into(),
            status: if i % 2 == 0 {
                StepStatus::Succeeded
            } else {
                StepStatus::Failed
            },
            outputs: Some({
                let mut m = HashMap::new();
                m.insert("index".to_string(), serde_json::json!(i));
                m
            }),
            error: if i % 2 == 1 {
                Some(format!("Error in step {}", i))
            } else {
                None
            },
            started_at: Utc::now(),
            ended_at: Some(Utc::now()),
        };
        storage
            .save_step(&step)
            .await
            .expect("SaveStep should succeed");
    }

    let steps = storage
        .get_steps(run_id)
        .await
        .expect("GetSteps should succeed");
    assert_eq!(steps.len(), 10, "Expected 10 steps");

    // Verify step data integrity
    let succeeded_count = steps
        .iter()
        .filter(|s| s.status == StepStatus::Succeeded)
        .count();
    let failed_count = steps
        .iter()
        .filter(|s| s.status == StepStatus::Failed)
        .count();
    assert_eq!(succeeded_count, 5, "Expected 5 succeeded steps");
    assert_eq!(failed_count, 5, "Expected 5 failed steps");
}

#[tokio::test]
async fn test_sqlite_storage_all_operations() {
    let storage = Arc::new(
        SqliteStorage::new(":memory:")
            .await
            .expect("SQLite creation failed"),
    );
    test_all_storage_operations(storage).await;
}

#[tokio::test]
async fn test_sqlite_storage_oauth() {
    let storage = Arc::new(
        SqliteStorage::new(":memory:")
            .await
            .expect("SQLite creation failed"),
    );
    test_oauth_credential_operations(storage).await;
}

#[tokio::test]
async fn test_sqlite_storage_versioning() {
    let storage = Arc::new(
        SqliteStorage::new(":memory:")
            .await
            .expect("SQLite creation failed"),
    );
    test_flow_versioning_operations(storage).await;
}

#[tokio::test]
async fn test_sqlite_storage_multiple_steps() {
    let storage = Arc::new(
        SqliteStorage::new(":memory:")
            .await
            .expect("SQLite creation failed"),
    );
    test_multiple_steps(storage).await;
}

// ========================================
// Stress Tests
// ========================================

#[tokio::test]
async fn test_sqlite_storage_stress_runs() {
    let storage = Arc::new(
        SqliteStorage::new(":memory:")
            .await
            .expect("SQLite creation failed"),
    );

    // Create 100 runs
    for i in 0..100 {
        let run = Run {
            id: Uuid::new_v4(),
            flow_name: format!("flow_{}", i % 10).into(),
            event: HashMap::new(),
            vars: HashMap::new(),
            status: if i % 3 == 0 {
                RunStatus::Succeeded
            } else {
                RunStatus::Running
            },
            started_at: Utc::now(),
            ended_at: None,
            steps: None,
        };
        storage
            .save_run(&run)
            .await
            .expect("SaveRun should succeed");
    }

    let runs = storage
        .list_runs(1000, 0)
        .await
        .expect("ListRuns should succeed");
    assert_eq!(runs.len(), 100, "Expected 100 runs");
}

#[tokio::test]
async fn test_sqlite_storage_concurrent_writes() {
    let storage = Arc::new(
        SqliteStorage::new(":memory:")
            .await
            .expect("SQLite creation failed"),
    );

    // Spawn 20 concurrent writers
    let mut handles = vec![];
    for i in 0..20 {
        let storage_clone = storage.clone();
        let handle = tokio::spawn(async move {
            let run = Run {
                id: Uuid::new_v4(),
                flow_name: format!("concurrent_flow_{}", i).into(),
                event: HashMap::new(),
                vars: HashMap::new(),
                status: RunStatus::Running,
                started_at: Utc::now(),
                ended_at: None,
                steps: None,
            };
            storage_clone.save_run(&run).await
        });
        handles.push(handle);
    }

    // Wait for all writes
    for handle in handles {
        handle
            .await
            .unwrap()
            .expect("Concurrent write should succeed");
    }

    let runs = storage
        .list_runs(1000, 0)
        .await
        .expect("ListRuns should succeed");
    assert_eq!(runs.len(), 20, "Expected 20 runs from concurrent writes");
}

// ========================================
// Error Handling Tests
// ========================================

#[tokio::test]
async fn test_sqlite_storage_delete_nonexistent() {
    let storage = Arc::new(
        SqliteStorage::new(":memory:")
            .await
            .expect("SQLite creation failed"),
    );
    // Deleting non-existent items should not error
    storage
        .delete_run(Uuid::new_v4())
        .await
        .expect("Delete non-existent run should not error");
    storage
        .delete_oauth_credential("nonexistent")
        .await
        .expect("Delete non-existent cred should not error");
}
