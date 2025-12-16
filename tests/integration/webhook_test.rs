use super::helpers::create_test_app_no_worker;
use chrono::Utc;
use crawlrs::domain::models::webhook::{WebhookEvent, WebhookEventType, WebhookStatus};
use crawlrs::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crawlrs::infrastructure::repositories::webhook_event_repo_impl::WebhookEventRepoImpl;
use crawlrs::workers::webhook_worker::WebhookWorker;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use std::sync::Arc;
use uuid::Uuid;
use wiremock::matchers::{header, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

#[tokio::test]
async fn test_webhook_delivery_success() {
    // 1. Setup real application infrastructure (DB, etc.)
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(WebhookEventRepoImpl::new(app.db_pool.clone()));

    // 2. Start a mock server (Receiver of the webhook)
    let mock_server = MockServer::start().await;

    // 3. Setup event
    let webhook_url = format!("{}/webhook", mock_server.uri());
    let event_id = Uuid::new_v4();
    let team_id = app.api_key.parse::<Uuid>().unwrap_or(Uuid::new_v4()); // Use generated team/key or random

    let event = WebhookEvent {
        id: event_id,
        team_id,
        webhook_id: Uuid::new_v4(),
        event_type: WebhookEventType::CrawlCompleted,
        payload: serde_json::json!({ "task_id": "123" }),
        webhook_url,
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
    };

    // Calculate signature dynamically
    let secret = "test_secret".as_bytes();
    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret).expect("HMAC can take key of any size");
    mac.update(event.payload.to_string().as_bytes());
    let signature = mac.finalize().into_bytes();
    let signature_hex = hex::encode(signature);

    // 4. Configure mock response (Receiver)
    Mock::given(method("POST"))
        .and(path("/webhook"))
        .and(header("User-Agent", "Crawlrs-Webhook/0.1.0"))
        .and(header("X-Crawlrs-Signature", signature_hex))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    // 5. Store event in REAL database
    repo.create(&event)
        .await
        .expect("Failed to create webhook event");

    // 6. Run the worker manually
    let worker = WebhookWorker::new(repo.clone(), "test_secret".to_string());

    // Process pending webhooks
    let result = worker.process_pending_webhooks().await;
    assert!(result.is_ok());

    // 7. Verify the event status was updated in REAL database
    let updated_event = repo.find_by_id(event_id).await.unwrap().unwrap();
    assert_eq!(updated_event.status, WebhookStatus::Delivered);
    assert_eq!(updated_event.response_status, Some(200));
    assert!(updated_event.delivered_at.is_some());
}

#[tokio::test]
async fn test_webhook_delivery_failure_retry() {
    // 1. Setup real application infrastructure
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(WebhookEventRepoImpl::new(app.db_pool.clone()));

    // 2. Start a mock server
    let mock_server = MockServer::start().await;

    // 3. Configure mock response to fail
    Mock::given(method("POST"))
        .and(path("/webhook"))
        .respond_with(ResponseTemplate::new(500))
        .mount(&mock_server)
        .await;

    // 4. Setup event
    let webhook_url = format!("{}/webhook", mock_server.uri());
    let event_id = Uuid::new_v4();
    let team_id = Uuid::new_v4();

    let event = WebhookEvent {
        id: event_id,
        team_id,
        webhook_id: Uuid::new_v4(),
        event_type: WebhookEventType::CrawlCompleted,
        payload: serde_json::json!({ "task_id": "123" }),
        webhook_url,
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
    };

    // 5. Store event in REAL database
    repo.create(&event)
        .await
        .expect("Failed to create webhook event");

    // 6. Run the worker
    let worker = WebhookWorker::new(repo.clone(), "test_secret".to_string());

    // Process pending webhooks
    let result = worker.process_pending_webhooks().await;
    assert!(result.is_ok());

    // 7. Verify the event status was updated
    let updated_event = repo.find_by_id(event_id).await.unwrap().unwrap();

    assert_eq!(updated_event.attempt_count, 1);
    assert_eq!(updated_event.response_status, Some(500));
    assert!(updated_event.next_retry_at.is_some());

    // Status depends on implementation logic (Failed or Pending with future date)
    // Assuming implementation sets it to Failed or Pending.
    // Let's verify it is NOT Delivered.
    assert_ne!(updated_event.status, WebhookStatus::Delivered);
}
