// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use super::helpers::create_test_app_no_worker;
use axum::{http::StatusCode, routing::post, Json, Router};
use chrono::Utc;
use crawlrs::domain::models::webhook::{WebhookEvent, WebhookEventType, WebhookStatus};
use crawlrs::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crawlrs::infrastructure::repositories::webhook_event_repo_impl::WebhookEventRepoImpl;
use crawlrs::workers::webhook_worker::WebhookWorker;
use serde_json::Value;
use std::sync::Arc;
use tokio::net::TcpListener;
use uuid::Uuid;

async fn start_test_server(success: bool) -> String {
    let app = if success {
        Router::new().route("/webhook", post(|Json(_): Json<Value>| async { "OK" }))
    } else {
        Router::new().route(
            "/webhook",
            post(|| async { StatusCode::INTERNAL_SERVER_ERROR }),
        )
    };

    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    format!("http://{}/webhook", addr)
}

#[tokio::test]
async fn test_webhook_delivery_success() {
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(WebhookEventRepoImpl::new(app.db_pool.clone()));

    let webhook_url = start_test_server(true).await;
    let event_id = Uuid::new_v4();
    let team_id = app.api_key.parse::<Uuid>().unwrap_or(Uuid::new_v4());

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

    repo.create(&event)
        .await
        .expect("Failed to create webhook event");

    let worker = WebhookWorker::new(repo.clone(), "test_secret".to_string());
    let result = worker.process_pending_webhooks().await;
    assert!(result.is_ok());

    let updated_event = repo.find_by_id(event_id).await.unwrap().unwrap();
    assert_eq!(updated_event.status, WebhookStatus::Delivered);
    assert_eq!(updated_event.response_status, Some(200));
}

#[tokio::test]
async fn test_webhook_delivery_failure_retry() {
    let app = create_test_app_no_worker().await;
    let repo = Arc::new(WebhookEventRepoImpl::new(app.db_pool.clone()));

    let webhook_url = start_test_server(false).await;
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

    repo.create(&event)
        .await
        .expect("Failed to create webhook event");

    let worker = WebhookWorker::new(repo.clone(), "test_secret".to_string());
    let result = worker.process_pending_webhooks().await;
    assert!(result.is_ok());

    let updated_event = repo.find_by_id(event_id).await.unwrap().unwrap();
    assert_eq!(updated_event.attempt_count, 1);
    assert_eq!(updated_event.response_status, Some(500));
    assert_ne!(updated_event.status, WebhookStatus::Delivered);
}
