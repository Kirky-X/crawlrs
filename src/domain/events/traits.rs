// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 领域事件特质
//!
//! 定义事件发布和订阅的核心接口。

use async_trait::async_trait;

use super::models::DomainEvent;

/// 领域事件处理器
///
/// 异步处理领域事件的处理器接口。
/// 每个处理器负责处理特定类型的事件。
#[async_trait]
pub trait EventHandler: Send + Sync {
    /// 处理事件
    async fn handle(&self, event: &dyn DomainEvent) -> Result<(), anyhow::Error>;
    /// 获取处理器名称
    fn name(&self) -> &'static str;
    /// 获取订阅的事件类型
    fn subscribe_to(&self) -> &'static [&'static str];
}

/// 事件发布器接口
///
/// 用于发布领域事件的接口。
#[async_trait]
pub trait EventPublisher: Send + Sync {
    /// 发布单个事件
    async fn publish(&self, event: &dyn DomainEvent) -> Result<(), anyhow::Error>;
    /// 发布多个事件
    async fn publish_batch(&self, events: Vec<&dyn DomainEvent>) -> Result<(), anyhow::Error>;
}

/// 事件订阅器接口
///
/// 用于订阅和接收领域事件的接口。
#[async_trait]
pub trait EventSubscriber: Send + Sync {
    /// 订阅事件类型
    fn subscribe_to(&self) -> &'static [&'static str];
    /// 获取订阅者名称
    fn name(&self) -> &'static str;
}

/// 事件总线
///
/// 协调事件的发布和订阅的核心组件。
#[async_trait]
pub trait EventBus: Send + Sync {
    /// 注册事件处理器
    async fn register_handler(&self, handler: Box<dyn EventHandler>) -> Result<(), anyhow::Error>;
    /// 发布事件
    async fn publish(&self, event: &dyn DomainEvent) -> Result<(), anyhow::Error>;
    /// 发布事件并等待处理完成
    async fn publish_sync(&self, event: &dyn DomainEvent) -> Result<(), anyhow::Error>;
    /// 取消注册处理器
    async fn unregister_handler(&self, handler_name: &str) -> Result<(), anyhow::Error>;
}

/// 事件存储接口
///
/// 用于持久化领域事件的接口。
#[async_trait]
pub trait EventStore: Send + Sync {
    /// 保存事件
    async fn save(&self, event: &dyn DomainEvent) -> Result<(), anyhow::Error>;
    /// 根据聚合根ID获取事件
    async fn get_by_aggregate_id(
        &self,
        aggregate_id: uuid::Uuid,
    ) -> Result<Vec<Box<dyn DomainEvent>>, anyhow::Error>;
    /// 获取所有事件（带分页）
    async fn get_all(
        &self,
        offset: u64,
        limit: u64,
    ) -> Result<Vec<Box<dyn DomainEvent>>, anyhow::Error>;
}

/// 事件元数据提供者
///
/// 用于从上下文中提取事件元数据的接口。
pub trait EventMetadataProvider: Send + Sync {
    /// 获取当前追踪ID
    fn get_trace_id(&self) -> Option<uuid::Uuid>;
    /// 获取当前租户ID
    fn get_tenant_id(&self) -> Option<uuid::Uuid>;
}

/// 事件处理结果
#[derive(Debug, Clone)]
pub enum EventProcessingResult {
    /// 处理成功
    Success,
    /// 处理失败，但已重试
    Retried,
    /// 处理失败，已跳过
    Skipped,
    /// 处理失败，已移入死信队列
    DeadLettered,
}

impl Default for EventProcessingResult {
    fn default() -> Self {
        EventProcessingResult::Success
    }
}
