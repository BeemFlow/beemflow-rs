//! Tests for analyzer

use super::*;
use crate::model::{Flow, Step};
use serde_json::json;
use std::collections::{HashMap, HashSet};

fn create_step(id: &str) -> Step {
    Step {
        id: id.to_string().into(),
        ..Default::default()
    }
}

#[test]
fn test_extract_step_refs_dot_notation() {
    let analyzer = DependencyAnalyzer::new();
    let template = "Result: {{ steps.foo.output }}";
    let refs = analyzer.extract_step_refs(template);
    assert!(refs.contains("foo"));
}

#[test]
fn test_extract_step_refs_bracket_notation() {
    let analyzer = DependencyAnalyzer::new();
    let template = "Result: {{ steps['bar'].output }}";
    let refs = analyzer.extract_step_refs(template);
    assert!(refs.contains("bar"));
}

#[test]
fn test_extract_multiple_refs() {
    let analyzer = DependencyAnalyzer::new();
    let template = "{{ steps.a.x }} and {{ steps.b.y }} and {{ steps['c'].z }}";
    let refs = analyzer.extract_step_refs(template);
    assert_eq!(refs.len(), 3);
    assert!(refs.contains("a"));
    assert!(refs.contains("b"));
    assert!(refs.contains("c"));
}

#[test]
fn test_analyze_step_with_template() {
    let analyzer = DependencyAnalyzer::new();
    let mut step = create_step("test");
    let mut with = HashMap::new();
    with.insert("text".to_string(), json!("{{ steps.foo.output }}"));
    step.with = Some(with);

    let deps = analyzer.analyze_step(&step);
    assert!(deps.contains("foo"));
}

#[test]
fn test_merge_auto_and_manual_deps() {
    let analyzer = DependencyAnalyzer::new();

    let mut auto = HashMap::new();
    auto.insert("step_a".to_string(), {
        let mut set = HashSet::new();
        set.insert("step_b".to_string());
        set
    });

    let mut manual = HashMap::new();
    manual.insert("step_a".to_string(), {
        let mut set = HashSet::new();
        set.insert("step_c".to_string());
        set
    });

    let merged = analyzer.merge_dependencies(auto, manual);
    let step_a_deps = merged.get("step_a").unwrap();

    assert_eq!(step_a_deps.len(), 2);
    assert!(step_a_deps.contains("step_b"));
    assert!(step_a_deps.contains("step_c"));
}

#[test]
fn test_topological_sort_simple() {
    let analyzer = DependencyAnalyzer::new();

    let step_a = create_step("a");
    let mut step_b = create_step("b");
    step_b.depends_on = Some(vec!["a".to_string()]);

    let flow = Flow {
        name: "test".to_string().into(),
        steps: vec![step_b, step_a], // Intentionally out of order
        ..Default::default()
    };

    let sorted = analyzer.topological_sort(&flow).unwrap();
    assert_eq!(sorted, vec!["a", "b"]);
}

#[test]
fn test_topological_sort_with_template_deps() {
    let analyzer = DependencyAnalyzer::new();

    let step_a = create_step("a");

    let mut step_b = create_step("b");
    let mut with = HashMap::new();
    with.insert("text".to_string(), json!("{{ steps.a.output }}"));
    step_b.with = Some(with);

    let flow = Flow {
        name: "test".to_string().into(),
        steps: vec![step_b, step_a], // b before a in YAML
        ..Default::default()
    };

    let sorted = analyzer.topological_sort(&flow).unwrap();
    assert_eq!(sorted, vec!["a", "b"]); // But a runs first
}

#[test]
fn test_detect_circular_dependency() {
    let analyzer = DependencyAnalyzer::new();

    let mut step_a = create_step("a");
    step_a.depends_on = Some(vec!["b".to_string()]);

    let mut step_b = create_step("b");
    step_b.depends_on = Some(vec!["a".to_string()]);

    let flow = Flow {
        name: "test".to_string().into(),
        steps: vec![step_a, step_b],
        ..Default::default()
    };

    let result = analyzer.topological_sort(&flow);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("Circular dependency")
    );
}

#[test]
fn test_invalid_reference() {
    let analyzer = DependencyAnalyzer::new();

    let mut step = create_step("a");
    step.depends_on = Some(vec!["nonexistent".to_string()]);

    let flow = Flow {
        name: "test".to_string().into(),
        steps: vec![step],
        ..Default::default()
    };

    let result = analyzer.topological_sort(&flow);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("non-existent step")
    );
}

#[test]
fn test_detect_template_refs_in_with_params() {
    let analyzer = DependencyAnalyzer::new();

    let step_a = create_step("step_a");

    let mut step_b = create_step("step_b");
    let mut with = HashMap::new();
    // This should auto-detect dependency on step_a
    with.insert(
        "text".to_string(),
        json!("Result: {{ steps.step_a.output }}"),
    );
    step_b.with = Some(with);

    let flow = Flow {
        name: "test".to_string().into(),
        steps: vec![step_b, step_a], // Intentionally reversed
        ..Default::default()
    };

    let graph = analyzer.build_dependency_graph(&flow);
    println!("Graph: {:?}", graph);

    let deps_b = graph.get("step_b").unwrap();
    assert!(
        deps_b.contains("step_a"),
        "step_b should depend on step_a from template"
    );

    let sorted = analyzer.topological_sort(&flow).unwrap();
    println!("Sorted: {:?}", sorted);
    assert_eq!(
        sorted,
        vec!["step_a", "step_b"],
        "step_a should run before step_b"
    );
}
