//! Event bus for async workflow communication
//!
//! Provides event publishing and subscription for workflow orchestration.

use crate::Result;
use async_trait::async_trait;
use parking_lot::RwLock;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Callback function for event handling
type EventCallback = Arc<dyn Fn(Value) + Send + Sync>;

/// Event bus trait for publishing and subscribing to events
#[async_trait]
pub trait EventBus: Send + Sync {
    /// Publish an event to a topic
    async fn publish(&self, topic: &str, payload: Value) -> Result<()>;

    /// Subscribe to a topic with a callback
    async fn subscribe(&self, topic: &str, callback: EventCallback) -> Result<()>;

    /// Unsubscribe from a topic
    async fn unsubscribe(&self, topic: &str) -> Result<()>;
}

/// In-process event bus using tokio broadcast channels
pub struct InProcEventBus {
    channels: Arc<RwLock<HashMap<String, broadcast::Sender<Value>>>>,
    callbacks: Arc<RwLock<HashMap<String, Vec<EventCallback>>>>,
}

impl InProcEventBus {
    /// Create a new in-process event bus
    pub fn new() -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
            callbacks: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

impl Default for InProcEventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EventBus for InProcEventBus {
    async fn publish(&self, topic: &str, payload: Value) -> Result<()> {
        // Get or create channel for this topic
        let sender = {
            let mut channels = self.channels.write();
            channels
                .entry(topic.to_string())
                .or_insert_with(|| broadcast::channel(100).0)
                .clone()
        };

        // Send to broadcast channel (ignore if no receivers)
        let _ = sender.send(payload.clone());

        // Call all registered callbacks
        if let Some(callbacks) = self.callbacks.read().get(topic) {
            for callback in callbacks {
                callback(payload.clone());
            }
        }

        Ok(())
    }

    async fn subscribe(&self, topic: &str, callback: EventCallback) -> Result<()> {
        self.callbacks
            .write()
            .entry(topic.to_string())
            .or_default()
            .push(callback);
        Ok(())
    }

    async fn unsubscribe(&self, topic: &str) -> Result<()> {
        self.callbacks.write().remove(topic);
        self.channels.write().remove(topic);
        Ok(())
    }
}

#[cfg(test)]
mod event_test;
