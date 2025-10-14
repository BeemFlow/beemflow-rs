//! Utility functions and helpers
//!
//! Common utilities used throughout BeemFlow.

use once_cell::sync::Lazy;
use regex::Regex;

use crate::config::{BlobConfig, Config};
use crate::storage::SqliteStorage;
use std::sync::Arc;
use tempfile::TempDir;

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

/// Test environment with isolated temporary directories (test builds only)
///
/// This struct provides a complete, isolated test environment that mirrors production:
/// - Temporary `.beemflow` directory (auto-cleaned on drop)
/// - SQLite database in the temp directory
/// - Config pointing to temp directories
/// - Complete Dependencies object ready to use
///
/// # Example
///
/// ```no_run
/// use beemflow::utils::TestEnvironment;
/// use beemflow::core::OperationRegistry;
///
/// #[tokio::test]
/// async fn my_test() {
///     let env = TestEnvironment::new().await;
///     let registry = OperationRegistry::new(env.deps);
///     // Cleanup happens automatically when env drops
/// }
/// ```
pub struct TestEnvironment {
    /// Temporary directory - kept alive for test duration
    /// When dropped, all temp files are automatically cleaned up
    _temp_dir: TempDir,

    /// Complete dependencies object ready to use in tests
    pub deps: crate::core::Dependencies,
}

impl TestEnvironment {
    /// Create a new isolated test environment
    ///
    /// Sets up:
    /// - Temporary root directory (auto-deleted on drop)
    /// - `.beemflow/` subdirectory
    /// - `.beemflow/flows/` subdirectory
    /// - `.beemflow/beemflow.db` SQLite database
    /// - Config pointing to these locations
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use beemflow::utils::TestEnvironment;
    /// # use beemflow::core::OperationRegistry;
    /// # async fn example() {
    /// let env = TestEnvironment::new().await;
    /// let registry = OperationRegistry::new(env.deps);
    /// // All dependencies available via env.deps.*
    /// # }
    /// ```
    pub async fn new() -> Self {
        Self::with_db_name("beemflow.db").await
    }

    /// Create a test environment with a custom database name
    ///
    /// Useful when you need multiple isolated environments in the same test
    pub async fn with_db_name(db_name: &str) -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let beemflow_dir = temp_dir.path().join(".beemflow");

        // Directories will be auto-created by SqliteStorage, FilesystemBlobStore, and save_flow
        let deps = crate::core::Dependencies {
            storage: Arc::new(
                SqliteStorage::new(beemflow_dir.join(db_name).to_str().unwrap())
                    .await
                    .expect("Failed to create SQLite storage"),
            ),
            engine: Arc::new(crate::engine::Engine::for_testing().await),
            registry_manager: Arc::new(crate::registry::RegistryManager::standard(None)),
            event_bus: Arc::new(crate::event::InProcEventBus::new()),
            config: Arc::new(Config {
                flows_dir: Some(beemflow_dir.join("flows").to_str().unwrap().to_string()),
                blob: Some(BlobConfig {
                    driver: Some("filesystem".to_string()),
                    bucket: None,
                    directory: Some(beemflow_dir.join("files").to_str().unwrap().to_string()),
                }),
                ..Default::default()
            }),
        };

        TestEnvironment {
            _temp_dir: temp_dir,
            deps,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_environment_creates_structure() {
        let env = TestEnvironment::new().await;

        // Verify storage is functional
        env.deps
            .storage
            .deploy_flow_version("test_flow", "1.0.0", "content")
            .await
            .expect("Should be able to write to database");

        let content = env
            .deps
            .storage
            .get_flow_version_content("test_flow", "1.0.0")
            .await
            .expect("Should be able to read from database");

        assert_eq!(content, Some("content".to_string()));
    }

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
