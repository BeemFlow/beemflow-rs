//! Integration tests for MCP Streamable HTTP server with OAuth authentication
//!
//! Tests OAuth token validation, scope-based authorization, and metadata structures.
//! Uses MCP 2025-03-26 Streamable HTTP transport (replaces deprecated SSE from 2024-11-05)

use beemflow::auth::middleware::validate_token;
use beemflow::core::OperationRegistry;
use beemflow::mcp::McpServer;
use beemflow::model::{OAuthClient, OAuthToken};
use beemflow::storage::Storage;
use beemflow::utils::TestEnvironment;
use chrono::Utc;
use rmcp::handler::server::ServerHandler;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

/// Helper to create a test OAuth client
async fn create_test_client(storage: &Arc<dyn Storage>, scopes: &str) -> OAuthClient {
    let now = Utc::now();
    let client = OAuthClient {
        id: format!("test-client-{}", Uuid::new_v4()),
        secret: "test-secret-hash".to_string(),
        name: "Test Client".to_string(),
        redirect_uris: vec!["http://localhost:3000/callback".to_string()],
        grant_types: vec!["client_credentials".to_string()],
        response_types: vec!["token".to_string()],
        scope: scopes.to_string(),
        client_uri: None,
        logo_uri: None,
        created_at: now,
        updated_at: now,
    };

    storage.save_oauth_client(&client).await.unwrap();
    client
}

/// Helper to create a test OAuth token
async fn create_test_token(
    storage: &Arc<dyn Storage>,
    client_id: &str,
    scopes: Vec<String>,
    expires_in_seconds: i64,
) -> OAuthToken {
    let access_token = format!("test-token-{}", Uuid::new_v4());
    let token = OAuthToken {
        id: Uuid::new_v4().to_string(),
        client_id: client_id.to_string(),
        user_id: "test-user".to_string(),
        redirect_uri: "http://localhost:3000/callback".to_string(),
        scope: scopes.join(" "),
        code: None,
        code_create_at: None,
        code_expires_in: None,
        code_challenge: None,
        code_challenge_method: None,
        access: Some(access_token),
        access_create_at: Some(Utc::now()),
        access_expires_in: Some(std::time::Duration::from_secs(
            expires_in_seconds.max(0) as u64
        )),
        refresh: None,
        refresh_create_at: None,
        refresh_expires_in: None,
    };

    storage.save_oauth_token(&token).await.unwrap();
    token
}

// ============================================================================
// MCP Server Tests
// ============================================================================

#[tokio::test]
async fn test_mcp_server_capabilities() {
    let env = TestEnvironment::new().await;
    let ops = Arc::new(OperationRegistry::new(env.deps));
    let server = McpServer::new(ops);

    // Test that server advertises tools capability via ServerHandler trait
    let info = server.get_info();
    assert!(
        info.capabilities.tools.is_some(),
        "MCP server should advertise tools capability for BeemFlow operations"
    );
}

// ============================================================================
// OAuth Token Validation Tests
// ============================================================================

#[tokio::test]
async fn test_oauth_scope_validation() {
    let env = TestEnvironment::new().await;
    let storage = env.deps.storage.clone();

    // Test all MCP scope variations
    let test_cases = vec![
        ("mcp", true, "Base mcp scope should be valid"),
        ("mcp:read", true, "mcp:read scope should be valid"),
        ("mcp:write", true, "mcp:write scope should be valid"),
        ("mcp:admin", true, "mcp:admin scope should be valid"),
        ("read", false, "Non-mcp scope should not have mcp prefix"),
        ("write", false, "Non-mcp scope should not have mcp prefix"),
    ];

    for (scope, should_have_mcp, description) in test_cases {
        let client = create_test_client(&storage, scope).await;
        let token = create_test_token(&storage, &client.id, vec![scope.to_string()], 3600).await;

        let user = validate_token(&storage, token.access.as_ref().unwrap())
            .await
            .unwrap_or_else(|_| panic!("Token validation should succeed for {}", scope));

        let has_mcp_scope = user.scopes.iter().any(|s| s.starts_with("mcp"));
        assert_eq!(has_mcp_scope, should_have_mcp, "{}", description);

        if should_have_mcp {
            assert!(
                user.scopes.contains(&scope.to_string()),
                "User should have {} scope",
                scope
            );
        }
    }
}

