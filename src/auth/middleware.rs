//! OAuth middleware for authentication, authorization, and rate limiting
//!
//! Provides type-safe extractors and middleware leveraging Rust's trait system
//! for production-grade OAuth security.

use crate::model::OAuthToken;
use crate::storage::Storage;
use crate::{BeemFlowError, Result};
use axum::{
    extract::{FromRequestParts, Request},
    http::{StatusCode, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use chrono::Utc;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration as StdDuration, SystemTime};

/// Authenticated user extracted from valid Bearer token
#[derive(Debug, Clone)]
pub struct AuthenticatedUser {
    pub user_id: String,
    pub client_id: String,
    pub scopes: Vec<String>,
    pub token: OAuthToken,
}

/// Required scopes for an endpoint
#[derive(Debug, Clone)]
pub struct RequiredScopes(pub Vec<String>);

impl RequiredScopes {
    pub fn any(scopes: &[&str]) -> Self {
        Self(scopes.iter().map(|s| s.to_string()).collect())
    }

    pub fn all(scopes: &[&str]) -> Self {
        Self(scopes.iter().map(|s| s.to_string()).collect())
    }
}

/// Scope validation strategy
pub trait ScopeValidator: Send + Sync {
    /// Check if provided scopes satisfy requirements
    fn validate(&self, provided: &[String], required: &RequiredScopes) -> bool;
}

/// Require ANY of the specified scopes
pub struct AnyScopeValidator;

impl ScopeValidator for AnyScopeValidator {
    fn validate(&self, provided: &[String], required: &RequiredScopes) -> bool {
        required.0.iter().any(|req| provided.contains(req))
    }
}

/// Require ALL of the specified scopes
pub struct AllScopesValidator;

impl ScopeValidator for AllScopesValidator {
    fn validate(&self, provided: &[String], required: &RequiredScopes) -> bool {
        required.0.iter().all(|req| provided.contains(req))
    }
}

/// State for OAuth middleware
#[derive(Clone)]
pub struct OAuthMiddlewareState {
    pub storage: Arc<dyn Storage>,
    pub rate_limiter: Arc<RwLock<HashMap<String, Vec<SystemTime>>>>,
    pub rate_limit_requests: usize,
    pub rate_limit_window: StdDuration,
}

impl OAuthMiddlewareState {
    pub fn new(storage: Arc<dyn Storage>) -> Self {
        Self {
            storage,
            rate_limiter: Arc::new(RwLock::new(HashMap::new())),
            rate_limit_requests: 100,
            rate_limit_window: StdDuration::from_secs(60),
        }
    }

    pub fn with_rate_limit(mut self, requests: usize, window: StdDuration) -> Self {
        self.rate_limit_requests = requests;
        self.rate_limit_window = window;
        self
    }
}

/// Extractor for authenticated user from Bearer token
impl<S> FromRequestParts<S> for AuthenticatedUser
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, String);

    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl std::future::Future<Output = std::result::Result<Self, Self::Rejection>> + Send {
        // Extract data from parts before moving into async block
        let oauth_state = parts.extensions.get::<OAuthMiddlewareState>().cloned();

        let token_result: std::result::Result<String, (StatusCode, String)> = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or((
                StatusCode::UNAUTHORIZED,
                "Missing Authorization header".to_string(),
            ))
            .and_then(|auth_header| {
                auth_header
                    .strip_prefix("Bearer ")
                    .map(|s| s.to_string())
                    .ok_or((
                        StatusCode::UNAUTHORIZED,
                        "Invalid Authorization header format".to_string(),
                    ))
            });

        async move {
            // Get OAuth middleware state from extensions (set by middleware)
            let oauth_state = oauth_state.ok_or((
                StatusCode::INTERNAL_SERVER_ERROR,
                "OAuth middleware not configured".to_string(),
            ))?;

            let token = token_result?;

            // Validate token against storage
            let oauth_token = oauth_state
                .storage
                .get_oauth_token_by_access(&token)
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        format!("Storage error: {}", e),
                    )
                })?
                .ok_or((
                    StatusCode::UNAUTHORIZED,
                    "Invalid or expired token".to_string(),
                ))?;

            // Check token expiration
            if let (Some(created), Some(expires_in)) =
                (oauth_token.access_create_at, oauth_token.access_expires_in)
            {
                let expires_at = created
                    + chrono::Duration::from_std(expires_in).map_err(|_| {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            "Invalid duration".to_string(),
                        )
                    })?;

                if Utc::now() > expires_at {
                    return Err((StatusCode::UNAUTHORIZED, "Token expired".to_string()));
                }
            }

            // Parse scopes
            let scopes: Vec<String> = oauth_token
                .scope
                .split_whitespace()
                .map(String::from)
                .collect();

            Ok(AuthenticatedUser {
                user_id: oauth_token.user_id.clone(),
                client_id: oauth_token.client_id.clone(),
                scopes,
                token: oauth_token,
            })
        }
    }
}

/// Extractor for authenticated user with required scopes
pub struct AuthenticatedUserWithScopes<V: ScopeValidator = AnyScopeValidator> {
    pub user: AuthenticatedUser,
    _validator: std::marker::PhantomData<V>,
}

impl<V: ScopeValidator + Default> AuthenticatedUserWithScopes<V> {
    pub fn new(
        user: AuthenticatedUser,
        required: &RequiredScopes,
    ) -> std::result::Result<Self, (StatusCode, String)> {
        let validator = V::default();
        if validator.validate(&user.scopes, required) {
            Ok(Self {
                user,
                _validator: std::marker::PhantomData,
            })
        } else {
            Err((StatusCode::FORBIDDEN, "Insufficient scopes".to_string()))
        }
    }
}

