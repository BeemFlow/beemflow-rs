//! OAuth 2.1 authorization server
//!
//! Implements a secure OAuth 2.1 server for provider integrations.
//! Supports PKCE, dynamic client registration, and secure token management.

use crate::model::*;
use crate::storage::Storage;
use axum::{
    Json, Router,
    extract::{Query, State},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::{Duration, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTime;
use uuid::Uuid;

/// OAuth server configuration
#[derive(Debug, Clone)]
pub struct OAuthConfig {
    pub issuer: String,
    pub client_id: Option<String>,
    pub client_secret: Option<String>,
    pub token_expiry: Duration,
    pub refresh_expiry: Duration,
    pub allow_localhost_redirects: bool,
}

impl Default for OAuthConfig {
    fn default() -> Self {
        Self {
            issuer: "http://localhost:3000".to_string(),
            client_id: None,
            client_secret: None,
            token_expiry: Duration::hours(1),
            refresh_expiry: Duration::days(30),
            allow_localhost_redirects: true,
        }
    }
}

/// OAuth server state
pub struct OAuthServerState {
    pub storage: Arc<dyn Storage>,
    pub config: OAuthConfig,
    pub rate_limiter: Arc<RwLock<HashMap<String, Vec<SystemTime>>>>,
}

/// Dynamic client registration request
#[derive(Debug, Deserialize)]
struct ClientRegistrationRequest {
    client_name: String,
    #[serde(default)]
    client_uri: Option<String>,
    #[serde(default)]
    logo_uri: Option<String>,
    redirect_uris: Vec<String>,
    #[serde(default)]
    grant_types: Vec<String>,
    #[serde(default)]
    response_types: Vec<String>,
    #[serde(default)]
    scope: Option<String>,
}

/// Client registration response
#[derive(Debug, Serialize)]
struct ClientRegistrationResponse {
    client_id: String,
    client_secret: String,
    client_name: String,
    redirect_uris: Vec<String>,
    grant_types: Vec<String>,
    response_types: Vec<String>,
    scope: String,
    client_id_issued_at: i64,
    client_secret_expires_at: i64,
}

/// Authorization request parameters
#[derive(Debug, Deserialize)]
struct AuthorizeRequest {
    client_id: String,
    redirect_uri: String,
    response_type: String,
    #[serde(default)]
    scope: Option<String>,
    #[serde(default)]
    state: Option<String>,
    #[serde(default)]
    code_challenge: Option<String>,
    #[serde(default)]
    code_challenge_method: Option<String>,
}

/// Token request parameters
#[derive(Debug, Deserialize)]
struct TokenRequest {
    grant_type: String,
    #[serde(default)]
    code: Option<String>,
    #[serde(default)]
    redirect_uri: Option<String>,
    #[serde(default)]
    _client_id: Option<String>,
    #[serde(default)]
    _client_secret: Option<String>,
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default)]
    code_verifier: Option<String>,
}

/// Token response
#[derive(Debug, Serialize)]
struct TokenResponse {
    access_token: String,
    token_type: String,
    expires_in: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    scope: Option<String>,
}

/// Create OAuth routes
pub fn create_oauth_routes(state: Arc<OAuthServerState>) -> Router {
    Router::new()
        .route(
            "/.well-known/oauth-authorization-server",
            get(handle_metadata_discovery),
        )
        .route("/oauth/register", post(handle_client_registration))
        .route("/oauth/authorize", get(handle_authorize))
        .route("/oauth/token", post(handle_token))
        .route("/oauth/revoke", post(handle_token_revocation))
        .route("/oauth/introspect", post(handle_token_introspection))
        .with_state(state)
}

/// Handle OAuth metadata discovery
async fn handle_metadata_discovery(
    State(state): State<Arc<OAuthServerState>>,
) -> impl IntoResponse {
    let metadata = serde_json::json!({
        "issuer": state.config.issuer,
        "authorization_endpoint": format!("{}/oauth/authorize", state.config.issuer),
        "token_endpoint": format!("{}/oauth/token", state.config.issuer),
        "response_types_supported": ["code"],
        "grant_types_supported": ["authorization_code", "refresh_token", "client_credentials"],
        "token_endpoint_auth_methods_supported": ["client_secret_basic", "client_secret_post"],
        "scopes_supported": ["mcp", "openid", "profile", "email"],
        "code_challenge_methods_supported": ["S256"],
        "registration_endpoint": format!("{}/oauth/register", state.config.issuer),
    });

    Json(metadata)
}

