//! HTTP server for BeemFlow
//!
//! Provides REST API for all BeemFlow operations with complete parity
//! with CLI and MCP interfaces.

pub mod response;
pub mod session;
pub mod template;
pub mod webhook;

use self::webhook::{WebhookManagerState, create_webhook_routes};
use crate::auth::{OAuthConfig, OAuthMiddlewareState, OAuthServerState, create_oauth_routes};
use crate::config::{Config, HttpConfig};
use crate::core::OperationRegistry;
use crate::{BeemFlowError, Result};
use axum::{
    Router,
    extract::{Json, Path as AxumPath, State},
    http::{Method, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{delete, get, post},
};
use parking_lot::RwLock;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::{
    LatencyUnit,
    cors::{Any, CorsLayer},
    services::ServeDir,
    trace::{DefaultMakeSpan, DefaultOnResponse, TraceLayer},
};

/// Application state shared across handlers
#[derive(Clone)]
pub struct AppState {
    registry: Arc<OperationRegistry>,
    session_store: Arc<session::SessionStore>,
    oauth_client: Arc<crate::auth::OAuthClientManager>,
    storage: Arc<dyn crate::storage::Storage>,
    template_renderer: Arc<template::TemplateRenderer>,
}

/// Error type for HTTP handlers with enhanced error details
#[derive(Debug)]
pub struct AppError(BeemFlowError);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, error_type, message) = match &self.0 {
            BeemFlowError::Validation(msg) => {
                (StatusCode::BAD_REQUEST, "validation_error", msg.clone())
            }
            BeemFlowError::Storage(e) => match e {
                crate::error::StorageError::NotFound(msg) => {
                    (StatusCode::NOT_FOUND, "not_found", msg.clone())
                }
                _ => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "storage_error",
                    e.to_string(),
                ),
            },
            BeemFlowError::StepExecution { step_id, message } => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "execution_error",
                format!("Step {} failed: {}", step_id, message),
            ),
            BeemFlowError::OAuth(msg) => (StatusCode::UNAUTHORIZED, "auth_error", msg.clone()),
            BeemFlowError::Adapter(msg) => (StatusCode::BAD_GATEWAY, "adapter_error", msg.clone()),
            BeemFlowError::Mcp(msg) => (StatusCode::BAD_GATEWAY, "mcp_error", msg.clone()),
            BeemFlowError::Network(e) => (StatusCode::BAD_GATEWAY, "network_error", e.to_string()),
            _ => (
                StatusCode::INTERNAL_SERVER_ERROR,
                "internal_error",
                self.0.to_string(),
            ),
        };

        // Log the error
        tracing::error!(
            error_type = error_type,
            status = %status,
            message = %message,
            "HTTP request error"
        );

        let body = json!({
            "error": {
                "type": error_type,
                "message": message,
                "status": status.as_u16(),
            }
        });

        (status, Json(body)).into_response()
    }
}

impl<E> From<E> for AppError
where
    E: Into<BeemFlowError>,
{
    fn from(err: E) -> Self {
        Self(err.into())
    }
}

// ============================================================================
// AUTO-GENERATED ROUTES - All operation routes are now generated from metadata
// ============================================================================
// The old handler macros are no longer needed - routes are auto-generated
// in build_operation_routes() from operation metadata

