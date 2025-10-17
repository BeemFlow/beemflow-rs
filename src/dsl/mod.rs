//! DSL parsing, validation, and templating

pub mod analyzer;
pub mod template;
pub mod validator;

use crate::{BeemFlowError, Flow, Result};
use std::collections::HashMap;
use std::path::Path;

// Re-export main types
pub use analyzer::DependencyAnalyzer;
pub use template::Templater;
pub use validator::Validator;

/// Default maximum flow file size (10MB) - prevents memory exhaustion from large files
const DEFAULT_MAX_FLOW_FILE_SIZE: u64 = 10 * 1024 * 1024;

// ============================================================================
// Parser Functions (formerly parser.rs)
// ============================================================================

/// Validate file size before reading to prevent memory exhaustion
fn validate_file_size<P: AsRef<Path>>(path: P, max_size: u64) -> Result<()> {
    let metadata = std::fs::metadata(path.as_ref())?;
    let file_size = metadata.len();

    if file_size > max_size {
        return Err(BeemFlowError::validation(format!(
            "Flow file exceeds maximum size of {} MB ({} bytes)",
            max_size / (1024 * 1024),
            max_size
        )));
    }

    Ok(())
}

/// Parse a flow from a file path
///
/// # Arguments
/// * `path` - Path to the flow file
/// * `max_file_size` - Optional maximum file size in bytes (default: 10MB)
pub fn parse_file<P: AsRef<Path>>(path: P, max_file_size: Option<u64>) -> Result<Flow> {
    let max_size = max_file_size.unwrap_or(DEFAULT_MAX_FLOW_FILE_SIZE);
    validate_file_size(&path, max_size)?;
    let content = std::fs::read_to_string(path)?;
    parse_string(&content, Some(max_size))
}

/// Parse a flow from a YAML string
///
/// # Arguments
/// * `content` - YAML content to parse
/// * `max_size` - Optional maximum content size in bytes (default: 10MB)
pub fn parse_string(content: &str, max_size: Option<u64>) -> Result<Flow> {
    let size_limit = max_size.unwrap_or(DEFAULT_MAX_FLOW_FILE_SIZE);

    // Validate content size to prevent YAML bombs and memory exhaustion
    if content.len() > size_limit as usize {
        return Err(BeemFlowError::validation(format!(
            "YAML content exceeds maximum size of {} MB ({} bytes)",
            size_limit / (1024 * 1024),
            size_limit
        )));
    }

    Ok(serde_yaml::from_str(content)?)
}

/// Load a flow: read, render with vars, parse, and validate
///
/// This is the one-stop function that combines all parsing steps:
/// 1. Read the file
/// 2. Render it with provided variables (template expansion)
/// 3. Parse the rendered YAML
/// 4. Validate the parsed flow
///
/// # Arguments
/// * `path` - Path to the flow file
/// * `vars` - Template variables for rendering
/// * `max_file_size` - Optional maximum file size in bytes (default: 10MB)
///
/// # Example
/// ```no_run
/// use beemflow::dsl;
/// use std::collections::HashMap;
///
/// let vars = HashMap::new();
/// let flow = dsl::load_flow("flow.yaml", vars, None).unwrap();
/// ```
pub fn load_flow<P: AsRef<Path>>(
    path: P,
    vars: HashMap<String, serde_json::Value>,
    max_file_size: Option<u64>,
) -> Result<Flow> {
    let max_size = max_file_size.unwrap_or(DEFAULT_MAX_FLOW_FILE_SIZE);
    validate_file_size(&path, max_size)?;
    let content = std::fs::read_to_string(path)?;
    let rendered = render_template(&content, vars)?;
    let flow = parse_string(&rendered, Some(max_size))?;
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
/// use beemflow::dsl;
/// use std::collections::HashMap;
/// use serde_json::json;
///
/// let template = "name: {{ name }}";
/// let mut vars = HashMap::new();
/// vars.insert("name".to_string(), json!("test_flow"));
///
/// let rendered = dsl::render_template(template, vars).unwrap();
/// assert!(rendered.contains("test_flow"));
/// ```
pub fn render_template(template: &str, vars: HashMap<String, serde_json::Value>) -> Result<String> {
    Templater::new()
        .render(template, &vars)
        .map_err(|e| BeemFlowError::validation(format!("Template rendering failed: {}", e)))
}

#[cfg(test)]
mod analyzer_test;
#[cfg(test)]
mod template_test;
