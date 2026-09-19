// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! sdforge-based SDK interface layer.
//!
//! Exposes domain services as HTTP endpoints via sdforge's `#[forge]` macro.
//! Each wrapper delegates to an existing domain trait (SearchServiceTrait,
//! TaskQueue, CrawlRepository) without modifying domain logic.
//!
//! Gate: only compiled when the `api-sdk` feature is enabled.

// sdforge #[forge] macro generates callers for these functions at compile time;
// they appear unused to the compiler but are wired into the HTTP router.
#![allow(dead_code)]

use sdforge::prelude::*;
use std::sync::Arc;
use uuid::Uuid;

use crate::domain::models::{Crawl, Task, TaskStatus, TaskType};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::services::search_service::{SearchQuery, SearchServiceTrait};
use crate::i18n::{t, I18nBundle, Locale};
use crate::presentation::middleware::auth_middleware::AuthState;
use crate::queue::task_queue::TaskQueue;

// ============================================================================
// DTOs — sdk-specific simplified request/response types.
// Decoupled from application DTOs to avoid coupling and validation complexity.
// team_id and api_key_id are NOT accepted from the request body — they are
// extracted from AuthState set by auth_middleware to prevent horizontal
// privilege escalation.
// ============================================================================

#[derive(serde::Deserialize)]
pub struct SdkSearchRequest {
    pub query: String,
    pub limit: Option<u32>,
}

#[derive(serde::Serialize)]
pub struct SdkSearchResult {
    pub title: String,
    pub url: String,
    pub description: Option<String>,
    pub engine: String,
}

#[derive(serde::Serialize)]
pub struct SdkSearchResponse {
    pub query: String,
    pub results: Vec<SdkSearchResult>,
    pub credits_used: u32,
}

#[derive(serde::Deserialize)]
pub struct SdkCreateTaskRequest {
    pub url: String,
    pub task_type: String,
}

#[derive(serde::Serialize)]
pub struct SdkCreateTaskResponse {
    pub id: Uuid,
    pub url: String,
    pub status: String,
}

#[derive(serde::Deserialize)]
pub struct SdkScrapeRequest {
    pub url: String,
}

#[derive(serde::Serialize)]
pub struct SdkScrapeResponse {
    pub id: Uuid,
    pub url: String,
}

#[derive(serde::Deserialize)]
pub struct SdkCreateCrawlRequest {
    pub name: String,
    pub url: String,
    pub seed_url: String,
}

#[derive(serde::Serialize)]
pub struct SdkCrawlResponse {
    pub id: Uuid,
    pub status: String,
    pub url: String,
}

// ============================================================================
// Wrapper functions — sdforge #[service_api] generates HTTP endpoints.
//
// Paths use version="v1" + path="/sdk/..." which sdforge resolves to
// "/api/v1/sdk/..." (the default prefix is /api/{version}).
// ============================================================================

#[forge(
    name = "sdk_search",
    version = "v1",
    path = "/sdk/search",
    method = "POST",
    description = "SDK search endpoint"
)]
async fn sdk_search(
    #[state] search_service: Arc<dyn SearchServiceTrait>,
    #[state] auth_state: AuthState,
    #[state] locale: Locale,
    #[state] i18n: Arc<I18nBundle>,
    req: SdkSearchRequest,
) -> Result<SdkSearchResponse, ApiError> {
    if req.query.trim().is_empty() {
        // FTL key: validation.ftl `sdk-query-empty`（locale 经 i18n 中间件注入）
        return Err(ApiError::InvalidInput {
            message: t(&locale, &i18n, "sdk-query-empty"),
            field: Some("query".to_string()),
            value: None,
        });
    }

    let query = SearchQuery {
        query: req.query.clone(),
        limit: req.limit,
        lang: None,
        country: None,
        engine: None,
        sources: None,
        crawl_results: None,
        crawl_config: None,
    };

    let resp = search_service
        .search(auth_state.team_id, auth_state.api_key_id, query)
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
            error_id: Uuid::new_v4().to_string(),
            source: None,
            context: None,
        })?;

    Ok(SdkSearchResponse {
        query: resp.query,
        results: resp
            .results
            .into_iter()
            .map(|r| SdkSearchResult {
                title: r.title,
                url: r.url,
                description: r.description,
                engine: r.engine,
            })
            .collect(),
        credits_used: resp.credits_used,
    })
}

#[forge(
    name = "sdk_create_task",
    version = "v1",
    path = "/sdk/tasks",
    method = "POST",
    description = "SDK create task endpoint"
)]
async fn sdk_create_task(
    #[state] queue: Arc<dyn TaskQueue>,
    #[state] auth_state: AuthState,
    #[state] locale: Locale,
    #[state] i18n: Arc<I18nBundle>,
    req: SdkCreateTaskRequest,
) -> Result<SdkCreateTaskResponse, ApiError> {
    if req.url.trim().is_empty() {
        // FTL key: validation.ftl `sdk-url-empty`
        return Err(ApiError::InvalidInput {
            message: t(&locale, &i18n, "sdk-url-empty"),
            field: Some("url".to_string()),
            value: None,
        });
    }

    let task_type = match req.task_type.as_str() {
        "scrape" => TaskType::Scrape,
        "crawl" => TaskType::Crawl,
        _ => {
            return Err(ApiError::InvalidInput {
                message: format!("unsupported task_type: {}", req.task_type),
                field: Some("task_type".to_string()),
                value: Some(serde_json::json!(req.task_type)),
            });
        }
    };

    let now = chrono::Utc::now();
    let task = Task {
        id: Uuid::new_v4(),
        task_type,
        status: TaskStatus::Queued,
        priority: 0,
        team_id: auth_state.team_id,
        api_key_id: auth_state.api_key_id,
        url: req.url.clone(),
        payload: serde_json::json!({}),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: now,
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: now,
        lock_token: None,
        lock_expires_at: None,
    };

    let created = queue.enqueue(task).await.map_err(|e| ApiError::Internal {
        message: e.to_string(),
        error_id: Uuid::new_v4().to_string(),
        source: None,
        context: None,
    })?;

    Ok(SdkCreateTaskResponse {
        id: created.id,
        url: created.url,
        status: format!("{:?}", created.status),
    })
}

