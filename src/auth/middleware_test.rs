//! Tests for middleware

use crate::auth::middleware::{
    AllScopesValidator, AnyScopeValidator, AuthenticatedUser, RequiredScopes, ScopeValidator,
    has_all_scopes, has_any_scope, has_scope,
};
use crate::model::OAuthToken;
use chrono::Utc;

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
