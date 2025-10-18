//! OAuth 2.0/2.1 authentication system
//!
//! Provides OAuth server and client functionality for BeemFlow:
//! - **Server**: OAuth 2.1 authorization server for MCP tools and ChatGPT
//! - **Client**: OAuth 2.0 client for connecting to external providers
//! - **Middleware**: Type-safe authentication and authorization middleware

pub mod client;
pub mod middleware;
pub mod server;

pub use client::{OAuthClientManager, create_test_oauth_client};
pub use middleware::{
    AuthenticatedUser, OAuthMiddlewareState, RequiredScopes, has_all_scopes, has_any_scope,
    has_scope, oauth_middleware, rate_limit_middleware, validate_token,
};
pub use server::{OAuthConfig, OAuthServerState, create_oauth_routes};

use crate::{Result, model::*};
use parking_lot::RwLock;
use std::sync::Arc;

/// OAuth server for providing authentication
pub struct OAuthServer {
    providers: Arc<RwLock<Vec<OAuthProvider>>>,
    clients: Arc<RwLock<Vec<OAuthClient>>>,
}

impl OAuthServer {
    /// Create a new OAuth server
    pub fn new() -> Self {
        Self {
            providers: Arc::new(RwLock::new(Vec::new())),
            clients: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Register an OAuth provider
    pub fn register_provider(&self, provider: OAuthProvider) -> Result<()> {
        provider.validate()?;
        self.providers.write().push(provider);
        Ok(())
    }

    /// Register an OAuth client
    pub fn register_client(&self, client: OAuthClient) -> Result<()> {
        self.clients.write().push(client);
        Ok(())
    }
}

impl Default for OAuthServer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod middleware_test;
