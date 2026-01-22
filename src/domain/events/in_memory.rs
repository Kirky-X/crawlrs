// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 内存事件总线实现
//!
//! 提供基于内存的事件总线实现，适用于开发测试和小规模部署。

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error};

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
