// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::models::webhook::WebhookEvent;
use anyhow::Result;
use async_trait::async_trait;
use shaku::Interface;

/// Webhook服务特质
///
/// 定义Webhook发送的核心逻辑
#[async_trait]
pub trait WebhookService: Interface + Send + Sync {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::webhook::{WebhookEvent, WebhookEventType, WebhookStatus};
    use chrono::Utc;
    use std::sync::Arc;
    use uuid::Uuid;

    struct MockWebhookService {
        sent_events: Arc<std::sync::Mutex<Vec<WebhookEvent>>>,
        should_fail: bool,
    }

    #[async_trait::async_trait]
    impl WebhookService for MockWebhookService {
        async fn send_webhook(&self, event: &WebhookEvent) -> Result<()> {
            self.sent_events.lock().unwrap().push(event.clone());
            if self.should_fail {
                Err(anyhow::anyhow!("Mock webhook failure"))
            } else {
                Ok(())
            }
        }
    }

    fn create_test_event() -> WebhookEvent {
        WebhookEvent {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            webhook_id: Uuid::new_v4(),
            event_type: WebhookEventType::ScrapeCompleted,
            payload: serde_json::json!({"url": "http://example.com", "status": "success"}),
            webhook_url: "https://example.com/webhook".to_string(),
            status: WebhookStatus::Pending,
            attempt_count: 0,
            max_retries: 3,
            response_status: None,
            response_body: None,
            error_message: None,
            next_retry_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            delivered_at: None,
        }
    }

    #[tokio::test]
    async fn test_webhook_service_sends_successfully() {
        let sent_events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let service = MockWebhookService {
            sent_events: sent_events.clone(),
            should_fail: false,
        };

        let event = create_test_event();
        let event_id = event.id;

        let result = service.send_webhook(&event).await;

        assert!(result.is_ok());
        let sent = sent_events.lock().unwrap();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].id, event_id);
    }

    #[tokio::test]
    async fn test_webhook_service_handles_failure() {
        let sent_events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let service = MockWebhookService {
            sent_events: sent_events.clone(),
            should_fail: true,
        };

        let event = create_test_event();

        let result = service.send_webhook(&event).await;

        assert!(result.is_err());
        let sent = sent_events.lock().unwrap();
        assert_eq!(sent.len(), 1);
    }
}
