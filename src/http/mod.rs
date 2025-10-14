//! HTTP server for BeemFlow
//!
//! Provides REST API for all BeemFlow operations with complete parity
//! with CLI and MCP interfaces.

pub mod response;
pub mod session;
pub mod template;
pub mod webhook;

use self::webhook::{WebhookManagerState, create_webhook_routes};
use crate::auth::{
    OAuthConfig, OAuthMiddlewareState, OAuthServerState,
    client::{OAuthClientState, create_oauth_client_routes},
    create_oauth_routes,
};
use crate::config::{Config, HttpConfig};
use crate::core::OperationRegistry;
use crate::{BeemFlowError, Result};
use axum::{
    Router,
    extract::{Json, Path as AxumPath},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::get,
};
use parking_lot::RwLock;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tower::ServiceBuilder;
use tower_http::{
    LatencyUnit,
    cors::CorsLayer,
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
                crate::error::StorageError::NotFound { entity, id } => (
                    StatusCode::NOT_FOUND,
                    "not_found",
                    format!("{} not found: {}", entity, id),
                ),
                _ => {
                    // Log full error details internally
                    tracing::error!("Storage error: {:?}", e);
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "storage_error",
                        "An internal storage error occurred".to_string(),
                    )
                }
            },
            BeemFlowError::StepExecution { step_id, message } => {
                // Log full error details internally
                tracing::error!("Step execution failed: {} - {}", step_id, message);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "execution_error",
                    "A step execution error occurred".to_string(),
                )
            }
            BeemFlowError::OAuth(msg) => (StatusCode::UNAUTHORIZED, "auth_error", msg.clone()),
            BeemFlowError::Adapter(msg) => (StatusCode::BAD_GATEWAY, "adapter_error", msg.clone()),
            BeemFlowError::Mcp(msg) => (StatusCode::BAD_GATEWAY, "mcp_error", msg.clone()),
            BeemFlowError::Network(e) => {
                // Log full error details internally
                tracing::error!("Network error: {:?}", e);
                (
                    StatusCode::BAD_GATEWAY,
                    "network_error",
                    "A network error occurred".to_string(),
                )
            }
            _ => {
                // Log full error details internally
                tracing::error!("Internal error: {:?}", self.0);
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal_error",
                    "An internal error occurred".to_string(),
                )
            }
        };

        // Log the sanitized error response
        tracing::debug!(
            error_type = error_type,
            status = %status,
            message = %message,
            "HTTP request error response"
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
        secure: false, // Default to false for local development
    });

    // Use centralized dependency creation from core module
    let dependencies = crate::core::create_dependencies(&config).await?;

    // Create registry (takes ownership, so we clone dependencies to keep using them below)
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
    )?);

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
        session_store: session_store.clone(),
    });

    // Create OAuth middleware state
    let oauth_middleware_state = Arc::new(OAuthMiddlewareState::new(dependencies.storage.clone()));

    // Create webhook manager state
    let webhook_state = WebhookManagerState {
        event_bus: state.registry.get_dependencies().event_bus.clone(),
        registry_manager: state.registry.get_dependencies().registry_manager.clone(),
    };

    // Build router with config for CORS
    // Note: All static assets are embedded in the binary - no file system access needed
    let app = build_router(
        state,
        webhook_state,
        oauth_server_state,
        oauth_middleware_state,
        session_store,
        &http_config,
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

/// Auto-generate routes from operation metadata using macro-generated registration functions
fn build_operation_routes(state: &AppState) -> Router {
    let deps = state.registry.get_dependencies();

    // Use generated registration functions from each operation group
    // These functions call the http_route() method on each operation
    Router::new()
        .merge(crate::core::flows::flows::register_http_routes(
            deps.clone(),
        ))
        .merge(crate::core::runs::runs::register_http_routes(deps.clone()))
        .merge(crate::core::tools::tools::register_http_routes(
            deps.clone(),
        ))
        .merge(crate::core::mcp::mcp::register_http_routes(deps.clone()))
        .merge(crate::core::events::events::register_http_routes(
            deps.clone(),
        ))
        .merge(crate::core::system::system::register_http_routes(
            deps.clone(),
        ))
}

/// Build the router with all endpoints
fn build_router(
    state: AppState,
    webhook_state: WebhookManagerState,
    oauth_server_state: Arc<OAuthServerState>,
    _oauth_middleware_state: Arc<OAuthMiddlewareState>,
    _session_store: Arc<session::SessionStore>,
    http_config: &HttpConfig,
) -> Router {
    // Create OAuth server routes (authorization server endpoints)
    let oauth_server_routes = create_oauth_routes(oauth_server_state);

    // Create OAuth client routes (for connecting TO external providers)
    let oauth_client_state = Arc::new(OAuthClientState {
        oauth_client: state.oauth_client.clone(),
        storage: state.storage.clone(),
        registry_manager: state.registry.get_dependencies().registry_manager.clone(),
        session_store: state.session_store.clone(),
        template_renderer: state.template_renderer.clone(),
    });
    let oauth_client_routes = create_oauth_client_routes(oauth_client_state);

    // Build auto-generated operation routes from metadata
    let operation_routes = build_operation_routes(&state);

    // Build application routes (system endpoints + operation routes)
    // Note: health_handler and metrics_handler don't use AppState, so we merge everything first
    let app_routes = Router::new()
        // System endpoints (special handlers not in operation registry)
        .route("/healthz", get(health_handler))
        .route("/metrics", get(metrics_handler))
        // Merge auto-generated operation routes
        .merge(operation_routes);

    // Merge all routes together

    Router::new()
        // Serve all embedded static assets (CSS, HTML, etc.) from a single handler
        // Assets are compiled into the binary - no file system access needed
        .route("/static/{*path}", get(serve_static_asset))
        // OAuth 2.1 Authorization Server (RFC 6749 & 8252)
        .merge(oauth_server_routes)
        // OAuth CLIENT routes (for connecting TO external providers)
        .merge(oauth_client_routes)
        // Webhook routes (including cron webhook)
        .nest(
            "/webhooks",
            create_webhook_routes().with_state(webhook_state),
        )
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
                // CORS layer for cross-origin requests (restrictive policy)
                .layer({
                    // Build allowed origins dynamically from config
                    let origin_localhost = format!("http://localhost:{}", http_config.port)
                        .parse::<axum::http::HeaderValue>()
                        .expect("valid header value");
                    let origin_127 = format!("http://127.0.0.1:{}", http_config.port)
                        .parse::<axum::http::HeaderValue>()
                        .expect("valid header value");

                    CorsLayer::new()
                        // Allow localhost origins based on configured port
                        .allow_origin([origin_localhost, origin_127])
                        // Only allow necessary HTTP methods
                        .allow_methods([
                            axum::http::Method::GET,
                            axum::http::Method::POST,
                            axum::http::Method::PUT,
                            axum::http::Method::PATCH,
                            axum::http::Method::DELETE,
                            axum::http::Method::OPTIONS,
                        ])
                        // Only allow necessary headers
                        .allow_headers([
                            axum::http::header::CONTENT_TYPE,
                            axum::http::header::AUTHORIZATION,
                            axum::http::header::HeaderName::from_static("x-requested-with"),
                        ])
                        .allow_credentials(true)
                }),
        )
}

