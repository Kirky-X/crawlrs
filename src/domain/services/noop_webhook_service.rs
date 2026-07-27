// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Noop Webhook 服务实现（webhook feature 关闭时使用）
//!
//! R-wh-002 / T024-T025：当 `webhook` feature 关闭时，`init_services`
//! 装配此 `NoopWebhookService` 替代 `WebhookServiceImpl`，所有方法返回 `Ok(())`，
//! 保证 handler 内 `trigger_completion`/`trigger_failure` 调用经 trait 走 Noop 放行。
//!
//! # 契约
//!
//! - `send_webhook` → `Ok(())`（不发送）
//! - `trigger_completion` → `Ok(())`（不触发完成通知）
//! - `trigger_failure` → `Ok(())`（不触发失败通知）
//! - 无副作用（不写日志、不写 DB、不发送 HTTP）

use anyhow::Result;
use async_trait::async_trait;

use crate::domain::models::{Task, WebhookEvent};
use crate::domain::services::webhook_service::WebhookService;

/// Noop Webhook 服务
///
/// R-wh-002 / T025：webhook feature 关闭时的 webhook 服务实现。
/// 所有方法返回 `Ok(())`，保证业务逻辑在无 webhook 投递时正常运转。
#[derive(Debug, Clone, Default)]
pub struct NoopWebhookService;

impl NoopWebhookService {
    /// 创建新的 NoopWebhookService
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl WebhookService for NoopWebhookService {
    /// 发送 webhook 事件 → 空操作成功（不发送）
    async fn send_webhook(&self, _event: &WebhookEvent) -> Result<()> {
        Ok(())
    }

    /// 触发任务完成 webhook → 空操作成功（不触发通知）
    async fn trigger_completion(&self, _task: &Task) -> Result<()> {
        Ok(())
    }

    /// 触发任务失败 webhook → 空操作成功（不触发通知）
    async fn trigger_failure(&self, _task: &Task, _error_msg: String) -> Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::task_domain::TaskType;
    use crate::domain::models::Task;
    use uuid::Uuid;

    // R-wh-002 / T024：以下测试钉住 Noop 放行契约。

    #[tokio::test]
    async fn test_noop_trigger_completion_returns_ok() {
        let svc = NoopWebhookService::new();
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Scrape,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        );
        let result = svc.trigger_completion(&task).await;
        assert!(
            result.is_ok(),
            "trigger_completion should always succeed (no webhook sent)"
        );
    }

    #[tokio::test]
    async fn test_noop_trigger_failure_returns_ok() {
        let svc = NoopWebhookService::new();
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        );
        let result = svc
            .trigger_failure(&task, "test error message".to_string())
            .await;
        assert!(
            result.is_ok(),
            "trigger_failure should always succeed (no webhook sent)"
        );
    }
}
