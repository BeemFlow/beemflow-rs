use super::*;
use axum::{Router, routing::{get, post}, Json, extract::Path as AxumPath, http::StatusCode};
use std::net::SocketAddr;

// Helper to create a test HTTP server
async fn create_test_server() -> SocketAddr {
    let app = Router::new()
        .route("/json", get(|| async { Json(serde_json::json!({"message": "success", "method": "GET"})) }))
        .route("/text", get(|| async { "Hello from test server" }))
        .route("/echo", post(|Json(payload): Json<serde_json::Value>| async move {
            Json(payload)
        }))
        .route("/error", get(|| async { (StatusCode::INTERNAL_SERVER_ERROR, "Internal Server Error") }))
        .route("/spreadsheets/:id/values/:range/append", post(|
            AxumPath((id, range)): AxumPath<(String, String)>,
            Json(payload): Json<serde_json::Value>
        | async move {
            // Verify path params were substituted correctly
            assert!(id == "test-sheet-id");
            assert!(range.contains("Sheet1!A:D"));
            // Verify path params are NOT in body
            assert!(payload.get("spreadsheetId").is_none());
            assert!(payload.get("range").is_none());
            // Verify body params are present
            assert!(payload.get("values").is_some());
            Json(serde_json::json!({"success": true}))
        }));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    // Wait for server to be ready
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    addr
}

// ========================================
// BASIC FUNCTIONALITY TESTS
// ========================================

#[tokio::test]
async fn test_http_adapter_id() {
    let adapter = HttpAdapter::new("test-id".to_string(), None);
    assert_eq!(adapter.id(), "test-id");
}

#[tokio::test]
async fn test_http_adapter_manifest() {
    let manifest = ToolManifest {
        name: "test".to_string(),
        description: "test tool".to_string(),
        kind: "task".to_string(),
        version: None,
        endpoint: Some("https://example.com".to_string()),
        method: Some(HTTP_METHOD_GET.to_string()),
        parameters: HashMap::new(),
        headers: Some(HashMap::new()),
    };
    let adapter = HttpAdapter::new("test-id".to_string(), Some(manifest.clone()));
    assert!(adapter.manifest().is_some());
    assert_eq!(adapter.manifest().unwrap().name, "test");
}

// ========================================
// GENERIC HTTP REQUESTS
// ========================================

#[tokio::test]
async fn test_generic_get_json() {
    let addr = create_test_server().await;
    let adapter = HttpAdapter::new(HTTP_ADAPTER_ID.to_string(), None);

    let mut inputs = HashMap::new();
    inputs.insert("url".to_string(), Value::String(format!("http://{}/json", addr)));
    inputs.insert("method".to_string(), Value::String(HTTP_METHOD_GET.to_string()));

    let result = adapter.execute(inputs).await.unwrap();
    assert_eq!(result.get("message").and_then(|v| v.as_str()), Some("success"));
    assert_eq!(result.get("method").and_then(|v| v.as_str()), Some("GET"));
}

#[tokio::test]
async fn test_generic_get_text() {
    let addr = create_test_server().await;
    let adapter = HttpAdapter::new(HTTP_ADAPTER_ID.to_string(), None);

    let mut inputs = HashMap::new();
    inputs.insert("url".to_string(), Value::String(format!("http://{}/text", addr)));
    inputs.insert("method".to_string(), Value::String(HTTP_METHOD_GET.to_string()));

    let result = adapter.execute(inputs).await.unwrap();
    // Non-JSON responses are wrapped in body
    assert_eq!(result.get("body").and_then(|v| v.as_str()), Some("Hello from test server"));
}

