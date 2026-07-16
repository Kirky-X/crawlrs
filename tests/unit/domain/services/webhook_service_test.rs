// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Webhook service external unit tests
//!
//! Supplements the embedded tests in `src/domain/services/webhook_service.rs` by
//! exercising the `WebhookManagementServiceImpl` (register/trigger/retry/list)
//! through the public trait interface, and verifying signature verification +
//! timestamp validation edge cases via the public API.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::Utc;
use serde_json::{json, Value};
use uuid::Uuid;

use crawlrs::domain::models::{Task, TaskStatus, TaskType, Webhook, WebhookEvent, WebhookEventType};
use crawlrs::domain::repositories::task_repository::RepositoryError;
use crawlrs::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crawlrs::domain::repositories::webhook_repository::WebhookRepository;
use crawlrs::domain::services::webhook_sender::WebhookSender;
use crawlrs::domain::services::webhook_service::{
    verify_webhook_signature, WebhookManagementService, WebhookManagementServiceImpl, WebhookService,
    WebhookServiceImpl,
};

// =============================================================================
// Mock Webhook Sender
// =============================================================================

struct MockWebhookSender {
    sent_count: AtomicU32,
    should_fail: bool,
    captured_payload: Mutex<Option<Value>>,
}

impl MockWebhookSender {
    fn new() -> Self {
        Self {
            sent_count: AtomicU32::new(0),
            should_fail: false,
            captured_payload: Mutex::new(None),
        }
    }

    fn failing() -> Self {
        Self {
            sent_count: AtomicU32::new(0),
            should_fail: true,
            captured_payload: Mutex::new(None),
        }
    }

    fn sent_count(&self) -> u32 {
        self.sent_count.load(Ordering::SeqCst)
    }

    fn captured_payload(&self) -> Option<Value> {
        self.captured_payload.lock().expect("payload lock").clone()
    }
}

impl Default for MockWebhookSender {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WebhookSender for MockWebhookSender {
    async fn send(
        &self,
        _url: &str,
        payload: &Value,
        _headers: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<()> {
        if self.should_fail {
            return Err(anyhow::anyhow!("send failed"));
        }
        self.sent_count.fetch_add(1, Ordering::SeqCst);
        *self.captured_payload.lock().expect("payload lock") = Some(payload.clone());
        Ok(())
    }

    async fn send_with_status(
        &self,
        _url: &str,
        _payload: &Value,
        _headers: Option<&HashMap<String, String>>,
    ) -> anyhow::Result<u16> {
        if self.should_fail {
            return Err(anyhow::anyhow!("send_with_status failed"));
        }
        self.sent_count.fetch_add(1, Ordering::SeqCst);
        Ok(200)
    }
}

// =============================================================================
// Mock Webhook Event Repository
// =============================================================================

struct MockWebhookEventRepository {
    created: Mutex<Vec<WebhookEvent>>,
    pending_events: Mutex<Vec<WebhookEvent>>,
    updated: Mutex<Vec<WebhookEvent>>,
    should_fail_create: bool,
}

impl MockWebhookEventRepository {
    fn new() -> Self {
        Self {
            created: Mutex::new(Vec::new()),
            pending_events: Mutex::new(Vec::new()),
            updated: Mutex::new(Vec::new()),
            should_fail_create: false,
        }
    }

    fn failing_create() -> Self {
        Self {
            created: Mutex::new(Vec::new()),
            pending_events: Mutex::new(Vec::new()),
            updated: Mutex::new(Vec::new()),
            should_fail_create: true,
        }
    }

    fn with_pending(events: Vec<WebhookEvent>) -> Self {
        Self {
            created: Mutex::new(Vec::new()),
            pending_events: Mutex::new(events),
            updated: Mutex::new(Vec::new()),
            should_fail_create: false,
        }
    }

    fn created_count(&self) -> usize {
        self.created.lock().expect("created lock").len()
    }

