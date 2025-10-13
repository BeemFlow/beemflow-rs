use super::*;
use crate::event::InProcEventBus;
use parking_lot::RwLock;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

#[tokio::test]
async fn test_event_bus_publish_subscribe() {
    let bus = InProcEventBus::new();
    let called = Arc::new(AtomicBool::new(false));
    let called_clone = called.clone();

    bus.subscribe(
        "test.topic",
        Arc::new(move |_| {
            called_clone.store(true, Ordering::SeqCst);
        }),
    )
    .await
    .unwrap();

    bus.publish("test.topic", json!("test")).await.unwrap();

    // Give callback time to execute
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    assert!(called.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_publish_without_subscribers() {
    let bus = InProcEventBus::new();

    // Should not error even without subscribers
    let result = bus.publish("test.topic", json!("test")).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_multiple_subscribers() {
    let bus = InProcEventBus::new();
    let count = Arc::new(AtomicU32::new(0));

    let count1 = count.clone();
    bus.subscribe(
        "multi.topic",
        Arc::new(move |_| {
            count1.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .await
    .unwrap();

    let count2 = count.clone();
    bus.subscribe(
        "multi.topic",
        Arc::new(move |_| {
            count2.fetch_add(1, Ordering::SeqCst);
        }),
    )
    .await
    .unwrap();

    bus.publish("multi.topic", json!("test")).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Both subscribers should have received the event
    assert_eq!(count.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn test_different_topics() {
    let bus = InProcEventBus::new();
    let called_a = Arc::new(AtomicBool::new(false));
    let called_b = Arc::new(AtomicBool::new(false));

    let called_a_clone = called_a.clone();
    bus.subscribe(
        "topic.a",
        Arc::new(move |_| {
            called_a_clone.store(true, Ordering::SeqCst);
        }),
    )
    .await
    .unwrap();

    let called_b_clone = called_b.clone();
    bus.subscribe(
        "topic.b",
        Arc::new(move |_| {
            called_b_clone.store(true, Ordering::SeqCst);
        }),
    )
    .await
    .unwrap();

    // Publish to topic A only
    bus.publish("topic.a", json!("test")).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    assert!(called_a.load(Ordering::SeqCst));
    assert!(!called_b.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_unsubscribe() {
    let bus = InProcEventBus::new();
    let called = Arc::new(AtomicBool::new(false));
    let called_clone = called.clone();

    bus.subscribe(
        "test.topic",
        Arc::new(move |_| {
            called_clone.store(true, Ordering::SeqCst);
        }),
    )
    .await
    .unwrap();

    // Unsubscribe before publishing
    bus.unsubscribe("test.topic").await.unwrap();

    bus.publish("test.topic", json!("test")).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    // Should not have been called
    assert!(!called.load(Ordering::SeqCst));
}

#[tokio::test]
async fn test_payload_types() {
    let bus = InProcEventBus::new();
    let payload = Arc::new(parking_lot::RwLock::new(None));

    let payload_clone = payload.clone();
    bus.subscribe(
        "test.topic",
        Arc::new(move |p| {
            *payload_clone.write() = Some(p);
        }),
    )
    .await
    .unwrap();

    // Test with JSON object
    let test_data = json!({"key": "value", "num": 42});
    bus.publish("test.topic", test_data.clone()).await.unwrap();

    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

    let received = payload.read().clone().unwrap();
    assert_eq!(received.get("key").and_then(|v| v.as_str()), Some("value"));
    assert_eq!(received.get("num").and_then(|v| v.as_i64()), Some(42));
}

#[tokio::test]
async fn test_publish_all_payload_types() {
    let bus = InProcEventBus::new();

    // String payload
    assert!(bus.publish("topic", json!("hello world")).await.is_ok());

    // Number payload
    assert!(bus.publish("topic", json!(42)).await.is_ok());
    assert!(bus.publish("topic", json!(42.5)).await.is_ok());

    // Boolean payload
    assert!(bus.publish("topic", json!(true)).await.is_ok());

    // Null payload
    assert!(bus.publish("topic", json!(null)).await.is_ok());

    // Array payload
    assert!(bus.publish("topic", json!([1, 2, 3])).await.is_ok());

    // Complex nested payload
    assert!(
        bus.publish(
            "topic",
            json!({
                "nested": {"deep": "value"},
                "array": [1, 2, 3]
            })
        )
        .await
        .is_ok()
    );
}

#[tokio::test]
async fn test_subscribe_multiple_messages() {
    let bus = Arc::new(InProcEventBus::new());
    let received = Arc::new(RwLock::new(Vec::new()));
    let received_clone = received.clone();

    bus.subscribe(
        "test.topic",
        Arc::new(move |payload| {
            received_clone.write().push(payload.clone());
        }),
    )
    .await
    .unwrap();

    // Publish multiple messages
    for i in 0..5 {
        bus.publish("test.topic", json!({"index": i}))
            .await
            .unwrap();
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    let messages = received.read().clone();
    assert_eq!(messages.len(), 5, "Should receive all 5 messages");

    // Verify order
    for (i, msg) in messages.iter().enumerate() {
        assert_eq!(msg.get("index").and_then(|v| v.as_i64()), Some(i as i64));
    }
}

#[tokio::test]
async fn test_unsubscribe_stops_delivery() {
    let bus = Arc::new(InProcEventBus::new());
    let count = Arc::new(RwLock::new(0));
    let count_clone = count.clone();

    bus.subscribe(
        "test.topic",
        Arc::new(move |_| {
            *count_clone.write() += 1;
        }),
    )
    .await
    .unwrap();

    // Publish first message
    bus.publish("test.topic", json!({"msg": 1})).await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

    let first_count = *count.read();
    assert_eq!(first_count, 1);

    // Unsubscribe
    bus.unsubscribe("test.topic").await.unwrap();

    // Publish second message (should not be received)
    bus.publish("test.topic", json!({"msg": 2})).await.unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;

    let final_count = *count.read();
    assert_eq!(
        final_count, 1,
        "Count should not increase after unsubscribe"
    );
}

#[tokio::test]
async fn test_concurrent_publishes() {
    let bus = Arc::new(InProcEventBus::new());
    let count = Arc::new(RwLock::new(0));
    let count_clone = count.clone();

    bus.subscribe(
        "concurrent.topic",
        Arc::new(move |_| {
            *count_clone.write() += 1;
        }),
    )
    .await
    .unwrap();

    // Spawn multiple concurrent publishers
    let mut handles = vec![];
    for _ in 0..10 {
        let bus_clone = bus.clone();
        let handle = tokio::spawn(async move {
            bus_clone
                .publish("concurrent.topic", json!({"data": "test"}))
                .await
        });
        handles.push(handle);
    }

    // Wait for all publishes to complete
    for handle in handles {
        handle.await.unwrap().unwrap();
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    let final_count = *count.read();
    assert_eq!(final_count, 10, "Should receive all 10 concurrent messages");
}

#[tokio::test]
async fn test_multiple_subscribers_same_topic() {
    let bus = Arc::new(InProcEventBus::new());
    let count1 = Arc::new(RwLock::new(0));
    let count2 = Arc::new(RwLock::new(0));
    let count1_clone = count1.clone();
    let count2_clone = count2.clone();

    // First subscriber
    bus.subscribe(
        "shared.topic",
        Arc::new(move |_| {
            *count1_clone.write() += 1;
        }),
    )
    .await
    .unwrap();

    // Second subscriber
    bus.subscribe(
        "shared.topic",
        Arc::new(move |_| {
            *count2_clone.write() += 1;
        }),
    )
    .await
    .unwrap();

    // Publish message
    bus.publish("shared.topic", json!({"msg": "test"}))
        .await
        .unwrap();
    tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

    // Both subscribers should receive the message
    assert_eq!(*count1.read(), 1, "First subscriber should receive message");
    assert_eq!(
        *count2.read(),
        1,
        "Second subscriber should receive message"
    );
}
