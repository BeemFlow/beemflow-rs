use crate::engine::{StepContext, context::is_valid_identifier};
use serde_json::Value;
use std::collections::HashMap;

#[test]
fn test_context_operations() {
    let ctx = StepContext::new(HashMap::new(), HashMap::new(), HashMap::new());

    ctx.set_output("test".to_string(), Value::String("value".to_string()));
    assert_eq!(ctx.get_output("test").unwrap().as_str().unwrap(), "value");

    ctx.set_var("var1".to_string(), Value::Number(42.into()));
    let snapshot = ctx.snapshot();
    assert_eq!(snapshot.vars.get("var1").unwrap().as_i64().unwrap(), 42);
}

#[test]
fn test_is_valid_identifier() {
    assert!(is_valid_identifier("valid_id"));
    assert!(is_valid_identifier("_private"));
    assert!(is_valid_identifier("Step123"));

    assert!(!is_valid_identifier(""));
    assert!(!is_valid_identifier("123start"));
    assert!(!is_valid_identifier("has-dash"));
    assert!(!is_valid_identifier("{{ template }}"));
}
