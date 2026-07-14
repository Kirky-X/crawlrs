// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::models::{WebhookEvent, WebhookStatus};
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crate::domain::services::webhook_service::WebhookService;
use crate::utils::retry_policy::RetryPolicy;
use crate::workers::worker::{ProcessResult, WorkerProcess};
use anyhow::Result;
use chrono::Utc;
use log::{error, info, warn};
#[cfg(feature = "metrics")]
use metrics::counter;
use std::sync::Arc;

/// Webhook Worker Trait
#[async_trait::async_trait]
pub trait WebhookWorkerTrait: Send + Sync {
    async fn run(&self);
}

/// Webhook工作器
///
/// 负责处理webhook事件的发送和重试
pub struct WebhookWorker {
    /// Webhook事件仓库
    repo: Arc<dyn WebhookEventRepository>,
    /// Webhook服务
    webhook_service: Arc<dyn WebhookService>,
    /// 重试策略
    retry_policy: RetryPolicy,
}

impl WebhookWorker {
    /// 创建新的webhook工作器
    pub fn new(
        repo: Arc<dyn WebhookEventRepository>,
        webhook_service: Arc<dyn WebhookService>,
        retry_policy: RetryPolicy,
    ) -> Self {
        Self {
            repo,
            webhook_service,
            retry_policy,
        }
    }

    /// 使用默认重试策略创建webhook工作器
    pub fn with_default_policy(
        repo: Arc<dyn WebhookEventRepository>,
        webhook_service: Arc<dyn WebhookService>,
    ) -> Self {
        Self::new(repo, webhook_service, RetryPolicy::default())
    }

    /// 处理待处理的webhook事件
    pub async fn process_pending_webhooks(&self) -> Result<()> {
        let pending_events = self
            .repo
            .find_pending(100)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to find pending events: {}", e))?;

        if !pending_events.is_empty() {
            info!("Processing {} pending webhook events", pending_events.len());

            for event in pending_events {
                if let Err(e) = self.process_webhook_event(event).await {
                    error!("Failed to process webhook event: {}", e);
                }
            }
        }

        Ok(())
    }

    /// 处理单个webhook事件
    async fn process_webhook_event(&self, mut event: WebhookEvent) -> Result<()> {
        info!(
            "Processing webhook event {} for URL {} (attempt {})",
            event.id, event.webhook_url, event.attempt_count
        );

        // 尝试发送webhook
        match self.webhook_service.send_webhook(&event).await {
            Ok(_) => {
                info!("Successfully delivered webhook {}", event.id);
                event.status = WebhookStatus::Delivered;
                event.delivered_at = Some(Utc::now());
                event.response_status = Some(200);
                self.repo
                    .update(&event)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to update event: {}", e))?;

                #[cfg(feature = "metrics")]
                counter!("webhook_delivery_success_total").increment(1);
                Ok(())
            }
            Err(e) => {
                error!("Failed to deliver webhook {}: {}", event.id, e);

                // 尝试解析错误中的 HTTP 状态码
                let error_msg = e.to_string();
                if error_msg.contains("status 500") {
                    event.response_status = Some(500);
                } else if error_msg.contains("status 400") {
                    event.response_status = Some(400);
                } else if error_msg.contains("status 404") {
                    event.response_status = Some(404);
                }

                self.handle_webhook_failure(event, e).await
            }
        }
    }