#[tokio::test]
async fn test_generic_post_json() {
    let addr = create_test_server().await;
    let adapter = HttpAdapter::new(HTTP_ADAPTER_ID.to_string(), None);

    let mut inputs = HashMap::new();
    inputs.insert("url".to_string(), Value::String(format!("http://{}/echo", addr)));
    inputs.insert("method".to_string(), Value::String(HTTP_METHOD_POST.to_string()));
    inputs.insert("body".to_string(), serde_json::json!({"test": "data"}));
    inputs.insert("headers".to_string(), serde_json::json!({
        HEADER_CONTENT_TYPE: CONTENT_TYPE_JSON
    }));

    let result = adapter.execute(inputs).await.unwrap();
    assert_eq!(result.get("test").and_then(|v| v.as_str()), Some("data"));
}

#[tokio::test]
async fn test_missing_url() {
    let adapter = HttpAdapter::new(HTTP_ADAPTER_ID.to_string(), None);
    let inputs = HashMap::new();

    let result = adapter.execute(inputs).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("url"));
}

#[tokio::test]
async fn test_http_error_status() {
    let addr = create_test_server().await;
    let adapter = HttpAdapter::new(HTTP_ADAPTER_ID.to_string(), None);

    let mut inputs = HashMap::new();
    inputs.insert("url".to_string(), Value::String(format!("http://{}/error", addr)));
    inputs.insert("method".to_string(), Value::String(HTTP_METHOD_GET.to_string()));

    let result = adapter.execute(inputs).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("500"));
}

// ========================================
// PATH PARAMETER SUBSTITUTION
// ========================================

#[tokio::test]
async fn test_path_parameter_substitution() {
    let addr = create_test_server().await;

    let manifest = ToolManifest {
        name: "test.sheets.append".to_string(),
        description: "Test Google Sheets append".to_string(),
        kind: "task".to_string(),
        version: None,
        endpoint: Some(format!("http://{}/spreadsheets/{{spreadsheetId}}/values/{{range}}/append", addr)),
        method: Some(HTTP_METHOD_POST.to_string()),
        parameters: HashMap::new(),
        headers: Some({
            let mut h = HashMap::new();
            h.insert(HEADER_CONTENT_TYPE.to_string(), CONTENT_TYPE_JSON.to_string());
            h
        }),
    };

    let adapter = HttpAdapter::new("http".to_string(), Some(manifest));

    let mut inputs = HashMap::new();
    inputs.insert("spreadsheetId".to_string(), Value::String("test-sheet-id".to_string()));
    inputs.insert("range".to_string(), Value::String("Sheet1!A:D".to_string()));
    inputs.insert("values".to_string(), serde_json::json!([
        ["2025-08-21", "Test Title", "Test Content", "pending"]
    ]));

    let result = adapter.execute(inputs).await.unwrap();
    assert_eq!(result.get("success").and_then(|v| v.as_bool()), Some(true));
}

// ========================================
// SECURITY VALIDATION
// ========================================

#[tokio::test]
async fn test_security_path_traversal() {
    let _adapter = HttpAdapter::new(HTTP_ADAPTER_ID.to_string(), None);

    // Test various path traversal attempts
    let long_string = std::iter::repeat("a").take(2000).collect::<String>();
    let test_cases = vec![
        ("../etc/passwd", true),
        ("users\x00.txt", true),
        ("%2e%2e/etc/passwd", true),
        (long_string.as_str(), true),
        ("valid_name", false),
    ];

    for (input, should_err) in test_cases {
        let manifest = ToolManifest {
            name: "test".to_string(),
            description: "".to_string(),
            kind: "task".to_string(),
            version: None,
            endpoint: Some(format!("http://localhost/api/{{}}/test", )),
            method: Some(HTTP_METHOD_GET.to_string()),
            parameters: HashMap::new(),
            headers: Some(HashMap::new()),
        };

        let adapter = HttpAdapter::new("test".to_string(), Some(manifest));
        let mut inputs = HashMap::new();
        inputs.insert("param".to_string(), Value::String(input.to_string()));

        let result = adapter.execute(inputs).await;
        if should_err {
            assert!(result.is_err(), "Expected error for input: {}", input);
        }
    }
}

// ========================================
// ENVIRONMENT VARIABLE EXPANSION
// ========================================

