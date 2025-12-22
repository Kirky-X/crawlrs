// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::models::webhook::WebhookEvent;
use anyhow::Result;
use async_trait::async_trait;

/// Webhook服务特质
///
/// 定义Webhook发送的核心逻辑
#[async_trait]
pub trait WebhookService: Send + Sync {
    /// 发送Webhook事件
    ///
    /// # 参数
    ///
    /// * `event` - Webhook事件
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 发送成功
    /// * `Err(anyhow::Error)` - 发送失败
    async fn send_webhook(&self, event: &WebhookEvent) -> Result<()>;
}
