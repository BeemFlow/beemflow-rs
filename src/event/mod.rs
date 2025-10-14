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
use uuid::Uuid;

/// Callback function for event handling
type EventCallback = Arc<dyn Fn(Value) + Send + Sync>;

/// Subscription ID for selective unsubscribe
pub type SubscriptionId = Uuid;

/// Topic subscriptions map: topic -> Vec<(subscription_id, callback)>
type TopicSubscriptions = HashMap<String, Vec<(SubscriptionId, EventCallback)>>;

/// Event bus trait for publishing and subscribing to events
#[async_trait]
pub trait EventBus: Send + Sync {
    /// Publish an event to a topic
    async fn publish(&self, topic: &str, payload: Value) -> Result<()>;

    /// Subscribe to a topic with a callback, returns a subscription ID
    async fn subscribe(&self, topic: &str, callback: EventCallback) -> Result<SubscriptionId>;

    /// Unsubscribe from a topic (removes ALL callbacks for that topic)
    async fn unsubscribe(&self, topic: &str) -> Result<()>;

    /// Unsubscribe a specific subscription by ID
    async fn unsubscribe_by_id(&self, subscription_id: SubscriptionId) -> Result<()>;
}

/// In-process event bus using tokio broadcast channels
pub struct InProcEventBus {
    channels: Arc<RwLock<HashMap<String, broadcast::Sender<Value>>>>,
    callbacks: Arc<RwLock<TopicSubscriptions>>,
    subscriptions: Arc<RwLock<HashMap<SubscriptionId, String>>>, // subscription_id -> topic
}

impl InProcEventBus {
    /// Create a new in-process event bus
    pub fn new() -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
            callbacks: Arc::new(RwLock::new(HashMap::new())),
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
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

        // Call all registered callbacks for this topic
        if let Some(callbacks) = self.callbacks.read().get(topic) {
            for (_id, callback) in callbacks {
                callback(payload.clone());
            }
        }

        Ok(())
    }

    async fn subscribe(&self, topic: &str, callback: EventCallback) -> Result<SubscriptionId> {
        let subscription_id = Uuid::new_v4();

        // Add callback to topic
        self.callbacks
            .write()
            .entry(topic.to_string())
            .or_default()
            .push((subscription_id, callback));

        // Track subscription ID -> topic mapping
        self.subscriptions
            .write()
            .insert(subscription_id, topic.to_string());

        Ok(subscription_id)
    }

    async fn unsubscribe(&self, topic: &str) -> Result<()> {
        // Remove all callbacks for this topic
        if let Some(callbacks) = self.callbacks.write().remove(topic) {
            // Remove subscription ID mappings
            let mut subscriptions = self.subscriptions.write();
            for (id, _) in callbacks {
                subscriptions.remove(&id);
            }
        }
        self.channels.write().remove(topic);
        Ok(())
    }

    async fn unsubscribe_by_id(&self, subscription_id: SubscriptionId) -> Result<()> {
        // Find the topic for this subscription
        let topic = self.subscriptions.write().remove(&subscription_id);

        if let Some(topic) = topic {
            // Check if we need to remove the topic entirely
            let should_remove_topic = {
                let mut callbacks_lock = self.callbacks.write();
                if let Some(callbacks) = callbacks_lock.get_mut(&topic) {
                    callbacks.retain(|(id, _)| *id != subscription_id);
                    let is_empty = callbacks.is_empty();
                    if is_empty {
                        callbacks_lock.remove(&topic);
                    }
                    is_empty
                } else {
                    false
                }
            }; // Lock released here

            // Remove channel if no callbacks remain
            if should_remove_topic {
                self.channels.write().remove(&topic);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod event_test;
