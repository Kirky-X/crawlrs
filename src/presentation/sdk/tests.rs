// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Tests for the sdforge-based SDK interface layer.
//!
//! Extracted from `mod.rs` as a file-based `mod tests` so that `#[path]`
//! resolves relative to this file's directory (`src/presentation/sdk/`),
//! which physically exists — inline `mod tests` would create a virtual
//! `src/presentation/sdk/tests/` directory that breaks `..` resolution.

use super::*;
use crate::common::test_helpers::create_test_db_pool;
use crate::domain::auth::ApiKeyScope;
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::services::search_service::SearchServiceTrait;
use crate::presentation::middleware::auth_middleware::AuthState;
use crate::queue::task_queue::TaskQueue;
use axum::Extension;
use axum_test::TestServer;
use serde_json::{json, Value};
use std::sync::Arc;
use uuid::Uuid;

// `mocks` 模块定义在 `src/presentation/sdk/mocks.rs`，由 `mod.rs` 通过
// `#[cfg(any(test, feature = "test-mocks"))] pub mod mocks;` 暴露。
// unit test 自动通过 `cfg(test)` 启用；integration tests
// (`tests/sdk_api_test.rs`) 通过 `cargo test --features test-mocks` 启用；
// production binary 默认不启用 `test-mocks`，mocks 完全不参与编译。

// ============ Test environment helpers ============

/// 检查 TEST_DATABASE_URL 是否设置；未设置时测试应早期返回。
/// sdk 测试需要 AuthState，而 AuthState 需要 DbPool，DbPool 构造需要真实 DB URL。
fn skip_if_no_db() -> bool {
    if std::env::var("TEST_DATABASE_URL").is_err() {
        eprintln!("[skip] TEST_DATABASE_URL not set — sdk tests require real DbPool");
        return true;
    }
    false
}

// `make_db_pool` helper 已集中到 `src/common/test_helpers.rs::create_test_db_pool`，
// 通过 `use crate::common::test_helpers::create_test_db_pool;` 引入。

