//! Flow integration tests
//!
//! This test suite validates all example and e2e flows can execute correctly
//! via the Engine API. These tests require valid API keys in .env file.
//!
//! For true end-to-end tests that run via CLI, use `make e2e`.

use beemflow::dsl::{Validator, parse_file};
use beemflow::{Engine};
use std::collections::HashMap;

/// Helper to create event data with timestamp for deduplication
fn create_test_event() -> HashMap<String, serde_json::Value> {
    let mut event = HashMap::new();
    event.insert(
        "timestamp".to_string(),
        serde_json::Value::String(chrono::Utc::now().timestamp().to_string()),
    );
    event
}

/// Helper to check if API key is available
fn has_openai_key() -> bool {
    std::env::var("OPENAI_API_KEY").is_ok() && !std::env::var("OPENAI_API_KEY").unwrap().is_empty()
}

fn has_anthropic_key() -> bool {
    std::env::var("ANTHROPIC_API_KEY").is_ok() && !std::env::var("ANTHROPIC_API_KEY").unwrap().is_empty()
}

fn has_airtable_key() -> bool {
    std::env::var("AIRTABLE_API_KEY").is_ok() && !std::env::var("AIRTABLE_API_KEY").unwrap().is_empty()
}

// ============================================================================
// E2E Flow Tests
// ============================================================================

#[tokio::test]
async fn test_e2e_fetch_and_summarize() {
    if !has_openai_key() {
        eprintln!("⚠️  Skipping test_e2e_fetch_and_summarize: OPENAI_API_KEY not set");
        return;
    }

    let flow = parse_file("flows/e2e/fetch_and_summarize.flow.yaml")
        .expect("Failed to parse fetch_and_summarize flow");

    assert!(Validator::validate(&flow).is_ok(), "Flow validation failed");

    let engine = Engine::default();
    let result = engine.execute(&flow, create_test_event()).await;

    assert!(
        result.is_ok(),
        "Flow execution failed: {:?}",
        result.err()
    );

    let outputs = result.unwrap().outputs;
    assert!(outputs.contains_key("fetch_page"), "Missing fetch_page output");
    assert!(outputs.contains_key("summarize"), "Missing summarize output");
    assert!(outputs.contains_key("print"), "Missing print output");
}

#[tokio::test]
async fn test_e2e_parallel_openai() {
    if !has_openai_key() {
        eprintln!("⚠️  Skipping test_e2e_parallel_openai: OPENAI_API_KEY not set");
        return;
    }

    let flow = parse_file("flows/e2e/parallel_openai.flow.yaml")
        .expect("Failed to parse parallel_openai flow");

    assert!(Validator::validate(&flow).is_ok(), "Flow validation failed");

    let engine = Engine::default();
    let result = engine.execute(&flow, create_test_event()).await;

    assert!(
        result.is_ok(),
        "Flow execution failed: {:?}",
        result.err()
    );

    let outputs = result.unwrap().outputs;
    assert!(outputs.contains_key("chat1"), "Missing chat1 output");
    assert!(outputs.contains_key("chat2"), "Missing chat2 output");
    assert!(outputs.contains_key("combine"), "Missing combine output");
}

#[tokio::test]
async fn test_e2e_airtable_integration() {
    if !has_airtable_key() {
        eprintln!("⚠️  Skipping test_e2e_airtable_integration: AIRTABLE_API_KEY not set");
        return;
    }

    let flow = parse_file("flows/e2e/airtable_integration.flow.yaml")
        .expect("Failed to parse airtable_integration flow");

    assert!(Validator::validate(&flow).is_ok(), "Flow validation failed");

    let engine = Engine::default();
    let result = engine.execute(&flow, create_test_event()).await;

    // Note: This might fail if MCP server is not properly configured
    // We just verify it parses and validates correctly
    if result.is_err() {
        eprintln!("⚠️  Airtable flow failed (expected if MCP server not configured): {:?}", result.err());
    }
}

// ============================================================================
// Integration Flow Tests
// ============================================================================