#[forge(
    name = "sdk_scrape",
    version = "v1",
    path = "/sdk/scrape",
    method = "POST",
    description = "SDK scrape endpoint"
)]
async fn sdk_scrape(
    #[state] queue: Arc<dyn TaskQueue>,
    #[state] auth_state: AuthState,
    #[state] locale: Locale,
    #[state] i18n: Arc<I18nBundle>,
    req: SdkScrapeRequest,
) -> Result<SdkScrapeResponse, ApiError> {
    if req.url.trim().is_empty() {
        // FTL key: validation.ftl `sdk-url-empty`
        return Err(ApiError::InvalidInput {
            message: t(&locale, &i18n, "sdk-url-empty"),
            field: Some("url".to_string()),
            value: None,
        });
    }

    let now = chrono::Utc::now();
    let task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id: auth_state.team_id,
        api_key_id: auth_state.api_key_id,
        url: req.url.clone(),
        payload: serde_json::json!({}),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: now,
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: now,
        lock_token: None,
        lock_expires_at: None,
    };

    let created = queue.enqueue(task).await.map_err(|e| ApiError::Internal {
        message: e.to_string(),
        error_id: Uuid::new_v4().to_string(),
        source: None,
        context: None,
    })?;

    Ok(SdkScrapeResponse {
        id: created.id,
        url: created.url,
    })
}

#[forge(
    name = "sdk_create_crawl",
    version = "v1",
    path = "/sdk/crawl",
    method = "POST",
    description = "SDK create crawl endpoint"
)]
async fn sdk_create_crawl(
    #[state] crawl_repo: Arc<dyn CrawlRepository>,
    #[state] auth_state: AuthState,
    #[state] locale: Locale,
    #[state] i18n: Arc<I18nBundle>,
    req: SdkCreateCrawlRequest,
) -> Result<SdkCrawlResponse, ApiError> {
    if req.url.trim().is_empty() {
        // FTL key: validation.ftl `sdk-url-empty`
        return Err(ApiError::InvalidInput {
            message: t(&locale, &i18n, "sdk-url-empty"),
            field: Some("url".to_string()),
            value: None,
        });
    }

    let crawl = Crawl::new(
        Uuid::new_v4(),
        auth_state.team_id,
        req.name,
        req.url,
        req.seed_url,
        serde_json::json!({}),
    );

    let created = crawl_repo
        .create(&crawl)
        .await
        .map_err(|e| ApiError::Internal {
            message: e.to_string(),
            error_id: Uuid::new_v4().to_string(),
            source: None,
            context: None,
        })?;

    Ok(SdkCrawlResponse {
        id: created.id,
        status: format!("{:?}", created.status),
        url: created.root_url,
    })
}

/// Build the SDK router from all `#[service_api]` registered routes.
///
/// Collects routes via sdforge's inventory system and returns an Axum router.
/// Callers must inject the required `Extension<Arc<dyn Trait>>` layers for
/// each `#[state]` parameter the wrappers depend on.
///
/// `openapi` feature 启用时额外挂载 `GET /openapi.json`，输出聚合自全部
/// `#[forge]` 路由的 OpenAPI 3.1 规范（相对本路由挂载点，生产路径为
/// `/v1/sdk/openapi.json`）。
pub fn build_sdk_router() -> axum::Router {
    let router = sdforge::http::build();

    #[cfg(feature = "openapi")]
    let router = router.route("/openapi.json", axum::routing::get(openapi_json_handler));

    router
}

/// OpenAPI 3.1 spec endpoint（`openapi` feature 门控）。
#[cfg(feature = "openapi")]
async fn openapi_json_handler() -> axum::Json<utoipa::openapi::OpenApi> {
    axum::Json(sdforge::openapi::generate_openapi_spec())
}

// mocks 仅在 test build 或显式启用 `test-mocks` feature 时编译。
// integration test 通过 `crawlrs::presentation::sdk::mocks::*` 路径复用 mock 实现，
// 必须以 `cargo test --features test-mocks` 运行（已加入 docker-compose 与 CI）。
// production binary 默认不启用 `test-mocks`，mocks 完全不参与编译，无 dead code 残留。
#[cfg(any(test, feature = "test-mocks"))]
pub mod mocks;

// tests.rs 依赖 test_helpers（`cfg(all(test, feature = "platform"))`）与
// dev-dependency axum-test，仅 test 构建可编译；`cargo check` 等非 test 构建
// 即使启用 test-mocks 也不能编译（E0432 unresolved import）。
#[cfg(all(test, feature = "test-mocks"))]
mod tests;
