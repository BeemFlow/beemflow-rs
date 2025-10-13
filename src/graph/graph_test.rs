
use super::*;
use crate::model::{Flow, Step, Trigger};
use crate::graph::GraphGenerator;

#[test]
fn test_generate_simple_flow() {
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
                use_: Some("http.fetch".to_string()),
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
    assert!(diagram.contains("step1 --> step2"));
}

#[test]
fn test_generate_parallel_flow() {
    let flow = Flow {
        name: "test".to_string(),
        description: None,
        version: None,
        on: Some(Trigger::Single("cli.manual".to_string())),
        cron: None,
        vars: None,
        steps: vec![
            Step {
                id: "parallel_block".to_string(),
                use_: None,
                with: None,
                depends_on: None,
                parallel: Some(true),
                if_: None,
                foreach: None,
                as_: None,
                do_: None,
                steps: Some(vec![
                    Step {
                        id: "task1".to_string(),
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
                        id: "task2".to_string(),
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
                ]),
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
    assert!(diagram.contains("parallel_block"));
    assert!(diagram.contains("task1"));
    assert!(diagram.contains("task2"));
}
