
use super::*;
use crate::dsl::Templater;
use std::collections::HashMap;
use serde_json::json;

#[test]
fn test_basic_template() {
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("name".to_string(), json!("BeemFlow"));
    
    let result = templater.render("Hello, {{ name }}!", &data).unwrap();
    assert_eq!(result, "Hello, BeemFlow!");
}

#[test]
fn test_nested_path() {
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("vars".to_string(), json!({
        "user": {
            "name": "Alice",
            "age": 30
        }
    }));
    
    let result = templater.render("Name: {{ vars.user.name }}", &data).unwrap();
    assert_eq!(result, "Name: Alice");
}

#[test]
fn test_array_access() {
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("items".to_string(), json!(["first", "second", "third"]));
    
    // minijinja uses bracket notation for array access
    let result = templater.render("{{ items[0] }}", &data).unwrap();
    assert_eq!(result, "first");
    
    let result2 = templater.render("{{ items[2] }}", &data).unwrap();
    assert_eq!(result2, "third");
}

#[test]
fn test_evaluate_expression_simple() {
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("count".to_string(), json!(42));
    
    let result = templater.evaluate_expression("{{ count }}", &data).unwrap();
    assert_eq!(result, json!(42));
}

#[test]
fn test_evaluate_expression_array() {
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("items".to_string(), json!(["a", "b", "c"]));
    
    let result = templater.evaluate_expression("{{ items }}", &data).unwrap();
    assert_eq!(result, json!(["a", "b", "c"]));
}

#[test]
fn test_evaluate_expression_nested() {
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("data".to_string(), json!({
        "rows": [
            {"name": "Alice", "age": 30},
            {"name": "Bob", "age": 25}
        ]
    }));
    
    let result = templater.evaluate_expression("{{ data.rows }}", &data).unwrap();
    assert!(result.is_array());
    assert_eq!(result.as_array().unwrap().len(), 2);
}

#[test]
fn test_evaluate_expression_array_index() {
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("data".to_string(), json!({
        "rows": ["first", "second", "third"]
    }));
    
    let result = templater.evaluate_expression("{{ data.rows[0] }}", &data).unwrap();
    assert_eq!(result, json!("first"));
}

#[test]
fn test_upper_filter() {
    // Tests minijinja's built-in upper filter
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("text".to_string(), json!("hello"));

    let result = templater.render("{{ text | upper }}", &data).unwrap();
    assert_eq!(result, "HELLO");
}

#[test]
fn test_lower_filter() {
    // Tests minijinja's built-in lower filter
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("text".to_string(), json!("WORLD"));

    let result = templater.render("{{ text | lower }}", &data).unwrap();
    assert_eq!(result, "world");
}

#[test]
fn test_length_filter() {
    // Tests minijinja's built-in length filter
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("items".to_string(), json!(["a", "b", "c"]));

    let result = templater.render("{{ items | length }}", &data).unwrap();
    assert_eq!(result, "3");
}

#[test]
fn test_join_filter() {
    // Tests minijinja's built-in join filter (note: uses parentheses, not colon)
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("items".to_string(), json!(["a", "b", "c"]));

    let result = templater.render("{{ items | join(\", \") }}", &data).unwrap();
    assert_eq!(result, "a, b, c");
}

#[test]
fn test_default_filter() {
    // Tests minijinja's built-in default filter
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("empty".to_string(), json!(""));
    data.insert("value".to_string(), json!("hello"));

    let result1 = templater.render("{{ empty | default('fallback') }}", &data).unwrap();
    assert_eq!(result1, "fallback");

    let result2 = templater.render("{{ value | default('fallback') }}", &data).unwrap();
    assert_eq!(result2, "hello");
}

#[test]
fn test_default_with_or_operator() {
    // Tests minijinja's built-in 'or' operator (preferred over default filter)
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("empty".to_string(), json!(null));
    data.insert("value".to_string(), json!("hello"));

    let result1 = templater.render("{{ empty or 'fallback' }}", &data).unwrap();
    assert_eq!(result1, "fallback");

    let result2 = templater.render("{{ value or 'fallback' }}", &data).unwrap();
    assert_eq!(result2, "hello");
}

#[test]
fn test_conditionals() {
    // Tests minijinja's built-in conditional statements (Jinja2/Django-style)
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("status".to_string(), json!("active"));
    data.insert("count".to_string(), json!(10));

    let result1 = templater.render(
        "{% if status == 'active' %}Active{% endif %}",
        &data
    ).unwrap();
    assert_eq!(result1, "Active");

    let result2 = templater.render(
        "{% if count > 5 %}Many{% else %}Few{% endif %}",
        &data
    ).unwrap();
    assert_eq!(result2, "Many");
}

#[test]
fn test_for_loop() {
    // Tests minijinja's built-in for loop with loop.last variable
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("items".to_string(), json!(["a", "b", "c"]));

    let result = templater.render(
        "{% for item in items %}{{ item }}{% if not loop.last %}, {% endif %}{% endfor %}",
        &data
    ).unwrap();
    assert_eq!(result, "a, b, c");
}