    /// 处理webhook发送失败
    async fn handle_webhook_failure(
        &self,
        mut event: WebhookEvent,
        error: anyhow::Error,
    ) -> Result<()> {
        let new_attempt_count = event.attempt_count + 1;

        // 检查是否应该重试
        if !self
            .retry_policy
            .should_retry_with_error(new_attempt_count as u32, &error)
        {
            // 达到最大重试次数或错误不可重试，移动到死信队列
            event.status = WebhookStatus::Dead;
            event.error_message = Some(error.to_string());
            event.attempt_count = new_attempt_count;

            warn!(
                "Webhook {} moved to dead letter state after {} attempts: {}",
                event.id, new_attempt_count, error
            );
            #[cfg(feature = "metrics")]
            counter!("webhook_dead_letter_total", "reason" => error.to_string()).increment(1);
        } else {
            // 计算下次重试时间
            let next_retry = self
                .retry_policy
                .next_retry_time(new_attempt_count as u32, Utc::now());

            event.status = WebhookStatus::Failed;
            event.attempt_count = new_attempt_count;
            event.next_retry_at = Some(next_retry);
            event.error_message = Some(error.to_string());

            info!(
                "Webhook {} will be retried at {} (attempt {})",
                event.id, next_retry, new_attempt_count
            );
            #[cfg(feature = "metrics")]
            counter!("webhook_retry_scheduled_total", "attempt" => new_attempt_count.to_string())
                .increment(1);
        }

        self.repo
            .update(&event)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to update event: {}", e))?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl WorkerProcess for WebhookWorker {
    fn name(&self) -> &str {
        "webhook_worker"
    }

    async fn process(&self) -> ProcessResult {
        if let Err(e) = self.process_pending_webhooks().await {
            return ProcessResult::Error(format!("Error processing pending webhooks: {}", e));
        }
        ProcessResult::Completed
    }
}

#[async_trait::async_trait]
impl WebhookWorkerTrait for WebhookWorker {
    async fn run(&self) {
        info!("Starting webhook worker loop");
        loop {
            if let Err(e) = self.process_pending_webhooks().await {
                error!("Error processing webhook events: {}", e);
            }
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{Task, WebhookEvent, WebhookEventType, WebhookStatus};
    use crate::domain::repositories::task_repository::RepositoryError;
    use anyhow::anyhow;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;
    use std::time::Duration;
    use uuid::Uuid;

    /// Mock WebhookEventRepository with configurable pending events and update tracking
    struct MockWebhookRepo {
        pending_events: Mutex<Vec<WebhookEvent>>,
        updated_events: Mutex<Vec<WebhookEvent>>,
        fail_find_pending: bool,
    }

    impl MockWebhookRepo {
        fn new(pending: Vec<WebhookEvent>) -> Self {
            Self {
                pending_events: Mutex::new(pending),
                updated_events: Mutex::new(vec![]),
                fail_find_pending: false,
            }
        }

        fn new_empty() -> Self {
            Self::new(vec![])
        }

        fn new_failing() -> Self {
            Self {
                pending_events: Mutex::new(vec![]),
                updated_events: Mutex::new(vec![]),
                fail_find_pending: true,
            }
        }

        fn updated_events(&self) -> Vec<WebhookEvent> {
            self.updated_events.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl WebhookEventRepository for MockWebhookRepo {
        async fn create(&self, event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError> {
            Ok(event.clone())
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<WebhookEvent>, RepositoryError> {
            Ok(None)
        }

        async fn find_pending(&self, _limit: u64) -> Result<Vec<WebhookEvent>, RepositoryError> {
            if self.fail_find_pending {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "repo unavailable"
                )));
            }
            Ok(self.pending_events.lock().unwrap().drain(..).collect())
        }

        async fn find_by_team_id_paginated(
            &self,
            _team_id: Uuid,
            _limit: u32,
            _offset: u32,
        ) -> Result<Vec<WebhookEvent>, RepositoryError> {
            Ok(vec![])
        }

        async fn count_by_team_id(&self, _team_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn update(&self, event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError> {
            self.updated_events.lock().unwrap().push(event.clone());
            Ok(event.clone())
        }
    }

    /// Mock WebhookService that can be configured to succeed or fail
    struct MockWebhookService {
        send_success: bool,
        send_count: AtomicU32,
    }

    impl MockWebhookService {
        fn new_success() -> Self {
            Self {
                send_success: true,
                send_count: AtomicU32::new(0),
            }
        }

        fn new_failure() -> Self {
            Self {
                send_success: false,
                send_count: AtomicU32::new(0),
            }
        }

        fn send_count(&self) -> u32 {
            self.send_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl WebhookService for MockWebhookService {
        async fn send_webhook(&self, _event: &WebhookEvent) -> Result<()> {
            self.send_count.fetch_add(1, Ordering::SeqCst);
            if self.send_success {
                Ok(())
            } else {
                Err(anyhow!("Failed to send webhook: connection refused"))
            }
        }

        async fn trigger_completion(&self, _task: &Task) -> Result<()> {
            Ok(())
        }

        async fn trigger_failure(&self, _task: &Task, _error_msg: String) -> Result<()> {
            Ok(())
        }
    }

    /// Mock WebhookService that fails with an HTTP 500 status in the error message
    struct MockWebhookService500;

    #[async_trait]
    impl WebhookService for MockWebhookService500 {
        async fn send_webhook(&self, _event: &WebhookEvent) -> Result<()> {
            Err(anyhow!(
                "Request failed with status 500: internal server error"
            ))
        }

        async fn trigger_completion(&self, _task: &Task) -> Result<()> {
            Ok(())
        }

        async fn trigger_failure(&self, _task: &Task, _error_msg: String) -> Result<()> {
            Ok(())
        }
    }

    fn make_test_event(attempt_count: i32) -> WebhookEvent {
        let mut event = WebhookEvent::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::nil(),
            WebhookEventType::ScrapeCompleted,
            serde_json::json!({"task_id": "abc"}),
            "https://example.com/webhook".to_string(),
        );
        event.attempt_count = attempt_count;
        event
    }

    fn make_worker(
        repo: Arc<dyn WebhookEventRepository>,
        service: Arc<dyn WebhookService>,
    ) -> WebhookWorker {
        WebhookWorker::with_default_policy(repo, service)
    }

    // ========== name() ==========

    #[test]
    fn test_worker_name() {
        let repo: Arc<dyn WebhookEventRepository> = Arc::new(MockWebhookRepo::new_empty());
        let service: Arc<dyn WebhookService> = Arc::new(MockWebhookService::new_success());
        let worker = make_worker(repo, service);
        assert_eq!(worker.name(), "webhook_worker");
    }

    // ========== new() / with_default_policy() ==========

    #[test]
    fn test_new_uses_provided_retry_policy() {
        let repo: Arc<dyn WebhookEventRepository> = Arc::new(MockWebhookRepo::new_empty());
        let service: Arc<dyn WebhookService> = Arc::new(MockWebhookService::new_success());
        let policy = RetryPolicy::fast();
        let worker = WebhookWorker::new(repo, service, policy);
        assert_eq!(worker.retry_policy.max_retries, 5);
        assert_eq!(
            worker.retry_policy.initial_backoff,
            Duration::from_millis(500)
        );
    }

    #[test]
    fn test_with_default_policy_uses_standard_policy() {
        let repo: Arc<dyn WebhookEventRepository> = Arc::new(MockWebhookRepo::new_empty());
        let service: Arc<dyn WebhookService> = Arc::new(MockWebhookService::new_success());
        let worker = make_worker(repo, service);
        assert_eq!(worker.retry_policy.max_retries, 5);
        assert_eq!(worker.retry_policy.initial_backoff, Duration::from_secs(1));
    }

    // ========== process_pending_webhooks ==========

    #[tokio::test]
    async fn test_process_pending_webhooks_empty() {
        let repo = Arc::new(MockWebhookRepo::new_empty());
        let service = Arc::new(MockWebhookService::new_success());
        let worker = make_worker(repo.clone(), service.clone());
        let result = worker.process_pending_webhooks().await;
        assert!(result.is_ok());
        assert_eq!(service.send_count(), 0);
        assert!(repo.updated_events().is_empty());
    }

    #[tokio::test]
    async fn test_process_pending_webhooks_success_delivers_event() {
        let event = make_test_event(0);
        let repo = Arc::new(MockWebhookRepo::new(vec![event]));
        let service = Arc::new(MockWebhookService::new_success());
        let worker = make_worker(repo.clone(), service.clone());
        let result = worker.process_pending_webhooks().await;
        assert!(result.is_ok());
        assert_eq!(service.send_count(), 1);
        let updated = repo.updated_events();
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].status, WebhookStatus::Delivered);
        assert!(updated[0].delivered_at.is_some());
        assert_eq!(updated[0].response_status, Some(200));
    }

    #[tokio::test]
    async fn test_process_pending_webhooks_multiple_events() {
        let events = vec![make_test_event(0), make_test_event(1), make_test_event(2)];
        let repo = Arc::new(MockWebhookRepo::new(events));
        let service = Arc::new(MockWebhookService::new_success());
        let worker = make_worker(repo.clone(), service.clone());
        let result = worker.process_pending_webhooks().await;
        assert!(result.is_ok());
        assert_eq!(service.send_count(), 3);
        let updated = repo.updated_events();
        assert_eq!(updated.len(), 3);
        for ev in &updated {
            assert_eq!(ev.status, WebhookStatus::Delivered);
        }
    }

    #[tokio::test]
    async fn test_process_pending_webhooks_repo_failure_returns_error() {
        let repo = Arc::new(MockWebhookRepo::new_failing());
        let service = Arc::new(MockWebhookService::new_success());
        let worker = make_worker(repo, service);
        let result = worker.process_pending_webhooks().await;
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Failed to find pending events"));
    }

    // ========== process() (WorkerProcess trait) ==========

    #[tokio::test]
    async fn test_process_returns_completed_on_success() {
        let repo = Arc::new(MockWebhookRepo::new_empty());
        let service = Arc::new(MockWebhookService::new_success());
        let worker = make_worker(repo, service);
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
    }

    #[tokio::test]
    async fn test_process_returns_error_on_repo_failure() {
        let repo = Arc::new(MockWebhookRepo::new_failing());
        let service = Arc::new(MockWebhookService::new_success());
        let worker = make_worker(repo, service);
        let result = worker.process().await;
        match result {
            ProcessResult::Error(msg) => {
                assert!(msg.contains("Error processing pending webhooks"));
            }
            _ => panic!("Expected ProcessResult::Error, got {:?}", result),
        }
    }

    // ========== webhook failure handling ==========

    #[tokio::test]
    async fn test_webhook_failure_schedules_retry() {
        let event = make_test_event(0);
        let repo = Arc::new(MockWebhookRepo::new(vec![event]));
        let service = Arc::new(MockWebhookService::new_failure());
        let worker = make_worker(repo.clone(), service);
        let result = worker.process_pending_webhooks().await;
        assert!(result.is_ok());
        let updated = repo.updated_events();
        assert_eq!(updated.len(), 1);
        // Should be marked as Failed (retryable) since attempt 1 < max_retries
        assert_eq!(updated[0].status, WebhookStatus::Failed);
        assert_eq!(updated[0].attempt_count, 1);
        assert!(updated[0].next_retry_at.is_some());
        assert!(updated[0].error_message.is_some());
    }

    #[tokio::test]
    async fn test_webhook_failure_moves_to_dead_after_max_retries() {
        // attempt_count starts at 4, new_attempt_count will be 5, which equals max_retries (5)
        let mut event = make_test_event(4);
        event.max_retries = 5;
        let repo = Arc::new(MockWebhookRepo::new(vec![event]));
        let service = Arc::new(MockWebhookService::new_failure());
        let worker = make_worker(repo.clone(), service);
        let result = worker.process_pending_webhooks().await;
        assert!(result.is_ok());
        let updated = repo.updated_events();
        assert_eq!(updated.len(), 1);
        // Should be moved to Dead state since max retries reached
        assert_eq!(updated[0].status, WebhookStatus::Dead);
        assert_eq!(updated[0].attempt_count, 5);
        assert!(updated[0].error_message.is_some());
    }

    #[tokio::test]
    async fn test_webhook_failure_with_non_retryable_error_moves_to_dead() {
        // Use a service that returns a non-retryable error
        struct NonRetryableService;
        #[async_trait]
        impl WebhookService for NonRetryableService {
            async fn send_webhook(&self, _event: &WebhookEvent) -> Result<()> {
                Err(anyhow!("status 400: bad request"))
            }
            async fn trigger_completion(&self, _task: &Task) -> Result<()> {
                Ok(())
            }
            async fn trigger_failure(&self, _task: &Task, _error_msg: String) -> Result<()> {
                Ok(())
            }
        }
        let event = make_test_event(0);
        let repo = Arc::new(MockWebhookRepo::new(vec![event]));
        let service: Arc<dyn WebhookService> = Arc::new(NonRetryableService);
        let worker = make_worker(repo.clone(), service);
        let result = worker.process_pending_webhooks().await;
        assert!(result.is_ok());
        let updated = repo.updated_events();
        assert_eq!(updated.len(), 1);
        // 400 is not retryable -> Dead
        assert_eq!(updated[0].status, WebhookStatus::Dead);
    }

    // ========== response status parsing ==========

    #[tokio::test]
    async fn test_webhook_failure_parses_500_status() {
        let event = make_test_event(0);
        let repo = Arc::new(MockWebhookRepo::new(vec![event]));
        let service: Arc<dyn WebhookService> = Arc::new(MockWebhookService500);
        let worker = make_worker(repo.clone(), service);
        let result = worker.process_pending_webhooks().await;
        assert!(result.is_ok());
        let updated = repo.updated_events();
        assert_eq!(updated.len(), 1);
        // Error message contains "status 500" -> response_status should be 500
        assert_eq!(updated[0].response_status, Some(500));
    }

    #[tokio::test]
    async fn test_webhook_failure_parses_400_status() {
        struct Service400;
        #[async_trait]
        impl WebhookService for Service400 {
            async fn send_webhook(&self, _event: &WebhookEvent) -> Result<()> {
                Err(anyhow!("failed with status 400: bad request"))
            }
            async fn trigger_completion(&self, _task: &Task) -> Result<()> {
                Ok(())
            }
            async fn trigger_failure(&self, _task: &Task, _error_msg: String) -> Result<()> {
                Ok(())
            }
        }
        let event = make_test_event(0);
        let repo = Arc::new(MockWebhookRepo::new(vec![event]));
        let service: Arc<dyn WebhookService> = Arc::new(Service400);
        let worker = make_worker(repo.clone(), service);
        worker.process_pending_webhooks().await.unwrap();
        let updated = repo.updated_events();
        assert_eq!(updated[0].response_status, Some(400));
    }

    #[tokio::test]
    async fn test_webhook_failure_parses_404_status() {
        struct Service404;
        #[async_trait]
        impl WebhookService for Service404 {
            async fn send_webhook(&self, _event: &WebhookEvent) -> Result<()> {
                Err(anyhow!("error: status 404 not found"))
            }
            async fn trigger_completion(&self, _task: &Task) -> Result<()> {
                Ok(())
            }
            async fn trigger_failure(&self, _task: &Task, _error_msg: String) -> Result<()> {
                Ok(())
            }
        }
        let event = make_test_event(0);
        let repo = Arc::new(MockWebhookRepo::new(vec![event]));
        let service: Arc<dyn WebhookService> = Arc::new(Service404);
        let worker = make_worker(repo.clone(), service);
        worker.process_pending_webhooks().await.unwrap();
        let updated = repo.updated_events();
        assert_eq!(updated[0].response_status, Some(404));
    }

    // ========== mixed success/failure ==========

    #[tokio::test]
    async fn test_process_pending_webhooks_continues_after_failure() {
        // First event fails, second succeeds - both should be processed
        let events = vec![make_test_event(0), make_test_event(0)];
        let repo = Arc::new(MockWebhookRepo::new(events));
        // Service that fails for first call, succeeds for second
        struct ToggleService {
            count: AtomicU32,
        }
        #[async_trait]
        impl WebhookService for ToggleService {
            async fn send_webhook(&self, _event: &WebhookEvent) -> Result<()> {
                let n = self.count.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    Err(anyhow!("timeout connecting to server"))
                } else {
                    Ok(())
                }
            }
            async fn trigger_completion(&self, _task: &Task) -> Result<()> {
                Ok(())
            }
            async fn trigger_failure(&self, _task: &Task, _error_msg: String) -> Result<()> {
                Ok(())
            }
        }
        let service: Arc<dyn WebhookService> = Arc::new(ToggleService {
            count: AtomicU32::new(0),
        });
        let worker = make_worker(repo.clone(), service);
        let result = worker.process_pending_webhooks().await;
        assert!(result.is_ok());
        let updated = repo.updated_events();
        assert_eq!(updated.len(), 2);
        // First event should be Failed (retryable), second should be Delivered
        assert_eq!(updated[0].status, WebhookStatus::Failed);
        assert_eq!(updated[1].status, WebhookStatus::Delivered);
    }

    // ========== retry policy behavior ==========

    #[tokio::test]
    async fn test_retry_policy_fast_changes_retry_behavior() {
        let event = make_test_event(0);
        let repo = Arc::new(MockWebhookRepo::new(vec![event]));
        let service = Arc::new(MockWebhookService::new_failure());
        let worker = WebhookWorker::new(repo.clone(), service, RetryPolicy::fast());
        worker.process_pending_webhooks().await.unwrap();
        let updated = repo.updated_events();
        // With fast policy, attempt 1 < max_retries(5), so should retry
        assert_eq!(updated[0].status, WebhookStatus::Failed);
        assert!(updated[0].next_retry_at.is_some());
    }

    #[test]
    fn test_retry_policy_default_max_retries() {
        let policy = RetryPolicy::default();
        assert_eq!(policy.max_retries, 5);
    }
}
