use super::*;
use crate::adapter::{AdapterRegistry, CoreAdapter};
use crate::dsl::Templater;
use crate::engine::Executor;
use crate::event::EventBus;
use crate::model::Step;
use crate::storage::Storage;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

fn setup_executor() -> Executor {
    let adapters = Arc::new(AdapterRegistry::new());
    adapters.register(Arc::new(CoreAdapter::new()));
    let templater = Arc::new(Templater::new());
    let event_bus: Arc<dyn EventBus> = Arc::new(crate::event::InProcEventBus::new());
    let storage: Arc<dyn Storage> = Arc::new(crate::storage::MemoryStorage::new());

    Executor::new(adapters, templater, event_bus, storage, None, 1000)
}

#[tokio::test]
async fn test_evaluate_condition() {
    let executor = setup_executor();

    let mut vars = HashMap::new();
    vars.insert("status".to_string(), Value::String("active".to_string()));
    let step_ctx = StepContext::new(HashMap::new(), vars, HashMap::new());

    let result = executor
        .evaluate_condition("{{ status == 'active' }}", &step_ctx)
        .await
        .unwrap();
    assert!(result);

    // Note: Inequality testing works but requires proper filter syntax in Minijinja
}

#[tokio::test]
async fn test_parallel_block_execution() {
    let executor = setup_executor();
    let step_ctx = StepContext::new(HashMap::new(), HashMap::new(), HashMap::new());

    let step = Step {
        id: "parallel_test".to_string().into(),
        parallel: Some(true),
        steps: Some(vec![
            Step {
                id: "task1".to_string().into(),
                use_: Some("core.echo".to_string()),
                with: Some({
                    let mut map = HashMap::new();
                    map.insert("text".to_string(), Value::String("Task 1".to_string()));
                    map
                }),
                ..Default::default()
            },
            Step {
                id: "task2".to_string().into(),
                use_: Some("core.echo".to_string()),
                with: Some({
                    let mut map = HashMap::new();
                    map.insert("text".to_string(), Value::String("Task 2".to_string()));
                    map
                }),
                ..Default::default()
            },
        ]),
        ..Default::default()
    };

    let result = executor
        .execute_parallel_block(&step, &step_ctx, "parallel_test")
        .await;
    assert!(result.is_ok());

    // Verify outputs were set
    assert!(step_ctx.get_output("task1").is_some());
    assert!(step_ctx.get_output("task2").is_some());
}

