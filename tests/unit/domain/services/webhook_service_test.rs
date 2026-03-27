// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Webhook service tests
//!
//! Tests for the WebhookService including webhook delivery and signature verification

use std::sync::Arc;
use uuid::Uuid;

use crawlrs::domain::models::webhook_model::{Webhook, WebhookEvent};
use crawlrs::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crawlrs::domain::services::webhook_service::{verify_webhook_signature, WebhookService};

// === Mock Webhook Sender ===

struct MockWebhookSender {
    should_fail: bool,
    events_sent: Arc<std::sync::Mutex<Vec<WebhookEvent>>>,
}

impl MockWebhookSender {
    fn new() -> Self {
        Self {
            should_fail: false,
            events_sent: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    fn failing() -> Self {
        Self {
            should_fail: true,
            events_sent: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    fn get_sent_count(&self) -> usize {
        let events = self.events_sent.lock().unwrap();
        events.len()
    }
}

#[async_trait::async_trait]
impl crawlrs::domain::services::webhook_sender::WebhookSender for MockWebhookSender {
    async fn send(&self, event: &WebhookEvent) -> Result<(), anyhow::Error> {
        if self.should_fail {
            return Err(anyhow::anyhow!("Webhook send failed"));
        }

        let mut events = self.events_sent.lock().unwrap();
        events.push(event.clone());
        Ok(())
    }
}

// === Mock Webhook Event Repository ===

struct MockWebhookEventRepository {
    events: Arc<std::sync::Mutex<Vec<WebhookEvent>>>,
    should_fail: Arc<std::sync::atomic::AtomicBool>,
}

impl MockWebhookEventRepository {
    fn new() -> Self {
        Self {
            events: Arc::new(std::sync::Mutex::new(Vec::new())),
            should_fail: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }
}

#[async_trait::async_trait]
impl WebhookEventRepository for MockWebhookEventRepository {
    async fn create(&self, event: &WebhookEvent) -> Result<WebhookEvent, anyhow::Error> {
        if self.should_fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(anyhow::anyhow!("Database error"));
        }

        let mut events = self.events.lock().unwrap();
        events.push(event.clone());
        Ok(event.clone())
    }

    async fn find_by_id(&self, _id: Uuid) -> Result<Option<WebhookEvent>, anyhow::Error> {
        Ok(None)
    }

    async fn find_by_webhook_id(&self, _webhook_id: Uuid) -> Result<Vec<WebhookEvent>, anyhow::Error> {
        Ok(vec![])
    }

    async fn mark_delivered(&self, _event_id: Uuid) -> Result<(), anyhow::Error> {
        Ok(())
    }

    async fn mark_failed(&self, _event_id: Uuid, _error: String) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

// === Helper Functions ===

fn create_test_service() -> (WebhookService, Arc<MockWebhookSender>) {
    let sender = Arc::new(MockWebhookSender::new());
    let secret = "test_secret".to_string();
    let repo = Arc::new(MockWebhookEventRepository::new());

    let service = WebhookService::new(sender.clone(), secret, repo);
    (service, sender)
}

fn create_test_task() -> crate::domain::models::task::Task {
    crate::domain::models::task::Task {
        id: Uuid::new_v4(),
        task_type: crate::domain::models::task::TaskType::Scrape,
        status: crate::domain::models::task::TaskStatus::Completed,
        priority: 0,
        team_id: Uuid::new_v4(),
        api_key_id: Uuid::new_v4(),
        url: "https://example.com".to_string(),
        payload: serde_json::json!({}),
        retry_count: 0,
        attempt_count: 1,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: chrono::Utc::now(),
        started_at: None,
        completed_at: Some(chrono::Utc::now()),
        crawl_id: None,
        updated_at: chrono::Utc::now(),
        lock_token: None,
        lock_expires_at: None,
    }
}

// === Unit Tests ===

#[tokio::test]
async fn test_send_webhook_success() {
    let (service, sender) = create_test_service();

    let event = WebhookEvent {
        id: Uuid::new_v4(),
        webhook_id: Uuid::new_v4(),
        event_type: crawlrs::domain::models::webhook_model::WebhookEventType::TaskCompleted,
        payload: serde_json::json!({"task_id": "test"}),
        delivered: false,
        error_message: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let result = service.send_webhook(&event).await;

    assert!(result.is_ok());
    assert_eq!(sender.get_sent_count(), 1);
}

#[tokio::test]
async fn test_send_webhook_failure() {
    let sender = Arc::new(MockWebhookSender::failing());
    let secret = "test_secret".to_string();
    let repo = Arc::new(MockWebhookEventRepository::new());
    let service = WebhookService::new(sender, secret, repo);

    let event = WebhookEvent {
        id: Uuid::new_v4(),
        webhook_id: Uuid::new_v4(),
        event_type: crawlrs::domain::models::webhook_model::WebhookEventType::TaskCompleted,
        payload: serde_json::json!({}),
        delivered: false,
        error_message: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let result = service.send_webhook(&event).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_trigger_completion() {
    let (service, sender) = create_test_service();
    let task = create_test_task();

    let result = service.trigger_completion(&task).await;

    assert!(result.is_ok());
    assert_eq!(sender.get_sent_count(), 1);
}

#[tokio::test]
async fn test_trigger_failure() {
    let (service, sender) = create_test_service();
    let task = create_test_task();

    let error_msg = "Task failed".to_string();
    let result = service.trigger_failure(&task, error_msg.clone()).await;

    assert!(result.is_ok());
    assert_eq!(sender.get_sent_count(), 1);
}

// === Signature Verification Tests ===

#[test]
fn test_verify_webhook_signature_valid() {
    let secret = "test_secret";
    let payload = r#"{"test": "data"}"#;
    let timestamp = 1234567890i64;
    
    // Create HMAC-SHA256 signature
    use hmac::{Hmac, Mac, NewHmac};
    use sha2::Sha256;
    
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes());
    mac.update(payload.as_bytes());
    mac.update(timestamp.to_string().as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    let result = verify_webhook_signature(secret, payload, timestamp, &signature);

    assert!(result);
}

#[test]
fn test_verify_webhook_signature_invalid() {
    let secret = "test_secret";
    let payload = r#"{"test": "data"}"#;
    let timestamp = 1234567890i64;
    let signature = "invalid_signature";

    let result = verify_webhook_signature(secret, payload, timestamp, signature);

    assert!(!result);
}

#[test]
fn test_verify_webhook_signature_different_payload() {
    let secret = "test_secret";
    let payload1 = r#"{"test": "data1"}"#;
    let payload2 = r#"{"test": "data2"}"#;
    let timestamp = 1234567890i64;
    
    use hmac::{Hmac, Mac, NewHmac};
    use sha2::Sha256;
    
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes());
    mac.update(payload1.as_bytes());
    mac.update(timestamp.to_string().as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    let result = verify_webhook_signature(secret, payload2, timestamp, &signature);

    assert!(!result);
}

#[test]
fn test_verify_webhook_signature_wrong_secret() {
    let secret1 = "secret1";
    let secret2 = "secret2";
    let payload = r#"{"test": "data"}"#;
    let timestamp = 1234567890i64;
    
    use hmac::{Hmac, Mac, NewHmac};
    use sha2::Sha256;
    
    let mut mac = Hmac::<Sha256>::new_from_slice(secret1.as_bytes());
    mac.update(payload.as_bytes());
    mac.update(timestamp.to_string().as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    let result = verify_webhook_signature(secret2, payload, timestamp, &signature);

    assert!(!result);
}

// === Edge Cases ===

#[tokio::test]
async fn test_send_webhook_with_empty_payload() {
    let (service, sender) = create_test_service();

    let event = WebhookEvent {
        id: Uuid::new_v4(),
        webhook_id: Uuid::new_v4(),
        event_type: crawlrs::domain::models::webhook_model::WebhookEventType::TaskCompleted,
        payload: serde_json::json!({}),
        delivered: false,
        error_message: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let result = service.send_webhook(&event).await;

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_send_webhook_with_large_payload() {
    let (service, sender) = create_test_service();

    let large_data = "x".repeat(10000);
    let event = WebhookEvent {
        id: Uuid::new_v4(),
        webhook_id: Uuid::new_v4(),
        event_type: crawlrs::domain::models::webhook_model::WebhookEventType::TaskCompleted,
        payload: serde_json::json!({"data": large_data}),
        delivered: false,
        error_message: None,
        created_at: chrono::Utc::now(),
        updated_at: chrono::Utc::now(),
    };

    let result = service.send_webhook(&event).await;

    assert!(result.is_ok());
}

#[test]
fn test_verify_signature_with_empty_payload() {
    let secret = "test_secret";
    let payload = "";
    let timestamp = 1234567890i64;
    
    use hmac::{Hmac, Mac, NewHmac};
    use sha2::Sha256;
    
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes());
    mac.update(payload.as_bytes());
    mac.update(timestamp.to_string().as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    let result = verify_webhook_signature(secret, payload, timestamp, &signature);

    assert!(result);
}

#[test]
fn test_verify_signature_with_zero_timestamp() {
    let secret = "test_secret";
    let payload = r#"{"test": "data"}"#;
    let timestamp = 0i64;
    
    use hmac::{Hmac, Mac, NewHmac};
    use sha2::Sha256;
    
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes());
    mac.update(payload.as_bytes());
    mac.update(timestamp.to_string().as_bytes());
    let signature = hex::encode(mac.finalize().into_bytes());

    let result = verify_webhook_signature(secret, payload, timestamp, &signature);

    assert!(result);
}

// === Multiple Event Types Tests ===

#[tokio::test]
async fn test_trigger_completion_event() {
    let (service, sender) = create_test_service();
    let mut task = create_test_task();
    task.status = crate::domain::models::task::TaskStatus::Completed;

    let result = service.trigger_completion(&task).await;

    assert!(result.is_ok());
    assert_eq!(sender.get_sent_count(), 1);
}

#[tokio::test]
async fn test_trigger_failure_event() {
    let (service, sender) = create_test_service();
    let mut task = create_test_task();
    task.status = crate::domain::models::task::TaskStatus::Failed;

    let result = service.trigger_failure(&task, "Test error".to_string()).await;

    assert!(result.is_ok());
    assert_eq!(sender.get_sent_count(), 1);
}

#[tokio::test]
async fn test_send_multiple_webhooks_concurrently() {
    let (service, sender) = create_test_service();

    let events: Vec<_> = (0..5)
        .map(|_| WebhookEvent {
            id: Uuid::new_v4(),
            webhook_id: Uuid::new_v4(),
            event_type: crawlrs::domain::models::webhook_model::WebhookEventType::TaskCompleted,
            payload: serde_json::json!({}),
            delivered: false,
            error_message: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        })
        .collect();

    let mut handles = vec![];
    for event in events {
        let service_clone = service.clone();
        let handle = tokio::spawn(async move {
            service_clone.send_webhook(&event).await
        });
        handles.push(handle);
    }

    for handle in handles {
        assert!(handle.await.is_ok());
    }

    assert_eq!(sender.get_sent_count(), 5);
}
