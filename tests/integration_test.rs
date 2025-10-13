//! Integration tests for BeemFlow
//!
//! Tests the complete system end-to-end

use beemflow::dsl::{Validator, parse_file, parse_string};
use beemflow::storage::Storage;
use beemflow::{Engine, Flow};
use std::collections::HashMap;

#[tokio::test]
async fn test_hello_world_flow() {
    // Parse the hello_world flow from examples
    let flow = parse_file("flows/examples/hello_world.flow.yaml").unwrap();

    // Validate it
    assert!(Validator::validate(&flow).is_ok());

    // Execute it
    let engine = Engine::default();
    let result = engine.execute(&flow, HashMap::new()).await.unwrap();
    let outputs = result.outputs;

    // Verify outputs
    assert!(outputs.contains_key("greet"));
    assert!(outputs.contains_key("greet_again"));

    // Check that greet has text field
    if let Some(greet_output) = outputs.get("greet") {
        let greet_map: HashMap<String, serde_json::Value> =
            serde_json::from_value(greet_output.clone()).unwrap();
        assert!(greet_map.contains_key("text"));
        assert_eq!(
            greet_map.get("text").unwrap().as_str().unwrap(),
            "Hello, world, I'm BeemFlow!"
        );
    } else {
        panic!("greet output not found");
    }
}

#[tokio::test]
async fn test_flow_validation() {
    // Valid flow
    let yaml = r#"
name: test_flow
on: cli.manual
steps:
  - id: step1
    use: core.echo
    with:
      text: "test"
"#;

    let flow = parse_string(yaml).unwrap();
    assert!(Validator::validate(&flow).is_ok());

    // Invalid flow - missing name
    let invalid_yaml = r#"
name: ""
on: cli.manual
steps:
  - id: step1
    use: core.echo
"#;

    let invalid_flow = parse_string(invalid_yaml).unwrap();

    // Validation should catch the empty name
    assert!(Validator::validate(&invalid_flow).is_err());
}

#[tokio::test]
async fn test_core_echo_adapter() {
    let yaml = r#"
name: echo_test
on: cli.manual
steps:
  - id: echo_step
    use: core.echo
    with:
      text: "Hello from Rust!"
"#;

    let flow = parse_string(yaml).unwrap();
    let engine = Engine::default();
    let result = engine.execute(&flow, HashMap::new()).await.unwrap();
    let outputs = result.outputs;

    assert!(outputs.contains_key("echo_step"));
    let echo_output: HashMap<String, serde_json::Value> =
        serde_json::from_value(outputs.get("echo_step").unwrap().clone()).unwrap();
    assert_eq!(
        echo_output.get("text").unwrap().as_str().unwrap(),
        "Hello from Rust!"
    );
}

#[tokio::test]
async fn test_template_rendering() {
    let yaml = r#"
name: template_test
on: cli.manual
vars:
  greeting: "Hello"
  name: "Rust"
steps:
  - id: templated_echo
    use: core.echo
    with:
      text: "{{ vars.greeting }}, {{ vars.name }}!"
"#;

    let flow = parse_string(yaml).unwrap();
    let engine = Engine::default();
    let result = engine.execute(&flow, HashMap::new()).await.unwrap();
    let outputs = result.outputs;

    assert!(outputs.contains_key("templated_echo"));
    let echo_output: HashMap<String, serde_json::Value> =
        serde_json::from_value(outputs.get("templated_echo").unwrap().clone()).unwrap();
    assert_eq!(
        echo_output.get("text").unwrap().as_str().unwrap(),
        "Hello, Rust!"
    );
}

#[tokio::test]
async fn test_multiple_steps_with_dependencies() {
    let yaml = r#"
name: multi_step_test
on: cli.manual
steps:
  - id: step1
    use: core.echo
    with:
      text: "First"
  - id: step2
    use: core.echo
    with:
      text: "Second: {{ outputs.step1.text }}"
  - id: step3
    use: core.echo
    with:
      text: "Third: {{ step1.text }} and {{ step2.text }}"
"#;

    let flow = parse_string(yaml).unwrap();
    let engine = Engine::default();
    let result = engine.execute(&flow, HashMap::new()).await.unwrap();
    let outputs = result.outputs;

    assert_eq!(outputs.len(), 3);
    assert!(outputs.contains_key("step1"));
    assert!(outputs.contains_key("step2"));
    assert!(outputs.contains_key("step3"));

    // Verify output propagation
    let step2: HashMap<String, serde_json::Value> =
        serde_json::from_value(outputs.get("step2").unwrap().clone()).unwrap();
    assert!(
        step2
            .get("text")
            .unwrap()
            .as_str()
            .unwrap()
            .contains("First")
    );
}