/// Start the HTTP server
pub async fn start_server(config: Config) -> Result<()> {
    // Initialize telemetry
    crate::telemetry::init(config.tracing.as_ref())?;

    // Ensure HTTP config exists (use defaults if not provided)
    let http_config = config.http.as_ref().cloned().unwrap_or_else(|| HttpConfig {
        host: "127.0.0.1".to_string(),
        port: crate::constants::DEFAULT_HTTP_PORT,
    });

    // Use centralized dependency creation from core module
    let dependencies = crate::core::create_dependencies(&config).await?;
    let registry = Arc::new(OperationRegistry::new(dependencies.clone()));

    // Create session store
    let session_store = Arc::new(session::SessionStore::new());

    // Create OAuth client manager
    let redirect_uri = format!(
        "http://{}:{}/oauth/callback",
        http_config.host, http_config.port
    );
    let oauth_client = Arc::new(crate::auth::OAuthClientManager::new(
        dependencies.storage.clone(),
        dependencies.registry_manager.clone(),
        redirect_uri,
    ));

    // Initialize template renderer
    let mut template_renderer = template::TemplateRenderer::new("static");
    template_renderer.load_oauth_templates().await?;
    let template_renderer = Arc::new(template_renderer);

    let state = AppState {
        registry,
        session_store: session_store.clone(),
        oauth_client,
        storage: dependencies.storage.clone(),
        template_renderer,
    };

    // Create OAuth server state
    let oauth_config = OAuthConfig {
        issuer: format!("http://{}:{}", http_config.host, http_config.port),
        ..Default::default()
    };

    let oauth_server_state = Arc::new(OAuthServerState {
        storage: dependencies.storage.clone(),
        config: oauth_config,
        rate_limiter: Arc::new(RwLock::new(HashMap::new())),
    });

    // Create OAuth middleware state
    let oauth_middleware_state = Arc::new(OAuthMiddlewareState::new(dependencies.storage.clone()));

    // Create webhook manager state
    let webhook_state = WebhookManagerState {
        event_bus: state.registry.get_dependencies().event_bus.clone(),
        registry_manager: state.registry.get_dependencies().registry_manager.clone(),
    };

    // Build router
    let app = build_router(
        state,
        webhook_state,
        oauth_server_state,
        oauth_middleware_state,
        session_store,
    );

    // Determine bind address
    let addr = format!("{}:{}", http_config.host, http_config.port);
    let socket_addr: SocketAddr = addr
        .parse()
        .map_err(|e| BeemFlowError::config(format!("Invalid address {}: {}", addr, e)))?;

    tracing::info!("Starting HTTP server on {}", socket_addr);

    // Start server
    let listener = tokio::net::TcpListener::bind(socket_addr).await?;
    axum::serve(listener, app)
        .await
        .map_err(|e| BeemFlowError::config(format!("Server error: {}", e)))?;

    Ok(())
}

/// Auto-generate routes from operation metadata
fn build_operation_routes(state: &AppState) -> Router<AppState> {
    use axum::body::Body;
    use axum::extract::Request;

    let mut router = Router::new();
    let metadata = state.registry.get_all_metadata();

    for (op_name, meta) in metadata {
        // Skip operations without HTTP endpoints
        let (Some(method), Some(path)) = (meta.http_method, meta.http_path) else {
            continue;
        };

        let op_name_for_handler = op_name.clone();
        let op_name_for_log = op_name.clone();
        let path_template = path.to_string();
        let path_for_log = path.to_string();
        let method_for_log = method.to_string();

        // Create a universal handler that works with any operation
        let handler = move |State(state): State<AppState>, req: Request<Body>| {
            let op_name = op_name_for_handler.clone();
            let path_template = path_template.clone();

            async move {
                // Extract path parameters by comparing URI with template
                let uri = req.uri().path();
                let mut input = json!({});

                // Parse path parameters from URI
                let path_parts: Vec<&str> =
                    path_template.split('/').filter(|s| !s.is_empty()).collect();
                let uri_parts: Vec<&str> = uri.split('/').filter(|s| !s.is_empty()).collect();

                if path_parts.len() == uri_parts.len() {
                    for (template_part, uri_part) in path_parts.iter().zip(uri_parts.iter()) {
                        if template_part.starts_with('{') && template_part.ends_with('}') {
                            let param_name = &template_part[1..template_part.len() - 1];
                            if let Some(obj) = input.as_object_mut() {
                                obj.insert(param_name.to_string(), json!(uri_part));
                            }
                        }
                    }
                }

                // Try to read JSON body if present
                let bytes = axum::body::to_bytes(req.into_body(), usize::MAX)
                    .await
                    .unwrap_or_default();
                if !bytes.is_empty()
                    && let Ok(body_json) = serde_json::from_slice::<Value>(&bytes)
                    && let Some(obj) = input.as_object_mut()
                    && let Some(body_obj) = body_json.as_object()
                {
                    for (k, v) in body_obj {
                        obj.insert(k.clone(), v.clone());
                    }
                }

                let result = state.registry.execute(&op_name, input).await?;
                Ok::<Json<Value>, AppError>(Json(result))
            }
        };

        // Add route based on method
        router = match method {
            "GET" => router.route(path, get(handler)),
            "POST" => router.route(path, post(handler)),
            "DELETE" => router.route(path, delete(handler)),
            "PUT" => router.route(path, axum::routing::put(handler)),
            "PATCH" => router.route(path, axum::routing::patch(handler)),
            _ => {
                tracing::warn!(
                    "Unsupported HTTP method '{}' for operation '{}'",
                    method,
                    &op_name_for_log
                );
                router
            }
        };

        tracing::debug!(
            "Auto-registered route: {} {} -> {}",
            method_for_log,
            path_for_log,
            op_name_for_log
        );
    }

    router
}