/// Handle dynamic client registration
async fn handle_client_registration(
    State(state): State<Arc<OAuthServerState>>,
    Json(req): Json<ClientRegistrationRequest>,
) -> Response {
    // Validate client name
    if req.client_name.is_empty() || req.client_name.len() > 100 {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "client_name must be between 1 and 100 characters",
        )
            .into_response();
    }

    // Validate redirect URIs
    if req.redirect_uris.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "redirect_uris is required",
        )
            .into_response();
    }

    for uri in &req.redirect_uris {
        if !is_valid_redirect_uri(uri, state.config.allow_localhost_redirects) {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                format!("Invalid redirect URI: {}", uri),
            )
                .into_response();
        }
    }

    // Generate credentials
    let client_id = Uuid::new_v4().to_string();
    let client_secret = generate_client_secret();

    // Default grant types and response types
    let grant_types = if req.grant_types.is_empty() {
        vec![
            "authorization_code".to_string(),
            "refresh_token".to_string(),
        ]
    } else {
        req.grant_types
    };

    let response_types = if req.response_types.is_empty() {
        vec!["code".to_string()]
    } else {
        req.response_types
    };

    let _scope = req.scope.unwrap_or_else(|| "mcp".to_string());

    // Create and save client
    let now = Utc::now();
    let client = OAuthClient {
        id: client_id.clone(),
        secret: client_secret.clone(),
        name: req.client_name.clone(),
        redirect_uris: req.redirect_uris.clone(),
        grant_types: grant_types.clone(),
        response_types: response_types.clone(),
        scope: _scope.clone(),
        client_uri: req.client_uri.clone(),
        logo_uri: req.logo_uri.clone(),
        created_at: now,
        updated_at: now,
    };

    // Save client to storage
    if let Err(e) = state.storage.save_oauth_client(&client).await {
        tracing::error!("Failed to save OAuth client: {}", e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "failed to save client",
        )
            .into_response();
    }

    let response = ClientRegistrationResponse {
        client_id,
        client_secret,
        client_name: req.client_name,
        redirect_uris: req.redirect_uris,
        grant_types,
        response_types,
        scope: _scope,
        client_id_issued_at: now.timestamp(),
        client_secret_expires_at: 0, // Never expires
    };

    Json(response).into_response()
}

/// Handle authorization request
async fn handle_authorize(
    State(state): State<Arc<OAuthServerState>>,
    Query(req): Query<AuthorizeRequest>,
) -> Response {
    // Validate client_id exists
    let client = match state.storage.get_oauth_client(&req.client_id).await {
        Ok(Some(c)) => c,
        Ok(None) => return (axum::http::StatusCode::BAD_REQUEST, "invalid_client").into_response(),
        Err(e) => {
            tracing::error!("Failed to get OAuth client: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "server_error",
            )
                .into_response();
        }
    };

    // Validate redirect_uri matches registered URIs
    if !client.redirect_uris.contains(&req.redirect_uri) {
        return (axum::http::StatusCode::BAD_REQUEST, "invalid_redirect_uri").into_response();
    }

    // Validate response_type
    if req.response_type != "code" {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "unsupported_response_type",
        )
            .into_response();
    }

    // Validate scope (default to "mcp" if not provided)
    let scope = req.scope.unwrap_or_else(|| "mcp".to_string());

    // Validate PKCE parameters if present
    let (code_challenge, code_challenge_method) = if let (Some(challenge), Some(method)) =
        (&req.code_challenge, &req.code_challenge_method)
    {
        if method != "S256" {
            return (axum::http::StatusCode::BAD_REQUEST, "invalid_request").into_response();
        }
        (Some(challenge.clone()), Some(method.clone()))
    } else {
        (None, None)
    };

    // For now, auto-approve for registered clients
    // In production, this would show a consent screen

    // Generate authorization code
    let code = generate_authorization_code();

    // Store authorization code with PKCE challenge, scope, and client info
    let token = OAuthToken {
        id: Uuid::new_v4().to_string(),
        client_id: req.client_id.clone(),
        user_id: "default_user".to_string(), // In production, get from authenticated session
        redirect_uri: req.redirect_uri.clone(),
        scope: scope.clone(),
        code: Some(code.clone()),
        code_create_at: Some(Utc::now()),
        code_expires_in: Some(std::time::Duration::from_secs(600)), // 10 minutes
        code_challenge,
        code_challenge_method,
        access: None,
        access_create_at: None,
        access_expires_in: None,
        refresh: None,
        refresh_create_at: None,
        refresh_expires_in: None,
    };

    if let Err(e) = state.storage.save_oauth_token(&token).await {
        tracing::error!("Failed to save authorization code: {}", e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            "server_error",
        )
            .into_response();
    }

    // Build redirect URL with state if provided
    let redirect_url = if let Some(state_param) = req.state {
        format!("{}?code={}&state={}", req.redirect_uri, code, state_param)
    } else {
        format!("{}?code={}", req.redirect_uri, code)
    };

    axum::response::Redirect::temporary(&redirect_url).into_response()
}