#[test]
fn test_flow_deserialization_all_fields() {
    let yaml = r#"
name: complete_flow
description: A flow with all fields
version: "1.0.0"
on:
  - cli.manual
  - schedule.cron
cron: "0 9 * * *"
vars:
  key1: "value1"
  key2: 42
steps:
  - id: step1
    use: core.echo
    with:
      text: "test"
    if: "{{ vars.key2 > 10 }}"
    retry:
      attempts: 3
      delay_sec: 5
catch:
  - id: error_handler
    use: core.echo
    with:
      text: "Error occurred"
"#;

    let flow: Flow = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(flow.name, "complete_flow");
    assert_eq!(flow.version.unwrap(), "1.0.0");
    assert!(flow.vars.is_some());
    assert!(flow.catch.is_some());
    assert_eq!(flow.steps.len(), 1);
    assert_eq!(flow.catch.unwrap().len(), 1);
}

#[tokio::test]
async fn test_storage_operations() {
    use beemflow::storage::{MemoryStorage, Storage};
    use chrono::Utc;
    use uuid::Uuid;

    let storage = MemoryStorage::new();

    // Test flow storage
    storage
        .save_flow("test_flow", "content", Some("1.0.0"))
        .await
        .unwrap();
    let retrieved = storage.get_flow("test_flow").await.unwrap();
    assert_eq!(retrieved.unwrap(), "content");

    // Test run storage
    let run = beemflow::model::Run {
        id: Uuid::new_v4(),
        flow_name: "test".to_string(),
        event: HashMap::new(),
        vars: HashMap::new(),
        status: beemflow::model::RunStatus::Running,
        started_at: Utc::now(),
        ended_at: None,
        steps: None,
    };

    storage.save_run(&run).await.unwrap();
    let retrieved_run = storage.get_run(run.id).await.unwrap();
    assert!(retrieved_run.is_some());
    assert_eq!(retrieved_run.unwrap().flow_name, "test");
}

#[test]
fn test_graph_generation() {
    use beemflow::graph::GraphGenerator;
    use beemflow::model::*;

    let flow = Flow {
        name: "test".to_string(),
        description: None,
        version: None,
        on: Some(Trigger::Single("cli.manual".to_string())),
        cron: None,
        vars: None,
        steps: vec![
            Step {
                id: "step1".to_string(),
                use_: Some("core.echo".to_string()),
                with: None,
                depends_on: None,
                parallel: None,
                if_: None,
                foreach: None,
                as_: None,
                do_: None,
                steps: None,
                retry: None,
                await_event: None,
                wait: None,
            },
            Step {
                id: "step2".to_string(),
                use_: Some("core.echo".to_string()),
                with: None,
                depends_on: None,
                parallel: None,
                if_: None,
                foreach: None,
                as_: None,
                do_: None,
                steps: None,
                retry: None,
                await_event: None,
                wait: None,
            },
        ],
        catch: None,
        mcp_servers: None,
    };

    let diagram = GraphGenerator::generate(&flow).unwrap();
    assert!(diagram.contains("graph TD"));
    assert!(diagram.contains("step1"));
    assert!(diagram.contains("step2"));
    assert!(diagram.contains("Start"));
    assert!(diagram.contains("End"));
}

// ============================================================================
// CLI and Server Integration Tests
// ============================================================================

#[tokio::test]
async fn test_cli_operations_with_fresh_database() {
    use beemflow::config::Config;
    use beemflow::core::{Dependencies, OperationRegistry};
    use beemflow::engine::Engine;
    use beemflow::event::InProcEventBus;
    use beemflow::registry::RegistryManager;
    use beemflow::storage::SqliteStorage;
    use std::sync::Arc;
    use tempfile::TempDir;

    // Create a temporary directory for the test database
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_cli.db");

    // Verify database doesn't exist
    assert!(!db_path.exists(), "Database should not exist initially");

    // Create storage with auto-creation
    let storage = Arc::new(SqliteStorage::new(db_path.to_str().unwrap()).await.unwrap());

    // Verify database was created
    assert!(db_path.exists(), "Database should be auto-created");

    // Create registry (simulating CLI initialization)
    let deps = Dependencies {
        storage: storage.clone(),
        engine: Arc::new(Engine::default()),
        registry_manager: Arc::new(RegistryManager::standard(None)),
        event_bus: Arc::new(InProcEventBus::new()) as Arc<dyn beemflow::event::EventBus>,
        config: Arc::new(Config::default()),
    };

    let registry = OperationRegistry::new(deps);

    // Test saving a flow
    let save_result = registry.execute("save_flow", serde_json::json!({
        "name": "test_flow",
        "content": "name: test_flow\non: cli.manual\nsteps:\n  - id: test\n    use: core.echo\n    with:\n      text: test"
    })).await;
    assert!(save_result.is_ok(), "Should be able to save flow");

    // Test listing flows - just verify it succeeds
    let list_result = registry.execute("list_flows", serde_json::json!({})).await;
    assert!(list_result.is_ok(), "Should be able to list flows");

    // Test getting flow
    let get_result = registry
        .execute(
            "get_flow",
            serde_json::json!({
                "name": "test_flow"
            }),
        )
        .await;
    assert!(get_result.is_ok(), "Should be able to get flow");
}

