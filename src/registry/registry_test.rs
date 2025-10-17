use super::*;

#[test]
fn test_registry_operations() {
    let mut registry = Registry::new();

    let entry = RegistryEntry {
        entry_type: "tool".to_string(),
        name: "test.tool".to_string(),
        display_name: None,
        icon: None,
        description: Some("Test tool".to_string()),
        kind: Some("task".to_string()),
        version: None,
        registry: Some("local".to_string()),
        parameters: None,
        endpoint: Some("https://example.com/api".to_string()),
        method: Some("GET".to_string()),
        headers: None,
        command: None,
        args: None,
        env: None,
        transport: None,
        port: None,
        client_id: None,
        client_secret: None,
        auth_url: None,
        token_url: None,
        scopes: None,
        auth_params: None,
        webhook: None,
    };

    registry.add(entry.clone());
    assert!(registry.get("test.tool").is_some());

    let list = registry.list_all();
    assert_eq!(list.len(), 1);

    registry.remove("test.tool");
    assert!(registry.get("test.tool").is_none());
}

#[tokio::test]
async fn test_default_registry() {
    let registry = DefaultRegistry::new();
    let entries = registry.list_servers().await.unwrap();

    // Should have entries from default.json
    assert!(!entries.is_empty());

    // Check for known tools
    let has_openai = entries.iter().any(|e| e.name.contains("openai"));
    assert!(has_openai, "Default registry should contain OpenAI tools");
}

#[tokio::test]
async fn test_local_registry() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Create a temporary registry file
    let mut temp_file = NamedTempFile::new().unwrap();
    let registry_content = r#"[
        {
            "type": "tool",
            "name": "local.test",
            "description": "Local test tool",
            "endpoint": "http://localhost:8080/test"
        }
    ]"#;
    temp_file.write_all(registry_content.as_bytes()).unwrap();

    let registry = LocalRegistry::new(temp_file.path().to_str().unwrap());
    let entries = registry.list_servers().await.unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].name, "local.test");
    assert_eq!(entries[0].registry, Some("local".to_string()));
}

#[tokio::test]
async fn test_registry_manager_priority() {
    use std::io::Write;
    use tempfile::NamedTempFile;

    // Create local registry with override
    let mut temp_file = NamedTempFile::new().unwrap();
    let registry_content = r#"[
        {
            "type": "tool",
            "name": "openai.chat_completion",
            "description": "LOCAL OVERRIDE",
            "endpoint": "http://localhost:9999/override"
        }
    ]"#;
    temp_file.write_all(registry_content.as_bytes()).unwrap();

    let local = LocalRegistry::new(temp_file.path().to_str().unwrap());
    let default = DefaultRegistry::new();

    let manager = RegistryManager::new(vec![Box::new(local), Box::new(default)]);

    // Get the entry - should use local override
    let entry = manager.get_server("openai.chat_completion").await.unwrap();
    assert!(entry.is_some());

    let entry = entry.unwrap();
    assert_eq!(entry.description, Some("LOCAL OVERRIDE".to_string()));
}

#[tokio::test]
async fn test_registry_manager_fallback() {
    let local = LocalRegistry::new("/nonexistent/path");
    let default = DefaultRegistry::new();

    let manager = RegistryManager::new(vec![Box::new(local), Box::new(default)]);

    // Should fall back to default even if local fails
    let entries = manager.list_all_servers().await.unwrap();
    assert!(
        !entries.is_empty(),
        "Should have entries from default registry"
    );
}