#[tokio::test]
async fn test_parallel_block_with_error() {
    let executor = setup_executor();
    let step_ctx = StepContext::new(HashMap::new(), HashMap::new(), HashMap::new());

    let step = Step {
        id: "parallel_error".to_string().into(),
        parallel: Some(true),
        steps: Some(vec![
            Step {
                id: "good_task".to_string().into(),
                use_: Some("core.echo".to_string()),
                with: Some({
                    let mut map = HashMap::new();
                    map.insert("text".to_string(), Value::String("Good".to_string()));
                    map
                }),
                ..Default::default()
            },
            Step {
                id: "bad_task".to_string().into(),
                use_: Some("nonexistent.adapter".to_string()),
                with: Some(HashMap::new()),
                ..Default::default()
            },
        ]),
        ..Default::default()
    };

    let result = executor
        .execute_parallel_block(&step, &step_ctx, "parallel_error")
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_foreach_sequential() {
    let executor = setup_executor();

    let mut vars = HashMap::new();
    vars.insert(
        "items".to_string(),
        Value::Array(vec![
            Value::String("alpha".to_string()),
            Value::String("beta".to_string()),
            Value::String("gamma".to_string()),
        ]),
    );
    let step_ctx = StepContext::new(HashMap::new(), vars, HashMap::new());

    let step = Step {
        id: "foreach_seq".to_string().into(),
        foreach: Some("{{ vars.items }}".to_string()),
        as_: Some("item".to_string()),
        do_: Some(vec![Step {
            id: "process".to_string().into(),
            use_: Some("core.echo".to_string()),
            with: Some({
                let mut map = HashMap::new();
                map.insert(
                    "text".to_string(),
                    Value::String("Processing {{ item }}".to_string()),
                );
                map
            }),
            ..Default::default()
        }]),
        ..Default::default()
    };

    let result = executor
        .execute_foreach_block(&step, &step_ctx, "foreach_seq")
        .await;
    if let Err(ref e) = result {
        eprintln!("foreach_sequential error: {}", e);
    }
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_foreach_parallel() {
    let executor = setup_executor();

    let mut vars = HashMap::new();
    vars.insert(
        "items".to_string(),
        Value::Array(vec![
            Value::String("alpha".to_string()),
            Value::String("beta".to_string()),
        ]),
    );
    let step_ctx = StepContext::new(HashMap::new(), vars, HashMap::new());

    let step = Step {
        id: "foreach_par".to_string().into(),
        foreach: Some("{{ vars.items }}".to_string()),
        as_: Some("item".to_string()),
        parallel: Some(true),
        do_: Some(vec![Step {
            id: "process_{{ item_index }}".to_string().into(),
            use_: Some("core.echo".to_string()),
            with: Some({
                let mut map = HashMap::new();
                map.insert(
                    "text".to_string(),
                    Value::String("Parallel {{ item }}".to_string()),
                );
                map
            }),
            ..Default::default()
        }]),
        ..Default::default()
    };

    let result = executor
        .execute_foreach_block(&step, &step_ctx, "foreach_par")
        .await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_foreach_empty_list() {
    let executor = setup_executor();

    let mut vars = HashMap::new();
    vars.insert("items".to_string(), Value::Array(vec![]));
    let step_ctx = StepContext::new(HashMap::new(), vars, HashMap::new());

    let step = Step {
        id: "foreach_empty".to_string().into(),
        foreach: Some("{{ vars.items }}".to_string()),
        as_: Some("item".to_string()),
        do_: Some(vec![Step {
            id: "process".to_string().into(),
            use_: Some("core.echo".to_string()),
            with: Some(HashMap::new()),
            ..Default::default()
        }]),
        ..Default::default()
    };

    let result = executor
        .execute_foreach_block(&step, &step_ctx, "foreach_empty")
        .await;
    assert!(result.is_ok());

    // Output should be an empty map
    let output = step_ctx.get_output("foreach_empty");
    assert!(output.is_some());
}

#[tokio::test]
async fn test_retry_logic() {
    let executor = setup_executor();

    // Create a step that will fail and should not retry successfully
    let step = Step {
        id: "retry_test".to_string().into(),
        use_: Some("nonexistent.adapter".to_string()),
        with: Some(HashMap::new()),
        retry: Some(crate::model::RetrySpec {
            attempts: 3,
            delay_sec: 1,
        }),
        ..Default::default()
    };

    let step_ctx = StepContext::new(HashMap::new(), HashMap::new(), HashMap::new());
    let result = executor
        .execute_single_step(&step, &step_ctx, "retry_test")
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_conditional_skip() {
    let executor = setup_executor();

    let mut vars = HashMap::new();
    vars.insert("enabled".to_string(), Value::Bool(false));
    let step_ctx = StepContext::new(HashMap::new(), vars, HashMap::new());

    let step = Step {
        id: "conditional_step".to_string().into(),
        use_: Some("core.echo".to_string()),
        if_: Some("{{ enabled }}".to_string()),
        with: Some({
            let mut map = HashMap::new();
            map.insert(
                "text".to_string(),
                Value::String("Should not execute".to_string()),
            );
            map
        }),
        ..Default::default()
    };

    let result = executor
        .execute_single_step(&step, &step_ctx, "conditional_step")
        .await;
    assert!(result.is_ok());

    // Output should not exist since step was skipped
    assert!(step_ctx.get_output("conditional_step").is_none());
}

#[tokio::test]
async fn test_wait_seconds() {
    let executor = setup_executor();
    let _step_ctx = StepContext::new(HashMap::new(), HashMap::new(), HashMap::new());

    let step = Step {
        id: "wait_test".to_string().into(),
        wait: Some(crate::model::WaitSpec {
            seconds: Some(1),
            until: None,
        }),
        ..Default::default()
    };

    let start = std::time::Instant::now();
    let result = executor.execute_wait(&step).await;
    assert!(result.is_ok());
    let duration = start.elapsed();

    assert!(duration.as_secs() >= 1);
}
