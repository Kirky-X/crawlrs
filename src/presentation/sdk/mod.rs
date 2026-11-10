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

use crate::domain::models::{Task, TaskStatus, TaskType};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::services::search_service::{SearchQuery, SearchServiceTrait};
use crate::i18n::{t, I18nBundle, Locale};
use crate::presentation::middleware::auth_middleware::AuthState;
use crate::queue::task_queue::TaskQueue;

// DTOs — sdk-specific simplified request/response types.
// Decoupled from application DTOs to avoid coupling and validation complexity.
// team_id and api_key_id are NOT accepted from the request body — they are
// extracted from AuthState set by auth_middleware to prevent horizontal
// privilege escalation.

/// tasks.url 的字节上限（与 HTTP 入口及 idx_tasks_url btree 行上限兼容）。
const MAX_TASK_URL_BYTES: usize = 2048;

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
    pub id: crate::domain::id::Id,
    pub url: String,
    pub status: String,
}

#[derive(serde::Deserialize)]
pub struct SdkScrapeRequest {
    pub url: String,
}

#[derive(serde::Serialize)]
pub struct SdkScrapeResponse {
    pub id: crate::domain::id::Id,
    pub url: String,
}

#[derive(serde::Deserialize)]
pub struct SdkCreateCrawlRequest {
    pub name: String,
    pub url: String,
    pub seed_url: String,
    /// 抓取深度（缺省 2；上限与 HTTP 入口同源 `MAX_CRAWL_DEPTH`）。
    ///
    /// 必须写进首任务 payload 的 config：worker 解析 task payload 时会
    /// 反序列化 `CrawlConfigDto`（max_depth 必填），缺省会直接 payload_invalid。
    #[serde(default)]
    pub max_depth: Option<u32>,
}

#[derive(serde::Serialize)]
pub struct SdkCrawlResponse {
    pub id: crate::domain::id::Id,
    pub status: String,
    pub url: String,
}