#[tokio::test]
async fn test_integration_http_patterns() {
    if !has_openai_key() || !has_anthropic_key() {
        eprintln!("⚠️  Skipping test_integration_http_patterns: API keys not set");
        return;
    }

    let flow = parse_file("flows/integration/http_patterns.flow.yaml")
        .expect("Failed to parse http_patterns flow");

    assert!(Validator::validate(&flow).is_ok(), "Flow validation failed");

    let engine = Engine::default();
    let result = engine.execute(&flow, create_test_event()).await;

    assert!(
        result.is_ok(),
        "Flow execution failed: {:?}",
        result.err()
    );

    let outputs = result.unwrap().outputs;
    assert!(outputs.contains_key("test_http_fetch"), "Missing test_http_fetch");
    assert!(outputs.contains_key("test_generic_http"), "Missing test_generic_http");
    assert!(outputs.contains_key("test_openai_manifest"), "Missing test_openai_manifest");
    assert!(outputs.contains_key("test_anthropic_manifest"), "Missing test_anthropic_manifest");
    assert!(outputs.contains_key("test_http_post"), "Missing test_http_post");
    assert!(outputs.contains_key("verify_results"), "Missing verify_results");
}

#[tokio::test]
async fn test_integration_simple_parallel() {
    let flow = parse_file("flows/integration/simple_parallel.flow.yaml")
        .expect("Failed to parse simple_parallel flow");

    assert!(Validator::validate(&flow).is_ok(), "Flow validation failed");

    let engine = Engine::default();
    let result = engine.execute(&flow, create_test_event()).await;

    assert!(
        result.is_ok(),
        "Flow execution failed: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn test_integration_nested_parallel() {
    let flow = parse_file("flows/integration/nested_parallel.flow.yaml")
        .expect("Failed to parse nested_parallel flow");

    assert!(Validator::validate(&flow).is_ok(), "Flow validation failed");

    let engine = Engine::default();
    let result = engine.execute(&flow, create_test_event()).await;

    assert!(
        result.is_ok(),
        "Flow execution failed: {:?}",
        result.err()
    );
}

#[tokio::test]
async fn test_integration_templating_system() {
    let flow = parse_file("flows/integration/templating_system.flow.yaml")
        .expect("Failed to parse templating_system flow");

    assert!(Validator::validate(&flow).is_ok(), "Flow validation failed");

    let engine = Engine::default();
    let result = engine.execute(&flow, create_test_event()).await;

    assert!(
        result.is_ok(),
        "Flow execution failed: {:?}",
        result.err()
    );
}

// ============================================================================
// Example Flow Tests (Basic Validation)
// ============================================================================

#[tokio::test]
async fn test_example_hello_world() {
    let flow = parse_file("flows/examples/hello_world.flow.yaml")
        .expect("Failed to parse hello_world flow");

    assert!(Validator::validate(&flow).is_ok(), "Flow validation failed");

    let engine = Engine::default();
    let result = engine.execute(&flow, create_test_event()).await;

    assert!(
        result.is_ok(),
        "Flow execution failed: {:?}",
        result.err()
    );

    let outputs = result.unwrap().outputs;
    assert!(outputs.contains_key("greet"), "Missing greet output");
}

#[test]
fn test_all_flows_parse_and_validate() {
    // Get all flow files
    let flow_files = vec![
        // E2E flows
        "flows/e2e/fetch_and_summarize.flow.yaml",
        "flows/e2e/parallel_openai.flow.yaml",
        "flows/e2e/airtable_integration.flow.yaml",

        // Integration flows
        "flows/integration/http_patterns.flow.yaml",
        "flows/integration/simple_parallel.flow.yaml",
        "flows/integration/nested_parallel.flow.yaml",
        "flows/integration/templating_system.flow.yaml",
        "flows/integration/parallel_execution.flow.yaml",
        "flows/integration/engine_comprehensive.flow.yaml",
        "flows/integration/spec_compliance_test.flow.yaml",
        "flows/integration/edge_cases.flow.yaml",
        "flows/integration/performance.flow.yaml",

        // Example flows
        "flows/examples/hello_world.flow.yaml",
        "flows/examples/memory_demo.flow.yaml",
    ];

    let mut failed = vec![];
    for flow_file in &flow_files {
        match parse_file(flow_file) {
            Ok(flow) => {
                if let Err(e) = Validator::validate(&flow) {
                    failed.push(format!("{}: validation failed - {}", flow_file, e));
                }
            }
            Err(e) => {
                failed.push(format!("{}: parse failed - {}", flow_file, e));
            }
        }
    }

    if !failed.is_empty() {
        panic!("Flow validation failures:\n{}", failed.join("\n"));
    }
}
