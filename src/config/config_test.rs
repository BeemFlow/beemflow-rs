use super::*;
use crate::config::{Config, McpServerConfig, RegistryConfig};
use std::fs;
use tempfile::TempDir;

#[test]
fn test_default_config() {
    let config = Config::default();
    assert_eq!(config.storage.driver, "sqlite");
    assert!(config.http.is_some());
    assert_eq!(config.http.unwrap().port, 3330);
}

#[test]
fn test_config_serialization() {
    let config = Config::default();
    let json = serde_json::to_string_pretty(&config).unwrap();
    let parsed: Config = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.storage.driver, "sqlite");
}

#[test]
fn test_config_validation() {
    let mut config = Config::default();
    assert!(config.validate().is_ok());

    config.storage.driver = String::new();
    assert!(config.validate().is_err());
}

#[test]
fn test_expand_env_value() {
    unsafe {
        std::env::set_var("TEST_VAR", "test_value");
    }
    assert_eq!(expand_env_value("$env:TEST_VAR"), "test_value");
    assert_eq!(expand_env_value("plain_value"), "plain_value");
}

#[test]
fn test_upsert_mcp_server() {
    let mut config = Config::default();

    config.upsert_mcp_server(
        "test".to_string(),
        McpServerConfig {
            command: "npx".to_string(),
            args: Some(vec!["-y".to_string(), "test-server".to_string()]),
            env: None,
            port: None,
            transport: Some("stdio".to_string()),
            endpoint: None,
        },
    );

    assert!(config.mcp_servers.is_some());
    assert!(config.mcp_servers.as_ref().unwrap().contains_key("test"));
}

#[test]
fn test_config_load_from_file() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("test_config.json");

    let config_content = r#"
{
    "storage": {
        "driver": "sqlite",
        "dsn": ":memory:"
    },
    "http": {
        "host": "localhost",
        "port": 3001
    }
}
"#;

    fs::write(&config_path, config_content).unwrap();
    let config = Config::load_from_path(&config_path).unwrap();

    assert_eq!(config.storage.driver, "sqlite");
    assert_eq!(config.storage.dsn, ":memory:");
    assert_eq!(config.http.unwrap().port, 3001);
}

#[test]
fn test_config_save_to_file() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("saved_config.json");

    let mut config = Config::default();
    config.http.as_mut().unwrap().port = 8080;

    config.save_to_path(&config_path).unwrap();

    let saved_content = fs::read_to_string(&config_path).unwrap();
    let loaded_config: Config = serde_json::from_str(&saved_content).unwrap();

    assert_eq!(loaded_config.http.unwrap().port, 8080);
}

#[test]
fn test_mcp_server_config_deserialize() {
    // Test URL string deserialization
    let url_config: McpServerConfig = serde_json::from_str("\"https://example.com/mcp\"").unwrap();
    assert_eq!(
        url_config.endpoint,
        Some("https://example.com/mcp".to_string())
    );
    assert_eq!(url_config.transport, Some("http".to_string()));

    // Test object deserialization
    let obj_config: McpServerConfig = serde_json::from_str(
        r#"
{
    "command": "node",
    "args": ["server.js"],
    "transport": "stdio"
}
"#,
    )
    .unwrap();
    assert_eq!(obj_config.command, "node");
    assert_eq!(obj_config.args, Some(vec!["server.js".to_string()]));
    assert_eq!(obj_config.transport, Some("stdio".to_string()));
}

#[test]
fn test_registry_config() {
    let reg = RegistryConfig {
        registry_type: "smithery".to_string(),
        url: Some("https://registry.smithery.ai/servers".to_string()),
        path: None,
        api_key: Some("test_key".to_string()),
    };

    assert_eq!(reg.registry_type, "smithery");
    assert_eq!(
        reg.url,
        Some("https://registry.smithery.ai/servers".to_string())
    );
    assert_eq!(reg.api_key, Some("test_key".to_string()));
}

