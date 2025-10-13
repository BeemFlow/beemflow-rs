use super::*;

#[test]
fn test_validate_redirect_uri() {
    // Valid HTTPS URIs
    assert!(is_valid_redirect_uri("https://example.com/callback", false));
    assert!(is_valid_redirect_uri("https://app.example.com/oauth/callback", false));

    // Valid localhost URIs (when allowed)
    assert!(is_valid_redirect_uri("http://localhost:3000/callback", true));
    assert!(is_valid_redirect_uri("http://127.0.0.1:8080/callback", true));

    // Invalid URIs
    assert!(!is_valid_redirect_uri("http://example.com/callback", false)); // HTTP not allowed
    assert!(!is_valid_redirect_uri("http://localhost:3000/callback", false)); // localhost not allowed
    assert!(!is_valid_redirect_uri("https://example.com/callback#fragment", false)); // No fragments
    assert!(!is_valid_redirect_uri("", false)); // Empty
    assert!(!is_valid_redirect_uri(&"a".repeat(3000), false)); // Too long
}

#[test]
fn test_generate_secrets() {
    let secret1 = generate_client_secret();
    let secret2 = generate_client_secret();

    // Should be different
    assert_ne!(secret1, secret2);

    // Should be valid base64
    assert!(!secret1.is_empty());
    assert!(!secret2.is_empty());

    // Should be reasonable length
    assert!(secret1.len() > 20);
}