/// Build the router with all endpoints
fn build_router(
    state: AppState,
    webhook_state: WebhookManagerState,
    oauth_server_state: Arc<OAuthServerState>,
    _oauth_middleware_state: Arc<OAuthMiddlewareState>,
    _session_store: Arc<session::SessionStore>,
) -> Router {
    // Create OAuth server routes (authorization server endpoints)
    let oauth_routes = create_oauth_routes(oauth_server_state);

    // Build auto-generated operation routes from metadata
    let operation_routes = build_operation_routes(&state);

    // Build AppState-based routes for OAuth and system endpoints
    let app_routes = Router::new()
        // OAuth UI endpoints (legacy/fallback)
        .route("/oauth/providers", get(oauth_providers_handler))
        .route("/oauth/providers/{provider}", get(oauth_provider_handler))
        .route(
            "/oauth/consent",
            get(oauth_consent_handler).post(oauth_consent_handler),
        )
        .route("/oauth/success", get(oauth_success_handler))
        .route("/oauth/callback", get(oauth_callback_handler))
        // OAuth Provider API endpoints
        .route(
            "/api/oauth/providers",
            get(list_oauth_providers_handler).post(create_oauth_provider_handler),
        )
        .route(
            "/api/oauth/providers/{id}",
            get(get_oauth_provider_handler)
                .post(update_oauth_provider_handler)
                .delete(delete_oauth_provider_handler),
        )
        // OAuth Credential API endpoints
        .route(
            "/api/oauth/credentials",
            get(list_oauth_credentials_handler),
        )
        .route(
            "/api/oauth/credentials/{id}",
            delete(delete_oauth_credential_handler),
        )
        .route(
            "/api/oauth/authorize/{provider}",
            get(authorize_oauth_provider_handler),
        )
        .route("/api/oauth/callback", get(oauth_api_callback_handler))
        // System endpoints (special handlers not in operation registry)
        .route("/healthz", get(health_handler))
        .route("/metrics", get(metrics_handler))
        // Cron webhook endpoint
        .route("/webhooks/cron", post(cron_webhook_handler))
        // Webhook endpoints
        .nest(
            "/webhooks",
            create_webhook_routes().with_state(webhook_state),
        )
        // Merge auto-generated operation routes
        .merge(operation_routes)
        // Add state to AppState routes
        .with_state(state);

    // Merge OAuth server routes (which have their own state) with our app routes

    Router::new()
        // Static file serving for OAuth UI
        .nest_service("/static", ServeDir::new("static"))
        // OAuth 2.1 Authorization Server (RFC 6749 & 8252)
        .merge(oauth_routes)
        // Application routes
        .merge(app_routes)
        // Add comprehensive middleware stack
        .layer(
            ServiceBuilder::new()
                // Session middleware for OAuth flows and authenticated requests
                .layer(axum::middleware::from_fn(session::session_middleware))
                // Tracing layer for request/response logging
                .layer(
                    TraceLayer::new_for_http()
                        .make_span_with(DefaultMakeSpan::new().include_headers(true))
                        .on_response(
                            DefaultOnResponse::new()
                                .level(tracing::Level::INFO)
                                .latency_unit(LatencyUnit::Micros),
                        ),
                )
                // CORS layer for cross-origin requests
                .layer(
                    CorsLayer::new()
                        .allow_origin(Any)
                        .allow_methods(Any)
                        .allow_headers(Any),
                ),
        )
}

// ============================================================================
// SYSTEM HANDLERS (Special cases not in operation registry)
// ============================================================================

async fn health_handler() -> Json<Value> {
    Json(json!({
        "status": "healthy",
        "timestamp": chrono::Utc::now().to_rfc3339(),
    }))
}

async fn metrics_handler() -> std::result::Result<(StatusCode, String), AppError> {
    let metrics = crate::telemetry::get_metrics()?;
    Ok((StatusCode::OK, metrics))
}

// ============================================================================
// OAUTH HANDLERS (OAuth-specific UI and API routes)
// ============================================================================