// Wrapper functions — sdforge #[forge] generates HTTP endpoints.
//
// Paths use version="v1" + path="/sdk/..." which sdforge resolves to
// "/api/v1/sdk/..." (the default prefix is /api/{version}).

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
        .map_err(|_e| ApiError::Internal {
            message: "Internal server error".to_string(),
            error_id: crate::domain::id::Id::generate().to_string(),
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
    // tasks.url 的普通 btree 索引有行上限：与 HTTP 入口同口径按字节限长。
    if req.url.len() > MAX_TASK_URL_BYTES {
        return Err(ApiError::InvalidInput {
            message: "url must be at most 2048 bytes".to_string(),
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
        id: crate::domain::id::Id::generate(),
        task_type,
        status: TaskStatus::Queued,
        priority: 0,
        team_id: auth_state.team_id,
        api_key_id: auth_state.api_key_id,
        url: req.url.clone(),
        // scrape 型任务执行前反序列化 ScrapeRequestDto（url 必填），空 payload
        // 必然 payload_invalid 失败；crawl 型任务由 crawl 管线播种（payload 需
        // crawl_id/depth/config），此处仅入队不播种
        payload: serde_json::json!({"url": req.url}),
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

    let created = queue.enqueue(task).await.map_err(|_e| ApiError::Internal {
        // 固定文案：不向 SDK 客户端回显 DB/队列内部错误文本
        message: "Internal server error".to_string(),
        error_id: crate::domain::id::Id::generate().to_string(),
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
    // tasks.url 的普通 btree 索引有行上限：与 HTTP 入口同口径按字节限长。
    if req.url.len() > MAX_TASK_URL_BYTES {
        return Err(ApiError::InvalidInput {
            message: "url must be at most 2048 bytes".to_string(),
            field: Some("url".to_string()),
            value: None,
        });
    }

    let now = chrono::Utc::now();
    let task = Task {
        id: crate::domain::id::Id::generate(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id: auth_state.team_id,
        api_key_id: auth_state.api_key_id,
        url: req.url.clone(),
        // worker 执行前反序列化 ScrapeRequestDto（url 必填 + deny_unknown_fields），
        // 空 payload 会导致任务必然以 payload_invalid 失败
        payload: serde_json::json!({"url": req.url}),
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

    let created = queue.enqueue(task).await.map_err(|_e| ApiError::Internal {
        // 固定文案：不向 SDK 客户端回显 DB/队列内部错误文本
        message: "Internal server error".to_string(),
        error_id: crate::domain::id::Id::generate().to_string(),
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
#[allow(clippy::too_many_arguments)] // forge `#[state]` 注入：每个依赖必须独立参数
async fn sdk_create_crawl(
    #[state] crawl_repo: Arc<dyn CrawlRepository>,
    #[state] queue: Arc<dyn TaskQueue>,
    #[state] auth_state: AuthState,
    #[state] locale: Locale,
    #[state] i18n: Arc<I18nBundle>,
    #[state] geo_repo: Option<
        Arc<dyn crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository>,
    >,
    #[state] rate_limiting_service: Option<
        Arc<dyn crate::domain::services::rate_limiting_service::RateLimitingService>,
    >,
    req: SdkCreateCrawlRequest,
) -> Result<SdkCrawlResponse, ApiError> {
    for (field, value) in [
        ("url", req.url.as_str()),
        ("seed_url", req.seed_url.as_str()),
    ] {
        if value.trim().is_empty() {
            // FTL key: validation.ftl `sdk-url-empty`
            return Err(ApiError::InvalidInput {
                message: t(&locale, &i18n, "sdk-url-empty"),
                field: Some(field.to_string()),
                value: None,
            });
        }
        // tasks.url 的普通 btree 索引有行上限：与 HTTP 入口同口径按字节限长。
        if value.len() > MAX_TASK_URL_BYTES {
            return Err(ApiError::InvalidInput {
                message: format!("{field} must be at most 2048 bytes"),
                field: Some(field.to_string()),
                value: None,
            });
        }
        // scheme 校验与 HTTP 入口同源（仅 http/https，SSRF 的第一道闸）。
        if crate::application::dto::crawl_request::is_http_url(value).is_err() {
            return Err(ApiError::InvalidInput {
                message: format!("{field} must start with http:// or https://"),
                field: Some(field.to_string()),
                value: None,
            });
        }
    }
    let max_depth = req.max_depth.unwrap_or(2);
    if max_depth > crate::application::dto::crawl_request::MAX_CRAWL_DEPTH {
        return Err(ApiError::InvalidInput {
            message: format!(
                "max_depth must be between 0 and {}",
                crate::application::dto::crawl_request::MAX_CRAWL_DEPTH
            ),
            field: Some("max_depth".to_string()),
            value: Some(serde_json::json!(max_depth)),
        });
    }
    if req.name.trim().is_empty() || req.name.chars().count() > 255 {
        return Err(ApiError::InvalidInput {
            message: "name must be 1-255 characters".to_string(),
            field: Some("name".to_string()),
            value: None,
        });
    }

    // per-key 配额（与 /v1/crawl 同口径；SDK 端点不计 credit，仅限流）。
    if let Some(service) = rate_limiting_service.as_ref() {
        use crate::domain::services::rate_limiting_service::RateLimitResult;
        match service
            .check_rate_limit(&auth_state.api_key_id.to_string(), "/sdk/crawl")
            .await
        {
            Ok(RateLimitResult::Allowed) => {}
            Ok(RateLimitResult::Denied { reason }) => {
                log::warn!(
                    "SDK crawl rate limited for key {}: {}",
                    auth_state.api_key_id,
                    reason
                );
                return Err(ApiError::RateLimitExceeded {
                    limit: 0,
                    window_seconds: 60,
                });
            }
            Ok(RateLimitResult::RetryAfter {
                retry_after_seconds,
            }) => {
                // 无 cache-capacity 数值可填，按“服务暂不可用 + Retry-After”语义返回，
                // 避免把秒数伪装成 window_seconds。
                return Err(ApiError::ServiceUnavailable {
                    service: "rate_limit".to_string(),
                    retry_after: Some(retry_after_seconds),
                    source: None,
                });
            }
            Err(e) => {
                log::error!("SDK crawl rate limit check failed: {}", e);
                return Err(ApiError::Internal {
                    message: "Internal server error".to_string(),
                    error_id: crate::domain::id::Id::generate().to_string(),
                    source: None,
                    context: None,
                });
            }
        }
    }

    // 团队域名黑名单：SDK 与 HTTP 入口同规则（命中即拒绝，且把黑名单写入首任务，
    // 链接提取侧据此过滤发现链接）。地理（国家）限制依赖请求源 IP，SDK 为
    // 服务端到服务端调用，无客户端 IP，故不做国家判定。
    let restrictions = match geo_repo.as_ref() {
        Some(repo) => repo
            .get_team_restrictions(auth_state.team_id)
            .await
            .map_err(|e| {
                log::error!(
                    "Failed to load geo restrictions for team {} on SDK crawl: {}",
                    auth_state.team_id,
                    e
                );
                ApiError::Internal {
                    message: "Internal server error".to_string(),
                    error_id: crate::domain::id::Id::generate().to_string(),
                    source: None,
                    context: None,
                }
            })?,
        None => crate::domain::services::team_service::TeamGeoRestrictions::default(),
    };
    if let Some(reason) =
        crate::domain::services::team_service::domain_blacklist_denial(&req.seed_url, &restrictions)
    {
        return Err(ApiError::InvalidInput {
            message: reason,
            field: Some("seed_url".to_string()),
            value: None,
        });
    }
    let domain_blacklist = restrictions
        .effective_domain_blacklist()
        .map_err(|message| ApiError::InvalidInput {
            message,
            field: Some("domain_blacklist".to_string()),
            value: None,
        })?;

    // SSRF 预检（与 HTTP 入口同一函数）：入队前拒绝内网/回环/元数据地址，
    // 不让内部地址占用队列与 worker。DNS 解析无内建缓存/超时，外层加 5s 上界，
    // 避免解析器故障把请求连接挂死。
    match tokio::time::timeout(
        std::time::Duration::from_secs(5),
        crate::presentation::handlers::check_ssrf_url(
            &req.seed_url,
            auth_state.team_id,
            auth_state.api_key_id,
        ),
    )
    .await
    {
        Ok(None) => {}
        Ok(Some(_)) => {
            return Err(ApiError::InvalidInput {
                message: "seed_url is not allowed (SSRF protection)".to_string(),
                field: Some("seed_url".to_string()),
                value: None,
            });
        }
        Err(_) => {
            return Err(ApiError::ServiceUnavailable {
                service: "ssrf_check".to_string(),
                retry_after: Some(1),
                source: None,
            });
        }
    }

    // DTO 只承载抓取语义；创建 crawl + 首任务 + 入队补偿统一走 application 编排，
    // HTTP 与 SDK 共用同一条不变式。
    let dto = crate::application::dto::crawl_request::CrawlRequestDto {
        url: req.seed_url,
        validated_url: None,
        name: Some(req.name),
        config: crate::application::dto::crawl_request::CrawlConfigDto {
            max_depth,
            include_patterns: None,
            exclude_patterns: None,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            max_pages: None,
            proxy: None,
            headers: None,
            extraction_rules: None,
            extraction_prompt: None,
            extraction_schema: None,
        },
        sync_wait_ms: None,
        expires_at: None,
        sitemap_seed: None,
        sitemap_url: None,
    };
    let now = chrono::Utc::now();
    let crawl =
        crate::application::use_cases::crawl_link_processor::persist_crawl_with_initial_task(
            crawl_repo.as_ref(),
            auth_state.team_id,
            auth_state.api_key_id,
            &dto,
            &req.url,
            domain_blacklist,
            now,
            |task| {
                let q = queue.clone();
                async move { q.enqueue(task).await }
            },
        )
        .await
        .map_err(|e| {
            // 不向 SDK 客户端回显 DB/队列内部错误文本；细节只进服务端日志
            log::error!("SDK crawl create failed: {}", e);
            ApiError::Internal {
                message: "Internal server error".to_string(),
                error_id: crate::domain::id::Id::generate().to_string(),
                source: None,
                context: None,
            }
        })?;

    Ok(SdkCrawlResponse {
        id: crawl.id,
        status: format!("{:?}", crate::domain::models::CrawlStatus::Queued),
        url: crawl.root_url,
    })
}

/// Build the SDK router from all `#[forge]` registered routes.
///
/// Collects routes via sdforge's inventory system and returns an Axum router.
/// Callers must inject the required `Extension<Arc<dyn Trait>>` layers for
/// each `#[state]` parameter the wrappers depend on.
///
/// `openapi` feature 启用时额外挂载 `GET /openapi.json`，输出聚合自全部
/// `#[forge]` 路由的 OpenAPI 3.1 规范。该路由相对 `build_sdk_router` 返回的
/// router 注册，而此 router 在 `bootstrap::routes` 中 merge 到根，故生产路径
/// 为 `/openapi.json`。
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