#[test]
fn test_load_mcp_servers_from_registry() {
    let temp_dir = TempDir::new().unwrap();
    let registry_path = temp_dir.path().join("registry.json");

    let registry_content = r#"
[
    {
        "type": "mcp_server",
        "name": "test_server",
        "command": "node",
        "args": ["server.js"],
        "transport": "stdio"
    }
]
"#;

    fs::write(&registry_path, registry_content).unwrap();

    let servers = load_mcp_servers_from_registry(&registry_path).unwrap();
    assert!(servers.contains_key("test_server"));

    let server = &servers["test_server"];
    assert_eq!(server.command, "node");
    assert_eq!(server.args, Some(vec!["server.js".to_string()]));
    assert_eq!(server.transport, Some("stdio".to_string()));
}

#[test]
fn test_get_merged_mcp_server_config() {
    let mut config = Config::default();

    // Add an MCP server to config
    config.upsert_mcp_server(
        "test_server".to_string(),
        McpServerConfig {
            command: "custom_node".to_string(),
            args: Some(vec!["custom_server.js".to_string()]),
            env: None,
            port: None,
            transport: Some("stdio".to_string()),
            endpoint: None,
        },
    );

    // This should return the config version since no registry is set up in test
    let result = config.get_merged_mcp_config("test_server");
    assert!(result.is_ok());

    let merged = result.unwrap();
    assert_eq!(merged.command, "custom_node");
    assert_eq!(merged.args, Some(vec!["custom_server.js".to_string()]));
}

#[test]
fn test_inject_env_vars_into_registry() {
    let mut reg_map: HashMap<String, Value> = serde_json::json!({
        "type": "smithery",
        "url": "$env:REGISTRY_URL",
        "apiKey": "$env:API_KEY"
    })
    .as_object()
    .unwrap()
    .clone()
    .into_iter()
    .collect();

    unsafe {
        std::env::set_var("REGISTRY_URL", "https://test.registry.com");
        std::env::set_var("API_KEY", "test_api_key");
    }

    inject_env_vars_into_registry(&mut reg_map);

    assert_eq!(reg_map["url"], "https://test.registry.com");
    assert_eq!(reg_map["apiKey"], "test_api_key");
    assert_eq!(reg_map["type"], "smithery");
}

#[test]
fn test_load_and_inject_registries() {
    let temp_dir = TempDir::new().unwrap();
    let config_path = temp_dir.path().join("config.json");

    let config_content = r#"
{
    "storage": {
        "driver": "sqlite",
        "dsn": ":memory:"
    },
    "registries": [
        {
            "type": "smithery",
            "url": "https://registry.smithery.ai/servers"
        }
    ]
}
"#;

    fs::write(&config_path, config_content).unwrap();

    unsafe {
        std::env::set_var("SMITHERY_API_KEY", "env_api_key");
    }

    let config = crate::config::load_and_inject_registries(&config_path).unwrap();

    assert!(config.registries.is_some());
    let registries = config.registries.unwrap();
    assert_eq!(registries.len(), 1);

    let registry = &registries[0];
    assert_eq!(registry.registry_type, "smithery");
    assert_eq!(
        registry.url,
        Some("https://registry.smithery.ai/servers".to_string())
    );
}

#[test]
fn test_validate_config() {
    let valid_config = r#"
{
    "storage": {
        "driver": "sqlite",
        "dsn": ":memory:"
    }
}
"#;

    assert!(validate_config(valid_config.as_bytes()).is_ok());

    let invalid_config = r#"
{
    "invalid": "config"
}
"#;

    assert!(validate_config(invalid_config.as_bytes()).is_err());
}

#[test]
fn test_config_validation_errors() {
    let mut config = Config::default();

    // Valid config should pass
    assert!(config.validate().is_ok());

    // Missing storage driver should fail
    config.storage.driver = String::new();
    assert!(config.validate().is_err());

    // Missing storage DSN should fail
    config.storage = crate::config::StorageConfig {
        driver: "sqlite".to_string(),
        dsn: String::new(),
    };
    assert!(config.validate().is_err());
}