// ============================================================================
// STATIC ASSET HANDLERS (Truly static assets only - CSS, JS, images)
// ============================================================================
//
// Note: HTML templates with Jinja2 syntax ({{ }}, {% %}) are NOT served here.
// They are embedded in TemplateRenderer and rendered with dynamic data.
// Only serve assets that don't need server-side processing.

/// Serve embedded static assets (CSS, JS, images, fonts)
///
/// Security: All assets are embedded at compile time from trusted sources.
/// No file system access needed - everything is compiled into the binary.
///
/// To add new static assets:
/// 1. Add the file to the static/ directory
/// 2. Add a match arm below with the path and content type
/// 3. Only add truly static files (CSS, JS, images) - NOT templates
async fn serve_static_asset(AxumPath(path): AxumPath<String>) -> impl IntoResponse {
    // Match on the requested path and return (content_type, content)
    let asset = match path.as_str() {
        // CSS files
        "oauth/style.css" => (
            "text/css; charset=utf-8",
            include_str!("../../static/oauth/style.css"),
        ),
        // Add more static assets here (JS, images, fonts, etc.)
        // Example: "js/app.js" => ("application/javascript", include_str!("../../static/js/app.js")),
        // Example: "images/logo.png" => ("image/png", include_bytes!("../../static/images/logo.png")),
        _ => {
            return (StatusCode::NOT_FOUND, "Asset not found").into_response();
        }
    };

    (
        StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, asset.0)],
        asset.1,
    )
        .into_response()
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

#[cfg(test)]
mod http_test;
#[cfg(test)]
mod response_test;
#[cfg(test)]
mod session_test;
#[cfg(test)]
mod template_test;
#[cfg(test)]
mod webhook_test;
