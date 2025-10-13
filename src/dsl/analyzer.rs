//! Automatic dependency detection from template references
//!
//! This module analyzes flow definitions to automatically detect step dependencies
//! by scanning template strings for `{{ steps.X }}` references.
//!
//! ## Hybrid Approach
//!
//! Dependencies are detected from two sources:
//! 1. **Automatic**: Scanning templates for `{{ steps.step_id }}` patterns
//! 2. **Manual**: Explicit `depends_on` field in step definitions
//!
//! The analyzer merges both sources to create a complete dependency graph.

use crate::error::{BeemFlowError, Result};
use crate::model::{Flow, Step};
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};

/// Analyzes flows to detect and resolve step dependencies
pub struct DependencyAnalyzer {
    /// Regex to match {{ steps.step_id }} or {{ steps['step_id'] }}
    step_ref_regex: Regex,
}

impl DependencyAnalyzer {
    /// Create a new dependency analyzer
    pub fn new() -> Self {
        Self {
            // Matches various forms:
            // - {{ steps.foo }}
            // - {{ steps.foo.output }}
            // - {{ steps['foo'] }}
            // - {{ steps["foo"] }}
            step_ref_regex: Regex::new(
                r#"steps\.([a-zA-Z0-9_-]+)|steps\['([^']+)'\]|steps\["([^"]+)"\]"#
            ).expect("step reference regex is valid"),
        }
    }

    /// Extract all step ID references from a template string
    fn extract_step_refs(&self, template: &str) -> HashSet<String> {
        let mut refs = HashSet::new();

        for cap in self.step_ref_regex.captures_iter(template) {
            // Check each capture group (handles different syntaxes)
            if let Some(step_id) = cap.get(1) {
                refs.insert(step_id.as_str().to_string());
            } else if let Some(step_id) = cap.get(2) {
                refs.insert(step_id.as_str().to_string());
            } else if let Some(step_id) = cap.get(3) {
                refs.insert(step_id.as_str().to_string());
            }
        }

        refs
    }

    /// Recursively extract step references from a JSON value
    fn extract_refs_from_value(&self, value: &Value) -> HashSet<String> {
        let mut refs = HashSet::new();

        match value {
            Value::String(s) => {
                refs.extend(self.extract_step_refs(s));
            }
            Value::Array(arr) => {
                for item in arr {
                    refs.extend(self.extract_refs_from_value(item));
                }
            }
            Value::Object(obj) => {
                for v in obj.values() {
                    refs.extend(self.extract_refs_from_value(v));
                }
            }
            _ => {}
        }

        refs
    }

    /// Analyze a single step to find all template-based dependencies
    fn analyze_step(&self, step: &Step) -> HashSet<String> {
        let mut deps = HashSet::new();

        // Check 'if' condition
        if let Some(ref condition) = step.if_ {
            deps.extend(self.extract_step_refs(condition));
        }

        // Check 'with' parameters (recursively)
        if let Some(ref with) = step.with {
            for value in with.values() {
                deps.extend(self.extract_refs_from_value(value));
            }
        }

        // Check 'foreach' expression
        if let Some(ref foreach_expr) = step.foreach {
            deps.extend(self.extract_step_refs(foreach_expr));
        }

        // Recursively check nested steps (do blocks)
        if let Some(ref nested_steps) = step.do_ {
            for nested_step in nested_steps {
                deps.extend(self.analyze_step(nested_step));
            }
        }

        // Recursively check parallel blocks
        if let Some(ref parallel_steps) = step.steps {
            for parallel_step in parallel_steps {
                deps.extend(self.analyze_step(parallel_step));
            }
        }

        deps
    }

    /// Build dependency graph from automatic detection (template refs)
    fn build_automatic_deps(&self, flow: &Flow) -> HashMap<String, HashSet<String>> {
        let mut graph = HashMap::new();

        for step in &flow.steps {
            let deps = self.analyze_step(step);
            graph.insert(step.id.clone(), deps);
        }

        graph
    }

    /// Build dependency graph from manual depends_on fields
    fn build_manual_deps(&self, flow: &Flow) -> HashMap<String, HashSet<String>> {
        let mut graph = HashMap::new();

        for step in &flow.steps {
            let deps = if let Some(ref depends_on) = step.depends_on {
                depends_on.iter().cloned().collect()
            } else {
                HashSet::new()
            };
            graph.insert(step.id.clone(), deps);
        }

        graph
    }

    /// Merge automatic and manual dependencies
    fn merge_dependencies(
        &self,
        auto: HashMap<String, HashSet<String>>,
        manual: HashMap<String, HashSet<String>>,
    ) -> HashMap<String, HashSet<String>> {
        let mut merged = auto;

        for (step_id, manual_deps) in manual {
            merged
                .entry(step_id)
                .or_insert_with(HashSet::new)
                .extend(manual_deps);
        }

        merged
    }

    /// Build complete dependency graph (auto + manual)
    pub fn build_dependency_graph(&self, flow: &Flow) -> HashMap<String, HashSet<String>> {
        let auto_deps = self.build_automatic_deps(flow);
        let manual_deps = self.build_manual_deps(flow);
        self.merge_dependencies(auto_deps, manual_deps)
    }

    /// Validate that all referenced steps exist
    fn validate_references(
        &self,
        flow: &Flow,
        graph: &HashMap<String, HashSet<String>>,
    ) -> Result<()> {
        let all_steps: HashSet<String> = flow.steps.iter().map(|s| s.id.clone()).collect();

        for (step_id, deps) in graph {
            for dep in deps {
                if !all_steps.contains(dep) {
                    return Err(BeemFlowError::validation(format!(
                        "Step '{}' depends on non-existent step '{}'",
                        step_id, dep
                    )));
                }
            }
        }

        Ok(())
    }

