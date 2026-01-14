// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Webhook Trigger Service
//!
//! Provides unified webhook triggering for task completion and failure events.
//! Consolidates webhook logic from scrape_worker.

use crate::application::dto::scrape_request::ScrapeRequestDto;
use crate::domain::models::task::Task;
use crate::domain::models::webhook::{WebhookEvent, WebhookEventType, WebhookStatus};
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use chrono::Utc;
use serde_json::json;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

/// Service for triggering webhooks
pub struct WebhookTriggerService<R: WebhookEventRepository> {
    repository: Arc<R>,
}

impl<R: WebhookEventRepository> WebhookTriggerService<R> {
    /// Create a new WebhookTriggerService
    pub fn new(repository: Arc<R>) -> Self {
        Self { repository }
    }

    /// Trigger webhook for task completion or failure
    ///
    /// # Arguments
    ///
    /// * `task` - The task that completed or failed
    /// * `event_type` - Type of webhook event
    /// * `error_msg` - Optional error message for failure events
    pub async fn trigger_task_webhook(
        &self,
        task: &Task,
        event_type: WebhookEventType,
        error_msg: Option<String>,
    ) {
        let webhook_url = self.extract_webhook_url(task);

        if let Some(url) = webhook_url {
            self.send_webhook(task, url, event_type, error_msg).await;
        }
    }

    /// Extract webhook URL from task payload
    fn extract_webhook_url(&self, task: &Task) -> Option<String> {
        // Try to parse as ScrapeRequestDto first
        if let Ok(req) = serde_json::from_value::<ScrapeRequestDto>(task.payload.clone()) {
            return req.webhook;
        }

        // Fall back to extracting from payload directly
        task.payload
            .get("webhook")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
    }

