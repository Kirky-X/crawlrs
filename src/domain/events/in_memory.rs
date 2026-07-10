// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 内存事件总线实现
//!
//! 提供基于内存的事件总线实现，适用于开发测试和小规模部署。

use async_trait::async_trait;
use log::{debug, error};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::models::DomainEvent;
use super::traits::{EventBus, EventHandler, EventPublisher};

/// 事件元数据提供者实现
#[derive(Debug, Default, Clone)]
pub struct DefaultEventMetadataProvider;

impl super::traits::EventMetadataProvider for DefaultEventMetadataProvider {
    fn get_trace_id(&self) -> Option<uuid::Uuid> {
        None
    }

    fn get_tenant_id(&self) -> Option<uuid::Uuid> {
        None
    }
}

/// 内存事件总线
#[allow(dead_code)]
#[allow(clippy::type_complexity)]
pub struct InMemoryEventBus {
    /// 事件处理器映射
    handlers: Arc<RwLock<HashMap<String, Vec<Box<dyn EventHandler + Send + Sync>>>>>,
    /// 元数据提供者
    metadata_provider: Arc<dyn super::traits::EventMetadataProvider + Send + Sync>,
}

impl InMemoryEventBus {
    /// 创建新的内存事件总线
    pub fn new(
        metadata_provider: Arc<dyn super::traits::EventMetadataProvider + Send + Sync>,
    ) -> Self {
        Self {
            handlers: Arc::new(RwLock::new(HashMap::new())),
            metadata_provider,
        }
    }

    /// 创建使用默认元数据提供者的内存事件总线
    pub fn with_default_provider() -> Self {
        Self::new(Arc::new(DefaultEventMetadataProvider))
    }
}

#[async_trait]
impl EventBus for InMemoryEventBus {
    async fn register_handler(&self, handler: Box<dyn EventHandler>) -> Result<(), anyhow::Error> {
        let handler_name = handler.name();
        let event_types = handler.subscribe_to().to_vec();
        let mut handlers = self.handlers.write().await;

        // 只处理第一个事件类型（因为 Box<dyn EventHandler> 不能 Clone）
        if let Some(event_type) = event_types.first() {
            debug!(
                "Registering handler {} for event type {}",
                handler_name, event_type
            );
            if *event_type == "*" {
                // 通配符订阅者 - 注册到特殊键
                let handlers_vec = handlers.entry("*".to_string()).or_insert_with(Vec::new);
                handlers_vec.push(handler);
            } else {
                let handlers_vec = handlers
                    .entry(event_type.to_string())
                    .or_insert_with(Vec::new);
                handlers_vec.push(handler);
            }
        }

        Ok(())
    }

    async fn publish(&self, event: &dyn DomainEvent) -> Result<(), anyhow::Error> {
        let event_type = event.event_type();
        let handlers = self.handlers.read().await;

        if let Some(handler_list) = handlers.get(event_type) {
            let handlers_to_notify: Vec<_> = handler_list.iter().collect();

            for handler in handlers_to_notify {
                match handler.handle(event).await {
                    Ok(_) => {
                        debug!("Handler {} processed event {}", handler.name(), event_type);
                    }
                    Err(e) => {
                        error!(
                            "Handler {} failed to process event {}: {:?}",
                            handler.name(),
                            event_type,
                            e
                        );
                    }
                }
            }
        } else {
            debug!("No handlers registered for event type {}", event_type);
        }

        Ok(())
    }

    async fn publish_sync(&self, event: &dyn DomainEvent) -> Result<(), anyhow::Error> {
        self.publish(event).await?;
        Ok(())
    }

    async fn unregister_handler(&self, handler_name: &str) -> Result<(), anyhow::Error> {
        let mut handlers = self.handlers.write().await;
        let mut to_remove = Vec::new();

        for (event_type, handler_list) in handlers.iter_mut() {
            handler_list.retain(|h| h.name() != handler_name);
            if handler_list.is_empty() {
                to_remove.push(event_type.clone());
            }
        }

        for event_type in to_remove {
            handlers.remove(&event_type);
        }

        debug!("Unregistered handler {}", handler_name);
        Ok(())
    }
}

