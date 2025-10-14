use super::{parse_string, render_template, load_flow};
use std::collections::HashMap;

#[test]
fn test_parse_hello_world() {
    let yaml = r#"
name: hello
description: Hello world flow
on: cli.manual
steps:
  - id: greet
use: core.echo
with:
  text: "Hello, world!"
"#;
    
    let flow = parse_string(yaml, None).unwrap();
    assert_eq!(flow.name, "hello");
    assert_eq!(flow.steps.len(), 1);
    assert_eq!(flow.steps[0].id, "greet");
    assert!(flow.steps[0].use_.is_some());
}

#[test]
fn test_parse_with_vars() {
    let yaml = r#"
name: test_vars
on: cli.manual
vars:
  API_URL: "https://api.example.com"
  TIMEOUT: 30
steps:
  - id: step1
use: http.fetch
with:
  url: "{{ vars.API_URL }}"
"#;
    
    let flow = parse_string(yaml, None).unwrap();
    assert_eq!(flow.name, "test_vars");
    assert!(flow.vars.is_some());
    let vars = flow.vars.unwrap();
    assert!(vars.contains_key("API_URL"));
}

#[test]
fn test_parse_parallel() {
    let yaml = r#"
name: test_parallel
on: cli.manual
steps:
  - id: parallel_block
parallel: true
steps:
  - id: task1
    use: core.echo
    with:
      text: "Task 1"
  - id: task2
    use: core.echo
    with:
      text: "Task 2"
"#;
    
    let flow = parse_string(yaml, None).unwrap();
    assert_eq!(flow.steps.len(), 1);
    assert_eq!(flow.steps[0].parallel, Some(true));
    assert!(flow.steps[0].steps.is_some());
    assert_eq!(flow.steps[0].steps.as_ref().unwrap().len(), 2);
}

#[test]
fn test_render() {
    use serde_json::json;
    
    let template = r#"name: {{ vars.flow_name }}
on: cli.manual
vars:
  api_url: {{ vars.api_url }}
steps:
  - id: step1
use: core.echo
with:
  text: "Hello {{ vars.name }}"
"#;
    
    let mut vars = HashMap::new();
    vars.insert("flow_name".to_string(), json!("test_flow"));
    vars.insert("api_url".to_string(), json!("https://api.example.com"));
    vars.insert("name".to_string(), json!("World"));
    
    let rendered = render_template(template, vars).unwrap();
    
    assert!(rendered.contains("name: test_flow"));
    assert!(rendered.contains("api_url: https://api.example.com"));
    assert!(rendered.contains("Hello World"));
}

#[test]
fn test_load_with_validation() {
    use tempfile::NamedTempFile;
    use std::io::Write;
    
    let yaml = r#"name: test_load
on: cli.manual
steps:
  - id: step1
use: core.echo
with:
  text: "Test"
"#;
    
    // Create temp file
    let mut temp_file = NamedTempFile::new().unwrap();
    write!(temp_file, "{}", yaml).unwrap();
    
    // Load and validate
    let flow = load_flow(temp_file.path(), HashMap::new(, None)).unwrap();
    assert_eq!(flow.name, "test_load");
    assert_eq!(flow.steps.len(), 1);
}

#[test]
fn test_load_with_template_vars() {
    use tempfile::NamedTempFile;
    use std::io::Write;
    use serde_json::json;
    
    let yaml = r#"name: {{ vars.name }}
on: cli.manual
steps:
  - id: step1
use: core.echo
with:
  text: "{{ vars.message }}"
"#;
    
    // Create temp file
    let mut temp_file = NamedTempFile::new().unwrap();
    write!(temp_file, "{}", yaml).unwrap();
    
    // Load with vars
    let mut vars = HashMap::new();
    vars.insert("name".to_string(), json!("templated_flow"));
    vars.insert("message".to_string(), json!("Hello from template"));
    
    let flow = load_flow(temp_file.path(), vars, None).unwrap();
    assert_eq!(flow.name, "templated_flow");
}

#[test]
fn test_parse_foreach() {
    let yaml = r#"
name: test_foreach
on: cli.manual
vars:
  items: ["a", "b", "c"]
steps:
  - id: process_items
foreach: "{{ vars.items }}"
as: item
do:
  - id: process
    use: core.echo
    with:
      text: "Processing {{ item }}"
"#;
    
    let flow = parse_string(yaml, None).unwrap();
    assert_eq!(flow.steps.len(), 1);
    assert!(flow.steps[0].foreach.is_some());
    assert!(flow.steps[0].as_.is_some());
    assert!(flow.steps[0].do_.is_some());
}

#[test]
fn test_parse_conditional() {
    let yaml = r#"
name: test_conditional
on: cli.manual
steps:
  - id: conditional_step
if: "{{ vars.enabled == true }}"
use: core.echo
with:
  text: "Enabled"
"#;
    
    let flow = parse_string(yaml, None).unwrap();
    assert_eq!(flow.steps.len(), 1);
    assert!(flow.steps[0].if_.is_some());
}

#[test]
fn test_parse_with_catch() {
    let yaml = r#"
name: test_catch
on: cli.manual
steps:
  - id: risky_step
use: might.fail
with:
  data: "test"
catch:
  - id: handle_error
use: core.echo
with:
  text: "Error handled"
"#;
    
    let flow = parse_string(yaml, None).unwrap();
    assert!(flow.catch.is_some());
    assert_eq!(flow.catch.as_ref().unwrap().len(), 1);
}

#[test]
fn test_parse_with_retry() {
    let yaml = r#"
name: test_retry
on: cli.manual
steps:
  - id: flaky_step
use: external.api
retry:
  attempts: 3
  delay_sec: 5
with:
  url: "https://api.example.com"
"#;
    
    let flow = parse_string(yaml, None).unwrap();
    assert!(flow.steps[0].retry.is_some());
    let retry = flow.steps[0].retry.as_ref().unwrap();
    assert_eq!(retry.attempts, 3);
    assert_eq!(retry.delay_sec, 5);
}

#[test]
fn test_parse_await_event() {
    let yaml = r#"
name: test_await
on: cli.manual
steps:
  - id: wait_for_approval
await_event:
  source: "slack"
  match:
    token: "approval_123"
  timeout: "1h"
"#;
    
    let flow = parse_string(yaml, None).unwrap();
    assert!(flow.steps[0].await_event.is_some());
    let await_spec = flow.steps[0].await_event.as_ref().unwrap();
    assert_eq!(await_spec.source, "slack");
    assert_eq!(await_spec.timeout, Some("1h".to_string()));
}

#[test]
fn test_parse_wait() {
    let yaml = r#"
name: test_wait
on: cli.manual
steps:
  - id: delay
wait:
  seconds: 30
"#;
    
    let flow = parse_string(yaml, None).unwrap();
    assert!(flow.steps[0].wait.is_some());
    let wait_spec = flow.steps[0].wait.as_ref().unwrap();
    assert_eq!(wait_spec.seconds, Some(30));
}

#[test]
fn test_parse_depends_on() {
    let yaml = r#"
name: test_deps
on: cli.manual
steps:
  - id: step1
use: core.echo
with:
  text: "First"
  - id: step2
depends_on: [step1]
use: core.echo
with:
  text: "Second"
"#;
    
    let flow = parse_string(yaml, None).unwrap();
    assert_eq!(flow.steps.len(), 2);
    assert!(flow.steps[1].depends_on.is_some());
    assert_eq!(flow.steps[1].depends_on.as_ref().unwrap(), &vec!["step1"]);
}