/// 构造测试用 AuthState，使用随机 team_id 和 api_key_id。
fn make_auth_state() -> AuthState {
    AuthState::new(
        create_test_db_pool(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        ApiKeyScope::default(),
    )
}

// ============ Shared mock implementations ============
//
// Mock structs now live in `src/presentation/sdk/mocks.rs` (compiled only
// under `#[cfg(test)]`). Both this unit-test module and the integration
// tests in `tests/sdk_api_test.rs` import them via the canonical path
// `crate::presentation::sdk::mocks::*` / `crawlrs::presentation::sdk::mocks::*`,
// eliminating the previous `include!` + `use crate as crawlrs;` hack.

use super::mocks::{
    MockCrawlRepository, MockCrawlRepositoryError, MockSearchService, MockSearchServiceError,
    MockTaskQueue, MockTaskQueueError,
};

// ============ TestServer builders ============

/// 构造 TestServer，注入成功版 mock 服务。
fn make_server_success() -> TestServer {
    let auth_state = make_auth_state();
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

/// 构造 TestServer，注入搜索服务失败版 mock（其他服务用成功版）。
fn make_server_search_error() -> TestServer {
    let auth_state = make_auth_state();
    let app = build_sdk_router()
        .layer(Extension(
            Arc::new(MockSearchServiceError) as Arc<dyn SearchServiceTrait>
        ))
        .layer(Extension(Arc::new(MockTaskQueue) as Arc<dyn TaskQueue>))
        .layer(Extension(
            Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>
        ))
        .layer(Extension(auth_state));
    TestServer::new(app).expect("failed to build TestServer")
}

/// 构造 TestServer，注入任务队列失败版 mock（其他服务用成功版）。
fn make_server_queue_error() -> TestServer {
    let auth_state = make_auth_state();
    let app = build_sdk_router()
        .layer(Extension(
            Arc::new(MockSearchService) as Arc<dyn SearchServiceTrait>
        ))
        .layer(Extension(Arc::new(MockTaskQueueError) as Arc<dyn TaskQueue>))
        .layer(Extension(
            Arc::new(MockCrawlRepository) as Arc<dyn CrawlRepository>
        ))
        .layer(Extension(auth_state));
    TestServer::new(app).expect("failed to build TestServer")
}

/// 构造 TestServer，注入 crawl 仓库失败版 mock（其他服务用成功版）。
fn make_server_crawl_repo_error() -> TestServer {
    let auth_state = make_auth_state();
    let app = build_sdk_router()
        .layer(Extension(
            Arc::new(MockSearchService) as Arc<dyn SearchServiceTrait>
        ))
        .layer(Extension(Arc::new(MockTaskQueue) as Arc<dyn TaskQueue>))
        .layer(Extension(
            Arc::new(MockCrawlRepositoryError) as Arc<dyn CrawlRepository>
        ))
        .layer(Extension(auth_state));
    TestServer::new(app).expect("failed to build TestServer")
}

// ============ sdk_search tests ============

/// sdk_search happy path：合法 query 应返回 200 + 搜索结果。
/// 覆盖 SearchQuery 构造、search_service.search 调用、响应映射。
#[tokio::test]
async fn test_sdk_search_happy_path() {
    if skip_if_no_db() {
        return;
    }
    let server = make_server_success();
    let body = json!({
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
    assert_eq!(payload["results"][0]["engine"], "mock");
    assert_eq!(payload["credits_used"], 1);
}

/// sdk_search 空 query（trim 后为空）应返回 400 InvalidInput。
/// 覆盖 query.trim().is_empty() 分支。
#[tokio::test]
async fn test_sdk_search_empty_query_returns_400() {
    if skip_if_no_db() {
        return;
    }
    let server = make_server_success();
    let body = json!({ "query": "   " });
    let res = server.post("/api/v1/sdk/search").json(&body).await;
    res.assert_status_bad_request();
}

/// sdk_search 无 query 字段应返回 400（反序列化失败，serde 拒绝缺失必填字段）。
#[tokio::test]
async fn test_sdk_search_missing_query_field_returns_422() {
    if skip_if_no_db() {
        return;
    }
    let server = make_server_success();
    let body = json!({ "limit": 5 });
    let res = server.post("/api/v1/sdk/search").json(&body).await;
    // axum 对 JSON 反序列化失败（missing field）默认返回 422 Unprocessable Entity
    assert_eq!(
        res.status_code().as_u16(),
        422,
        "Expected status code 422 (Unprocessable Entity) for missing field, got {}",
        res.status_code()
    );
}

/// sdk_search 搜索服务返回错误时应返回 500 Internal。
/// 覆盖 map_err 分支，ApiError::Internal 构造。
#[tokio::test]
async fn test_sdk_search_service_error_returns_500() {
    if skip_if_no_db() {
        return;
    }
    let server = make_server_search_error();
    let body = json!({ "query": "test" });
    let res = server.post("/api/v1/sdk/search").json(&body).await;
    assert_eq!(res.status_code().as_u16(), 500);
}

// ============ sdk_create_task tests ============

/// sdk_create_task happy path（task_type=scrape）：应返回 200 + status=Queued。
/// 覆盖 task_type 匹配、Task 构造、enqueue、响应映射。
#[tokio::test]
async fn test_sdk_create_task_scrape_happy_path() {
    if skip_if_no_db() {
        return;
    }
    let server = make_server_success();
    let body = json!({
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

/// sdk_create_task happy path（task_type=crawl）：应返回 200 + status=Queued。
/// 覆盖 task_type="crawl" 分支。
#[tokio::test]
async fn test_sdk_create_task_crawl_happy_path() {
    if skip_if_no_db() {
        return;
    }
    let server = make_server_success();
    let body = json!({
        "url": "https://example.com/crawl",
        "task_type": "crawl"
    });
    let res = server.post("/api/v1/sdk/tasks").json(&body).await;
    res.assert_status_ok();
    let payload: Value = res.json();
    assert!(payload["id"].is_string());
    assert_eq!(payload["url"], "https://example.com/crawl");
    assert_eq!(payload["status"], "Queued");
}

/// sdk_create_task 空 url 应返回 400 InvalidInput。
/// 覆盖 url.trim().is_empty() 分支。
#[tokio::test]
async fn test_sdk_create_task_empty_url_returns_400() {
    if skip_if_no_db() {
        return;
    }
    let server = make_server_success();
    let body = json!({ "url": "  ", "task_type": "scrape" });
    let res = server.post("/api/v1/sdk/tasks").json(&body).await;
    res.assert_status_bad_request();
}

/// sdk_create_task 无效 task_type 应返回 400 InvalidInput。
/// 覆盖 _ => InvalidInput 分支。
#[tokio::test]
async fn test_sdk_create_task_invalid_type_returns_400() {
    if skip_if_no_db() {
        return;
    }
    let server = make_server_success();
    let body = json!({
        "url": "https://example.com",
        "task_type": "invalid_type"
    });
    let res = server.post("/api/v1/sdk/tasks").json(&body).await;
    res.assert_status_bad_request();
}

/// sdk_create_task 队列入队失败应返回 500 Internal。
/// 覆盖 map_err 分支，ApiError::Internal 构造。
#[tokio::test]
async fn test_sdk_create_task_queue_error_returns_500() {
    if skip_if_no_db() {
        return;
    }
    let server = make_server_queue_error();
    let body = json!({
        "url": "https://example.com",
        "task_type": "scrape"
    });
    let res = server.post("/api/v1/sdk/tasks").json(&body).await;
    assert_eq!(res.status_code().as_u16(), 500);
}

// ============ sdk_scrape tests ============

/// sdk_scrape happy path：应返回 200 + id + url。
/// 覆盖 Task 构造、enqueue、响应映射。
#[tokio::test]
async fn test_sdk_scrape_happy_path() {
    if skip_if_no_db() {
        return;
    }
    let server = make_server_success();
    let body = json!({ "url": "https://example.com/page" });
    let res = server.post("/api/v1/sdk/scrape").json(&body).await;
    res.assert_status_ok();
    let payload: Value = res.json();
    assert!(payload["id"].is_string());
    assert_eq!(payload["url"], "https://example.com/page");
}

/// sdk_scrape 空 url 应返回 400 InvalidInput。
/// 覆盖 url.trim().is_empty() 分支。
#[tokio::test]
async fn test_sdk_scrape_empty_url_returns_400() {
    if skip_if_no_db() {
        return;
    }
    let server = make_server_success();
    let body = json!({ "url": "" });
    let res = server.post("/api/v1/sdk/scrape").json(&body).await;
    res.assert_status_bad_request();
}

/// sdk_scrape 队列入队失败应返回 500 Internal。
/// 覆盖 map_err 分支，ApiError::Internal 构造。
#[tokio::test]
async fn test_sdk_scrape_queue_error_returns_500() {
    if skip_if_no_db() {
        return;
    }
    let server = make_server_queue_error();
    let body = json!({ "url": "https://example.com/page" });
    let res = server.post("/api/v1/sdk/scrape").json(&body).await;
    assert_eq!(res.status_code().as_u16(), 500);
}

// ============ sdk_create_crawl tests ============

/// sdk_create_crawl happy path：应返回 200 + status=Queued + url。
/// 覆盖 Crawl::new、crawl_repo.create、响应映射。
#[tokio::test]
async fn test_sdk_create_crawl_happy_path() {
    if skip_if_no_db() {
        return;
    }
    let server = make_server_success();
    let body = json!({
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

/// sdk_create_crawl 空 url 应返回 400 InvalidInput。
/// 覆盖 url.trim().is_empty() 分支。
#[tokio::test]
async fn test_sdk_create_crawl_empty_url_returns_400() {
    if skip_if_no_db() {
        return;
    }
    let server = make_server_success();
    let body = json!({
        "name": "test crawl",
        "url": "  ",
        "seed_url": "https://example.com/start"
    });
    let res = server.post("/api/v1/sdk/crawl").json(&body).await;
    res.assert_status_bad_request();
}

/// sdk_create_crawl 仓库创建失败应返回 500 Internal。
/// 覆盖 map_err 分支，ApiError::Internal 构造。
#[tokio::test]
async fn test_sdk_create_crawl_repo_error_returns_500() {
    if skip_if_no_db() {
        return;
    }
    let server = make_server_crawl_repo_error();
    let body = json!({
        "name": "test crawl",
        "url": "https://example.com",
        "seed_url": "https://example.com/start"
    });
    let res = server.post("/api/v1/sdk/crawl").json(&body).await;
    assert_eq!(res.status_code().as_u16(), 500);
}