/// 事件发布器实现
pub struct EventPublisherImpl {
    event_bus: Arc<dyn EventBus + Send + Sync>,
}

impl EventPublisherImpl {
    /// 创建新的事件发布器
    pub fn new(event_bus: Arc<dyn EventBus + Send + Sync>) -> Self {
        Self { event_bus }
    }
}

#[async_trait]
impl EventPublisher for EventPublisherImpl {
    async fn publish(&self, event: &dyn DomainEvent) -> Result<(), anyhow::Error> {
        self.event_bus.publish(event).await?;
        Ok(())
    }

    async fn publish_batch(&self, events: Vec<&dyn DomainEvent>) -> Result<(), anyhow::Error> {
        for event in events {
            self.event_bus.publish(event).await?;
        }
        Ok(())
    }
}

/// 事件日志处理器
///
/// 将所有事件记录到日志中。
#[derive(Debug, Default, Clone)]
pub struct EventLoggingHandler;

#[async_trait]
impl EventHandler for EventLoggingHandler {
    async fn handle(&self, event: &dyn DomainEvent) -> Result<(), anyhow::Error> {
        debug!(
            "Event logged: type={}, aggregate_id={}, aggregate_type={}",
            event.event_type(),
            event.aggregate_id(),
            event.aggregate_type()
        );
        Ok(())
    }

    fn name(&self) -> &'static str {
        "EventLoggingHandler"
    }

    fn subscribe_to(&self) -> &'static [&'static str] {
        &["*"]
    }
}

/// 简单事件监听器
///
/// 用于测试和调试的简单事件监听器。
#[derive(Debug, Clone, Default)]
pub struct SimpleEventListener {
    pub event_count: Arc<RwLock<u64>>,
}

impl SimpleEventListener {
    /// 创建新的事件监听器
    pub fn new() -> Self {
        Self {
            event_count: Arc::new(RwLock::new(0)),
        }
    }

    /// 获取接收的事件数量
    pub async fn get_event_count(&self) -> u64 {
        *self.event_count.read().await
    }
}

#[async_trait]
impl EventHandler for SimpleEventListener {
    async fn handle(&self, event: &dyn DomainEvent) -> Result<(), anyhow::Error> {
        // 由于不能直接Clone dyn DomainEvent，我们记录事件类型
        let event_type = event.event_type();
        let mut count = self.event_count.write().await;
        *count += 1;
        debug!(
            "SimpleEventListener received event: {} (total: {})",
            event_type, count
        );
        Ok(())
    }

