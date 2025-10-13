//! Flow parser for YAML and JSON

use crate::dsl::{Templater, Validator};
use crate::{BeemFlowError, Flow, Result};
use std::collections::HashMap;
use std::path::Path;

/// Parse a flow from a file path
pub fn parse_file<P: AsRef<Path>>(path: P) -> Result<Flow> {
    let content = std::fs::read_to_string(path)?;
    parse_string(&content)
}

/// Parse a flow from a YAML string
pub fn parse_string(content: &str) -> Result<Flow> {
    Ok(serde_yaml::from_str(content)?)
}

/// Parse a flow from JSON
#[allow(dead_code)]
pub fn parse_json(content: &str) -> Result<Flow> {
    Ok(serde_json::from_str(content)?)
}

/// Load a flow: read, render with vars, parse, and validate
///
/// This is the one-stop function that combines all parsing steps:
/// 1. Read the file
/// 2. Render it with provided variables (template expansion)
/// 3. Parse the rendered YAML
/// 4. Validate the parsed flow
///
/// # Example
/// ```no_run
/// use beemflow::dsl::parser::load;
/// use std::collections::HashMap;
///
/// let vars = HashMap::new();
/// let flow = load("flow.yaml", vars).unwrap();
/// ```
pub fn load<P: AsRef<Path>>(path: P, vars: HashMap<String, serde_json::Value>) -> Result<Flow> {
    let content = std::fs::read_to_string(path)?;
    let rendered = render(&content, vars)?;
    let flow = parse_string(&rendered)?;
    Validator::validate(&flow)?;
    Ok(flow)
}

/// Render a template string with variables
///
/// Uses minijinja to expand template expressions before parsing.
/// This is useful for pre-rendering flow definitions with known variables.
///
/// # Example
/// ```no_run
/// use beemflow::dsl::parser::render;
/// use std::collections::HashMap;
/// use serde_json::json;
///
/// let template = "name: {{ name }}";
/// let mut vars = HashMap::new();
/// vars.insert("name".to_string(), json!("test_flow"));
///
/// let rendered = render(template, vars).unwrap();
/// assert!(rendered.contains("test_flow"));
/// ```
pub fn render(template: &str, vars: HashMap<String, serde_json::Value>) -> Result<String> {
    Templater::new()
        .render(template, &vars)
        .map_err(|e| BeemFlowError::validation(format!("Template rendering failed: {}", e)))
}
