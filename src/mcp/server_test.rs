use crate::engine::Engine;
use crate::storage::MemoryStorage;
use crate::registry::RegistryManager;
use crate::event::InProcEventBus;
use crate::config::Config;

#[tokio::test]
async fn test_mcp_server_creation() {
    let registry_manager = RegistryManager::new(Vec::new());
    
    let deps = Dependencies {
        storage: Arc::new(MemoryStorage::new()),
        engine: Arc::new(Engine::default()),
        registry_manager: Arc::new(registry_manager),
        event_bus: Arc::new(InProcEventBus::new()),
        config: Arc::new(Config::default()),
    };
    
    let ops = Arc::new(OperationRegistry::new(deps));
    let server = McpServer::new(ops);
    
    // Test tools list generation
    let tools = server.get_tools_list();
    assert!(!tools.is_empty());
    assert!(tools.iter().any(|t| t.name == "beemflow_start_run"));
}

#[tokio::test]
async fn test_handle_initialize() {
    let registry_manager = RegistryManager::new(Vec::new());
    
    let deps = Dependencies {
        storage: Arc::new(MemoryStorage::new()),
        engine: Arc::new(Engine::default()),
        registry_manager: Arc::new(registry_manager),
        event_bus: Arc::new(InProcEventBus::new()),
        config: Arc::new(Config::default()),
    };
    
    let ops = Arc::new(OperationRegistry::new(deps));
    let server = McpServer::new(ops);
    
    // Test initialize request
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(1)),
        method: "initialize".to_string(),
        params: Some(json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
        })),
    };
    
    let response = server.handle_initialize(request).await;
    assert!(response.error.is_none());
    assert!(response.result.is_some());
    
    // Verify initialized
    let state = server.state.read().await;
    assert!(state.initialized);
}

#[tokio::test]
async fn test_handle_tools_list() {
    let registry_manager = RegistryManager::new(Vec::new());
    
    let deps = Dependencies {
        storage: Arc::new(MemoryStorage::new()),
        engine: Arc::new(Engine::default()),
        registry_manager: Arc::new(registry_manager),
        event_bus: Arc::new(InProcEventBus::new()),
        config: Arc::new(Config::default()),
    };
    
    let ops = Arc::new(OperationRegistry::new(deps));
    let server = McpServer::new(ops);
    
    // Initialize first
    {
        let mut state = server.state.write().await;
        state.initialized = true;
    }
    
    // Test tools/list request
    let request = JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        id: Some(json!(2)),
        method: "tools/list".to_string(),
        params: None,
    };
    
    let response = server.handle_tools_list(request).await;
    assert!(response.error.is_none());
    
    if let Some(result) = response.result {
        if let Some(tools) = result.get("tools") {
            assert!(tools.is_array());
            let tools_array = tools.as_array().unwrap();
            assert!(!tools_array.is_empty());
        } else {
            panic!("No tools field in response");
        }
    } else {
        panic!("No result in response");
    }
}
