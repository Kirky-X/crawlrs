// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Webhook Event Builder
//!
//! 封装 `WebhookEvent` 与 payload 的构造逻辑，
//! 将"如何构建事件"与"如何发送事件"解耦。
//! `WebhookServiceImpl` 保留调度与持久化职责，
//! 本模块专注于事件对象的纯函数构造。

use crate::domain::models::{Task, WebhookEvent, WebhookEventType};
use chrono::Utc;
use serde_json::json;
use uuid::Uuid;

/// Webhook 事件构造器
///
/// 提供静态方法从 `Task` 或显式参数构建 `WebhookEvent` 及其 payload。
/// 所有方法均为无状态纯函数，不依赖 I/O 或外部服务。
pub struct WebhookEventBuilder;

impl WebhookEventBuilder {
    /// 从 `Task` 构建任务 webhook 的 JSON payload。
    ///
    /// # Arguments
    ///
    /// * `task` - 目标任务（提取 id, url）
    /// * `error_msg` - 若为 `Some`，payload 标记 `"failed"` 并包含 error 字段；
    ///   若为 `None`，标记 `"completed"`
    ///
    /// # Returns
    ///
    /// 包含 `task_id`, `status`, `url`, `timestamp` 及可选 `error` 的 JSON 对象
    pub fn build_task_payload(task: &Task, error_msg: Option<&str>) -> serde_json::Value {
        let mut payload = json!({
            "task_id": task.id,
            "status": if error_msg.is_some() { "failed" } else { "completed" },
            "url": task.url.clone(),
            "timestamp": Utc::now().timestamp(),
        });

        if let Some(msg) = error_msg {
            payload["error"] = json!(msg);
        }

        payload
    }

    /// 从 `Task` 构建完整的 `WebhookEvent`（任务通知场景）。
    ///
    /// # Arguments
    ///
    /// * `task` - 目标任务
    /// * `event_type` - 事件类型（completed/failed 等）
    /// * `payload` - 已构造的 JSON payload
    /// * `webhook_url` - 目标 webhook URL
    ///
    /// # Returns
    ///
    /// 新的 `WebhookEvent`（webhook_id 为 `Uuid::nil()`，表示来自任务通知而非注册端点）
    pub fn build_task_event(
        task: &Task,
        event_type: WebhookEventType,
        payload: serde_json::Value,
        webhook_url: String,
    ) -> WebhookEvent {
        WebhookEvent::new(
            Uuid::new_v4(),
            task.team_id,
            Uuid::nil(),
            event_type,
            payload,
            webhook_url,
        )
    }

    /// 从显式参数构建 `WebhookEvent`（管理接口触发场景）。
    ///
    /// # Arguments
    ///
    /// * `team_id` - 团队 ID
    /// * `webhook_id` - 已注册的 webhook ID
    /// * `event_type` - 事件类型
    /// * `payload` - JSON payload
    /// * `webhook_url` - 目标 webhook URL
    ///
    /// # Returns
    ///
    /// 新的 `WebhookEvent`
    pub fn build_triggered_event(
        team_id: Uuid,
        webhook_id: Uuid,
        event_type: WebhookEventType,
        payload: serde_json::Value,
        webhook_url: String,
    ) -> WebhookEvent {
        WebhookEvent::new(
            Uuid::new_v4(),
            team_id,
            webhook_id,
            event_type,
            payload,
            webhook_url,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{TaskStatus, TaskType};
    use chrono::Utc;
    use serde_json::json;

    fn create_test_task() -> Task {
        let now = Utc::now();
        Task {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            url: "http://example.com".to_string(),
            task_type: TaskType::Scrape,
            status: TaskStatus::Completed,
            payload: json!({"url": "http://example.com"}),
            attempt_count: 1,
            max_retries: 3,
            scheduled_at: None,
            created_at: now,
            updated_at: now,
            priority: 0,
            retry_count: 0,
            expires_at: None,
            started_at: None,
            completed_at: None,
            crawl_id: None,
            lock_token: None,
            lock_expires_at: None,
        }
    }

    #[test]
    fn test_build_task_payload_completed() {
        let task = create_test_task();
        let payload = WebhookEventBuilder::build_task_payload(&task, None);

        assert_eq!(payload["task_id"], task.id.to_string());
        assert_eq!(payload["status"], "completed");
        assert_eq!(payload["url"], task.url);
        assert!(payload["timestamp"].as_i64().is_some());
        assert!(payload.get("error").is_none());
    }

    #[test]
    fn test_build_task_payload_failed() {
        let task = create_test_task();
        let payload = WebhookEventBuilder::build_task_payload(&task, Some("timeout"));

        assert_eq!(payload["status"], "failed");
        assert_eq!(payload["error"], "timeout");
    }

    #[test]
    fn test_build_task_event() {
        let task = create_test_task();
        let payload = json!({"task_id": task.id, "status": "completed"});
        let event = WebhookEventBuilder::build_task_event(
            &task,
            WebhookEventType::ScrapeCompleted,
            payload.clone(),
            "https://hook.example.com".to_string(),
        );

        assert_eq!(event.team_id, task.team_id);
        assert_eq!(event.webhook_id, Uuid::nil());
        assert_eq!(event.event_type, WebhookEventType::ScrapeCompleted);
        assert_eq!(event.webhook_url, "https://hook.example.com");
    }

    #[test]
    fn test_build_triggered_event() {
        let team_id = Uuid::new_v4();
        let webhook_id = Uuid::new_v4();
        let payload = json!({"test": true});
        let event = WebhookEventBuilder::build_triggered_event(
            team_id,
            webhook_id,
            WebhookEventType::CrawlCompleted,
            payload,
            "https://hook.example.com".to_string(),
        );

        assert_eq!(event.team_id, team_id);
        assert_eq!(event.webhook_id, webhook_id);
        assert_eq!(event.event_type, WebhookEventType::CrawlCompleted);
    }
}