impl Default for AnyScopeValidator {
    fn default() -> Self {
        Self
    }
}

impl Default for AllScopesValidator {
    fn default() -> Self {
        Self
    }
}

/// Middleware to inject OAuth state into request extensions
pub async fn oauth_middleware(req: Request, next: Next) -> Response {
    // OAuth state should be in app state, extract and add to extensions
    // This allows extractors to access it
    // Note: This is set up in the router configuration
    next.run(req).await
}

/// Rate limiting middleware
pub async fn rate_limit_middleware(
    req: Request,
    next: Next,
    oauth_state: Arc<OAuthMiddlewareState>,
) -> Response {
    // Extract client identifier (IP or OAuth client_id)
    let identifier = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(String::from)
        .unwrap_or_else(|| {
            req.headers()
                .get("X-Forwarded-For")
                .and_then(|v| v.to_str().ok())
                .unwrap_or("unknown")
                .to_string()
        });

    // Check rate limit (scope the lock tightly to avoid holding across await)
    let is_rate_limited = {
        let mut rate_limiter = oauth_state.rate_limiter.write();
        let now = SystemTime::now();
        let window_start = now - oauth_state.rate_limit_window;

        let requests = rate_limiter.entry(identifier.clone()).or_default();

        // Remove old requests outside the window
        requests.retain(|&time| time > window_start);

        // Check if limit exceeded
        if requests.len() >= oauth_state.rate_limit_requests {
            true
        } else {
            // Add current request
            requests.push(now);
            false
        }
    };

    if is_rate_limited {
        return (StatusCode::TOO_MANY_REQUESTS, "Rate limit exceeded").into_response();
    }

    next.run(req).await
}

/// Helper to extract Bearer token from request
pub fn extract_bearer_token(parts: &Parts) -> Option<String> {
    parts
        .headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(String::from)
}

/// Validate token and return authenticated user
pub async fn validate_token(storage: &Arc<dyn Storage>, token: &str) -> Result<AuthenticatedUser> {
    // Get token from storage
    let oauth_token = storage
        .get_oauth_token_by_access(token)
        .await?
        .ok_or_else(|| BeemFlowError::auth("Invalid or expired token"))?;

    // Check token expiration
    if let (Some(created), Some(expires_in)) =
        (oauth_token.access_create_at, oauth_token.access_expires_in)
    {
        let expires_at = created
            + chrono::Duration::from_std(expires_in)
                .map_err(|_| BeemFlowError::auth("Invalid duration"))?;

        if Utc::now() > expires_at {
            return Err(BeemFlowError::auth("Token expired"));
        }
    }

    // Parse scopes
    let scopes: Vec<String> = oauth_token
        .scope
        .split_whitespace()
        .map(String::from)
        .collect();

    Ok(AuthenticatedUser {
        user_id: oauth_token.user_id.clone(),
        client_id: oauth_token.client_id.clone(),
        scopes,
        token: oauth_token,
    })
}

/// Check if user has required scopes
pub fn has_scope(user: &AuthenticatedUser, scope: &str) -> bool {
    user.scopes.iter().any(|s| s == scope)
}

/// Check if user has any of the required scopes
pub fn has_any_scope(user: &AuthenticatedUser, scopes: &[&str]) -> bool {
    scopes.iter().any(|scope| has_scope(user, scope))
}

/// Check if user has all of the required scopes
pub fn has_all_scopes(user: &AuthenticatedUser, scopes: &[&str]) -> bool {
    scopes.iter().all(|scope| has_scope(user, scope))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_scope_validators() {
        let provided = vec!["read".to_string(), "write".to_string()];
        let required = RequiredScopes::any(&["read", "admin"]);

        let validator = AnyScopeValidator;
        assert!(validator.validate(&provided, &required));

        let required_all = RequiredScopes::all(&["read", "write"]);
        let validator_all = AllScopesValidator;
        assert!(validator_all.validate(&provided, &required_all));

        let required_missing = RequiredScopes::all(&["read", "admin"]);
        assert!(!validator_all.validate(&provided, &required_missing));
    }

    #[test]
    fn test_scope_helpers() {
        let user = AuthenticatedUser {
            user_id: "user123".to_string(),
            client_id: "client123".to_string(),
            scopes: vec!["read".to_string(), "write".to_string()],
            token: OAuthToken {
                id: "token123".to_string(),
                client_id: "client123".to_string(),
                user_id: "user123".to_string(),
                redirect_uri: "http://localhost".to_string(),
                scope: "read write".to_string(),
                code: None,
                code_create_at: None,
                code_expires_in: None,
                code_challenge: None,
                code_challenge_method: None,
                access: Some("access_token".to_string()),
                access_create_at: Some(Utc::now()),
                access_expires_in: Some(std::time::Duration::from_secs(3600)),
                refresh: None,
                refresh_create_at: None,
                refresh_expires_in: None,
            },
        };

        assert!(has_scope(&user, "read"));
        assert!(has_scope(&user, "write"));
        assert!(!has_scope(&user, "admin"));

        assert!(has_any_scope(&user, &["read", "admin"]));
        assert!(!has_any_scope(&user, &["admin", "delete"]));

        assert!(has_all_scopes(&user, &["read", "write"]));
        assert!(!has_all_scopes(&user, &["read", "admin"]));
    }
}
