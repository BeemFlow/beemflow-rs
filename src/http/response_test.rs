//! Tests for response

use crate::http::response::{write_http_error, write_http_json, write_http_json_indent};
use axum::http::StatusCode;
use serde_json::json;

#[tokio::test]
async fn test_write_http_error() {
    let response = write_http_error("Test error", StatusCode::BAD_REQUEST);
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn test_write_http_json() {
    let data = json!({"test": "value"});
    let response = write_http_json(data);
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_write_http_json_indent() {
    let data = json!({"test": "value"});
    let response = write_http_json_indent(data);
    assert_eq!(response.status(), StatusCode::OK);
}