async fn oauth_providers_handler(State(state): State<AppState>) -> impl IntoResponse {
    // Fetch providers from registry and storage
    let registry_manager = state.registry.get_dependencies().registry_manager.clone();

    let registry_providers = match registry_manager.list_oauth_providers().await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("Failed to list OAuth providers from registry: {}", e);
            vec![]
        }
    };

    let storage_providers = match state.storage.list_oauth_providers().await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("Failed to list OAuth providers from storage: {}", e);
            vec![]
        }
    };

    // Build provider data for template from registry providers
    let mut provider_data: Vec<Value> = registry_providers
        .iter()
        .map(|entry| {
            let name = entry.display_name.as_ref().unwrap_or(&entry.name);

            // Map provider name to emoji icon
            let icon = match name.to_lowercase().as_str() {
                name if name.contains("github") => "🐙",
                name if name.contains("google") => "🔍",
                name if name.contains("microsoft") => "🪟",
                name if name.contains("slack") => "💬",
                _ => "🔗",
            };

            // Format scopes
            let scopes_str = entry
                .scopes
                .as_ref()
                .map(|s| s.join(", "))
                .unwrap_or_else(|| "None".to_string());

            json!({
                "id": entry.name,
                "name": name,
                "icon": icon,
                "scopes_str": scopes_str,
            })
        })
        .collect();

    // Add storage providers
    for p in storage_providers.iter() {
        // Map provider name to emoji icon
        let icon = match p.name.to_lowercase().as_str() {
            name if name.contains("github") => "🐙",
            name if name.contains("google") => "🔍",
            name if name.contains("microsoft") => "🪟",
            name if name.contains("slack") => "💬",
            _ => "🔗",
        };

        // Format scopes
        let scopes_str = p
            .scopes
            .as_ref()
            .map(|s| s.join(", "))
            .unwrap_or_else(|| "None".to_string());

        provider_data.push(json!({
            "id": p.id,
            "name": p.name,
            "icon": icon,
            "scopes_str": scopes_str,
        }));
    }

    // Render template with provider data
    let template_data = json!({
        "providers": provider_data
    });

    match state
        .template_renderer
        .render_json("providers", &template_data)
    {
        Ok(html) => Html(html),
        Err(e) => {
            tracing::error!("Failed to render providers template: {}", e);
            Html(format!(
                r#"<!DOCTYPE html>
<html>
<head><title>Error</title></head>
<body><h1>Template Error</h1><p>{}</p></body>
</html>"#,
                e
            ))
        }
    }
}

async fn oauth_consent_handler(
    State(_state): State<AppState>,
    method: Method,
    axum::extract::Form(form): axum::extract::Form<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    match method {
        Method::GET => {
            // Show consent form
            Html(include_str!("../../static/oauth/consent.html")).into_response()
        }
        Method::POST => {
            // Handle consent response
            let action = form.get("action").cloned().unwrap_or_default();

            match action.as_str() {
                "approve" => {
                    // Handle approval - redirect to success page
                    Redirect::to("/oauth/success").into_response()
                }
                "deny" => {
                    // Handle denial - redirect back to providers
                    Redirect::to("/oauth/providers").into_response()
                }
                _ => {
                    // Invalid action
                    (StatusCode::BAD_REQUEST, "Invalid action").into_response()
                }
            }
        }
        _ => (StatusCode::METHOD_NOT_ALLOWED, "Method not allowed").into_response(),
    }
}

async fn oauth_success_handler() -> impl IntoResponse {
    Html(include_str!("../../static/oauth/success.html"))
}