/// Handle token request
async fn handle_token(
    State(state): State<Arc<OAuthServerState>>,
    axum::Form(req): axum::Form<TokenRequest>,
) -> Response {
    match req.grant_type.as_str() {
        "authorization_code" => handle_authorization_code_grant(state, req).await,
        "refresh_token" => handle_refresh_token_grant(state, req).await,
        "client_credentials" => handle_client_credentials_grant(state, req).await,
        _ => (
            axum::http::StatusCode::BAD_REQUEST,
            format!("Unsupported grant_type: {}", req.grant_type),
        )
            .into_response(),
    }
}

/// Handle authorization code grant
async fn handle_authorization_code_grant(
    state: Arc<OAuthServerState>,
    req: TokenRequest,
) -> Response {
    let Some(code) = req.code else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_request", "error_description": "code is required"})),
        )
            .into_response();
    };

    // Verify authorization code exists and hasn't expired
    let mut token = match state.storage.get_oauth_token_by_code(&code).await {
        Ok(Some(t)) => t,
        Ok(None) => return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_grant", "error_description": "authorization code not found"}))).into_response(),
        Err(e) => {
            tracing::error!("Failed to get OAuth token: {}", e);
            return (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({"error": "server_error"}))).into_response();
        }
    };

    // Check if code has expired
    if let (Some(created), Some(expires_in)) = (token.code_create_at, token.code_expires_in)
        && Utc::now()
            > created + chrono::Duration::from_std(expires_in).unwrap_or(chrono::Duration::zero())
    {
        // Delete expired code
        let _ = state.storage.delete_oauth_token_by_code(&code).await;
        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_grant", "error_description": "authorization code expired"}))).into_response();
    }

    // Verify PKCE if code_verifier was provided
    if let Some(code_verifier) = req.code_verifier {
        if let Some(stored_challenge) = &token.code_challenge {
            // Verify code_challenge = BASE64URL-ENCODE(SHA256(ASCII(code_verifier)))
            use sha2::{Digest, Sha256};
            let mut hasher = Sha256::new();
            hasher.update(code_verifier.as_bytes());
            let hash = hasher.finalize();
            let computed_challenge =
                base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, hash);

            if &computed_challenge != stored_challenge {
                return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_grant", "error_description": "PKCE validation failed"}))).into_response();
            }
        }
    } else if token.code_challenge.is_some() {
        // PKCE was required but verifier not provided
        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_request", "error_description": "code_verifier is required"}))).into_response();
    }

    // Verify redirect_uri matches
    if let Some(redirect_uri) = req.redirect_uri
        && redirect_uri != token.redirect_uri
    {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_grant", "error_description": "redirect_uri mismatch"})),
        )
            .into_response();
    }

    // Generate access and refresh tokens
    let access_token = generate_access_token();
    let refresh_token = generate_refresh_token();

    // Update token record with access and refresh tokens
    token.access = Some(access_token.clone());
    token.access_create_at = Some(Utc::now());
    token.access_expires_in = Some(std::time::Duration::from_secs(
        state.config.token_expiry.num_seconds() as u64,
    ));
    token.refresh = Some(refresh_token.clone());
    token.refresh_create_at = Some(Utc::now());
    token.refresh_expires_in = Some(std::time::Duration::from_secs(
        state.config.refresh_expiry.num_seconds() as u64,
    ));

    // Delete the authorization code (one-time use)
    if let Err(e) = state.storage.delete_oauth_token_by_code(&code).await {
        tracing::warn!("Failed to delete used authorization code: {}", e);
    }

    // Save updated token with access/refresh tokens
    if let Err(e) = state.storage.save_oauth_token(&token).await {
        tracing::error!("Failed to save OAuth token: {}", e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "server_error"})),
        )
            .into_response();
    }

    let response = TokenResponse {
        access_token,
        token_type: "Bearer".to_string(),
        expires_in: state.config.token_expiry.num_seconds(),
        refresh_token: Some(refresh_token),
        scope: Some(token.scope),
    };

    Json(response).into_response()
}