    fn updated_count(&self) -> usize {
        self.updated.lock().expect("updated lock").len()
    }
}

impl Default for MockWebhookEventRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WebhookEventRepository for MockWebhookEventRepository {
    async fn create(&self, event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError> {
        if self.should_fail_create {
            return Err(RepositoryError::Database(anyhow::anyhow!("repo down")));
        }
        self.created.lock().expect("created lock").push(event.clone());
        Ok(event.clone())
    }

    async fn find_by_id(&self, _id: Uuid) -> Result<Option<WebhookEvent>, RepositoryError> {
        Ok(None)
    }

    async fn find_pending(&self, limit: u64) -> Result<Vec<WebhookEvent>, RepositoryError> {
        let events = self.pending_events.lock().expect("pending lock").clone();
        Ok(events.into_iter().take(limit as usize).collect())
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
        self.updated.lock().expect("updated lock").push(event.clone());
        Ok(event.clone())
    }
}

// =============================================================================
// Mock Webhook Repository
// =============================================================================

struct MockWebhookRepository {
    webhooks: Mutex<Vec<Webhook>>,
    should_fail: bool,
}

impl MockWebhookRepository {
    fn new() -> Self {
        Self {
            webhooks: Mutex::new(Vec::new()),
            should_fail: false,
        }
    }

    fn failing() -> Self {
        Self {
            webhooks: Mutex::new(Vec::new()),
            should_fail: true,
        }
    }