async fn oauth_callback_handler(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
    headers: axum::http::HeaderMap,
) -> impl IntoResponse {
    // Check for OAuth error response
    if let Some(error) = params.get("error") {
        let error_desc = params
            .get("error_description")
            .map(|s| s.as_str())
            .unwrap_or("Unknown error");
        tracing::error!("OAuth authorization failed: {} - {}", error, error_desc);
        return (
            StatusCode::BAD_REQUEST,
            Html(format!(
                r#"<!DOCTYPE html>
<html>
<head><title>OAuth Error</title></head>
<body>
    <h1>OAuth Authorization Failed</h1>
    <p>Error: {}</p>
    <p>Description: {}</p>
    <a href="/oauth/providers">Try again</a>
</body>
</html>"#,
                error, error_desc
            )),
        )
            .into_response();
    }

    // Get authorization code and state from query params
    let code = match params.get("code") {
        Some(c) => c,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Html(
                    r#"<!DOCTYPE html>
<html>
<head><title>OAuth Error</title></head>
<body>
    <h1>Missing Authorization Code</h1>
    <a href="/oauth/providers">Try again</a>
</body>
</html>"#,
                ),
            )
                .into_response();
        }
    };

    let state_param = match params.get("state") {
        Some(s) => s,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Html(
                    r#"<!DOCTYPE html>
<html>
<head><title>OAuth Error</title></head>
<body>
    <h1>Missing State Parameter</h1>
    <a href="/oauth/providers">Try again</a>
</body>
</html>"#,
                ),
            )
                .into_response();
        }
    };

    // Get session ID from cookie header
    let session_id = match headers.get(axum::http::header::COOKIE) {
        Some(cookie_header) => {
            // Parse cookie header to extract beemflow_session
            let cookie_str = cookie_header.to_str().unwrap_or("");
            let session_id = cookie_str
                .split(';')
                .map(|c| c.trim())
                .find_map(|c| c.strip_prefix("beemflow_session="));

            match session_id {
                Some(sid) => sid,
                None => {
                    tracing::error!("No beemflow_session cookie found in OAuth callback");
                    return (
                        StatusCode::BAD_REQUEST,
                        Html(
                            r#"<!DOCTYPE html>
<html>
<head><title>OAuth Error</title></head>
<body>
    <h1>Session Not Found</h1>
    <p>Your session has expired or was not found. Please try again.</p>
    <a href="/oauth/providers">Try again</a>
</body>
</html>"#,
                        ),
                    )
                        .into_response();
                }
            }
        }
        None => {
            tracing::error!("No Cookie header found in OAuth callback");
            return (
                StatusCode::BAD_REQUEST,
                Html(
                    r#"<!DOCTYPE html>
<html>
<head><title>OAuth Error</title></head>
<body>
    <h1>Session Not Found</h1>
    <p>Your session has expired or was not found. Please try again.</p>
    <a href="/oauth/providers">Try again</a>
</body>
</html>"#,
                ),
            )
                .into_response();
        }
    };

    // Get session from store
    let session = match state.session_store.get_session(session_id) {
        Some(s) => s,
        None => {
            tracing::error!("Invalid or expired session: {}", session_id);
            return (
                StatusCode::BAD_REQUEST,
                Html(
                    r#"<!DOCTYPE html>
<html>
<head><title>OAuth Error</title></head>
<body>
    <h1>Session Expired</h1>
    <p>Your session has expired. Please try again.</p>
    <a href="/oauth/providers">Try again</a>
</body>
</html>"#,
                ),
            )
                .into_response();
        }
    };

    // Validate state parameter (CSRF protection)
    let stored_state = match session.data.get("oauth_state").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            tracing::error!("No oauth_state found in session");
            return (
                StatusCode::BAD_REQUEST,
                Html(
                    r#"<!DOCTYPE html>
<html>
<head><title>OAuth Error</title></head>
<body>
    <h1>Invalid Session</h1>
    <p>Session is missing OAuth state. Please try again.</p>
    <a href="/oauth/providers">Try again</a>
</body>
</html>"#,
                ),
            )
                .into_response();
        }
    };

    if stored_state != state_param {
        tracing::error!(
            "State parameter mismatch - possible CSRF attack. Expected: {}, Got: {}",
            stored_state,
            state_param
        );
        return (
            StatusCode::BAD_REQUEST,
            Html(
                r#"<!DOCTYPE html>
<html>
<head><title>OAuth Error</title></head>
<body>
    <h1>Security Error</h1>
    <p>State parameter mismatch. Possible CSRF attack detected.</p>
    <a href="/oauth/providers">Try again</a>
</body>
</html>"#,
            ),
        )
            .into_response();
    }

    // Get stored OAuth parameters from session
    let code_verifier = match session
        .data
        .get("oauth_code_verifier")
        .and_then(|v| v.as_str())
    {
        Some(cv) => cv,
        None => {
            tracing::error!("No code_verifier found in session");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(
                    r#"<!DOCTYPE html>
<html>
<head><title>OAuth Error</title></head>
<body>
    <h1>Session Error</h1>
    <p>Missing code verifier. Please try again.</p>
    <a href="/oauth/providers">Try again</a>
</body>
</html>"#,
                ),
            )
                .into_response();
        }
    };

    let provider_id = match session
        .data
        .get("oauth_provider_id")
        .and_then(|v| v.as_str())
    {
        Some(p) => p,
        None => {
            tracing::error!("No provider_id found in session");
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(
                    r#"<!DOCTYPE html>
<html>
<head><title>OAuth Error</title></head>
<body>
    <h1>Session Error</h1>
    <p>Missing provider ID. Please try again.</p>
    <a href="/oauth/providers">Try again</a>
</body>
</html>"#,
                ),
            )
                .into_response();
        }
    };

    let integration = session
        .data
        .get("oauth_integration")
        .and_then(|v| v.as_str())
        .unwrap_or("default");

    // Exchange authorization code for tokens using oauth2 crate
    match state
        .oauth_client
        .exchange_code(provider_id, code, code_verifier, integration)
        .await
    {
        Ok(credential) => {
            tracing::info!(
                "Successfully exchanged OAuth code for tokens: {}:{}",
                credential.provider,
                credential.integration
            );

            // Clean up session
            state.session_store.delete_session(session_id);

            // Redirect to success page
            Redirect::to("/oauth/success").into_response()
        }
        Err(e) => {
            tracing::error!("Failed to exchange authorization code: {}", e);

            // Clean up session even on error
            state.session_store.delete_session(session_id);

            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Html(format!(
                    r#"<!DOCTYPE html>
<html>
<head><title>OAuth Error</title></head>
<body>
    <h1>Token Exchange Failed</h1>
    <p>Failed to exchange authorization code for tokens: {}</p>
    <a href="/oauth/providers">Try again</a>
</body>
</html>"#,
                    e
                )),
            )
                .into_response()
        }
    }
}