    /// Send webhook event
    async fn send_webhook(
        &self,
        task: &Task,
        webhook_url: String,
        event_type: WebhookEventType,
        error_msg: Option<String>,
    ) {
        info!(
            "Triggering webhook {:?} for task {} (url: {})",
            event_type, task.id, webhook_url
        );

        let mut payload = json!({
            "task_id": task.id,
            "status": if error_msg.is_some() { "failed" } else { "completed" },
            "url": task.url,
            "timestamp": Utc::now().timestamp()
        });

        if let Some(msg) = error_msg {
            payload["error"] = json!(msg);
        }

        let event = WebhookEvent {
            id: Uuid::new_v4(),
            team_id: task.team_id,
            webhook_id: Uuid::nil(),
            event_type,
            payload,
            webhook_url,
            status: WebhookStatus::Pending,
            attempt_count: 0,
            max_retries: 5,
            response_status: None,
            response_body: None,
            error_message: None,
            next_retry_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        if let Err(e) = self.repository.create(&event).await {
            error!("Failed to create webhook event for task {}: {}", task.id, e);
        }
    }

    /// Trigger webhook for task completion
    pub async fn trigger_completion(&self, task: &Task) {
        let event_type = match task.task_type {
            crate::domain::models::task::TaskType::Scrape => WebhookEventType::ScrapeCompleted,
            crate::domain::models::task::TaskType::Crawl => WebhookEventType::CrawlCompleted,
            crate::domain::models::task::TaskType::Extract => {
                WebhookEventType::Custom("extract.completed".to_string())
            }
            _ => WebhookEventType::Custom("task.completed".to_string()),
        };

        self.trigger_task_webhook(task, event_type, None).await;
    }

    /// Trigger webhook for task failure
    pub async fn trigger_failure(&self, task: &Task, error_msg: String) {
        let event_type = match task.task_type {
            crate::domain::models::task::TaskType::Scrape => WebhookEventType::ScrapeFailed,
            crate::domain::models::task::TaskType::Crawl => WebhookEventType::CrawlFailed,
            crate::domain::models::task::TaskType::Extract => {
                WebhookEventType::Custom("extract.failed".to_string())
            }
            _ => WebhookEventType::Custom("task.failed".to_string()),
        };

        self.trigger_task_webhook(task, event_type, Some(error_msg))
            .await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::task::TaskStatus;
    use async_trait::async_trait;

    struct MockWebhookEventRepository {
        created_events: Arc<std::sync::Mutex<Vec<WebhookEvent>>>,
    }

    #[async_trait]
    impl WebhookEventRepository for MockWebhookEventRepository {
        async fn create(&self, event: &WebhookEvent) -> Result<WebhookEvent, anyhow::Error> {
            self.created_events.lock().unwrap().push(event.clone());
            Ok(event.clone())
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<WebhookEvent>, anyhow::Error> {
            unimplemented!()
        }

        async fn find_pending(&self, _limit: i32) -> Result<Vec<WebhookEvent>, anyhow::Error> {
            Ok(vec![])
        }

        async fn mark_sent(
            &self,
            _id: Uuid,
            _status: i32,
            _response_body: String,
        ) -> Result<(), anyhow::Error> {
            unimplemented!()
        }

        async fn mark_failed(&self, _id: Uuid, _error: String) -> Result<(), anyhow::Error> {
            unimplemented!()
        }

        async fn update_next_retry(
            &self,
            _id: Uuid,
            _next_retry: chrono::DateTime<Utc>,
        ) -> Result<(), anyhow::Error> {
            unimplemented!()
        }

        async fn find_by_status(
            &self,
            _status: WebhookStatus,
        ) -> Result<Vec<WebhookEvent>, anyhow::Error> {
            Ok(vec![])
        }

        async fn count_by_status(&self, _status: WebhookStatus) -> Result<i64, anyhow::Error> {
            Ok(0)
        }

        async fn delete(&self, _id: Uuid) -> Result<(), anyhow::Error> {
            unimplemented!()
        }

        async fn bulk_update_status(
            &self,
            _ids: &[Uuid],
            _status: WebhookStatus,
        ) -> Result<u64, anyhow::Error> {
            Ok(0)
        }
    }

    fn create_test_task() -> Task {
        Task {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            url: "http://example.com".to_string(),
            task_type: crate::domain::models::task::TaskType::Scrape,
            status: TaskStatus::Completed,
            payload: serde_json::json!({
                "url": "http://example.com",
                "webhook": "https://example.com/webhook"
            }),
            attempt_count: 1,
            max_retries: 3,
            scheduled_at: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[tokio::test]
    async fn test_trigger_completion() {
        let created_events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let repo = MockWebhookEventRepository {
            created_events: created_events.clone(),
        };

        let service = WebhookTriggerService::new(Arc::new(repo));
        let task = create_test_task();

        service.trigger_completion(&task).await;

        let events = created_events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].webhook_url, "https://example.com/webhook");
        assert!(events[0].payload["error"].is_null());
    }

    #[tokio::test]
    async fn test_trigger_failure() {
        let created_events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let repo = MockWebhookEventRepository {
            created_events: created_events.clone(),
        };

        let service = WebhookTriggerService::new(Arc::new(repo));
        let task = create_test_task();

        service
            .trigger_failure(&task, "Timeout error".to_string())
            .await;

        let events = created_events.lock().unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].webhook_url, "https://example.com/webhook");
        assert_eq!(events[0].payload["error"], "Timeout error");
    }

    #[tokio::test]
    async fn test_no_webhook_no_trigger() {
        let created_events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let repo = MockWebhookEventRepository {
            created_events: created_events.clone(),
        };

        let service = WebhookTriggerService::new(Arc::new(repo));

        let mut task = create_test_task();
        task.payload = serde_json::json!({"url": "http://example.com"}); // No webhook

        service.trigger_completion(&task).await;

        let events = created_events.lock().unwrap();
        assert!(events.is_empty()); // No webhook, no trigger
    }
}
