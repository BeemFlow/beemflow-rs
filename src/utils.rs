//! Utility functions and helpers
//!
//! Common utilities used throughout BeemFlow.

use once_cell::sync::Lazy;
use regex::Regex;
use tracing::{debug, error, info, warn};

/// Log a debug message
#[inline]
#[allow(dead_code)]
pub fn log_debug(msg: &str) {
    debug!("{}", msg);
}

/// Log an info message
#[inline]
#[allow(dead_code)]
pub fn log_info(msg: &str) {
    info!("{}", msg);
}

/// Log a warning
#[inline]
#[allow(dead_code)]
pub fn log_warn(msg: &str) {
    warn!("{}", msg);
}

/// Log an error
#[inline]
#[allow(dead_code)]
pub fn log_error(msg: &str) {
    error!("{}", msg);
}

/// Safe string reference for JSON values (zero-copy)
#[inline]
#[allow(dead_code)]
pub fn safe_str(val: &serde_json::Value) -> Option<&str> {
    val.as_str()
}

/// Safe string conversion for JSON values (allocates)
#[inline]
#[allow(dead_code)]
pub fn safe_string(val: &serde_json::Value) -> Option<String> {
    val.as_str().map(|s| s.to_string())
}

/// Safe map conversion for JSON values
#[inline]
#[allow(dead_code)]
pub fn safe_map(val: &serde_json::Value) -> Option<&serde_json::Map<String, serde_json::Value>> {
    val.as_object()
}

/// Expand environment variable references in config values
///
/// Supports `$env:VARNAME` syntax anywhere in the string.
///
/// # Examples
/// ```no_run
/// use beemflow::utils::expand_env_value;
///
/// // In real usage:
/// let value = expand_env_value("Bearer $env:API_KEY");
/// // If API_KEY=secret123, returns: "Bearer secret123"
/// // If API_KEY not set, returns: "Bearer $env:API_KEY"
/// ```
pub fn expand_env_value(value: &str) -> String {
    // Pattern matches $env:VARNAME format where VARNAME starts with letter/underscore
    // followed by alphanumeric/underscore characters
    static ENV_VAR_PATTERN: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r"\$env:([A-Za-z_][A-Za-z0-9_]*)").expect("Invalid environment variable regex")
    });

    ENV_VAR_PATTERN
        .replace_all(value, |caps: &regex::Captures| {
            let var_name = &caps[1];
            std::env::var(var_name).unwrap_or_else(|_| caps[0].to_string())
        })
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expand_env_value() {
        // Test simple replacement
        unsafe {
            std::env::set_var("TEST_VAR", "test_value");
        }
        assert_eq!(expand_env_value("$env:TEST_VAR"), "test_value");

        // Test within string
        assert_eq!(
            expand_env_value("Bearer $env:TEST_VAR"),
            "Bearer test_value"
        );

        // Test missing variable (unchanged)
        assert_eq!(expand_env_value("$env:MISSING_VAR"), "$env:MISSING_VAR");

        // Test multiple replacements
        unsafe {
            std::env::set_var("VAR1", "value1");
            std::env::set_var("VAR2", "value2");
        }
        assert_eq!(
            expand_env_value("$env:VAR1 and $env:VAR2"),
            "value1 and value2"
        );

        // Test literal string (no expansion)
        assert_eq!(expand_env_value("literal_value"), "literal_value");

        // Cleanup
        unsafe {
            std::env::remove_var("TEST_VAR");
            std::env::remove_var("VAR1");
            std::env::remove_var("VAR2");
        }
    }
}
