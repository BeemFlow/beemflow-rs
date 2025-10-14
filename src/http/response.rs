//! HTTP response helpers for standardized API responses
//!
//! Provides helpers matching Go's WriteHTTPError and WriteHTTPJSON behavior
//! for maintaining API compatibility.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

/// HTTP error response matching Go's HTTPErrorResponse format
#[derive(Debug, Serialize)]
struct HttpErrorResponse {
    error: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    message: Option<String>,
    code: u16,
}

/// Write a standardized HTTP error response
///
/// Matches Go's WriteHTTPError behavior:
/// - Sets Content-Type: application/json
/// - Returns JSON with {error, message, code} format
/// - Falls back to plain text if JSON encoding fails
///
/// # Example
/// ```ignore
/// return write_http_error("Invalid request", StatusCode::BAD_REQUEST);
/// ```
pub fn write_http_error(message: impl Into<String>, status: StatusCode) -> Response {
    let message = message.into();
    let response = HttpErrorResponse {
        error: status
            .canonical_reason()
            .unwrap_or("Unknown Error")
            .to_string(),
        message: if message.is_empty() {
            None
        } else {
            Some(message.clone())
        },
        code: status.as_u16(),
    };

    match serde_json::to_vec(&response) {
        Ok(_) => (status, Json(response)).into_response(),
        Err(_) => {
            // Fallback to plain text if JSON marshaling fails
            (status, format!("Error: {}", message)).into_response()
        }
    }
}

/// Write a JSON response with proper headers
///
/// Matches Go's WriteHTTPJSON behavior:
/// - Sets Content-Type: application/json
/// - Returns 200 OK with JSON body
/// - Returns 500 error if JSON encoding fails
///
/// # Example
/// ```ignore
/// return write_http_json(json!({"status": "ok"}));
/// ```
pub fn write_http_json<T: Serialize>(value: T) -> Response {
    match serde_json::to_value(&value) {
        Ok(json_value) => (StatusCode::OK, Json(json_value)).into_response(),
        Err(err) => {
            // Return error if encoding fails
            write_http_error(
                format!("Failed to encode response: {}", err),
                StatusCode::INTERNAL_SERVER_ERROR,
            )
        }
    }
}

/// Write a pretty JSON response with indentation
///
/// Matches Go's WriteHTTPJSONIndent behavior:
/// - Sets Content-Type: application/json
/// - Returns 200 OK with pretty-printed JSON body
/// - Returns 500 error if JSON encoding fails
///
/// # Example
/// ```ignore
/// return write_http_json_indent(json!({"status": "ok"}));
/// ```
pub fn write_http_json_indent<T: Serialize>(value: T) -> Response {
    match serde_json::to_string_pretty(&value) {
        Ok(json_str) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, "application/json")],
            json_str,
        )
            .into_response(),
        Err(err) => write_http_error(
            format!("Failed to encode response: {}", err),
            StatusCode::INTERNAL_SERVER_ERROR,
        ),
    }
}

/// Create a success JSON response with data
///
/// Convenience helper for common success responses.
pub fn success_response<T: Serialize>(data: T) -> Response {
    write_http_json(data)
}

/// Create an error response from a BeemFlowError
///
/// Maps error types to appropriate HTTP status codes.
pub fn error_from_beemflow(err: crate::BeemFlowError) -> Response {
    use crate::BeemFlowError;

    let (status, message) = match err {
        BeemFlowError::Validation(msg) => (StatusCode::BAD_REQUEST, msg),
        BeemFlowError::Storage(e) => match e {
            crate::error::StorageError::NotFound { entity, id } => (
                StatusCode::NOT_FOUND,
                format!("{} not found: {}", entity, id),
            ),
            _ => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        },
        BeemFlowError::StepExecution { step_id, message } => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Step {} failed: {}", step_id, message),
        ),
        BeemFlowError::OAuth(msg) => (StatusCode::UNAUTHORIZED, msg),
        BeemFlowError::Adapter(msg) => (StatusCode::BAD_GATEWAY, msg),
        BeemFlowError::Mcp(msg) => (StatusCode::BAD_GATEWAY, msg),
        BeemFlowError::Network(e) => (StatusCode::BAD_GATEWAY, e.to_string()),
        _ => (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    };

    write_http_error(message, status)
}