    /// Detect circular dependencies using DFS
    fn detect_cycles(&self, graph: &HashMap<String, HashSet<String>>) -> Result<()> {
        let mut visited = HashSet::new();
        let mut rec_stack = HashSet::new();

        fn visit(
            node: &str,
            graph: &HashMap<String, HashSet<String>>,
            visited: &mut HashSet<String>,
            rec_stack: &mut HashSet<String>,
        ) -> Result<()> {
            if rec_stack.contains(node) {
                return Err(BeemFlowError::validation(format!(
                    "Circular dependency detected involving step '{}'",
                    node
                )));
            }

            if visited.contains(node) {
                return Ok(());
            }

            rec_stack.insert(node.to_string());

            if let Some(deps) = graph.get(node) {
                for dep in deps {
                    visit(dep, graph, visited, rec_stack)?;
                }
            }

            rec_stack.remove(node);
            visited.insert(node.to_string());

            Ok(())
        }

        for node in graph.keys() {
            if !visited.contains(node) {
                visit(node, graph, &mut visited, &mut rec_stack)?;
            }
        }

        Ok(())
    }

    /// Perform topological sort on steps based on dependencies
    ///
    /// Returns step IDs in execution order (dependencies before dependents)
    pub fn topological_sort(&self, flow: &Flow) -> Result<Vec<String>> {
        let graph = self.build_dependency_graph(flow);

        // Validate references
        self.validate_references(flow, &graph)?;

        // Detect cycles
        self.detect_cycles(&graph)?;

        // Perform topological sort using DFS
        let mut sorted = Vec::new();
        let mut visited = HashSet::new();

        fn visit(
            node: &str,
            graph: &HashMap<String, HashSet<String>>,
            visited: &mut HashSet<String>,
            sorted: &mut Vec<String>,
        ) {
            if visited.contains(node) {
                return;
            }

            visited.insert(node.to_string());

            // Visit dependencies first
            if let Some(deps) = graph.get(node) {
                for dep in deps {
                    visit(dep, graph, visited, sorted);
                }
            }

            // Add this node after its dependencies
            sorted.push(node.to_string());
        }

        // Visit all steps
        for step in &flow.steps {
            if !visited.contains(&step.id) {
                visit(&step.id, &graph, &mut visited, &mut sorted);
            }
        }

        Ok(sorted)
    }

    /// Find groups of steps that can run in parallel
    ///
    /// Returns a vector of groups, where each group contains step IDs
    /// that have no dependencies on each other and can execute concurrently.
    pub fn find_parallel_groups(&self, flow: &Flow) -> Result<Vec<Vec<String>>> {
        let sorted = self.topological_sort(flow)?;
        let graph = self.build_dependency_graph(flow);

        let mut groups: Vec<Vec<String>> = Vec::new();
        let mut completed = HashSet::new();

        for step_id in sorted {
            let deps = graph.get(&step_id).cloned().unwrap_or_default();

            // Check if all dependencies are satisfied
            let ready = deps.iter().all(|d| completed.contains(d));

            if !ready {
                // This shouldn't happen if topological sort is correct
                return Err(BeemFlowError::adapter(format!(
                    "Step '{}' not ready (unsatisfied dependencies)",
                    step_id
                )));
            }

            // Can this step run in parallel with the current group?
            if let Some(current_group) = groups.last_mut() {
                // Check if any step in current group is a dependency
                let conflicts_with_current = current_group.iter().any(|g| deps.contains(g));

                if !conflicts_with_current {
                    // No conflict - add to current group
                    current_group.push(step_id.clone());
                } else {
                    // Conflict - start new group
                    groups.push(vec![step_id.clone()]);
                }
            } else {
                // First group
                groups.push(vec![step_id.clone()]);
            }

            completed.insert(step_id);
        }

        Ok(groups)
    }
}

impl Default for DependencyAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn create_step(id: &str) -> Step {
        Step {
            id: id.to_string(),
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

        let mut step_a = create_step("a");
        let mut step_b = create_step("b");
        step_b.depends_on = Some(vec!["a".to_string()]);

        let flow = Flow {
            name: "test".to_string(),
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
            name: "test".to_string(),
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
            name: "test".to_string(),
            steps: vec![step_a, step_b],
            ..Default::default()
        };

        let result = analyzer.topological_sort(&flow);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Circular dependency"));
    }

    #[test]
    fn test_invalid_reference() {
        let analyzer = DependencyAnalyzer::new();

        let mut step = create_step("a");
        step.depends_on = Some(vec!["nonexistent".to_string()]);

        let flow = Flow {
            name: "test".to_string(),
            steps: vec![step],
            ..Default::default()
        };

        let result = analyzer.topological_sort(&flow);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("non-existent step"));
    }

    #[test]
    fn test_detect_template_refs_in_with_params() {
        let analyzer = DependencyAnalyzer::new();

        let step_a = create_step("step_a");

        let mut step_b = create_step("step_b");
        let mut with = HashMap::new();
        // This should auto-detect dependency on step_a
        with.insert("text".to_string(), json!("Result: {{ steps.step_a.output }}"));
        step_b.with = Some(with);

        let flow = Flow {
            name: "test".to_string(),
            steps: vec![step_b, step_a],  // Intentionally reversed
            ..Default::default()
        };

        let graph = analyzer.build_dependency_graph(&flow);
        println!("Graph: {:?}", graph);

        let deps_b = graph.get("step_b").unwrap();
        assert!(deps_b.contains("step_a"), "step_b should depend on step_a from template");

        let sorted = analyzer.topological_sort(&flow).unwrap();
        println!("Sorted: {:?}", sorted);
        assert_eq!(sorted, vec!["step_a", "step_b"], "step_a should run before step_b");
    }
}