/// Handle cron webhook notifications
async fn cron_webhook_handler(Json(payload): Json<Value>) -> impl IntoResponse {
    // Log the webhook notification
    tracing::info!("Received cron webhook: {:?}", payload);

    // Extract notification details
    if let (Some(event), Some(flow_name), Some(success)) = (
        payload.get("event").and_then(|v| v.as_str()),
        payload.get("flow_name").and_then(|v| v.as_str()),
        payload.get("success").and_then(|v| v.as_bool()),
    ) && event == "cron_job_completed"
    {
        if success {
            tracing::info!("Cron job completed successfully: {}", flow_name);
        } else {
            let error = payload
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown error");
            tracing::warn!("Cron job failed: {} - {}", flow_name, error);
        }
    }

    // Return success response
    (StatusCode::OK, Json(json!({"status": "received"})))
}

async fn oauth_provider_handler(
    State(state): State<AppState>,
    axum::extract::Path(provider): axum::extract::Path<String>,
) -> impl IntoResponse {
    // Get default scopes for the provider from registry
    let registry_manager = state.registry.get_dependencies().registry_manager.clone();
    let scopes = match registry_manager.get_oauth_provider(&provider).await {
        Ok(Some(entry)) => entry.scopes.unwrap_or_else(|| vec!["read".to_string()]),
        _ => vec!["read".to_string()],
    };

    // Convert Vec<String> to Vec<&str>
    let scope_refs: Vec<&str> = scopes.iter().map(|s| s.as_str()).collect();

    // Build authorization URL using oauth2 crate
    let (auth_url, state_param, code_verifier) = match state
        .oauth_client
        .build_auth_url(&provider, &scope_refs, None)
        .await
    {
        Ok(result) => result,
        Err(e) => {
            tracing::error!("Failed to build auth URL for {}: {}", provider, e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("Failed to build authorization URL: {}", e),
            )
                .into_response();
        }
    };

    // Create session to store OAuth state
    let session = state
        .session_store
        .create_session("oauth_flow", chrono::Duration::minutes(10));

    // Store OAuth parameters in session for callback validation
    state
        .session_store
        .update_session(&session.id, "oauth_state".to_string(), json!(state_param));
    state.session_store.update_session(
        &session.id,
        "oauth_code_verifier".to_string(),
        json!(code_verifier),
    );
    state.session_store.update_session(
        &session.id,
        "oauth_provider_id".to_string(),
        json!(provider),
    );
    state.session_store.update_session(
        &session.id,
        "oauth_integration".to_string(),
        json!("default"),
    );

    // Set session cookie and redirect to auth URL
    let cookie = session::set_session_cookie(&session.id, session.expires_at);

    (
        StatusCode::FOUND,
        [
            (axum::http::header::LOCATION, auth_url.as_str()),
            (axum::http::header::SET_COOKIE, cookie.as_str()),
        ],
    )
        .into_response()
}

// ============================================================================
// OAUTH PROVIDER API HANDLERS
// ============================================================================

