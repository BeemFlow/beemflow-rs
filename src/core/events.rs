//! Event operations module
//!
//! All operations for event publishing.

use super::*;
use beemflow_core_macros::{operation, operation_group};
use schemars::JsonSchema;

#[operation_group(events)]
pub mod events {
    use super::*;

    #[derive(Deserialize, JsonSchema)]
    #[schemars(description = "Input for publishing an event")]
    pub struct PublishInput {
        #[schemars(description = "Topic to publish the event to")]
        pub topic: String,
        #[schemars(description = "Event payload as a JSON object")]
        pub payload: HashMap<String, Value>,
    }

    /// Publish an event
    #[operation(
        name = "publish_event",
        input = PublishInput,
        http = "POST /events",
        cli = "events publish <TOPIC> [--payload <JSON>]",
        description = "Publish an event"
    )]
    pub struct Publish {
        pub deps: Arc<Dependencies>,
    }

    #[async_trait]
    impl Operation for Publish {
        type Input = PublishInput;
        type Output = Value;

        async fn execute(&self, input: Self::Input) -> Result<Self::Output> {
            let payload = serde_json::to_value(input.payload)?;
            self.deps.event_bus.publish(&input.topic, payload).await?;

            Ok(serde_json::json!({
                "status": "published",
                "topic": input.topic
            }))
        }
    }
}