    fn with_webhooks(webhooks: Vec<Webhook>) -> Self {
        Self {
            webhooks: Mutex::new(webhooks),
            should_fail: false,
        }
    }
}

impl Default for MockWebhookRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl WebhookRepository for MockWebhookRepository {
    async fn create(&self, webhook: &Webhook) -> Result<Webhook, RepositoryError> {
        if self.should_fail {
            return Err(RepositoryError::Database(anyhow::anyhow!("repo down")));
        }
        self.webhooks.lock().expect("lock").push(webhook.clone());
        Ok(webhook.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Webhook>, RepositoryError> {
        if self.should_fail {
            return Err(RepositoryError::Database(anyhow::anyhow!("repo down")));
        }
        let webhooks = self.webhooks.lock().expect("lock");
        Ok(webhooks.iter().find(|w| w.id == id).cloned())
    }

    async fn find_by_team_id(&self, team_id: Uuid) -> Result<Vec<Webhook>, RepositoryError> {
        if self.should_fail {
            return Err(RepositoryError::Database(anyhow::anyhow!("repo down")));
        }
        let webhooks = self.webhooks.lock().expect("lock");
        Ok(webhooks.iter().filter(|w| w.team_id == team_id).cloned().collect())
    }
}

// =============================================================================
// Helpers
// =============================================================================

fn create_test_task() -> Task {
    let now = Utc::now();
    Task {
        id: Uuid::new_v4(),
        team_id: Uuid::new_v4(),
        api_key_id: Uuid::new_v4(),
        url: "http://example.com".to_string(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Completed,
        payload: json!({
            "url": "http://example.com",
            "webhook": "https://example.com/webhook"
        }),
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

fn create_test_event() -> WebhookEvent {
    WebhookEvent::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::nil(),
        WebhookEventType::ScrapeCompleted,
        json!({"task_id": "abc"}),
        "https://example.com/webhook".to_string(),
    )
}

fn make_webhook_service(
    sender: Arc<dyn WebhookSender>,
    repo: Arc<dyn WebhookEventRepository>,
    secret: &str,
) -> WebhookServiceImpl {
    WebhookServiceImpl::new(sender, secret.to_string(), repo)
}

fn make_management_service(
    webhook_repo: Arc<dyn WebhookRepository>,
    event_repo: Arc<dyn WebhookEventRepository>,
    webhook_service: Arc<dyn WebhookService>,
) -> WebhookManagementServiceImpl {
    WebhookManagementServiceImpl::new(webhook_repo, event_repo, webhook_service)
}

// =============================================================================
// WebhookServiceImpl tests via public trait
// =============================================================================

#[tokio::test]
async fn test_send_webhook_success_via_trait() {
    let sender = Arc::new(MockWebhookSender::new());
    let repo: Arc<dyn WebhookEventRepository> = Arc::new(MockWebhookEventRepository::new());
    let service: Arc<dyn WebhookService> =
        Arc::new(make_webhook_service(sender.clone(), repo, "secret"));

    let event = create_test_event();
    let result = service.send_webhook(&event).await;
    assert!(result.is_ok());
    assert_eq!(sender.sent_count(), 1);
}

#[tokio::test]
async fn test_send_webhook_failure_via_trait() {
    let sender: Arc<dyn WebhookSender> = Arc::new(MockWebhookSender::failing());
    let repo: Arc<dyn WebhookEventRepository> = Arc::new(MockWebhookEventRepository::new());
    let service: Arc<dyn WebhookService> =
        Arc::new(make_webhook_service(sender, repo, "secret"));

    let event = create_test_event();
    let result = service.send_webhook(&event).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_trigger_completion_success_via_trait() {
    let sender = Arc::new(MockWebhookSender::new());
    let repo = Arc::new(MockWebhookEventRepository::new());
    let service: Arc<dyn WebhookService> =
        Arc::new(make_webhook_service(sender.clone(), repo, "secret"));

    let task = create_test_task();
    let result = service.trigger_completion(&task).await;
    assert!(result.is_ok());
    assert_eq!(sender.sent_count(), 1);
}

#[tokio::test]
async fn test_trigger_failure_success_via_trait() {
    let sender = Arc::new(MockWebhookSender::new());
    let repo = Arc::new(MockWebhookEventRepository::new());
    let service: Arc<dyn WebhookService> =
        Arc::new(make_webhook_service(sender.clone(), repo, "secret"));

    let task = create_test_task();
    let result = service.trigger_failure(&task, "error msg".to_string()).await;
    assert!(result.is_ok());
    assert_eq!(sender.sent_count(), 1);
}

#[tokio::test]
async fn test_trigger_completion_no_webhook_returns_ok() {
    let sender = Arc::new(MockWebhookSender::new());
    let repo: Arc<dyn WebhookEventRepository> = Arc::new(MockWebhookEventRepository::new());
    let service: Arc<dyn WebhookService> =
        Arc::new(make_webhook_service(sender.clone(), repo, "secret"));

    let mut task = create_test_task();
    task.payload = json!({"url": "http://example.com"}); // no webhook

    let result = service.trigger_completion(&task).await;
    assert!(result.is_ok());
    assert_eq!(sender.sent_count(), 0);
}

#[tokio::test]
async fn test_trigger_failure_includes_error_in_payload() {
    let sender = Arc::new(MockWebhookSender::new());
    let repo: Arc<dyn WebhookEventRepository> = Arc::new(MockWebhookEventRepository::new());
    let service: Arc<dyn WebhookService> =
        Arc::new(make_webhook_service(sender.clone(), repo, "secret"));

    let task = create_test_task();
    service
        .trigger_failure(&task, "scrape blew up".to_string())
        .await
        .expect("trigger should succeed");

    let payload = sender.captured_payload().expect("payload captured");
    assert_eq!(payload["status"], json!("failed"));
    assert_eq!(payload["error"], json!("scrape blew up"));
    assert_eq!(payload["task_id"], json!(task.id));
}

#[tokio::test]
async fn test_trigger_completion_payload_has_completed_status() {
    let sender = Arc::new(MockWebhookSender::new());
    let repo: Arc<dyn WebhookEventRepository> = Arc::new(MockWebhookEventRepository::new());
    let service: Arc<dyn WebhookService> =
        Arc::new(make_webhook_service(sender.clone(), repo, "secret"));

    let task = create_test_task();
    service.trigger_completion(&task).await.expect("trigger ok");

    let payload = sender.captured_payload().expect("payload captured");
    assert_eq!(payload["status"], json!("completed"));
    assert!(payload.get("error").is_none() || payload["error"].is_null());
}

// =============================================================================
// WebhookManagementServiceImpl tests (covers previously uncovered lines)
// =============================================================================

#[tokio::test]
async fn test_register_webhook_success() {
    let webhook_repo = Arc::new(MockWebhookRepository::new());
    let event_repo: Arc<dyn WebhookEventRepository> = Arc::new(MockWebhookEventRepository::new());
    let sender: Arc<dyn WebhookSender> = Arc::new(MockWebhookSender::new());
    let webhook_service: Arc<dyn WebhookService> =
        Arc::new(make_webhook_service(sender, event_repo, "secret"));

    let mgmt = make_management_service(
        webhook_repo.clone(),
        event_repo,
        webhook_service,
    );

    let team_id = Uuid::new_v4();
    let result = mgmt
        .register_webhook(team_id, "https://example.com/hook".to_string())
        .await;

    assert!(result.is_ok());
    let webhook = result.unwrap();
    assert_eq!(webhook.team_id, team_id);
    assert_eq!(webhook.url, "https://example.com/hook");
}

#[tokio::test]
async fn test_register_webhook_invalid_url_returns_error() {
    let webhook_repo = Arc::new(MockWebhookRepository::new());
    let event_repo: Arc<dyn WebhookEventRepository> = Arc::new(MockWebhookEventRepository::new());
    let sender: Arc<dyn WebhookSender> = Arc::new(MockWebhookSender::new());
    let webhook_service: Arc<dyn WebhookService> =
        Arc::new(make_webhook_service(sender, event_repo, "secret"));

    let mgmt = make_management_service(webhook_repo, event_repo, webhook_service);

    let result = mgmt
        .register_webhook(Uuid::new_v4(), "ftp://bad-scheme.com".to_string())
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Invalid webhook URL"), "got: {}", err);
}

#[tokio::test]
async fn test_register_webhook_empty_url_returns_error() {
    let webhook_repo = Arc::new(MockWebhookRepository::new());
    let event_repo: Arc<dyn WebhookEventRepository> = Arc::new(MockWebhookEventRepository::new());
    let sender: Arc<dyn WebhookSender> = Arc::new(MockWebhookSender::new());
    let webhook_service: Arc<dyn WebhookService> =
        Arc::new(make_webhook_service(sender, event_repo, "secret"));

    let mgmt = make_management_service(webhook_repo, event_repo, webhook_service);

    let result = mgmt.register_webhook(Uuid::new_v4(), "".to_string()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_register_webhook_repo_failure_propagates() {
    let webhook_repo = Arc::new(MockWebhookRepository::failing());
    let event_repo: Arc<dyn WebhookEventRepository> = Arc::new(MockWebhookEventRepository::new());
    let sender: Arc<dyn WebhookSender> = Arc::new(MockWebhookSender::new());
    let webhook_service: Arc<dyn WebhookService> =
        Arc::new(make_webhook_service(sender, event_repo, "secret"));

    let mgmt = make_management_service(webhook_repo, event_repo, webhook_service);

    let result = mgmt
        .register_webhook(Uuid::new_v4(), "https://example.com/hook".to_string())
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Failed to create webhook"), "got: {}", err);
}

#[tokio::test]
async fn test_trigger_webhook_success() {
    let team_id = Uuid::new_v4();
    let webhook = Webhook::new(
        Uuid::new_v4(),
        team_id,
        "https://example.com/hook".to_string(),
    );
    let webhook_id = webhook.id;

    let webhook_repo = Arc::new(MockWebhookRepository::with_webhooks(vec![webhook]));
    let event_repo = Arc::new(MockWebhookEventRepository::new());
    let sender = Arc::new(MockWebhookSender::new());
    let webhook_service: Arc<dyn WebhookService> =
        Arc::new(make_webhook_service(sender.clone(), event_repo.clone(), "secret"));

    let mgmt = make_management_service(
        webhook_repo,
        event_repo.clone(),
        webhook_service,
    );

    let result = mgmt
        .trigger_webhook(
            webhook_id,
            WebhookEventType::ScrapeCompleted,
            json!({"key": "value"}),
        )
        .await;

    assert!(result.is_ok());
    assert_eq!(sender.sent_count(), 1);
    assert_eq!(event_repo.created_count(), 1);
}

#[tokio::test]
async fn test_trigger_webhook_not_found_returns_error() {
    let webhook_repo = Arc::new(MockWebhookRepository::new());
    let event_repo: Arc<dyn WebhookEventRepository> = Arc::new(MockWebhookEventRepository::new());
    let sender: Arc<dyn WebhookSender> = Arc::new(MockWebhookSender::new());
    let webhook_service: Arc<dyn WebhookService> =
        Arc::new(make_webhook_service(sender, event_repo.clone(), "secret"));

    let mgmt = make_management_service(webhook_repo, event_repo, webhook_service);

    let result = mgmt
        .trigger_webhook(
            Uuid::new_v4(),
            WebhookEventType::ScrapeCompleted,
            json!({}),
        )
        .await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Webhook not found"), "got: {}", err);
}

#[tokio::test]
async fn test_trigger_webhook_sender_failure_propagates() {
    let team_id = Uuid::new_v4();
    let webhook = Webhook::new(
        Uuid::new_v4(),
        team_id,
        "https://example.com/hook".to_string(),
    );
    let webhook_id = webhook.id;

    let webhook_repo = Arc::new(MockWebhookRepository::with_webhooks(vec![webhook]));
    let event_repo = Arc::new(MockWebhookEventRepository::new());
    let sender: Arc<dyn WebhookSender> = Arc::new(MockWebhookSender::failing());
    let webhook_service: Arc<dyn WebhookService> =
        Arc::new(make_webhook_service(sender, event_repo.clone(), "secret"));

    let mgmt = make_management_service(webhook_repo, event_repo, webhook_service);

    let result = mgmt
        .trigger_webhook(webhook_id, WebhookEventType::ScrapeCompleted, json!({}))
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_retry_failed_no_pending_returns_zero() {
    let webhook_repo = Arc::new(MockWebhookRepository::new());
    let event_repo: Arc<dyn WebhookEventRepository> = Arc::new(MockWebhookEventRepository::new());
    let sender: Arc<dyn WebhookSender> = Arc::new(MockWebhookSender::new());
    let webhook_service: Arc<dyn WebhookService> =
        Arc::new(make_webhook_service(sender, event_repo.clone(), "secret"));

    let mgmt = make_management_service(webhook_repo, event_repo, webhook_service);

    let count = mgmt.retry_failed(10).await.expect("should succeed");
    assert_eq!(count, 0);
}

#[tokio::test]
async fn test_retry_failed_with_pending_events() {
    let mut event = create_test_event();
    event.status = crawlrs::domain::models::WebhookStatus::Pending;
    event.attempt_count = 0;
    event.max_retries = 5;

    let webhook_repo = Arc::new(MockWebhookRepository::new());
    let event_repo: Arc<dyn WebhookEventRepository> =
        Arc::new(MockWebhookEventRepository::with_pending(vec![event]));
    let sender = Arc::new(MockWebhookSender::new());
    let webhook_service: Arc<dyn WebhookService> =
        Arc::new(make_webhook_service(sender.clone(), event_repo.clone(), "secret"));

    let mgmt = make_management_service(webhook_repo, event_repo.clone(), webhook_service);

    let count = mgmt.retry_failed(10).await.expect("should succeed");
    assert_eq!(count, 1);
    assert_eq!(sender.sent_count(), 1);
}

#[tokio::test]
async fn test_list_webhooks_returns_team_webhooks() {
    let team_id = Uuid::new_v4();
    let webhook1 = Webhook::new(Uuid::new_v4(), team_id, "https://example.com/1".to_string());
    let webhook2 = Webhook::new(Uuid::new_v4(), team_id, "https://example.com/2".to_string());
    let other_team_webhook =
        Webhook::new(Uuid::new_v4(), Uuid::new_v4(), "https://example.com/3".to_string());

    let webhook_repo = Arc::new(MockWebhookRepository::with_webhooks(vec![
        webhook1,
        webhook2,
        other_team_webhook,
    ]));
    let event_repo: Arc<dyn WebhookEventRepository> = Arc::new(MockWebhookEventRepository::new());
    let sender: Arc<dyn WebhookSender> = Arc::new(MockWebhookSender::new());
    let webhook_service: Arc<dyn WebhookService> =
        Arc::new(make_webhook_service(sender, event_repo.clone(), "secret"));

    let mgmt = make_management_service(webhook_repo, event_repo, webhook_service);

    let webhooks = mgmt
        .list_webhooks(team_id)
        .await
        .expect("list_webhooks should succeed");

    assert_eq!(webhooks.len(), 2);
    assert!(webhooks.iter().all(|w| w.team_id == team_id));
}

#[tokio::test]
async fn test_list_webhooks_repo_failure_propagates() {
    let webhook_repo = Arc::new(MockWebhookRepository::failing());
    let event_repo: Arc<dyn WebhookEventRepository> = Arc::new(MockWebhookEventRepository::new());
    let sender: Arc<dyn WebhookSender> = Arc::new(MockWebhookSender::new());
    let webhook_service: Arc<dyn WebhookService> =
        Arc::new(make_webhook_service(sender, event_repo, "secret"));

    let mgmt = make_management_service(webhook_repo, event_repo, webhook_service);

    let result = mgmt.list_webhooks(Uuid::new_v4()).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_list_webhooks_empty_team_returns_empty() {
    let webhook_repo = Arc::new(MockWebhookRepository::new());
    let event_repo: Arc<dyn WebhookEventRepository> = Arc::new(MockWebhookEventRepository::new());
    let sender: Arc<dyn WebhookSender> = Arc::new(MockWebhookSender::new());
    let webhook_service: Arc<dyn WebhookService> =
        Arc::new(make_webhook_service(sender, event_repo, "secret"));

    let mgmt = make_management_service(webhook_repo, event_repo, webhook_service);

    let webhooks = mgmt
        .list_webhooks(Uuid::new_v4())
        .await
        .expect("should succeed");
    assert!(webhooks.is_empty());
}

// =============================================================================
// verify_webhook_signature tests via public API
// =============================================================================

fn generate_test_signature(secret: &str, payload: &str, timestamp: i64) -> String {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let message = format!("{}.{}", timestamp, payload);
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC key");
    mac.update(message.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

#[test]
fn test_verify_signature_valid_current_timestamp() {
    let secret = "mysecret";
    let payload = r#"{"task_id":"abc"}"#;
    let timestamp = Utc::now().timestamp();
    let signature = generate_test_signature(secret, payload, timestamp);

    assert!(verify_webhook_signature(secret, payload, timestamp, &signature));
}

#[test]
fn test_verify_signature_invalid_signature_returns_false() {
    let secret = "mysecret";
    let payload = r#"{"task_id":"abc"}"#;
    let timestamp = Utc::now().timestamp();

    assert!(!verify_webhook_signature(secret, payload, timestamp, "deadbeef"));
}

#[test]
fn test_verify_signature_wrong_secret_returns_false() {
    let payload = r#"{"task_id":"abc"}"#;
    let timestamp = Utc::now().timestamp();
    let signature = generate_test_signature("real-secret", payload, timestamp);

    assert!(!verify_webhook_signature("wrong-secret", payload, timestamp, &signature));
}

#[test]
fn test_verify_signature_wrong_payload_returns_false() {
    let secret = "mysecret";
    let timestamp = Utc::now().timestamp();
    let signature = generate_test_signature(secret, r#"{"a":1}"#, timestamp);

    assert!(!verify_webhook_signature(secret, r#"{"a":2}"#, timestamp, &signature));
}

#[test]
fn test_verify_signature_expired_timestamp_returns_false() {
    let secret = "mysecret";
    let payload = r#"{"task_id":"abc"}"#;
    // 10 minutes ago — outside the 5-minute window
    let timestamp = Utc::now().timestamp() - 600;
    let signature = generate_test_signature(secret, payload, timestamp);

    assert!(!verify_webhook_signature(secret, payload, timestamp, &signature));
}

#[test]
fn test_verify_signature_future_timestamp_outside_window_returns_false() {
    let secret = "mysecret";
    let payload = r#"{"task_id":"abc"}"#;
    // 10 minutes in the future — outside the 5-minute window
    let timestamp = Utc::now().timestamp() + 600;
    let signature = generate_test_signature(secret, payload, timestamp);

    assert!(!verify_webhook_signature(secret, payload, timestamp, &signature));
}

#[test]
fn test_verify_signature_empty_payload_valid() {
    let secret = "mysecret";
    let payload = "";
    let timestamp = Utc::now().timestamp();
    let signature = generate_test_signature(secret, payload, timestamp);

    assert!(verify_webhook_signature(secret, payload, timestamp, &signature));
}

#[test]
fn test_verify_signature_different_length_signature_returns_false() {
    let secret = "mysecret";
    let payload = r#"{"test":"data"}"#;
    let timestamp = Utc::now().timestamp();
    // Short signature — different length
    assert!(!verify_webhook_signature(secret, payload, timestamp, "short"));
}

// =============================================================================
// Webhook model tests
// =============================================================================

#[test]
fn test_webhook_new_sets_fields() {
    let id = Uuid::new_v4();
    let team_id = Uuid::new_v4();
    let url = "https://example.com/hook".to_string();

    let webhook = Webhook::new(id, team_id, url.clone());

    assert_eq!(webhook.id, id);
    assert_eq!(webhook.team_id, team_id);
    assert_eq!(webhook.url, url);
    assert!(webhook.created_at <= Utc::now());
}

#[test]
fn test_webhook_validate_url_valid_https() {
    let webhook = Webhook::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "https://example.com/hook".to_string(),
    );
    assert!(webhook.validate_url().is_ok());
}

#[test]
fn test_webhook_validate_url_valid_http() {
    let webhook = Webhook::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "http://example.com/hook".to_string(),
    );
    assert!(webhook.validate_url().is_ok());
}

#[test]
fn test_webhook_validate_url_empty_returns_error() {
    let webhook = Webhook::new(Uuid::new_v4(), Uuid::new_v4(), "".to_string());
    assert!(webhook.validate_url().is_err());
}

#[test]
fn test_webhook_validate_url_invalid_scheme_returns_error() {
    let webhook = Webhook::new(
        Uuid::new_v4(),
        Uuid::new_v4(),
        "ftp://example.com/hook".to_string(),
    );
    assert!(webhook.validate_url().is_err());
}