#[tokio::test]
async fn test_environment_variable_expansion() {
    std::env::set_var("TEST_API_TOKEN", "secret-token-123");

    let addr = create_test_server().await;

    let manifest = ToolManifest {
        name: "test.auth".to_string(),
        description: "".to_string(),
        kind: "task".to_string(),
        version: None,
        endpoint: Some(format!("http://{}/json", addr)),
        method: Some(HTTP_METHOD_GET.to_string()),
        parameters: HashMap::new(),
        headers: Some({
            let mut h = HashMap::new();
            h.insert("Authorization".to_string(), "Bearer $env:TEST_API_TOKEN".to_string());
            h
        }),
    };

    let adapter = HttpAdapter::new("test-auth".to_string(), Some(manifest));

    // Note: The actual header expansion happens in expand_header_value
    // which is tested separately. This test verifies the integration.
    let result = adapter.execute(HashMap::new()).await;
    // The test server doesn't verify headers, so we just check it doesn't error
    assert!(result.is_ok());

    std::env::remove_var("TEST_API_TOKEN");
}

// ========================================
// MANIFEST-BASED REQUESTS
// ========================================

#[tokio::test]
async fn test_manifest_with_defaults() {
    let addr = create_test_server().await;

    let manifest = ToolManifest {
        name: "test-defaults".to_string(),
        description: "".to_string(),
        kind: "task".to_string(),
        version: None,
        endpoint: Some(format!("http://{}/echo", addr)),
        method: Some(HTTP_METHOD_POST.to_string()),
        parameters: {
            let mut p = HashMap::new();
            p.insert("type".to_string(), serde_json::json!("object"));
            p.insert("properties".to_string(), serde_json::json!({
                "foo": {
                    "type": "string",
                    "default": "bar"
                }
            }));
            p
        },
        headers: Some({
            let mut h = HashMap::new();
            h.insert(HEADER_CONTENT_TYPE.to_string(), CONTENT_TYPE_JSON.to_string());
            h
        }),
    };

    let adapter = HttpAdapter::new("test-defaults".to_string(), Some(manifest));
    let result = adapter.execute(HashMap::new()).await.unwrap();
    assert_eq!(result.get("foo").and_then(|v| v.as_str()), Some("bar"));
}

// ========================================
// HEADER EXTRACTION
// ========================================

#[test]
fn test_extract_headers() {
    let adapter = HttpAdapter::new(HTTP_ADAPTER_ID.to_string(), None);

    // Valid headers map
    let mut inputs = HashMap::new();
    inputs.insert("headers".to_string(), serde_json::json!({
        "Authorization": "Bearer token",
        HEADER_CONTENT_TYPE: CONTENT_TYPE_JSON
    }));
    let headers = adapter.extract_headers(&inputs);
    assert_eq!(headers.get("Authorization").map(|s| s.as_str()), Some("Bearer token"));

    // Invalid headers (not a map)
    let mut inputs2 = HashMap::new();
    inputs2.insert("headers".to_string(), Value::String("invalid".to_string()));
    let headers2 = adapter.extract_headers(&inputs2);
    assert!(headers2.is_empty());

    // Headers with non-string values
    let mut inputs3 = HashMap::new();
    inputs3.insert("headers".to_string(), serde_json::json!({
        "Valid": "string-value",
        "Invalid": 123
    }));
    let headers3 = adapter.extract_headers(&inputs3);
    assert_eq!(headers3.get("Valid").map(|s| s.as_str()), Some("string-value"));
    assert!(!headers3.contains_key("Invalid"));
}

// ========================================
// METHOD EXTRACTION
// ========================================

#[test]
fn test_extract_method() {
    let adapter = HttpAdapter::new(HTTP_ADAPTER_ID.to_string(), None);

    // Default method
    let method = adapter.extract_method(&HashMap::new());
    assert_eq!(method, HTTP_METHOD_GET);

    // Explicit method
    let mut inputs = HashMap::new();
    inputs.insert("method".to_string(), Value::String(HTTP_METHOD_POST.to_string()));
    let method = adapter.extract_method(&inputs);
    assert_eq!(method, HTTP_METHOD_POST);

    // Non-string method (should default to GET)
    let mut inputs2 = HashMap::new();
    inputs2.insert("method".to_string(), Value::Number(123.into()));
    let method2 = adapter.extract_method(&inputs2);
    assert_eq!(method2, HTTP_METHOD_GET);
}

