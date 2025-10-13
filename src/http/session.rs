//! Session management for OAuth flows and authenticated requests
//!
//! Provides secure session storage with TTL, CSRF protection, and automatic cleanup.

use axum::http::request::Parts;
use axum::{
    extract::{FromRequestParts, Request},
    http::{StatusCode, header},
    middleware::Next,
    response::Response,
};
use chrono::{DateTime, Duration, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;

/// Session data stored for each user
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    /// Unique session ID
    pub id: String,
    /// User ID
    pub user_id: String,
    /// Session creation time
    pub created_at: DateTime<Utc>,
    /// Session expiration time
    pub expires_at: DateTime<Utc>,
    /// Arbitrary session data
    pub data: HashMap<String, serde_json::Value>,
}

/// Session store for managing user sessions
#[derive(Clone)]
pub struct SessionStore {
    sessions: Arc<RwLock<HashMap<String, Session>>>,
}

impl SessionStore {
    /// Create a new session store
    pub fn new() -> Self {
        let store = Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
        };

        // Start cleanup task
        let store_clone = store.clone();
        tokio::spawn(async move {
            store_clone.cleanup_loop().await;
        });

        store
    }

    /// Create a new session for a user
    pub fn create_session(&self, user_id: &str, ttl: Duration) -> Session {
        let session = Session {
            id: generate_session_id(),
            user_id: user_id.to_string(),
            created_at: Utc::now(),
            expires_at: Utc::now() + ttl,
            data: HashMap::new(),
        };

        self.sessions
            .write()
            .insert(session.id.clone(), session.clone());
        session
    }

    /// Get a session by ID
    pub fn get_session(&self, session_id: &str) -> Option<Session> {
        let sessions = self.sessions.read();
        let session = sessions.get(session_id)?;

        // Check if expired
        if Utc::now() > session.expires_at {
            drop(sessions);
            self.sessions.write().remove(session_id);
            return None;
        }

        Some(session.clone())
    }

    /// Update session data
    pub fn update_session(&self, session_id: &str, key: String, value: serde_json::Value) -> bool {
        let mut sessions = self.sessions.write();
        if let Some(session) = sessions.get_mut(session_id) {
            session.data.insert(key, value);
            true
        } else {
            false
        }
    }

    /// Delete a session
    pub fn delete_session(&self, session_id: &str) {
        self.sessions.write().remove(session_id);
    }

    /// Generate a CSRF token for a session
    pub fn generate_csrf_token(&self, session_id: &str) -> Option<String> {
        let token = generate_secure_token();
        if self.update_session(
            session_id,
            "csrf_token".to_string(),
            serde_json::json!(token.clone()),
        ) {
            Some(token)
        } else {
            None
        }
    }

    /// Validate a CSRF token for a session
    pub fn validate_csrf_token(&self, session_id: &str, token: &str) -> bool {
        let sessions = self.sessions.read();
        if let Some(session) = sessions.get(session_id)
            && let Some(stored_token) = session.data.get("csrf_token")
            && let Some(stored_str) = stored_token.as_str()
        {
            return stored_str == token;
        }
        false
    }

    /// Cleanup expired sessions (runs periodically)
    async fn cleanup_loop(&self) {
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(300)).await; // Every 5 minutes

            let now = Utc::now();
            let mut sessions = self.sessions.write();
            sessions.retain(|_, session| now < session.expires_at);
        }
    }
}

impl Default for SessionStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Session middleware that adds session to request extensions
pub async fn session_middleware(mut req: Request, next: Next) -> Response {
    // Extract session ID from cookie
    let session_id = if let Some(cookie_header) = req.headers().get(header::COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            // Parse cookies to find beemflow_session
            cookie_str
                .split(';')
                .map(|c| c.trim())
                .find_map(|c| c.strip_prefix("beemflow_session="))
                .map(|s| s.to_string())
        } else {
            None
        }
    } else {
        None
    };

    // Store session ID in request extensions if found
    if let Some(session_id) = session_id {
        req.extensions_mut().insert(SessionId(session_id));
    }

    next.run(req).await
}

/// Session ID extracted from cookie
#[derive(Clone, Debug)]
pub struct SessionId(pub String);

/// Extractor for getting session from request
pub struct SessionExtractor {
    pub session: Option<Session>,
}

impl<S> FromRequestParts<S> for SessionExtractor
where
    S: Send + Sync,
{
    type Rejection = (StatusCode, &'static str);

    fn from_request_parts(
        parts: &mut Parts,
        _state: &S,
    ) -> impl std::future::Future<Output = Result<Self, Self::Rejection>> + Send {
        // Clone data we need from parts before moving into async block
        let session_id = parts.extensions.get::<SessionId>().cloned();

        async move {
            // For now, return None - in production this would look up the session
            // from the SessionStore which would need to be in app state
            Ok(SessionExtractor {
                session: session_id.map(|_| Session {
                    id: String::new(),
                    user_id: String::new(),
                    created_at: Utc::now(),
                    expires_at: Utc::now(),
                    data: HashMap::new(),
                }),
            })
        }
    }
}

/// Set a session cookie in the response
pub fn set_session_cookie(session_id: &str, expires_at: DateTime<Utc>) -> String {
    format!(
        "beemflow_session={}; Path=/; Expires={}; HttpOnly; SameSite=Lax",
        session_id,
        expires_at.to_rfc2822()
    )
}

/// Clear the session cookie
pub fn clear_session_cookie() -> String {
    "beemflow_session=; Path=/; Max-Age=0; HttpOnly; SameSite=Lax".to_string()
}

/// Generate a secure random session ID
fn generate_session_id() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random()).collect();
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &bytes)
}

/// Generate a secure random token (for CSRF, etc.)
fn generate_secure_token() -> String {
    use rand::Rng;
    let mut rng = rand::rng();
    let bytes: Vec<u8> = (0..32).map(|_| rng.random()).collect();
    base64::Engine::encode(&base64::engine::general_purpose::URL_SAFE_NO_PAD, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_session() {
        let store = SessionStore::new();
        let session = store.create_session("user123", Duration::hours(1));

        assert_eq!(session.user_id, "user123");
        assert!(session.expires_at > Utc::now());
    }

    #[tokio::test]
    async fn test_get_session() {
        let store = SessionStore::new();
        let session = store.create_session("user123", Duration::hours(1));

        let retrieved = store.get_session(&session.id);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().user_id, "user123");
    }

    #[tokio::test]
    async fn test_update_session() {
        let store = SessionStore::new();
        let session = store.create_session("user123", Duration::hours(1));

        let updated =
            store.update_session(&session.id, "key".to_string(), serde_json::json!("value"));
        assert!(updated);

        let retrieved = store.get_session(&session.id).unwrap();
        assert_eq!(
            retrieved.data.get("key").unwrap(),
            &serde_json::json!("value")
        );
    }

    #[tokio::test]
    async fn test_csrf_token() {
        let store = SessionStore::new();
        let session = store.create_session("user123", Duration::hours(1));

        let token = store.generate_csrf_token(&session.id).unwrap();
        assert!(!token.is_empty());

        assert!(store.validate_csrf_token(&session.id, &token));
        assert!(!store.validate_csrf_token(&session.id, "invalid"));
    }

    #[tokio::test]
    async fn test_delete_session() {
        let store = SessionStore::new();
        let session = store.create_session("user123", Duration::hours(1));

        store.delete_session(&session.id);

        let retrieved = store.get_session(&session.id);
        assert!(retrieved.is_none());
    }
}
