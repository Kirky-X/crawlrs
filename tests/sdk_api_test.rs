// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Integration tests for the sdforge-based SDK API layer.
//!
//! Validates that the 4 endpoints exposed by `src/presentation/sdk/mod.rs`
//! correctly delegate to domain services (SearchServiceTrait, TaskQueue,
//! CrawlRepository) and properly handle success + failure cases.
//!
//! Task9: api-sdk feature removed; sdforge is non-optional, tests always run.

#[path = "common/mod.rs"]
mod common;

use std::sync::Arc;

use axum::Extension;
use axum_test::TestServer;
use serde_json::{json, Value};
use uuid::Uuid;

use common::helpers::db_pool::create_test_pool_or_panic;

use crawlrs::domain::auth::ApiKeyScope;
use crawlrs::domain::repositories::crawl_repository::CrawlRepository;
use crawlrs::domain::services::search_service::SearchServiceTrait;
use crawlrs::presentation::middleware::auth_middleware::AuthState;
use crawlrs::presentation::sdk::build_sdk_router;
use crawlrs::presentation::sdk::mocks::{MockCrawlRepository, MockSearchService, MockTaskQueue};
use crawlrs::queue::task_queue::TaskQueue;

/// Build a TestServer with mock services + AuthState Extension injected.
///
/// SDK endpoints use `#[state] auth_state: AuthState` which sdforge resolves
/// from the axum Extension layer — without it, every request returns 500
/// "Missing request extension: AuthState".
fn make_server() -> TestServer {
    let team_id = Uuid::parse_str(TEAM_ID).expect("valid TEAM_ID uuid");
    let api_key_id = Uuid::parse_str(API_KEY_ID).expect("valid API_KEY_ID uuid");
    let pool = create_test_pool_or_panic();
    let auth_state = AuthState::new(pool, team_id, api_key_id, ApiKeyScope::default());

    let app = build_sdk_router()
        .layer(Extension(
            Arc::new(MockSearchService) as Arc<dyn SearchServiceTrait>
        ))
        .layer(Extension(Arc::new(MockTaskQueue) as Arc<dyn TaskQueue>))
        .layer(Extension(
            Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>
        ))
        .layer(Extension(auth_state));
    TestServer::new(app).expect("failed to build TestServer")
}

const TEAM_ID: &str = "00000000-0000-0000-0000-000000000001";
const API_KEY_ID: &str = "00000000-0000-0000-0000-000000000002";

// ============================================================================
// /api/v1/sdk/search
// ============================================================================

#[tokio::test]
async fn test_sdk_search_success() {
    let server = make_server();
    let body = json!({
        "team_id": TEAM_ID,
        "api_key_id": API_KEY_ID,
        "query": "rust web framework",
        "limit": 5
    });
    let res = server.post("/api/v1/sdk/search").json(&body).await;
    res.assert_status_ok();
    let payload: Value = res.json();
    assert_eq!(payload["query"], "rust web framework");
    assert!(payload["results"].is_array());
    assert_eq!(payload["results"][0]["title"], "Mock Title");
    assert_eq!(payload["results"][0]["url"], "https://example.com");
    assert_eq!(payload["credits_used"], 1);
}

#[tokio::test]
async fn test_sdk_search_empty_query_returns_400() {
    let server = make_server();
    let body = json!({
        "team_id": TEAM_ID,
        "api_key_id": API_KEY_ID,
        "query": "   "
    });
    let res = server.post("/api/v1/sdk/search").json(&body).await;
    res.assert_status_bad_request();
}

// ============================================================================
// /api/v1/sdk/tasks
// ============================================================================

#[tokio::test]
async fn test_sdk_create_task_success() {
    let server = make_server();
    let body = json!({
        "team_id": TEAM_ID,
        "api_key_id": API_KEY_ID,
        "url": "https://example.com",
        "task_type": "scrape"
    });
    let res = server.post("/api/v1/sdk/tasks").json(&body).await;
    res.assert_status_ok();
    let payload: Value = res.json();
    assert!(payload["id"].is_string());
    assert_eq!(payload["url"], "https://example.com");
    assert_eq!(payload["status"], "Queued");
}

#[tokio::test]
async fn test_sdk_create_task_invalid_type_returns_400() {
    let server = make_server();
    let body = json!({
        "team_id": TEAM_ID,
        "api_key_id": API_KEY_ID,
        "url": "https://example.com",
        "task_type": "invalid_type"
    });
    let res = server.post("/api/v1/sdk/tasks").json(&body).await;
    res.assert_status_bad_request();
}

// ============================================================================
// /api/v1/sdk/scrape
// ============================================================================

#[tokio::test]
async fn test_sdk_scrape_success() {
    let server = make_server();
    let body = json!({
        "team_id": TEAM_ID,
        "api_key_id": API_KEY_ID,
        "url": "https://example.com/page"
    });
    let res = server.post("/api/v1/sdk/scrape").json(&body).await;
    res.assert_status_ok();
    let payload: Value = res.json();
    assert!(payload["id"].is_string());
    assert_eq!(payload["url"], "https://example.com/page");
}

#[tokio::test]
async fn test_sdk_scrape_empty_url_returns_400() {
    let server = make_server();
    let body = json!({
        "team_id": TEAM_ID,
        "api_key_id": API_KEY_ID,
        "url": ""
    });
    let res = server.post("/api/v1/sdk/scrape").json(&body).await;
    res.assert_status_bad_request();
}

// ============================================================================
// /api/v1/sdk/crawl
// ============================================================================

#[tokio::test]
async fn test_sdk_create_crawl_success() {
    let server = make_server();
    let body = json!({
        "team_id": TEAM_ID,
        "name": "test crawl",
        "url": "https://example.com",
        "seed_url": "https://example.com/start"
    });
    let res = server.post("/api/v1/sdk/crawl").json(&body).await;
    res.assert_status_ok();
    let payload: Value = res.json();
    assert!(payload["id"].is_string());
    assert_eq!(payload["status"], "Queued");
    assert_eq!(payload["url"], "https://example.com");
}

#[tokio::test]
async fn test_sdk_create_crawl_empty_url_returns_400() {
    let server = make_server();
    let body = json!({
        "team_id": TEAM_ID,
        "name": "test crawl",
        "url": "  ",
        "seed_url": "https://example.com/start"
    });
    let res = server.post("/api/v1/sdk/crawl").json(&body).await;
    res.assert_status_bad_request();
}