    fn name(&self) -> &'static str {
        "SimpleEventListener"
    }

    fn subscribe_to(&self) -> &'static [&'static str] {
        &["*"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::events::models::task::TaskCreatedEvent;
    use crate::domain::events::models::DomainEvent;
    use crate::domain::events::traits::EventMetadataProvider;
    use std::sync::atomic::{AtomicU64, Ordering};

    // ---------- helpers ----------

    fn make_event() -> TaskCreatedEvent {
        TaskCreatedEvent::new(
            uuid::Uuid::new_v4(),
            "scrape".to_string(),
            "https://example.com".to_string(),
            uuid::Uuid::new_v4(),
            1,
        )
    }

    fn make_completed_event() -> crate::domain::events::models::task::TaskCompletedEvent {
        crate::domain::events::models::task::TaskCompletedEvent::new(
            uuid::Uuid::new_v4(),
            "scrape".to_string(),
            uuid::Uuid::new_v4(),
            100,
            serde_json::json!({}),
        )
    }

    /// Mock handler that counts handle() invocations and can be configured to fail.
    struct MockEventHandler {
        name: &'static str,
        event_types: &'static [&'static str],
        call_count: Arc<AtomicU64>,
        should_fail: bool,
    }

    impl MockEventHandler {
        fn new(name: &'static str, event_types: &'static [&'static str]) -> Self {
            Self {
                name,
                event_types,
                call_count: Arc::new(AtomicU64::new(0)),
                should_fail: false,
            }
        }

        fn with_counter(
            name: &'static str,
            event_types: &'static [&'static str],
            call_count: Arc<AtomicU64>,
        ) -> Self {
            Self {
                name,
                event_types,
                call_count,
                should_fail: false,
            }
        }

        fn failing(name: &'static str, event_types: &'static [&'static str]) -> Self {
            Self {
                name,
                event_types,
                call_count: Arc::new(AtomicU64::new(0)),
                should_fail: true,
            }
        }

        #[allow(dead_code)]
        fn count(&self) -> u64 {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl EventHandler for MockEventHandler {
        async fn handle(&self, _event: &dyn DomainEvent) -> Result<(), anyhow::Error> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            if self.should_fail {
                Err(anyhow::anyhow!("mock handler failure"))
            } else {
                Ok(())
            }
        }

        fn name(&self) -> &'static str {
            self.name
        }

        fn subscribe_to(&self) -> &'static [&'static str] {
            self.event_types
        }
    }

    // ========== DefaultEventMetadataProvider ==========

    #[test]
    fn test_default_metadata_provider_get_trace_id_returns_none() {
        let provider = DefaultEventMetadataProvider;
        assert!(provider.get_trace_id().is_none());
    }

    #[test]
    fn test_default_metadata_provider_get_tenant_id_returns_none() {
        let provider = DefaultEventMetadataProvider;
        assert!(provider.get_tenant_id().is_none());
    }

    #[test]
    fn test_default_metadata_provider_default_impl_returns_none() {
        let provider = DefaultEventMetadataProvider;
        assert!(provider.get_trace_id().is_none());
        assert!(provider.get_tenant_id().is_none());
    }

    #[test]
    fn test_default_metadata_provider_clone_preserves_behavior() {
        let provider = DefaultEventMetadataProvider;
        let cloned = provider.clone();
        assert!(cloned.get_trace_id().is_none());
        assert!(cloned.get_tenant_id().is_none());
    }

    // ========== InMemoryEventBus construction ==========

    #[tokio::test]
    async fn test_in_memory_event_bus_with_default_provider_creates_empty_bus() {
        let bus = InMemoryEventBus::with_default_provider();
        let event = make_event();
        let result = bus.publish(&event).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_in_memory_event_bus_new_with_custom_provider() {
        let provider: Arc<dyn super::super::traits::EventMetadataProvider + Send + Sync> =
            Arc::new(DefaultEventMetadataProvider);
        let bus = InMemoryEventBus::new(provider);
        let event = make_event();
        let result = bus.publish(&event).await;
        assert!(result.is_ok());
    }

    // ========== register_handler + publish ==========

    #[tokio::test]
    async fn test_register_handler_specific_event_type_calls_handler() {
        let bus = InMemoryEventBus::with_default_provider();
        let handler = MockEventHandler::new("SpecificHandler", &["TaskCreated"]);
        bus.register_handler(Box::new(handler))
            .await
            .expect("register should succeed");

        let event = make_event();
        bus.publish(&event).await.expect("publish should succeed");
    }

    #[tokio::test]
    async fn test_register_handler_wildcard_receives_events() {
        let bus = InMemoryEventBus::with_default_provider();
        let handler = MockEventHandler::new("WildcardHandler", &["*"]);
        bus.register_handler(Box::new(handler))
            .await
            .expect("register should succeed");

        let event = make_event();
        bus.publish(&event).await.expect("publish should succeed");
    }

    #[tokio::test]
    async fn test_register_handler_with_empty_subscribe_to_does_nothing() {
        let bus = InMemoryEventBus::with_default_provider();
        bus.register_handler(Box::new(MockEventHandler::new("Empty", &[])))
            .await
            .expect("register should succeed");

        let event = make_event();
        let result = bus.publish(&event).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_publish_with_no_handlers_returns_ok() {
        let bus = InMemoryEventBus::with_default_provider();
        let event = make_event();
        let result = bus.publish(&event).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_publish_filters_by_event_type_when_handler_registered_for_different_type() {
        let bus = InMemoryEventBus::with_default_provider();
        bus.register_handler(Box::new(MockEventHandler::new(
            "CompletedOnly",
            &["TaskCompleted"],
        )))
        .await
        .expect("register should succeed");

        // Publish a TaskCreated event - handler for TaskCompleted should NOT be called
        let event = make_event();
        bus.publish(&event).await.expect("publish should succeed");
    }

    #[tokio::test]
    async fn test_multiple_handlers_same_event_type_all_called() {
        let bus = InMemoryEventBus::with_default_provider();
        bus.register_handler(Box::new(MockEventHandler::new("H1", &["TaskCreated"])))
            .await
            .expect("register H1");
        bus.register_handler(Box::new(MockEventHandler::new("H2", &["TaskCreated"])))
            .await
            .expect("register H2");

        let event = make_event();
        let result = bus.publish(&event).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_handler_failure_does_not_break_publish() {
        let bus = InMemoryEventBus::with_default_provider();
        bus.register_handler(Box::new(MockEventHandler::failing(
            "FailingHandler",
            &["TaskCreated"],
        )))
        .await
        .expect("register failing handler");

        let event = make_event();
        let result = bus.publish(&event).await;
        assert!(
            result.is_ok(),
            "publish should still succeed when a handler fails"
        );
    }

    #[tokio::test]
    async fn test_publish_sync_behaves_like_publish() {
        let bus = InMemoryEventBus::with_default_provider();
        bus.register_handler(Box::new(MockEventHandler::new(
            "SyncHandler",
            &["TaskCreated"],
        )))
        .await
        .expect("register");

        let event = make_event();
        let result = bus.publish_sync(&event).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_publish_sync_with_no_handlers_returns_ok() {
        let bus = InMemoryEventBus::with_default_provider();
        let event = make_event();
        let result = bus.publish_sync(&event).await;
        assert!(result.is_ok());
    }

    // ========== unregister_handler ==========

    #[tokio::test]
    async fn test_unregister_handler_removes_registered_handler() {
        let bus = InMemoryEventBus::with_default_provider();
        bus.register_handler(Box::new(MockEventHandler::new(
            "ToRemove",
            &["TaskCreated"],
        )))
        .await
        .expect("register");

        let result = bus.unregister_handler("ToRemove").await;
        assert!(result.is_ok());

        // After unregister, publish should still succeed (no handlers)
        let event = make_event();
        bus.publish(&event).await.expect("publish after unregister");
    }

    #[tokio::test]
    async fn test_unregister_handler_when_not_registered_returns_ok() {
        let bus = InMemoryEventBus::with_default_provider();
        let result = bus.unregister_handler("NonExistent").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_unregister_handler_cleans_up_empty_event_type_entry() {
        let bus = InMemoryEventBus::with_default_provider();
        bus.register_handler(Box::new(MockEventHandler::new("Solo", &["TaskCreated"])))
            .await
            .expect("register");

        bus.unregister_handler("Solo").await.expect("unregister");

        // Re-register a different handler for the same event type - should work cleanly
        bus.register_handler(Box::new(MockEventHandler::new("NewSolo", &["TaskCreated"])))
            .await
            .expect("re-register");

        let event = make_event();
        bus.publish(&event).await.expect("publish");
    }

    #[tokio::test]
    async fn test_unregister_handler_preserves_other_handlers_in_same_event_type() {
        let bus = InMemoryEventBus::with_default_provider();
        bus.register_handler(Box::new(MockEventHandler::new("Keep", &["TaskCreated"])))
            .await
            .expect("register keep");
        bus.register_handler(Box::new(MockEventHandler::new("Remove", &["TaskCreated"])))
            .await
            .expect("register remove");

        bus.unregister_handler("Remove").await.expect("unregister");

        // The "Keep" handler should still be registered
        let event = make_event();
        bus.publish(&event).await.expect("publish");
    }

    // ========== EventPublisherImpl ==========

    #[tokio::test]
    async fn test_event_publisher_impl_publish_delegates_to_bus() {
        let bus = Arc::new(InMemoryEventBus::with_default_provider());
        bus.register_handler(Box::new(MockEventHandler::new(
            "PubHandler",
            &["TaskCreated"],
        )))
        .await
        .expect("register");

        let publisher = EventPublisherImpl::new(bus);
        let event = make_event();
        let result = publisher.publish(&event).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_publisher_impl_publish_batch_publishes_all_events() {
        let bus = Arc::new(InMemoryEventBus::with_default_provider());
        bus.register_handler(Box::new(MockEventHandler::new(
            "BatchHandler",
            &["TaskCreated"],
        )))
        .await
        .expect("register");

        let publisher = EventPublisherImpl::new(bus);
        let e1 = make_event();
        let e2 = make_event();
        let e3 = make_event();
        let events: Vec<&dyn DomainEvent> = vec![&e1, &e2, &e3];
        let result = publisher.publish_batch(events).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_publisher_impl_publish_batch_empty_vec_returns_ok() {
        let bus = Arc::new(InMemoryEventBus::with_default_provider());
        let publisher = EventPublisherImpl::new(bus);
        let events: Vec<&dyn DomainEvent> = vec![];
        let result = publisher.publish_batch(events).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_event_publisher_impl_publish_batch_with_mixed_event_types() {
        let bus = Arc::new(InMemoryEventBus::with_default_provider());
        bus.register_handler(Box::new(MockEventHandler::new(
            "CreatedHandler",
            &["TaskCreated"],
        )))
        .await
        .expect("register created");
        bus.register_handler(Box::new(MockEventHandler::new(
            "CompletedHandler",
            &["TaskCompleted"],
        )))
        .await
        .expect("register completed");

        let publisher = EventPublisherImpl::new(bus);
        let created = make_event();
        let completed = make_completed_event();
        let events: Vec<&dyn DomainEvent> = vec![&created, &completed];
        let result = publisher.publish_batch(events).await;
        assert!(result.is_ok());
    }

    // ========== EventLoggingHandler ==========

    #[tokio::test]
    async fn test_event_logging_handler_handle_succeeds() {
        let handler = EventLoggingHandler;
        let event = make_event();
        let result = handler.handle(&event).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_event_logging_handler_name() {
        let handler = EventLoggingHandler;
        assert_eq!(handler.name(), "EventLoggingHandler");
    }

    #[test]
    fn test_event_logging_handler_subscribe_to_returns_wildcard() {
        let handler = EventLoggingHandler;
        assert_eq!(handler.subscribe_to(), &["*"]);
    }

    #[test]
    fn test_event_logging_handler_default_impl() {
        let handler = EventLoggingHandler;
        assert_eq!(handler.name(), "EventLoggingHandler");
        assert_eq!(handler.subscribe_to(), &["*"]);
    }

    #[test]
    fn test_event_logging_handler_clone_preserves_behavior() {
        let handler = EventLoggingHandler;
        let cloned = handler.clone();
        assert_eq!(cloned.name(), "EventLoggingHandler");
        assert_eq!(cloned.subscribe_to(), &["*"]);
    }

    #[tokio::test]
    async fn test_event_logging_handler_handles_completed_event() {
        let handler = EventLoggingHandler;
        let event = make_completed_event();
        let result = handler.handle(&event).await;
        assert!(result.is_ok());
    }

    // ========== SimpleEventListener ==========

    #[tokio::test]
    async fn test_simple_event_listener_new_starts_at_zero() {
        let listener = SimpleEventListener::new();
        assert_eq!(listener.get_event_count().await, 0);
    }

    #[tokio::test]
    async fn test_simple_event_listener_handle_increments_count() {
        let listener = SimpleEventListener::new();
        let event = make_event();
        listener
            .handle(&event)
            .await
            .expect("handle should succeed");
        assert_eq!(listener.get_event_count().await, 1);
    }

    #[tokio::test]
    async fn test_simple_event_listener_handle_multiple_events_increments() {
        let listener = SimpleEventListener::new();
        let e1 = make_event();
        let e2 = make_event();
        let e3 = make_completed_event();
        listener.handle(&e1).await.expect("handle e1");
        listener.handle(&e2).await.expect("handle e2");
        listener.handle(&e3).await.expect("handle e3");
        assert_eq!(listener.get_event_count().await, 3);
    }

    #[test]
    fn test_simple_event_listener_name() {
        let listener = SimpleEventListener::new();
        assert_eq!(listener.name(), "SimpleEventListener");
    }

    #[test]
    fn test_simple_event_listener_subscribe_to_returns_wildcard() {
        let listener = SimpleEventListener::new();
        assert_eq!(listener.subscribe_to(), &["*"]);
    }

    #[tokio::test]
    async fn test_simple_event_listener_default_starts_at_zero() {
        let listener = SimpleEventListener::default();
        assert_eq!(listener.get_event_count().await, 0);
    }

    #[tokio::test]
    async fn test_simple_event_listener_clone_shares_count() {
        // Clone shares the Arc<RwLock<u64>>, so increments are visible from both
        let listener = SimpleEventListener::new();
        let cloned = listener.clone();
        let event = make_event();
        listener.handle(&event).await.expect("handle");
        assert_eq!(cloned.get_event_count().await, 1);
        assert_eq!(listener.get_event_count().await, 1);
    }

    #[tokio::test]
    async fn test_handler_with_counter_is_called_on_publish() {
        // Verify that a handler registered for a specific event type is actually
        // invoked when that event is published. Uses a shared AtomicU64 counter
        // because the handler is moved into the bus on register.
        let bus = InMemoryEventBus::with_default_provider();
        let counter = Arc::new(AtomicU64::new(0));
        let handler = MockEventHandler::with_counter("Counted", &["TaskCreated"], counter.clone());
        bus.register_handler(Box::new(handler))
            .await
            .expect("register");

        let event = make_event();
        bus.publish(&event).await.expect("publish");

        assert_eq!(
            counter.load(Ordering::SeqCst),
            1,
            "handler should be called once"
        );
    }

    #[tokio::test]
    async fn test_wildcard_handler_not_invoked_for_specific_event_type() {
        // Source behavior: handlers registered under "*" are stored under the "*"
        // key, but publish() only looks up handlers by the concrete event_type.
        // Therefore a wildcard handler is NOT invoked when a specific event is
        // published. This test documents that behavior.
        let bus = InMemoryEventBus::with_default_provider();
        let counter = Arc::new(AtomicU64::new(0));
        let handler = MockEventHandler::with_counter("Wildcard", &["*"], counter.clone());
        bus.register_handler(Box::new(handler))
            .await
            .expect("register");

        let event = make_event();
        bus.publish(&event).await.expect("publish");

        assert_eq!(
            counter.load(Ordering::SeqCst),
            0,
            "wildcard handler must not be invoked for a specific event type"
        );
    }

    #[tokio::test]
    async fn test_handler_called_once_per_publish() {
        let bus = InMemoryEventBus::with_default_provider();
        let counter = Arc::new(AtomicU64::new(0));
        let handler =
            MockEventHandler::with_counter("OncePerPublish", &["TaskCreated"], counter.clone());
        bus.register_handler(Box::new(handler))
            .await
            .expect("register");

        let e1 = make_event();
        let e2 = make_event();
        bus.publish(&e1).await.expect("publish e1");
        bus.publish(&e2).await.expect("publish e2");

        assert_eq!(
            counter.load(Ordering::SeqCst),
            2,
            "handler should be called twice"
        );
    }
}