/// Handle refresh token grant
async fn handle_refresh_token_grant(state: Arc<OAuthServerState>, req: TokenRequest) -> Response {
    let Some(refresh_token_value) = req.refresh_token else {
        return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_request", "error_description": "refresh_token is required"}))).into_response();
    };

    // Verify refresh token exists and hasn't expired
    let mut token = match state
        .storage
        .get_oauth_token_by_refresh(&refresh_token_value)
        .await
    {
        Ok(Some(t)) => t,
        Ok(None) => return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_grant", "error_description": "refresh token not found"})),
        )
            .into_response(),
        Err(e) => {
            tracing::error!("Failed to get OAuth token: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "server_error"})),
            )
                .into_response();
        }
    };

    // Check if refresh token has expired
    if let (Some(created), Some(expires_in)) = (token.refresh_create_at, token.refresh_expires_in)
        && Utc::now()
            > created + chrono::Duration::from_std(expires_in).unwrap_or(chrono::Duration::zero())
    {
        // Delete expired refresh token
        let _ = state
            .storage
            .delete_oauth_token_by_refresh(&refresh_token_value)
            .await;
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_grant", "error_description": "refresh token expired"})),
        )
            .into_response();
    }

    // Generate new access token and rotate refresh token for security
    let access_token = generate_access_token();
    let new_refresh_token = generate_refresh_token();

    // Delete old refresh token
    let _ = state
        .storage
        .delete_oauth_token_by_refresh(&refresh_token_value)
        .await;

    // Update token record with new access and refresh tokens
    token.access = Some(access_token.clone());
    token.access_create_at = Some(Utc::now());
    token.access_expires_in = Some(std::time::Duration::from_secs(
        state.config.token_expiry.num_seconds() as u64,
    ));
    token.refresh = Some(new_refresh_token.clone());
    token.refresh_create_at = Some(Utc::now());
    token.refresh_expires_in = Some(std::time::Duration::from_secs(
        state.config.refresh_expiry.num_seconds() as u64,
    ));

    // Save updated token
    if let Err(e) = state.storage.save_oauth_token(&token).await {
        tracing::error!("Failed to save OAuth token: {}", e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "server_error"})),
        )
            .into_response();
    }

    let response = TokenResponse {
        access_token,
        token_type: "Bearer".to_string(),
        expires_in: state.config.token_expiry.num_seconds(),
        refresh_token: Some(new_refresh_token), // Rotate refresh token for security
        scope: Some(token.scope),
    };

    Json(response).into_response()
}

/// Handle client credentials grant
async fn handle_client_credentials_grant(
    state: Arc<OAuthServerState>,
    req: TokenRequest,
) -> Response {
    // Extract client credentials from request
    let client_id = match req._client_id {
        Some(id) => id,
        None => return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_request", "error_description": "client_id is required"})),
        )
            .into_response(),
    };

    let client_secret = match req._client_secret {
        Some(secret) => secret,
        None => return (axum::http::StatusCode::BAD_REQUEST, Json(json!({"error": "invalid_request", "error_description": "client_secret is required"}))).into_response(),
    };

    // Verify client credentials
    let client = match state.storage.get_oauth_client(&client_id).await {
        Ok(Some(c)) => c,
        Ok(None) => {
            return (
                axum::http::StatusCode::UNAUTHORIZED,
                Json(json!({"error": "invalid_client", "error_description": "client not found"})),
            )
                .into_response();
        }
        Err(e) => {
            tracing::error!("Failed to get OAuth client: {}", e);
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "server_error"})),
            )
                .into_response();
        }
    };

    // Verify client secret matches
    if client.secret != client_secret {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(json!({"error": "invalid_client", "error_description": "invalid client_secret"})),
        )
            .into_response();
    }

    // Generate access token
    let access_token = generate_access_token();

    // Create token record for client credentials grant
    let token = OAuthToken {
        id: Uuid::new_v4().to_string(),
        client_id: client_id.clone(),
        user_id: format!("client:{}", client_id), // Machine-to-machine, no user
        redirect_uri: String::new(),
        scope: client.scope.clone(),
        code: None,
        code_create_at: None,
        code_expires_in: None,
        code_challenge: None,
        code_challenge_method: None,
        access: Some(access_token.clone()),
        access_create_at: Some(Utc::now()),
        access_expires_in: Some(std::time::Duration::from_secs(
            state.config.token_expiry.num_seconds() as u64,
        )),
        refresh: None, // No refresh token for client credentials
        refresh_create_at: None,
        refresh_expires_in: None,
    };

    // Save token
    if let Err(e) = state.storage.save_oauth_token(&token).await {
        tracing::error!("Failed to save OAuth token: {}", e);
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": "server_error"})),
        )
            .into_response();
    }

    let response = TokenResponse {
        access_token,
        token_type: "Bearer".to_string(),
        expires_in: state.config.token_expiry.num_seconds(),
        refresh_token: None, // No refresh token for client credentials
        scope: Some(client.scope),
    };

    Json(response).into_response()
}