/// List all OAuth providers (from both registry and storage)
async fn list_oauth_providers_handler(
    State(state): State<AppState>,
) -> std::result::Result<Json<Value>, AppError> {
    // Get built-in providers from registry
    let registry_manager = state.registry.get_dependencies().registry_manager.clone();
    let registry_providers = registry_manager.list_oauth_providers().await?;

    // Get custom providers from storage
    let storage_providers = state.storage.list_oauth_providers().await?;

    // Convert registry entries to simplified provider format
    let mut all_providers: Vec<Value> = registry_providers
        .iter()
        .map(|entry| {
            json!({
                "id": entry.name,
                "name": entry.display_name.as_ref().unwrap_or(&entry.name),
                "auth_url": entry.auth_url,
                "token_url": entry.token_url,
                "scopes": entry.scopes,
                "source": "registry",
                // Don't expose client_id/secret for registry providers
            })
        })
        .collect();

    // Add storage providers
    for provider in storage_providers {
        all_providers.push(json!({
            "id": provider.id,
            "name": provider.name,
            "auth_url": provider.auth_url,
            "token_url": provider.token_url,
            "scopes": provider.scopes,
            "source": "storage",
        }));
    }

    Ok(Json(json!({ "providers": all_providers })))
}

/// Get a specific OAuth provider (checks registry first, then storage)
async fn get_oauth_provider_handler(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> std::result::Result<Json<Value>, AppError> {
    // Check registry first for built-in providers
    let registry_manager = state.registry.get_dependencies().registry_manager.clone();
    if let Some(entry) = registry_manager.get_server(&id).await?
        && entry.entry_type == "oauth_provider"
    {
        return Ok(Json(json!({
            "id": entry.name,
            "name": entry.display_name.as_ref().unwrap_or(&entry.name),
            "auth_url": entry.auth_url,
            "token_url": entry.token_url,
            "scopes": entry.scopes,
            "source": "registry",
            // Don't expose client_id/secret for registry providers
        })));
    }

    // Fall back to storage for custom providers
    let provider = state
        .storage
        .get_oauth_provider(&id)
        .await?
        .ok_or_else(|| BeemFlowError::validation(format!("Provider '{}' not found", id)))?;

    Ok(Json(json!({
        "id": provider.id,
        "name": provider.name,
        "auth_url": provider.auth_url,
        "token_url": provider.token_url,
        "scopes": provider.scopes,
        "source": "storage",
    })))
}

/// Create a new OAuth provider
async fn create_oauth_provider_handler(
    State(state): State<AppState>,
    Json(mut provider): Json<crate::model::OAuthProvider>,
) -> std::result::Result<Json<Value>, AppError> {
    // Validate provider
    provider.validate()?;

    // Ensure timestamps are set
    let now = chrono::Utc::now();
    provider.created_at = now;
    provider.updated_at = now;

    // Save provider
    state.storage.save_oauth_provider(&provider).await?;

    Ok(Json(json!({
        "success": true,
        "provider": provider,
    })))
}

/// Update an existing OAuth provider
async fn update_oauth_provider_handler(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
    Json(mut provider): Json<crate::model::OAuthProvider>,
) -> std::result::Result<Json<Value>, AppError> {
    // Verify provider exists
    let existing = state
        .storage
        .get_oauth_provider(&id)
        .await?
        .ok_or_else(|| BeemFlowError::validation(format!("Provider '{}' not found", id)))?;

    // Update fields while preserving ID and created_at
    provider.id = id;
    provider.created_at = existing.created_at;
    provider.updated_at = chrono::Utc::now();

    // Validate and save
    provider.validate()?;
    state.storage.save_oauth_provider(&provider).await?;

    Ok(Json(json!({
        "success": true,
        "provider": provider,
    })))
}

/// Delete an OAuth provider
async fn delete_oauth_provider_handler(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> std::result::Result<Json<Value>, AppError> {
    state.storage.delete_oauth_provider(&id).await?;
    Ok(Json(json!({ "success": true })))
}

// ============================================================================
// OAUTH CREDENTIAL API HANDLERS
// ============================================================================

/// List all OAuth credentials
async fn list_oauth_credentials_handler(
    State(state): State<AppState>,
) -> std::result::Result<Json<Value>, AppError> {
    let credentials = state.storage.list_oauth_credentials().await?;

    // Redact access tokens for security
    let safe_credentials: Vec<Value> = credentials
        .iter()
        .map(|cred| {
            json!({
                "id": cred.id,
                "provider": cred.provider,
                "integration": cred.integration,
                "scope": cred.scope,
                "expires_at": cred.expires_at,
                "created_at": cred.created_at,
                "updated_at": cred.updated_at,
                "has_refresh_token": cred.refresh_token.is_some(),
            })
        })
        .collect();

    Ok(Json(json!({ "credentials": safe_credentials })))
}

/// Delete an OAuth credential
async fn delete_oauth_credential_handler(
    State(state): State<AppState>,
    AxumPath(id): AxumPath<String>,
) -> std::result::Result<Json<Value>, AppError> {
    state.storage.delete_oauth_credential(&id).await?;
    Ok(Json(json!({ "success": true })))
}

/// Initiate OAuth authorization flow for a provider
async fn authorize_oauth_provider_handler(
    State(state): State<AppState>,
    AxumPath(provider_id): AxumPath<String>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> std::result::Result<impl IntoResponse, AppError> {
    // Get scopes from query parameters (comma-separated)
    let scopes = params
        .get("scopes")
        .map(|s| s.split(',').map(|s| s.trim()).collect::<Vec<_>>())
        .unwrap_or_else(|| vec!["read"]);

    // Get integration name (optional)
    let integration = params.get("integration").map(|s| s.as_str());

    // Build authorization URL
    let (auth_url, state_param, code_verifier) = state
        .oauth_client
        .build_auth_url(&provider_id, &scopes, integration)
        .await?;

    // Store state and code_verifier in session for callback validation
    let session = state
        .session_store
        .create_session("oauth_flow", chrono::Duration::minutes(10));

    state
        .session_store
        .update_session(&session.id, "oauth_state".to_string(), json!(state_param));
    state.session_store.update_session(
        &session.id,
        "oauth_code_verifier".to_string(),
        json!(code_verifier),
    );
    state.session_store.update_session(
        &session.id,
        "oauth_provider_id".to_string(),
        json!(provider_id),
    );
    state.session_store.update_session(
        &session.id,
        "oauth_integration".to_string(),
        json!(integration.unwrap_or("default")),
    );

    // Return authorization URL and session info
    Ok(Json(json!({
        "authorization_url": auth_url,
        "session_id": session.id,
        "expires_at": session.expires_at,
    })))
}

/// Handle OAuth callback and exchange code for tokens
async fn oauth_api_callback_handler(
    State(state): State<AppState>,
    axum::extract::Query(params): axum::extract::Query<HashMap<String, String>>,
) -> std::result::Result<Json<Value>, AppError> {
    // Get authorization code and state
    let code = params
        .get("code")
        .ok_or_else(|| BeemFlowError::auth("Missing authorization code"))?;

    let state_param = params
        .get("state")
        .ok_or_else(|| BeemFlowError::auth("Missing state parameter"))?;

    // For simplicity, we'll look through all sessions to find the matching state
    // In production, this would use cookies or query parameters to identify the session
    // For now, we'll extract these from a session_id query parameter or stored state

    // Get session_id from query or error
    let session_id = params
        .get("session_id")
        .ok_or_else(|| BeemFlowError::auth("Missing session_id parameter"))?;

    let session = state
        .session_store
        .get_session(session_id)
        .ok_or_else(|| BeemFlowError::auth("Invalid or expired session"))?;

    // Validate state parameter matches
    let stored_state = session
        .data
        .get("oauth_state")
        .and_then(|v| v.as_str())
        .ok_or_else(|| BeemFlowError::auth("No state found in session"))?;

    if stored_state != state_param {
        return Err(BeemFlowError::auth("State parameter mismatch - possible CSRF attack").into());
    }

    // Get stored code_verifier and provider_id
    let code_verifier = session
        .data
        .get("oauth_code_verifier")
        .and_then(|v| v.as_str())
        .ok_or_else(|| BeemFlowError::auth("No code verifier found in session"))?;

    let provider_id = session
        .data
        .get("oauth_provider_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| BeemFlowError::auth("No provider ID found in session"))?;

    let stored_integration = session
        .data
        .get("oauth_integration")
        .and_then(|v| v.as_str())
        .unwrap_or("default");

    // Exchange code for tokens
    let credential = state
        .oauth_client
        .exchange_code(provider_id, code, code_verifier, stored_integration)
        .await?;

    // Clean up session
    state.session_store.delete_session(session_id);

    Ok(Json(json!({
        "success": true,
        "credential": {
            "id": credential.id,
            "provider": credential.provider,
            "integration": credential.integration,
            "scope": credential.scope,
            "expires_at": credential.expires_at,
            "created_at": credential.created_at,
        }
    })))
}

#[cfg(test)]
mod http_test;