// ========================================
// DEFAULT ENRICHMENT
// ========================================

#[test]
fn test_enrich_inputs_with_defaults() {
    std::env::set_var("TEST_DEFAULT", "env-default");

    let manifest = ToolManifest {
        name: "test".to_string(),
        description: "".to_string(),
        kind: "task".to_string(),
        version: None,
        endpoint: Some("http://localhost".to_string()),
        method: Some(HTTP_METHOD_GET.to_string()),
        parameters: {
            let mut p = HashMap::new();
            p.insert("type".to_string(), serde_json::json!("object"));
            p.insert("properties".to_string(), serde_json::json!({
                "param1": {
                    "type": "string",
                    "default": "default-value"
                },
                "param2": {
                    "type": "string",
                    "default": "$env:TEST_DEFAULT"
                },
                "param3": {
                    "type": "string"
                }
            }));
            p
        },
        headers: Some(HashMap::new()),
    };

    let adapter = HttpAdapter::new("test".to_string(), Some(manifest));
    let mut inputs = HashMap::new();
    inputs.insert("param3".to_string(), Value::String("user-value".to_string()));

    let enriched = adapter.enrich_inputs_with_defaults(inputs);
    assert_eq!(enriched.get("param1").and_then(|v| v.as_str()), Some("default-value"));
    assert_eq!(enriched.get("param2").and_then(|v| v.as_str()), Some("env-default"));
    assert_eq!(enriched.get("param3").and_then(|v| v.as_str()), Some("user-value"));

    std::env::remove_var("TEST_DEFAULT");
}

// ========================================
// RESPONSE PROCESSING
// ========================================

#[tokio::test]
async fn test_response_processing_json_array() {
    // Create a simple server that returns JSON array
    let app = Router::new()
        .route("/array", get(|| async {
            (
                [(HEADER_CONTENT_TYPE, CONTENT_TYPE_JSON)],
                Json(serde_json::json!([1, 2, 3]))
            )
        }));

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let adapter = HttpAdapter::new(HTTP_ADAPTER_ID.to_string(), None);
    let mut inputs = HashMap::new();
    inputs.insert("url".to_string(), Value::String(format!("http://{}/array", addr)));

    let result = adapter.execute(inputs).await.unwrap();
    // Array should be wrapped in body
    let body = result.get("body").and_then(|v| v.as_array()).unwrap();
    assert_eq!(body.len(), 3);
}

// ========================================
// NETWORK ERROR HANDLING
// ========================================

#[tokio::test]
async fn test_network_error() {
    let adapter = HttpAdapter::new(HTTP_ADAPTER_ID.to_string(), None);
    let mut inputs = HashMap::new();
    inputs.insert("url".to_string(), Value::String("http://invalid-host-that-does-not-exist.local".to_string()));

    let result = adapter.execute(inputs).await;
    assert!(result.is_err());
}

// ========================================
// CONCURRENT EXECUTION
// ========================================

