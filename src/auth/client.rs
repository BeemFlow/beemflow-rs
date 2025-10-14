//! OAuth 2.0 client for connecting to external providers
//!
//! Manages OAuth credentials and token refresh for external services
//! like Google, Twitter, GitHub, etc.

use crate::model::OAuthCredential;
use crate::registry::RegistryManager;
use crate::storage::Storage;
use crate::{BeemFlowError, Result};
use chrono::{Duration, Utc};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, PkceCodeChallenge,
    PkceCodeVerifier, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl,
    basic::BasicClient,
};
use std::sync::Arc;
use uuid::Uuid;

/// OAuth provider configuration
#[derive(Debug, Clone)]
struct ProviderConfig {
    client_id: String,
    client_secret: String,
    auth_url: String,
    token_url: String,
}

/// OAuth client manager for handling tokens from external providers
///
/// This manages OAuth credentials and automatically refreshes expired tokens.
/// Providers can come from the registry (default.json) or be stored in the database.
#[derive(Clone)]
pub struct OAuthClientManager {
    storage: Arc<dyn Storage>,
    registry_manager: Arc<RegistryManager>,
    redirect_uri: String,
    http_client: reqwest::Client,
}

impl OAuthClientManager {
    /// Create a new OAuth client manager
    ///
    /// Returns an error if the HTTP client cannot be built (e.g., TLS initialization failure)
    pub fn new(
        storage: Arc<dyn Storage>,
        registry_manager: Arc<RegistryManager>,
        redirect_uri: String,
    ) -> Result<Self> {
        // Create HTTP client with security settings
        // Disable redirects to prevent authorization code interception
        let http_client = reqwest::ClientBuilder::new()
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| {
                BeemFlowError::config(format!("Failed to build HTTP client for OAuth: {}", e))
            })?;