#[test]
fn test_scoped_access() {
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("vars".to_string(), json!({"name": "BeemFlow"}));
    data.insert("env".to_string(), json!({"USER": "alice"}));
    data.insert("secrets".to_string(), json!({"API_KEY": "secret123"}));
    data.insert("outputs".to_string(), json!({"step1": {"result": "success"}}));
    
    let result1 = templater.render("{{ vars.name }}", &data).unwrap();
    assert_eq!(result1, "BeemFlow");
    
    let result2 = templater.render("{{ env.USER }}", &data).unwrap();
    assert_eq!(result2, "alice");
    
    let result3 = templater.render("{{ secrets.API_KEY }}", &data).unwrap();
    assert_eq!(result3, "secret123");
    
    let result4 = templater.render("{{ outputs.step1.result }}", &data).unwrap();
    assert_eq!(result4, "success");
}

#[test]
fn test_complex_nested_access() {
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("data".to_string(), json!({
        "rows": [
            {"name": "Alice", "emails": ["alice@example.com", "alice@work.com"]},
            {"name": "Bob", "emails": ["bob@example.com"]}
        ]
    }));
    
    // Access nested array within array - use bracket notation
    let result = templater.render("{{ data.rows[0].emails[0] }}", &data).unwrap();
    assert_eq!(result, "alice@example.com");
}

#[test]
fn test_arithmetic() {
    // Tests minijinja's built-in arithmetic operators
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("a".to_string(), json!(5));
    data.insert("b".to_string(), json!(10));

    let result1 = templater.render("{{ a + b }}", &data).unwrap();
    assert_eq!(result1, "15");

    let result2 = templater.render("{{ b - a }}", &data).unwrap();
    assert_eq!(result2, "5");

    let result3 = templater.render("{{ a * b }}", &data).unwrap();
    assert_eq!(result3, "50");
}

#[test]
fn test_comparison_operators() {
    // Tests minijinja's built-in comparison operators
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("a".to_string(), json!(5));
    data.insert("b".to_string(), json!(10));

    let result1 = templater.render(
        "{% if a < b %}Less{% endif %}",
        &data
    ).unwrap();
    assert_eq!(result1, "Less");

    let result2 = templater.render(
        "{% if b > a %}Greater{% endif %}",
        &data
    ).unwrap();
    assert_eq!(result2, "Greater");

    let result3 = templater.render(
        "{% if a == 5 %}Equal{% endif %}",
        &data
    ).unwrap();
    assert_eq!(result3, "Equal");
}

#[test]
fn test_logical_operators() {
    // Tests minijinja's built-in logical operators (and, or, not)
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("status".to_string(), json!("active"));
    data.insert("count".to_string(), json!(10));

    let result1 = templater.render(
        "{% if status == 'active' and count > 5 %}Both{% endif %}",
        &data
    ).unwrap();
    assert_eq!(result1, "Both");

    let result2 = templater.render(
        "{% if status == 'inactive' or count > 5 %}Either{% endif %}",
        &data
    ).unwrap();
    assert_eq!(result2, "Either");

    let result3 = templater.render(
        "{% if not (status == 'inactive') %}Not{% endif %}",
        &data
    ).unwrap();
    assert_eq!(result3, "Not");
}

#[test]
fn test_default_operator_with_or() {
    // Tests minijinja's built-in 'or' operator for defaults (native syntax)
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("empty".to_string(), json!(null));
    data.insert("value".to_string(), json!("hello"));

    let result1 = templater.render("{{ empty or 'fallback' }}", &data).unwrap();
    assert_eq!(result1, "fallback");

    let result2 = templater.render("{{ value or 'fallback' }}", &data).unwrap();
    assert_eq!(result2, "hello");
}

#[test]
fn test_array_access_brackets() {
    // Tests minijinja's bracket notation for array access (NOT dot notation)
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("items".to_string(), json!(["first", "second", "third"]));
    data.insert("data".to_string(), json!({
        "rows": ["a", "b", "c"]
    }));

    let result1 = templater.render("{{ items[0] }}", &data).unwrap();
    assert_eq!(result1, "first");

    let result2 = templater.render("{{ items[2] }}", &data).unwrap();
    assert_eq!(result2, "third");

    let result3 = templater.render("{{ data.rows[1] }}", &data).unwrap();
    assert_eq!(result3, "b");
}

#[test]
fn test_complex_expression() {
    let templater = Templater::new();
    let mut data = HashMap::new();
    data.insert("values".to_string(), json!([10, 20, 30]));
    data.insert("default_val".to_string(), json!(null));

    // Test complex expression with or operator and array access
    let result = templater.render("{{ values[0] or 'none' }}", &data).unwrap();
    assert_eq!(result, "10");

    let result2 = templater.render("{{ default_val or values[1] }}", &data).unwrap();
    assert_eq!(result2, "20");
}
}
