use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

/// Test structs matching the manager.rs implementation
#[derive(Debug, Serialize)]
struct JsonRpcRequest {
    jsonrpc: String,
    id: i64,
    method: String,
    params: Value,
}

#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[serde(rename = "jsonrpc")]
    _jsonrpc: String,
    #[serde(rename = "id")]
    _id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcError {
    code: i32,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    _data: Option<Value>,
}

#[test]
fn test_jsonrpc_request_serialization() {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: 1,
        method: "initialize".to_string(),
        params: json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "beemflow",
                "version": "0.2.0",
            }
        }),
    };

    let serialized = serde_json::to_string(&request).unwrap();
    assert!(serialized.contains(r#""jsonrpc":"2.0""#));
    assert!(serialized.contains(r#""id":1"#));
    assert!(serialized.contains(r#""method":"initialize""#));
    assert!(serialized.contains(r#""protocolVersion":"2024-11-05""#));
}

#[test]
fn test_jsonrpc_response_deserialization_with_result() {
    // This is the exact format that was causing the bug
    let json_str = r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[{"name":"test_tool","description":"A test tool","inputSchema":{"type":"object","properties":{}}}]}}"#;

    let response: JsonRpcResponse = serde_json::from_str(json_str).unwrap();

    assert!(response.result.is_some());
    assert!(response.error.is_none());

    let result = response.result.unwrap();
    assert!(result.get("tools").is_some());
    let tools = result.get("tools").unwrap().as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].get("name").unwrap().as_str().unwrap(), "test_tool");
}

#[test]
fn test_jsonrpc_response_deserialization_with_error() {
    let json_str =
        r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32601,"message":"Method not found"}}"#;

    let response: JsonRpcResponse = serde_json::from_str(json_str).unwrap();

    assert!(response.result.is_none());
    assert!(response.error.is_some());

    let error = response.error.unwrap();
    assert_eq!(error.code, -32601);
    assert_eq!(error.message, "Method not found");
}

#[test]
fn test_jsonrpc_response_deserialization_empty_result() {
    let json_str = r#"{"jsonrpc":"2.0","id":2,"result":{}}"#;

    let response: JsonRpcResponse = serde_json::from_str(json_str).unwrap();

    assert!(response.result.is_some());
    assert!(response.error.is_none());
    assert!(response.result.unwrap().is_object());
}

#[test]
fn test_jsonrpc_response_missing_optional_fields() {
    // Response with only required fields
    let json_str = r#"{"jsonrpc":"2.0","id":3}"#;

    let response: Result<JsonRpcResponse, _> = serde_json::from_str(json_str);
    // This should succeed - result and error are optional
    assert!(response.is_ok());

    let response = response.unwrap();
    assert!(response.result.is_none());
    assert!(response.error.is_none());
}

#[test]
fn test_jsonrpc_response_with_tools_list() {
    // Simulating a real tools/list response
    let json_str = r#"{
        "jsonrpc": "2.0",
        "id": 2,
        "result": {
            "tools": [
                {
                    "name": "filesystem_read",
                    "description": "Read file contents",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"}
                        },
                        "required": ["path"]
                    }
                },
                {
                    "name": "filesystem_write",
                    "description": "Write file contents",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "path": {"type": "string"},
                            "content": {"type": "string"}
                        },
                        "required": ["path", "content"]
                    }
                }
            ]
        }
    }"#;

    let response: JsonRpcResponse = serde_json::from_str(json_str).unwrap();

    assert!(response.result.is_some());
    let result = response.result.unwrap();
    let tools = result.get("tools").unwrap().as_array().unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(
        tools[0].get("name").unwrap().as_str().unwrap(),
        "filesystem_read"
    );
    assert_eq!(
        tools[1].get("name").unwrap().as_str().unwrap(),
        "filesystem_write"
    );
}

#[test]
fn test_jsonrpc_response_with_tool_call_result() {
    // Simulating a tools/call response
    let json_str = r#"{
        "jsonrpc": "2.0",
        "id": 5,
        "result": {
            "content": [
                {
                    "type": "text",
                    "text": "File contents here"
                }
            ]
        }
    }"#;

    let response: JsonRpcResponse = serde_json::from_str(json_str).unwrap();

    assert!(response.result.is_some());
    let result = response.result.unwrap();
    assert!(result.get("content").is_some());
}

#[test]
fn test_jsonrpc_error_deserialization_with_data() {
    let json_str = r#"{
        "jsonrpc": "2.0",
        "id": 1,
        "error": {
            "code": -32602,
            "message": "Invalid params",
            "data": {"expected": "string", "got": "number"}
        }
    }"#;

    let response: JsonRpcResponse = serde_json::from_str(json_str).unwrap();

    assert!(response.error.is_some());
    let error = response.error.unwrap();
    assert_eq!(error.code, -32602);
    assert_eq!(error.message, "Invalid params");
}

#[test]
fn test_jsonrpc_request_tools_call() {
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: 3,
        method: "tools/call".to_string(),
        params: json!({
            "name": "filesystem_read",
            "arguments": {
                "path": "/tmp/test.txt"
            }
        }),
    };

    let serialized = serde_json::to_string(&request).unwrap();
    assert!(serialized.contains(r#""method":"tools/call""#));
    assert!(serialized.contains(r#""name":"filesystem_read""#));
}

#[test]
fn test_jsonrpc_notification_format() {
    // Notifications don't have an ID
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
    });

    let serialized = serde_json::to_string(&notification).unwrap();
    assert!(serialized.contains(r#""jsonrpc":"2.0""#));
    assert!(serialized.contains(r#""method":"notifications/initialized""#));
    assert!(!serialized.contains(r#""id""#));
}

#[test]
fn test_jsonrpc_response_real_world_format() {
    // Test the exact error message from the bug report
    // "missing field `_jsonrpc` at line 1 column 224"
    // This suggests the JSON had jsonrpc field but we expected _jsonrpc

    let json_str = r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{"logging":{},"prompts":{"listChanged":true},"resources":{"subscribe":true,"listChanged":true},"tools":{"listChanged":true}},"serverInfo":{"name":"example-server","version":"1.0.0"}}}"#;

    let response: Result<JsonRpcResponse, _> = serde_json::from_str(json_str);
    assert!(
        response.is_ok(),
        "Failed to deserialize real-world response: {:?}",
        response.err()
    );

    let response = response.unwrap();
    assert!(response.result.is_some());
}

#[test]
fn test_jsonrpc_batch_responses() {
    // Test that we can deserialize multiple responses in sequence
    let responses = [
        r#"{"jsonrpc":"2.0","id":1,"result":{"initialized":true}}"#,
        r#"{"jsonrpc":"2.0","id":2,"result":{"tools":[]}}"#,
        r#"{"jsonrpc":"2.0","id":3,"error":{"code":-32600,"message":"Invalid Request"}}"#,
    ];

    for (i, json_str) in responses.iter().enumerate() {
        let response: Result<JsonRpcResponse, _> = serde_json::from_str(json_str);
        assert!(
            response.is_ok(),
            "Failed to deserialize response {}: {:?}",
            i,
            response.err()
        );
    }
}

#[test]
fn test_jsonrpc_response_preserve_extra_fields() {
    // JSON-RPC allows extra fields - make sure we don't break on them
    let json_str =
        r#"{"jsonrpc":"2.0","id":1,"result":{"foo":"bar"},"_meta":{"timestamp":123456}}"#;

    let response: Result<JsonRpcResponse, _> = serde_json::from_str(json_str);
    assert!(response.is_ok());
}