        Ok(Self {
            storage,
            registry_manager,
            redirect_uri,
            http_client,
        })
    }

    /// Get OAuth provider from registry or storage
    ///
    /// First checks the registry (where default providers like Google, GitHub are defined),
    /// then falls back to storage for custom user-created providers.
    async fn get_provider(&self, provider_id: &str) -> Result<ProviderConfig> {
        // Try registry first (default providers with $env: variables expanded)
        if let Some(entry) = self
            .registry_manager
            .get_oauth_provider(provider_id)
            .await?
        {
            return Ok(ProviderConfig {
                client_id: entry.client_id.ok_or_else(|| {
                    BeemFlowError::auth(format!(
                        "OAuth provider '{}' missing client_id",
                        provider_id
                    ))
                })?,
                client_secret: entry.client_secret.ok_or_else(|| {
                    BeemFlowError::auth(format!(
                        "OAuth provider '{}' missing client_secret",
                        provider_id
                    ))
                })?,
                auth_url: entry.auth_url.ok_or_else(|| {
                    BeemFlowError::auth(format!(
                        "OAuth provider '{}' missing auth_url",
                        provider_id
                    ))
                })?,
                token_url: entry.token_url.ok_or_else(|| {
                    BeemFlowError::auth(format!(
                        "OAuth provider '{}' missing token_url",
                        provider_id
                    ))
                })?,
            });
        }

        // Fall back to storage for custom providers
        let provider = self
            .storage
            .get_oauth_provider(provider_id)
            .await?
            .ok_or_else(|| {
                BeemFlowError::auth(format!(
                    "OAuth provider '{}' not found in registry or storage",
                    provider_id
                ))
            })?;

        Ok(ProviderConfig {
            client_id: provider.client_id,
            client_secret: provider.client_secret,
            auth_url: provider.auth_url,
            token_url: provider.token_url,
        })
    }

    /// Build authorization URL for a provider using oauth2 crate
    ///
    /// Returns the URL to redirect the user to for authorization and the PKCE code verifier.
    ///
    /// # Parameters
    /// * `custom_state` - Optional custom state to send to OAuth provider. If provided,
    ///   this will be used instead of generating a random CSRF token. Use this to embed
    ///   session IDs or other context in the state parameter.
    ///
    /// # Returns
    /// `(auth_url, code_verifier)` - The caller is responsible for storing any CSRF tokens
    pub async fn build_auth_url(
        &self,
        provider_id: &str,
        scopes: &[&str],
        _integration: Option<&str>,
        custom_state: Option<String>,
    ) -> Result<(String, String)> {
        // Get provider configuration from registry or storage
        let config = self.get_provider(provider_id).await?;

        // Build OAuth client using oauth2 crate
        // Note: Can't extract this to a helper due to oauth2's typestate pattern
        let client = BasicClient::new(ClientId::new(config.client_id))
            .set_client_secret(ClientSecret::new(config.client_secret))
            .set_auth_uri(
                AuthUrl::new(config.auth_url)
                    .map_err(|e| BeemFlowError::auth(format!("Invalid auth URL: {}", e)))?,
            )
            .set_token_uri(
                TokenUrl::new(config.token_url)
                    .map_err(|e| BeemFlowError::auth(format!("Invalid token URL: {}", e)))?,
            )
            .set_redirect_uri(
                RedirectUrl::new(self.redirect_uri.clone())
                    .map_err(|e| BeemFlowError::auth(format!("Invalid redirect URI: {}", e)))?,
            );

        // Generate PKCE challenge
        let (pkce_challenge, pkce_verifier) = PkceCodeChallenge::new_random_sha256();

        // Build authorization URL with PKCE
        // Use custom state if provided, otherwise generate random CSRF token
        let auth_url = if let Some(custom) = custom_state {
            // Use custom state (e.g., "{csrf_token}:{session_id}")
            // The oauth2 crate's CsrfToken is just a wrapper around a string
            let (url, _) = client
                .authorize_url(|| CsrfToken::new(custom))
                .add_scopes(scopes.iter().map(|s| Scope::new(s.to_string())))
                .set_pkce_challenge(pkce_challenge)
                .url();
            url
        } else {
            // Generate random CSRF token (for flows that don't need custom state)
            let (url, _) = client
                .authorize_url(CsrfToken::new_random)
                .add_scopes(scopes.iter().map(|s| Scope::new(s.to_string())))
                .set_pkce_challenge(pkce_challenge)
                .url();
            url
        };

        Ok((auth_url.to_string(), pkce_verifier.secret().clone()))
    }

    /// Exchange authorization code for access token using oauth2 crate
    ///
    /// After user authorizes, exchange the authorization code for tokens
    /// and store the credential.
    pub async fn exchange_code(
        &self,
        provider_id: &str,
        code: &str,
        code_verifier: &str,
        integration: &str,
    ) -> Result<OAuthCredential> {
        // Get provider configuration from registry or storage
        let config = self.get_provider(provider_id).await?;

        // Build OAuth client using oauth2 crate
        // Note: Can't extract this to a helper due to oauth2's typestate pattern
        let client = BasicClient::new(ClientId::new(config.client_id))
            .set_client_secret(ClientSecret::new(config.client_secret))
            .set_auth_uri(
                AuthUrl::new(config.auth_url)
                    .map_err(|e| BeemFlowError::auth(format!("Invalid auth URL: {}", e)))?,
            )
            .set_token_uri(
                TokenUrl::new(config.token_url)
                    .map_err(|e| BeemFlowError::auth(format!("Invalid token URL: {}", e)))?,
            )
            .set_redirect_uri(
                RedirectUrl::new(self.redirect_uri.clone())
                    .map_err(|e| BeemFlowError::auth(format!("Invalid redirect URI: {}", e)))?,
            );

        // Exchange code for token with PKCE verifier using cached HTTP client
        let token_result = client
            .exchange_code(AuthorizationCode::new(code.to_string()))
            .set_pkce_verifier(PkceCodeVerifier::new(code_verifier.to_string()))
            .request_async(&self.http_client)
            .await
            .map_err(|e| BeemFlowError::auth(format!("Token exchange failed: {}", e)))?;

        // Extract token details
        let now = Utc::now();
        let expires_at = token_result
            .expires_in()
            .map(|duration| now + Duration::seconds(duration.as_secs() as i64));

        let credential = OAuthCredential {
            id: Uuid::new_v4().to_string(),
            provider: provider_id.to_string(),
            integration: integration.to_string(),
            access_token: token_result.access_token().secret().clone(),
            refresh_token: token_result.refresh_token().map(|t| t.secret().clone()),
            expires_at,
            scope: token_result.scopes().map(|scopes| {
                scopes
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join(" ")
            }),
            created_at: now,
            updated_at: now,
        };

        // Save credential
        self.storage.save_oauth_credential(&credential).await?;

        tracing::info!(
            "Successfully exchanged authorization code for {}:{}",
            provider_id,
            integration
        );

        Ok(credential)
    }

    /// Get a valid OAuth access token for the given provider and integration
    ///
    /// Automatically refreshes the token if it's expired.
    ///
    /// # Example
    /// ```no_run
    /// use beemflow::auth::OAuthClientManager;
    /// use beemflow::storage::MemoryStorage;
    /// use beemflow::registry::RegistryManager;
    /// use std::sync::Arc;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let storage = Arc::new(MemoryStorage::new());
    /// let registry_manager = Arc::new(RegistryManager::standard(None));
    /// let client = OAuthClientManager::new(
    ///     storage,
    ///     registry_manager,
    ///     "http://localhost:3000/oauth/callback".to_string()
    /// )?;
    ///
    /// let token = client.get_token("google", "sheets").await?;
    /// println!("Access token: {}", token);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn get_token(&self, provider: &str, integration: &str) -> Result<String> {
        let cred = self
            .storage
            .get_oauth_credential(provider, integration)
            .await
            .map_err(|e| {
                BeemFlowError::OAuth(format!(
                    "Failed to get OAuth credential for {}:{} - {}",
                    provider, integration, e
                ))
            })?
            .ok_or_else(|| {
                BeemFlowError::OAuth(format!(
                    "OAuth credential not found for {}:{}",
                    provider, integration
                ))
            })?;

        // Check if token needs refresh
        if Self::needs_refresh(&cred) {
            tracing::info!("Token expired for {}:{}, refreshing", provider, integration);

            let mut cred_mut = cred.clone();
            match self.refresh_token(&mut cred_mut).await {
                Ok(()) => {
                    tracing::info!(
                        "Successfully refreshed token for {}:{}",
                        provider,
                        integration
                    );
                    // Return the refreshed token
                    return Ok(cred_mut.access_token);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to refresh OAuth token for {}:{}: {}. Using expired token.",
                        provider,
                        integration,
                        e
                    );
                    // Continue with expired token rather than failing
                }
            }
        }

        Ok(cred.access_token)
    }

    /// Refresh an expired OAuth token using oauth2 crate
    ///
    /// Uses the refresh token to obtain a new access token from the provider.
    pub async fn refresh_token(&self, cred: &mut OAuthCredential) -> Result<()> {
        let refresh_token_str = cred.refresh_token.as_ref().ok_or_else(|| {
            BeemFlowError::OAuth(format!(
                "No refresh token available for {}:{}",
                cred.provider, cred.integration
            ))
        })?;

        // Get OAuth provider configuration from registry or storage
        let config = self.get_provider(&cred.provider).await.map_err(|e| {
            BeemFlowError::OAuth(format!(
                "Failed to get OAuth provider {}: {}",
                cred.provider, e
            ))
        })?;

        // Build OAuth client using oauth2 crate
        // Note: Can't extract this to a helper due to oauth2's typestate pattern
        let client = BasicClient::new(ClientId::new(config.client_id))
            .set_client_secret(ClientSecret::new(config.client_secret))
            .set_auth_uri(
                AuthUrl::new(config.auth_url)
                    .map_err(|e| BeemFlowError::auth(format!("Invalid auth URL: {}", e)))?,
            )
            .set_token_uri(
                TokenUrl::new(config.token_url)
                    .map_err(|e| BeemFlowError::auth(format!("Invalid token URL: {}", e)))?,
            )
            .set_redirect_uri(
                RedirectUrl::new(self.redirect_uri.clone())
                    .map_err(|e| BeemFlowError::auth(format!("Invalid redirect URI: {}", e)))?,
            );

        // Refresh the token using cached HTTP client
        let token_result = client
            .exchange_refresh_token(&RefreshToken::new(refresh_token_str.clone()))
            .request_async(&self.http_client)
            .await
            .map_err(|e| BeemFlowError::OAuth(format!("Token refresh failed: {}", e)))?;

        // Extract new token info
        let new_access_token = token_result.access_token().secret().clone();
        let new_expires_at = token_result
            .expires_in()
            .map(|duration| Utc::now() + Duration::seconds(duration.as_secs() as i64));

        // Update local credential object for return value
        cred.access_token = new_access_token.clone();
        if let Some(new_refresh) = token_result.refresh_token() {
            cred.refresh_token = Some(new_refresh.secret().clone());
        }
        cred.expires_at = new_expires_at;
        cred.updated_at = Utc::now();

        // Use storage's dedicated refresh method (more efficient than full save)
        self.storage
            .refresh_oauth_credential(&cred.id, &new_access_token, new_expires_at)
            .await
            .map_err(|e| {
                BeemFlowError::OAuth(format!("Failed to save refreshed credential: {}", e))
            })?;

        Ok(())
    }

    /// Check if a credential needs token refresh
    fn needs_refresh(cred: &OAuthCredential) -> bool {
        if let Some(expires_at) = cred.expires_at {
            // Add 5-minute buffer before expiry
            let buffer = Duration::minutes(5);
            Utc::now() + buffer >= expires_at
        } else {
            false
        }
    }
}

#[cfg(test)]
mod client_test {
    include!("client_test.rs");
}
