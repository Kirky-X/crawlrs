// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use axum::{
    extract::{ConnectInfo, Extension, Path},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::json;
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::error;
use uuid::Uuid;

use crate::application::dto::crawl_request::CrawlRequestDto;
use crate::application::use_cases::crawl_use_case::{CrawlUseCase, CrawlUseCaseError};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::repositories::webhook_repository::WebhookRepository;
use crate::domain::services::rate_limiting_service::{RateLimitResult, RateLimitingService};
use crate::domain::services::team_service::TeamService;
use crate::infrastructure::geolocation::GeoLocationService;
use crate::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl;
use crate::presentation::handlers::task_handler::wait_for_tasks_completion;
use validator::Validate;

fn is_internal_url(url_str: &str) -> bool {
    if let Ok(url) = url::Url::parse(url_str) {
        if let Some(host) = url.host_str() {
            // 简单检查是否为本地或私有IP
            return host == "localhost"
                || host == "127.0.0.1"
                || host.starts_with("192.168.")
                || host.starts_with("10.")
                || host.starts_with("172.");
        }
    }
    false
}

/// 创建新的爬取任务
#[allow(clippy::too_many_arguments)]
pub async fn create_crawl<CR, TR, WR, SRR, GR>(
    Extension(crawl_repo): Extension<Arc<CR>>,
    Extension(task_repo): Extension<Arc<TR>>,
    Extension(webhook_repo): Extension<Arc<WR>>,
    Extension(scrape_result_repo): Extension<Arc<SRR>>,
    Extension(geo_restriction_repo): Extension<Arc<GR>>,
    Extension(team_service): Extension<Arc<TeamService>>,
    Extension(task_repository_impl): Extension<Arc<TaskRepositoryImpl>>,
    Extension(rate_limiting_service): Extension<Arc<dyn RateLimitingService>>,
    Extension(team_id): Extension<Uuid>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<CrawlRequestDto>,
) -> impl IntoResponse
where
    CR: CrawlRepository + 'static,
    TR: TaskRepository + 'static,
    WR: WebhookRepository + 'static,
    SRR: ScrapeResultRepository + 'static,
    GR: GeoRestrictionRepository + 'static,
{
    if let Err(e) = payload.validate() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(json!({
                "success": false,
                "error": e.to_string()
            })),
        )
            .into_response();
    }

    if is_internal_url(&payload.url) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "SSRF protection: Internal URLs are not allowed"
            })),
        )
            .into_response();
    }

    // 1. 检查限流
    match rate_limiting_service
        .check_rate_limit("default_api_key", "/v1/crawl")
        .await
    {
        Ok(RateLimitResult::Denied { reason }) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "success": false,
                    "error": format!("Rate limit exceeded: {}", reason)
                })),
            )
                .into_response();
        }
        Ok(RateLimitResult::RetryAfter {
            retry_after_seconds,
        }) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
                    "success": false,
                    "error": "Rate limit exceeded, please retry later",
                    "retry_after_seconds": retry_after_seconds
                })),
            )
                .into_response();
        }
        Err(e) => {
            error!("Rate limiting service error: {}", e);
            // 降级：如果限流服务出错，允许继续
        }
        _ => {}
    }

    // 2. 检查配额
    if let Err(e) = rate_limiting_service
        .check_and_deduct_quota(
            team_id,
            10, // 爬取任务通常比抓取消耗更多，假设消耗 10 Credits
            crate::domain::models::credits::CreditsTransactionType::Crawl,
            format!("Crawl URL: {}", payload.url),
            None, // 初始扣费，尚未创建具体任务ID
        )
        .await
    {
        return (
            StatusCode::PAYMENT_REQUIRED,
            Json(json!({
                "success": false,
                "error": e.to_string()
            })),
        )
            .into_response();
    }

    let use_case = CrawlUseCase::new(
        crawl_repo,
        task_repo,
        webhook_repo,
        scrape_result_repo,
        geo_restriction_repo,
        team_service,
    );

    let client_ip = addr.ip().to_string();
    match use_case
        .create_crawl(team_id, payload.clone(), &client_ip)
        .await
    {
        Ok(crawl) => {
            // 处理同步等待逻辑
            let sync_wait_ms = payload.sync_wait_ms.unwrap_or(5000);
            let mut waited_time_ms = 0u64;

            if sync_wait_ms > 0 {
                let wait_start = std::time::Instant::now();

                // 获取爬取任务的初始任务ID
                // 注意：这里我们需要获取与爬取任务关联的任务ID
                // 由于create_crawl返回的是Crawl对象，我们需要查询相关的任务
                match task_repository_impl.find_by_crawl_id(crawl.id).await {
                    Ok(tasks) => {
                        if !tasks.is_empty() {
                            let task_ids: Vec<uuid::Uuid> =
                                tasks.iter().map(|task| task.id).collect();

                            // 调用智能轮询等待函数
                            match wait_for_tasks_completion(
                                task_repository_impl.as_ref(),
                                &task_ids,
                                team_id,
                                sync_wait_ms,
                                1000, // 基础轮询间隔1秒
                            )
                            .await
                            {
                                Ok(_) => {
                                    waited_time_ms = wait_start.elapsed().as_millis() as u64;
                                }
                                Err(e) => {
                                    error!("Failed to wait for task completion: {:?}", e);
                                    // 即使等待失败，也返回已创建的爬取任务信息
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to find tasks for crawl {}: {:?}", crawl.id, e);
                        // 即使查询失败，也返回已创建的爬取任务信息
                    }
                }
            }

            // 根据同步等待结果设置响应状态
            let status_code = if sync_wait_ms > 0 && waited_time_ms >= sync_wait_ms as u64 {
                StatusCode::ACCEPTED // 同步等待超时，任务已接受但可能未完成
            } else {
                StatusCode::CREATED // 任务已创建（可能已完成）
            };

            (status_code, Json(crawl)).into_response()
        }
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            (status, Json(json!({ "error": msg }))).into_response()
        }
    }
}

/// 获取爬取任务详情
pub async fn get_crawl<CR, TR, WR, SRR, GR>(
    Extension(crawl_repo): Extension<Arc<CR>>,
    Extension(task_repo): Extension<Arc<TR>>,
    Extension(webhook_repo): Extension<Arc<WR>>,
    Extension(scrape_result_repo): Extension<Arc<SRR>>,
    Extension(_geo_restriction_repo): Extension<Arc<GR>>,
    Path(crawl_id): Path<Uuid>,
) -> impl IntoResponse
where
    CR: CrawlRepository + 'static,
    TR: TaskRepository + 'static,
    WR: WebhookRepository + 'static,
    SRR: ScrapeResultRepository + 'static,
    GR: GeoRestrictionRepository + 'static,
{
    let use_case = CrawlUseCase::new(
        crawl_repo,
        task_repo,
        webhook_repo,
        scrape_result_repo,
        _geo_restriction_repo.clone(),
        Arc::new(TeamService::new(
            GeoLocationService::new(),
            _geo_restriction_repo.clone(),
        )),
    );
    match use_case.get_crawl(crawl_id).await {
        Ok(Some(crawl)) => (StatusCode::OK, Json(crawl)).into_response(),
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            (status, Json(json!({ "error": msg }))).into_response()
        }
    }
}