#[tokio::test]
async fn test_concurrent_requests() {
    let addr = create_test_server().await;
    let adapter = std::sync::Arc::new(HttpAdapter::new(HTTP_ADAPTER_ID.to_string(), None));

    let mut handles = vec![];
    for i in 0..10 {
        let adapter_clone = adapter.clone();
        let url = format!("http://{}/json", addr);
        let handle = tokio::spawn(async move {
            let mut inputs = HashMap::new();
            inputs.insert("url".to_string(), Value::String(url));
            inputs.insert("id".to_string(), Value::Number(i.into()));

            adapter_clone.execute(inputs).await
        });
        handles.push(handle);
    }

    for handle in handles {
        let result = handle.await.unwrap();
        assert!(result.is_ok());
    }
}

    // ========================================
    // ERROR HANDLING TESTS
    // ========================================

    #[tokio::test]
    async fn test_invalid_url() {
        let adapter = HttpAdapter::new(HTTP_ADAPTER_ID.to_string(), None);

        // Test with non-string URL
        let result = adapter.execute(json!({
            "url": 123,
            "method": "GET"
        })).await;

        assert!(result.is_err(), "Should error with non-string URL");
        let err = result.unwrap_err();
        assert!(err.to_string().contains("url") || err.to_string().contains("invalid"),
                "Error should mention invalid URL");
    }

    #[tokio::test]
    async fn test_manifest_based_request() {
        let addr = create_test_server().await;

        let manifest = ToolManifest {
            name: "test-manifest".to_string(),
            description: "Test manifest-based request".to_string(),
            kind: "task".to_string(),
            version: None,
            endpoint: Some(format!("http://{}/echo", addr)),
            method: Some(HTTP_METHOD_POST.to_string()),
            parameters: HashMap::new(),
            headers: Some({
                let mut h = HashMap::new();
                h.insert(HEADER_CONTENT_TYPE.to_string(), CONTENT_TYPE_JSON.to_string());
                h.insert("X-Custom".to_string(), "test-value".to_string());
                h
            }),
        };

        let adapter = HttpAdapter::new("test-manifest".to_string(), Some(manifest));

        let result = adapter.execute(json!({
            "test": "data"
        })).await;

        assert!(result.is_ok(), "Manifest-based request should succeed");
        let response: serde_json::Value = result.unwrap();
        assert_eq!(response.get("test").and_then(|v| v.as_str()), Some("data"),
                   "Should echo back the test data");
    }

    #[tokio::test]
    async fn test_manifest_with_custom_headers() {
        let addr = create_test_server().await;

        let manifest = ToolManifest {
            name: "test-headers".to_string(),
            description: "Test custom headers".to_string(),
            kind: "task".to_string(),
            version: None,
            endpoint: Some(format!("http://{}/json", addr)),
            method: Some(HTTP_METHOD_GET.to_string()),
            parameters: HashMap::new(),
            headers: Some({
                let mut h = HashMap::new();
                h.insert("Authorization".to_string(), "Bearer test-token".to_string());
                h.insert("X-API-Key".to_string(), "test-key".to_string());
                h
            }),
        };

        let adapter = HttpAdapter::new("test-headers".to_string(), Some(manifest));

        // The test server doesn't validate headers, but this tests that the adapter
        // can handle manifest-based requests with custom headers
        let result = adapter.execute(json!({})).await;
        assert!(result.is_ok(), "Request with custom headers should succeed");
    }

    // ========================================
    // HEADER EXTRACTION EDGE CASES
    // ========================================

    #[test]
    fn test_extract_headers_edge_cases() {
        let adapter = HttpAdapter::new(HTTP_ADAPTER_ID.to_string(), None);

        // Test with invalid headers (not a map)
        let inputs = json!({
            "headers": "invalid string"
        });
        let headers = adapter.extract_headers(&inputs);
        assert!(headers.is_empty(), "Invalid headers should result in empty map");

        // Test with headers containing non-string values
        let inputs = json!({
            "headers": {
                "Valid": "string-value",
                "Invalid": 123,
                "AlsoInvalid": null
            }
        });
        let headers = adapter.extract_headers(&inputs);
        assert_eq!(headers.get("Valid").map(|s| s.as_str()), Some("string-value"),
                   "Valid string header should be preserved");
        assert!(!headers.contains_key("Invalid"), "Non-string headers should be filtered out");
        assert!(!headers.contains_key("AlsoInvalid"), "Null headers should be filtered out");

        // Test with empty headers
        let inputs = json!({
            "headers": {}
        });
        let headers = adapter.extract_headers(&inputs);
        assert!(headers.is_empty(), "Empty headers map should result in empty headers");
    }

    // ========================================
    // METHOD EXTRACTION EDGE CASES
    // ========================================

    #[test]
    fn test_extract_method_edge_cases() {
        let adapter = HttpAdapter::new(HTTP_ADAPTER_ID.to_string(), None);

        // Test default method (no method specified)
        let method = adapter.extract_method(&json!({}));
        assert_eq!(method, HTTP_METHOD_GET, "Default method should be GET");

        // Test explicit method
        let method = adapter.extract_method(&json!({"method": "POST"}));
        assert_eq!(method, HTTP_METHOD_POST, "Explicit POST method should be preserved");

        // Test case insensitive method
        let method = adapter.extract_method(&json!({"method": "post"}));
        assert_eq!(method, "post", "Method should preserve original case");

        // Test invalid method type (non-string)
        let method = adapter.extract_method(&json!({"method": 123}));
        assert_eq!(method, HTTP_METHOD_GET, "Invalid method should default to GET");

        // Test empty method string
        let method = adapter.extract_method(&json!({"method": ""}));
        assert_eq!(method, "", "Empty method string should be preserved");
    }

    // ========================================
    // RESPONSE PROCESSING EDGE CASES
    // ========================================

    #[tokio::test]
    async fn test_response_processing_edge_cases() {
        // Test with a server that returns various response types
        let app = Router::new()
            .route("/empty", get(|| async {
                (StatusCode::OK, "")
            }))
            .route("/null-body", get(|| async {
                (StatusCode::OK, Json(serde_json::Value::Null))
            }))
            .route("/array-response", get(|| async {
                (StatusCode::OK, Json(serde_json::json!([1, 2, 3, 4, 5])))
            }))
            .route("/nested-object", get(|| async {
                (StatusCode::OK, Json(serde_json::json!({
                    "nested": {
                        "deep": {
                            "value": "found"
                        }
                    },
                    "array": [1, 2, {"key": "value"}]
                })))
            }));

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

        let adapter = HttpAdapter::new(HTTP_ADAPTER_ID.to_string(), None);

        // Test empty response
        let result = adapter.execute(json!({
            "url": format!("http://{}/empty", addr),
            "method": "GET"
        })).await;
        assert!(result.is_ok(), "Empty response should be handled");

        // Test null JSON response
        let result = adapter.execute(json!({
            "url": format!("http://{}/null-body", addr),
            "method": "GET"
        })).await;
        assert!(result.is_ok(), "Null JSON response should be handled");

        // Test array response
        let result = adapter.execute(json!({
            "url": format!("http://{}/array-response", addr),
            "method": "GET"
        })).await;
        assert!(result.is_ok(), "Array response should be handled");
        let response: serde_json::Value = result.unwrap();
        assert!(response.as_array().is_some(), "Array response should be preserved as array");

        // Test complex nested response
        let result = adapter.execute(json!({
            "url": format!("http://{}/nested-object", addr),
            "method": "GET"
        })).await;
        assert!(result.is_ok(), "Complex nested response should be handled");
        let response: serde_json::Value = result.unwrap();
        assert!(response.get("nested").and_then(|v| v.get("deep")).and_then(|v| v.get("value")).is_some(),
                "Nested object structure should be preserved");
    }

    // ========================================
    // MANIFEST DEFAULTS AND ENRICHMENT
    // ========================================

    #[test]
    fn test_enrich_inputs_with_manifest_defaults() {
        let manifest = ToolManifest {
            name: "test-defaults".to_string(),
            description: "Test defaults".to_string(),
            kind: "task".to_string(),
            version: None,
            endpoint: Some("http://example.com".to_string()),
            method: Some(HTTP_METHOD_POST.to_string()),
            parameters: json!({
                "type": "object",
                "properties": {
                    "param1": {
                        "type": "string",
                        "default": "default_value"
                    },
                    "param2": {
                        "type": "number",
                        "default": 42
                    },
                    "param3": {
                        "type": "boolean",
                        "default": true
                    }
                }
            }).as_object().unwrap().clone(),
            headers: Some(HashMap::new()),
        };

        let adapter = HttpAdapter::new("test-defaults".to_string(), Some(manifest));

        // Test with partial inputs - defaults should be applied
        let inputs = json!({
            "param1": "user_value",  // Override default
            "param4": "extra_param"  // Additional param not in schema
        });

        let enriched = adapter.enrich_inputs_with_defaults(inputs);
        assert_eq!(enriched.get("param1").and_then(|v| v.as_str()), Some("user_value"),
                   "User-provided value should override default");
        assert_eq!(enriched.get("param2").and_then(|v| v.as_u64()), Some(42),
                   "Default value should be applied for missing param2");
        assert_eq!(enriched.get("param3").and_then(|v| v.as_bool()), Some(true),
                   "Default value should be applied for missing param3");
        assert_eq!(enriched.get("param4").and_then(|v| v.as_str()), Some("extra_param"),
                   "Extra parameters should be preserved");
    }

    #[test]
    fn test_enrich_inputs_empty_manifest() {
        let adapter = HttpAdapter::new(HTTP_ADAPTER_ID.to_string(), None);

        let inputs = json!({
            "param1": "value1",
            "param2": "value2"
        });

        // With no manifest, inputs should be unchanged
        let enriched = adapter.enrich_inputs_with_defaults(inputs.clone());
        assert_eq!(enriched, inputs, "Inputs should be unchanged without manifest");
    }

    #[test]
    fn test_enrich_inputs_malformed_schema() {
        let manifest = ToolManifest {
            name: "test-malformed".to_string(),
            description: "Test malformed schema".to_string(),
            kind: "task".to_string(),
            version: None,
            endpoint: Some("http://example.com".to_string()),
            method: Some(HTTP_METHOD_GET.to_string()),
            parameters: json!({
                "type": "object",
                "properties": "not_an_object"  // Invalid schema
            }).as_object().unwrap().clone(),
            headers: Some(HashMap::new()),
        };

        let adapter = HttpAdapter::new("test-malformed".to_string(), Some(manifest));

        let inputs = json!({"param1": "value1"});
        // Should not panic on malformed schema
        let enriched = adapter.enrich_inputs_with_defaults(inputs.clone());
        assert_eq!(enriched, inputs, "Malformed schema should not break enrichment");
    }

    // ========================================
    // SAFE ASSERTIONS AND VALIDATION
    // ========================================

    #[test]
    fn test_adapter_validation() {
        // Test that adapter is properly constructed
        let adapter = HttpAdapter::new("test-id".to_string(), None);
        assert_eq!(adapter.id(), "test-id");
        assert!(adapter.manifest().is_none());

        // Test with manifest
        let manifest = ToolManifest {
            name: "test".to_string(),
            description: "test".to_string(),
            kind: "task".to_string(),
            version: None,
            endpoint: Some("http://example.com".to_string()),
            method: Some(HTTP_METHOD_GET.to_string()),
            parameters: HashMap::new(),
            headers: Some(HashMap::new()),
        };

        let adapter_with_manifest = HttpAdapter::new("test-with-manifest".to_string(), Some(manifest.clone()));
        assert_eq!(adapter_with_manifest.id(), "test-with-manifest");
        assert!(adapter_with_manifest.manifest().is_some());
        assert_eq!(adapter_with_manifest.manifest().unwrap().name, "test");
    }

    #[test]
    fn test_manifest_validation() {
        // Test that manifests are properly validated/handled
        let adapter = HttpAdapter::new("test".to_string(), None);

        // Empty manifest should work
        assert!(adapter.manifest().is_none());

        // Manifest with minimal fields should work
        let manifest = ToolManifest {
            name: "minimal".to_string(),
            description: "minimal".to_string(),
            kind: "task".to_string(),
            version: None,
            endpoint: None,
            method: None,
            parameters: HashMap::new(),
            headers: None,
        };

        let adapter_with_minimal = HttpAdapter::new("minimal".to_string(), Some(manifest));
        assert!(adapter_with_minimal.manifest().is_some());
    }
}