/// Validate redirect URI
fn is_valid_redirect_uri(uri: &str, allow_localhost: bool) -> bool {
    if uri.is_empty() || uri.len() > 2048 {
        return false;
    }

    // Parse URL
    if let Ok(parsed) = url::Url::parse(uri) {
        // Must be HTTPS or localhost
        if parsed.scheme() != "https"
            && (!allow_localhost
                || (parsed.host_str() != Some("localhost")
                    && parsed.host_str() != Some("127.0.0.1")))
        {
            return false;
        }

        // No fragments allowed (OAuth 2.1 security)
        if parsed.fragment().is_some() {
            return false;
        }

        true
    } else {
        false
    }
}

/// Generate secure client secret
fn generate_client_secret() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random()).collect();
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &bytes)
}

/// Generate authorization code
fn generate_authorization_code() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random()).collect();
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &bytes)
}

/// Generate access token
fn generate_access_token() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random()).collect();
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &bytes)
}

/// Generate refresh token
fn generate_refresh_token() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random()).collect();
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &bytes)
}

/// Handle token revocation (RFC 7009)
async fn handle_token_revocation(
    State(state): State<Arc<OAuthServerState>>,
    axum::Form(params): axum::Form<HashMap<String, String>>,
) -> Response {
    let Some(token) = params.get("token") else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_request"})),
        )
            .into_response();
    };

    // Try to revoke as access token first, then refresh token
    let _ = state.storage.delete_oauth_token_by_access(token).await;
    let _ = state.storage.delete_oauth_token_by_refresh(token).await;

    // Always return 200 per RFC 7009 (even if token doesn't exist)
    (axum::http::StatusCode::OK, Json(json!({}))).into_response()
}

/// Handle token introspection (RFC 7662)
async fn handle_token_introspection(
    State(state): State<Arc<OAuthServerState>>,
    axum::Form(params): axum::Form<HashMap<String, String>>,
) -> Response {
    let Some(token_value) = params.get("token") else {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(json!({"error": "invalid_request"})),
        )
            .into_response();
    };

    // Look up token
    let token = match state.storage.get_oauth_token_by_access(token_value).await {
        Ok(Some(t)) => t,
        _ => {
            // Token not found or error - return inactive
            return Json(json!({"active": false})).into_response();
        }
    };

    // Check if token is expired
    if let (Some(created), Some(expires_in)) = (token.access_create_at, token.access_expires_in) {
        let expires_at =
            created + chrono::Duration::from_std(expires_in).unwrap_or(chrono::Duration::zero());
        if Utc::now() > expires_at {
            return Json(json!({"active": false})).into_response();
        }
    }

    // Token is active
    Json(json!({
        "active": true,
        "scope": token.scope,
        "client_id": token.client_id,
        "username": token.user_id,
        "exp": token.access_create_at
            .and_then(|created| token.access_expires_in.map(|dur| created + chrono::Duration::from_std(dur).unwrap_or(chrono::Duration::zero())))
            .map(|dt| dt.timestamp()),
    })).into_response()
}

#[cfg(test)]
mod server_test {
    include!("server_test.rs");
}