/// 获取爬取任务结果
pub async fn get_crawl_results<CR, TR, WR, SRR, GR>(
    Extension(crawl_repo): Extension<Arc<CR>>,
    Extension(task_repo): Extension<Arc<TR>>,
    Extension(webhook_repo): Extension<Arc<WR>>,
    Extension(scrape_result_repo): Extension<Arc<SRR>>,
    Extension(_geo_restriction_repo): Extension<Arc<GR>>,
    Extension(team_id): Extension<Uuid>,
    Path(crawl_id): Path<Uuid>,
) -> impl IntoResponse
where
    CR: CrawlRepository + 'static,
    TR: TaskRepository + 'static,
    WR: WebhookRepository + 'static,
    SRR: ScrapeResultRepository + 'static,
    GR: GeoRestrictionRepository + 'static,
{
    let use_case = CrawlUseCase::new(
        crawl_repo,
        task_repo,
        webhook_repo,
        scrape_result_repo,
        _geo_restriction_repo.clone(),
        Arc::new(TeamService::new(
            GeoLocationService::new(),
            _geo_restriction_repo.clone(),
        )),
    );

    match use_case.get_crawl_results(crawl_id, team_id).await {
        Ok(results) => (StatusCode::OK, Json(results)).into_response(),
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            (status, Json(json!({ "error": msg }))).into_response()
        }
    }
}

/// 取消进行中的爬取任务
pub async fn cancel_crawl<CR, TR, WR, SRR, GR>(
    Extension(crawl_repo): Extension<Arc<CR>>,
    Extension(task_repo): Extension<Arc<TR>>,
    Extension(webhook_repo): Extension<Arc<WR>>,
    Extension(scrape_result_repo): Extension<Arc<SRR>>,
    Extension(_geo_restriction_repo): Extension<Arc<GR>>,
    Extension(team_id): Extension<Uuid>,
    Path(crawl_id): Path<Uuid>,
) -> impl IntoResponse
where
    CR: CrawlRepository + 'static,
    TR: TaskRepository + 'static,
    WR: WebhookRepository + 'static,
    SRR: ScrapeResultRepository + 'static,
    GR: GeoRestrictionRepository + 'static,
{
    let use_case = CrawlUseCase::new(
        crawl_repo,
        task_repo,
        webhook_repo,
        scrape_result_repo,
        _geo_restriction_repo.clone(),
        Arc::new(TeamService::new(
            GeoLocationService::new(),
            _geo_restriction_repo.clone(),
        )),
    );

    match use_case.cancel_crawl(crawl_id, team_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            (status, Json(json!({ "error": msg }))).into_response()
        }
    }
}

impl From<CrawlUseCaseError> for (StatusCode, String) {
    fn from(err: CrawlUseCaseError) -> Self {
        match err {
            CrawlUseCaseError::ValidationError(msg) => (StatusCode::BAD_REQUEST, msg),
            CrawlUseCaseError::Repository(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            CrawlUseCaseError::NotFound => (StatusCode::NOT_FOUND, "Crawl not found".to_string()),
            CrawlUseCaseError::Anyhow(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        }
    }
}