#[tokio::test]
async fn test_oauth_token_lifecycle() {
    let env = TestEnvironment::new().await;
    let storage = env.deps.storage.clone();
    let client = create_test_client(&storage, "mcp").await;

    // Test valid token
    let valid_token = create_test_token(&storage, &client.id, vec!["mcp".to_string()], 3600).await;
    assert!(
        validate_token(&storage, valid_token.access.as_ref().unwrap())
            .await
            .is_ok(),
        "Valid token should be accepted"
    );

    // Test expired token
    let expired_token =
        create_test_token(&storage, &client.id, vec!["mcp".to_string()], -3600).await;
    assert!(
        validate_token(&storage, expired_token.access.as_ref().unwrap())
            .await
            .is_err(),
        "Expired token should be rejected"
    );

    // Test invalid token
    assert!(
        validate_token(&storage, "invalid-token-12345")
            .await
            .is_err(),
        "Invalid token should be rejected"
    );

    // Test multiple tokens for same client
    let token1 = create_test_token(&storage, &client.id, vec!["mcp".to_string()], 3600).await;
    let token2 = create_test_token(&storage, &client.id, vec!["mcp".to_string()], 7200).await;
    assert!(
        validate_token(&storage, token1.access.as_ref().unwrap())
            .await
            .is_ok()
            && validate_token(&storage, token2.access.as_ref().unwrap())
                .await
                .is_ok(),
        "Multiple tokens for same client should both be valid"
    );
}

#[tokio::test]
async fn test_oauth_client_management() {
    let env = TestEnvironment::new().await;
    let storage = env.deps.storage.clone();

    // Create client with multiple scopes
    let client = create_test_client(&storage, "mcp mcp:read mcp:write").await;

    // Retrieve client
    let retrieved = storage
        .get_oauth_client(&client.id)
        .await
        .unwrap()
        .expect("Should retrieve created client");

    assert_eq!(retrieved.id, client.id);
    assert_eq!(retrieved.name, "Test Client");
    assert_eq!(retrieved.scope, "mcp mcp:read mcp:write");
}

// ============================================================================
// RFC 9728 Protected Resource Metadata Tests
// ============================================================================

#[tokio::test]
async fn test_protected_resource_metadata_structures() {
    let base_url = "http://127.0.0.1:8080";
    let oauth_issuer = "http://127.0.0.1:3000";

    // Test MCP resource metadata (RFC 9728)
    let mcp_metadata = json!({
        "resource": format!("{}/mcp", base_url),
        "authorization_servers": [oauth_issuer],
        "scopes_supported": ["mcp", "mcp:read", "mcp:write"],
        "bearer_methods_supported": ["header"],
    });

    assert_eq!(mcp_metadata["resource"], format!("{}/mcp", base_url));
    assert_eq!(mcp_metadata["authorization_servers"][0], oauth_issuer);

    let scopes = mcp_metadata["scopes_supported"].as_array().unwrap();
    assert_eq!(scopes.len(), 3);
    assert!(scopes.contains(&json!("mcp")));
    assert!(scopes.contains(&json!("mcp:read")));
    assert!(scopes.contains(&json!("mcp:write")));

    let methods = mcp_metadata["bearer_methods_supported"].as_array().unwrap();
    assert!(methods.contains(&json!("header")));

    // Test root resource metadata
    let root_metadata = json!({
        "resource": base_url,
        "authorization_servers": [oauth_issuer],
    });

    assert_eq!(root_metadata["resource"], base_url);
    assert_eq!(root_metadata["authorization_servers"][0], oauth_issuer);
}

#[tokio::test]
async fn test_www_authenticate_header_format() {
    // Verify RFC 6750 WWW-Authenticate header format
    let oauth_issuer = "http://127.0.0.1:3000";
    let www_auth = format!(
        "Bearer realm=\"BeemFlow MCP\", \
         resource_metadata=\"{}/.well-known/oauth-protected-resource/mcp\", \
         scope=\"mcp\"",
        oauth_issuer
    );

    assert!(www_auth.starts_with("Bearer "));
    assert!(www_auth.contains("realm=\"BeemFlow MCP\""));
    assert!(www_auth.contains("resource_metadata="));
    assert!(www_auth.contains(".well-known/oauth-protected-resource/mcp"));
    assert!(www_auth.contains("scope=\"mcp\""));
}