#[tokio::test]
async fn test_mcp_server_with_fresh_database() {
    use beemflow::config::Config;
    use beemflow::core::{Dependencies, OperationRegistry};
    use beemflow::engine::Engine;
    use beemflow::event::InProcEventBus;
    use beemflow::mcp::McpServer;
    use beemflow::registry::RegistryManager;
    use beemflow::storage::SqliteStorage;
    use std::sync::Arc;
    use tempfile::TempDir;

    // Create a temporary directory for the test database
    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("test_mcp.db");

    // Verify database doesn't exist
    assert!(!db_path.exists(), "Database should not exist initially");

    // Create storage with auto-creation (simulating MCP server startup)
    let storage = Arc::new(SqliteStorage::new(db_path.to_str().unwrap()).await.unwrap());

    // Verify database was created
    assert!(
        db_path.exists(),
        "Database should be auto-created on MCP startup"
    );

    // Create dependencies
    let deps = Dependencies {
        storage: storage.clone(),
        engine: Arc::new(Engine::default()),
        registry_manager: Arc::new(RegistryManager::standard(None)),
        event_bus: Arc::new(InProcEventBus::new()) as Arc<dyn beemflow::event::EventBus>,
        config: Arc::new(Config::default()),
    };

    let registry = Arc::new(OperationRegistry::new(deps));

    // Create MCP server - if this succeeds, the database is functional
    let _server = McpServer::new(registry.clone());

    // Verify server can perform operations through registry
    let list_result = registry.execute("list_flows", serde_json::json!({})).await;
    assert!(
        list_result.is_ok(),
        "Fresh database should support list_flows"
    );

    // Test saving a flow through the registry (as MCP would)
    let save_result = registry.execute("save_flow", serde_json::json!({
        "name": "mcp_test_flow",
        "content": "name: mcp_test\non: cli.manual\nsteps:\n  - id: test\n    use: core.echo\n    with:\n      text: test"
    })).await;
    assert!(
        save_result.is_ok(),
        "MCP server should be able to save flows: {:?}",
        save_result.as_ref().err()
    );

    // Verify flow was saved
    let get_result = registry
        .execute(
            "get_flow",
            serde_json::json!({
                "name": "mcp_test_flow"
            }),
        )
        .await;
    assert!(get_result.is_ok(), "Should be able to retrieve saved flow");
}

#[tokio::test]
async fn test_cli_reuses_existing_database() {
    use beemflow::storage::SqliteStorage;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let db_path = temp_dir.path().join("reuse.db");
    let db_path_str = db_path.to_str().unwrap();

    // First connection - create and populate database
    {
        let storage = SqliteStorage::new(db_path_str).await.unwrap();
        storage
            .save_flow("persisted_flow", "content", None)
            .await
            .unwrap();
    }

    // Second connection - should reuse existing database
    {
        let storage = SqliteStorage::new(db_path_str).await.unwrap();
        let flows = storage.list_flows().await.unwrap();
        assert_eq!(flows.len(), 1, "Should find persisted flow");
        assert_eq!(flows[0], "persisted_flow");
    }

    // Verify database file still exists
    assert!(db_path.exists(), "Database file should persist");
}

#[tokio::test]
async fn test_cli_with_missing_parent_directory() {
    use beemflow::storage::SqliteStorage;
    use tempfile::TempDir;

    let temp_dir = TempDir::new().unwrap();
    let nested_path = temp_dir.path().join("level1").join("level2").join("cli.db");
    let nested_path_str = nested_path.to_str().unwrap();

    // Parent directories should not exist
    assert!(
        !nested_path.parent().unwrap().exists(),
        "Parent dirs should not exist"
    );

    // Create storage - should auto-create all parent directories
    let storage = SqliteStorage::new(nested_path_str).await.unwrap();

    // Verify parent directories were created
    assert!(
        nested_path.parent().unwrap().exists(),
        "Parent directories should be created"
    );
    assert!(nested_path.exists(), "Database should exist");

    // Verify it's functional
    storage
        .save_flow("nested_flow", "content", None)
        .await
        .unwrap();
    let flows = storage.list_flows().await.unwrap();
    assert_eq!(flows.len(), 1);
}
