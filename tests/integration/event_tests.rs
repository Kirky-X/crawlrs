// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Integration tests for domain events module

use crawlrs::domain::events::models::{DomainEvent, TaskCreatedEvent, TaskCompletedEvent, EventMetadata};
use crawlrs::domain::events::traits::{EventHandler, EventBus, EventPublisher};
use crawlrs::domain::events::in_memory::{InMemoryEventBus, EventPublisherImpl, EventLoggingHandler, SimpleEventListener};
use std::sync::Arc;
use uuid::Uuid;
use chrono::Utc;

#[tokio::test]
async fn test_event_handler_registration() {
    let event_bus = InMemoryEventBus::with_default_provider();
    let handler = Box::new(EventLoggingHandler);

    let result = event_bus.register_handler(handler).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_event_publishing() {
    let event_bus = Arc::new(InMemoryEventBus::with_default_provider());
    let listener = Arc::new(SimpleEventListener::new());

    // Register the listener
    event_bus
        .register_handler(Box::new(listener.clone()) as Box<dyn EventHandler>)
        .await
        .unwrap();

    // Create a test event
    let event = TaskCreatedEvent {
        task_id: Uuid::new_v4(),
        team_id: Uuid::new_v4(),
        task_type: "scrape".to_string(),
        url: "https://example.com".to_string(),
        metadata: EventMetadata::default(),
    };

    // Publish the event
    let result = event_bus.publish(&event).await;
    assert!(result.is_ok());

    // Verify the listener received the event
    let count = listener.get_event_count().await;
    assert_eq!(count, 1);
}

#[tokio::test]
async fn test_event_logging_handler() {
    let handler = EventLoggingHandler;
    let event = TaskCompletedEvent {
        task_id: Uuid::new_v4(),
        team_id: Uuid::new_v4(),
        duration_ms: 1000,
        result_count: 5,
        metadata: EventMetadata::default(),
    };

    let result = handler.handle(&event).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_event_publisher_impl() {
    let event_bus = Arc::new(InMemoryEventBus::with_default_provider());
    let publisher = EventPublisherImpl::new(event_bus.clone());

    let event = TaskCreatedEvent {
        task_id: Uuid::new_v4(),
        team_id: Uuid::new_v4(),
        task_type: "crawl".to_string(),
        url: "https://example.org".to_string(),
        metadata: EventMetadata::default(),
    };

    let result = publisher.publish(&event).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_event_bus_unregister() {
    let event_bus = Arc::new(InMemoryEventBus::with_default_provider());
    let handler = Box::new(EventLoggingHandler);

    // Register handler
    event_bus.register_handler(handler).await.unwrap();

    // Unregister handler
    let result = event_bus.unregister_handler("EventLoggingHandler").await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_task_created_event() {
    let task_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();

    let event = TaskCreatedEvent {
        task_id,
        team_id,
        task_type: "search".to_string(),
        url: "https://example.com/search?q=test".to_string(),
        metadata: EventMetadata::default(),
    };

    assert_eq!(event.event_type(), "task.created");
    assert_eq!(event.task_id, task_id);
    assert_eq!(event.team_id, team_id);
}

#[tokio::test]
async fn test_task_completed_event() {
    let task_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();

    let event = TaskCompletedEvent {
        task_id,
        team_id,
        duration_ms: 5000,
        result_count: 10,
        metadata: EventMetadata::default(),
    };

    assert_eq!(event.event_type(), "task.completed");
    assert_eq!(event.task_id, task_id);
    assert_eq!(event.duration_ms, 5000);
}

#[tokio::test]
async fn test_wildcard_event_subscription() {
    let event_bus = Arc::new(InMemoryEventBus::with_default_provider());
    let listener = Arc::new(SimpleEventListener::new());

    // Register wildcard listener
    event_bus
        .register_handler(Box::new(listener.clone()) as Box<dyn EventHandler>)
        .await
        .unwrap();

    // Publish different event types
    let task_event = TaskCreatedEvent {
        task_id: Uuid::new_v4(),
        team_id: Uuid::new_v4(),
        task_type: "scrape".to_string(),
        url: "https://example.com".to_string(),
        metadata: EventMetadata::default(),
    };

    let completed_event = TaskCompletedEvent {
        task_id: Uuid::new_v4(),
        team_id: Uuid::new_v4(),
        duration_ms: 1000,
        result_count: 5,
        metadata: EventMetadata::default(),
    };

    event_bus.publish(&task_event).await.unwrap();
    event_bus.publish(&completed_event).await.unwrap();

    // Both events should be received by the wildcard listener
    let count = listener.get_event_count().await;
    assert_eq!(count, 2);
}
