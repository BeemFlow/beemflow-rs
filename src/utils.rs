//! Utility functions and helpers
//!
//! Common utilities used throughout BeemFlow.

use once_cell::sync::Lazy;
use regex::Regex;

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
