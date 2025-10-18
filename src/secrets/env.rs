//! Environment variable based secrets provider
//!
//! This is the default and simplest secrets provider. It reads secrets directly
//! from environment variables, with .env file support via dotenvy.

use super::*;

/// Default secrets provider that reads from environment variables
///
/// This is the ONLY place in the entire codebase where:
/// - `dotenvy::dotenv()` is called
/// - `std::env::var()` and `std::env::vars()` are called
///
/// All other code must use the SecretsProvider trait.
///
/// ## Design
///
/// - **Zero-sized type**: No runtime overhead
/// - **Simple 1:1 mapping**: Environment variables directly become secrets
/// - **No filtering**: All environment variables are accessible
/// - **.env support**: Automatically loads .env file on creation
///
/// ## Usage
///
/// ```no_run
/// use beemflow::secrets::{SecretsProvider, EnvSecretsProvider};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let provider = EnvSecretsProvider::new();
/// let api_key = provider.get_secret("OPENAI_API_KEY").await?;
/// # Ok(())
/// # }
/// ```
pub struct EnvSecretsProvider;

impl EnvSecretsProvider {
    /// Create a new environment-based secrets provider
    ///
    /// This loads the .env file if present. This is the ONLY place where
    /// `dotenvy::dotenv()` is called.
    ///
    /// ## .env File Loading
    ///
    /// The .env file is loaded from the current directory or any parent directory.
    /// If no .env file is found, this is not an error - the provider will still
    /// work with system environment variables.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use beemflow::secrets::EnvSecretsProvider;
    ///
    /// // Loads .env file and makes all environment variables available as secrets
    /// let provider = EnvSecretsProvider::new();
    /// ```
    pub fn new() -> Self {
        // Load .env file - this is the ONLY place in the codebase where this is called
        // Failure is not an error - .env files are optional
        let _ = dotenvy::dotenv();

        Self
    }
}

impl Default for EnvSecretsProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl SecretsProvider for EnvSecretsProvider {
    async fn get_secret(&self, key: &str) -> Result<Option<String>> {
        // Simple: environment variables directly become secrets
        // This is the ONLY place where std::env::var() should be called
        Ok(std::env::var(key).ok())
    }

    async fn get_all_secrets(&self) -> Result<HashMap<String, String>> {
        // Return ALL environment variables
        // This is the ONLY place where std::env::vars() should be called
        Ok(std::env::vars().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_env_provider_get_secret() {
        unsafe {
            std::env::set_var("TEST_ENV_VAR", "test_value");
        }

        let provider = EnvSecretsProvider::new();
        let result = provider.get_secret("TEST_ENV_VAR").await.unwrap();

        assert_eq!(result, Some("test_value".to_string()));

        unsafe {
            std::env::remove_var("TEST_ENV_VAR");
        }
    }

    #[tokio::test]
    async fn test_env_provider_missing_secret() {
        let provider = EnvSecretsProvider::new();
        let result = provider.get_secret("NONEXISTENT_VAR").await.unwrap();

        assert_eq!(result, None);
    }

    #[tokio::test]
    async fn test_env_provider_get_all_secrets() {
        unsafe {
            std::env::set_var("TEST_VAR_1", "value1");
            std::env::set_var("TEST_VAR_2", "value2");
        }

        let provider = EnvSecretsProvider::new();
        let all_secrets = provider.get_all_secrets().await.unwrap();

        assert!(all_secrets.contains_key("TEST_VAR_1"));
        assert!(all_secrets.contains_key("TEST_VAR_2"));
        assert_eq!(all_secrets.get("TEST_VAR_1").unwrap(), "value1");
        assert_eq!(all_secrets.get("TEST_VAR_2").unwrap(), "value2");

        unsafe {
            std::env::remove_var("TEST_VAR_1");
            std::env::remove_var("TEST_VAR_2");
        }
    }

    #[tokio::test]
    async fn test_env_provider_default() {
        let provider = EnvSecretsProvider::default();
        unsafe {
            std::env::set_var("DEFAULT_TEST", "works");
        }

        let result = provider.get_secret("DEFAULT_TEST").await.unwrap();
        assert_eq!(result, Some("works".to_string()));

        unsafe {
            std::env::remove_var("DEFAULT_TEST");
        }
    }
}
