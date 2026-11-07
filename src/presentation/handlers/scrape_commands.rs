// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Crawl command handlers — POST/DELETE scrape mutation operations.

use axum::{
    extract::ConnectInfo,
    extract::{Extension, Json, Path},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use log::error;
use std::sync::Arc;

use crate::{
    application::dto::scrape_request::ScrapeRequestDto,
    application::dto::scrape_response::{CancelScrapeResponseDto, ScrapeResponseDto},
    common::constants::crawl_task::MAX_SYNC_WAIT_MS,
    domain::models::{Task, TaskType},
    domain::repositories::geo_restriction_repository::GeoRestrictionRepository,
    domain::repositories::task_repository::TaskRepository,
    domain::services::rate_limiting_service::RateLimitingService,
    domain::services::team_service::TeamService,
    i18n::{I18nBundle, Locale},
    presentation::extractors::CrawlRsDeps,
    presentation::handlers::credits::{CreditCharge, CreditFlow},
    presentation::handlers::response_builder::{
        errors, errors_locale, json_rejection_response, success_response, ApiResponse,
    },
    presentation::handlers::task_queries::handle_sync_wait_and_get_status,
    presentation::handlers::{check_ssrf_url, sync_wait_status_code},
    presentation::helpers::rate_limit_helper::check_rate_limit,
    presentation::middleware::auth_middleware::AuthState,
    queue::task_queue::TaskQueue,
};
use std::net::SocketAddr;

/// Create a new scrape task.
///
/// # Arguments
///
/// * `deps` - Aggregated application dependencies (`CrawlRsDeps`).
/// * `payload` - Scrape request DTO.
///
/// # Errors
///
/// Returns 422 if `sync_wait_ms` exceeds the maximum, 400 for SSRF
/// violations, 402 for insufficient credits, 500 for enqueue failure.
pub async fn create_scrape(
    CrawlRsDeps {
        queue,
        settings: _settings,
        task_repo: task_repository,
        rate_limiting_service,
        auth_state,
    }: CrawlRsDeps,
    // scrape 入口补齐地理限制检查。geo repo / team_service 以 Option 注入——
    // teams-off（Extension 未装配）时为 None，跳过检查，行为与单租户降级一致。
    Extension(geo_restriction_repo): Extension<Option<Arc<dyn GeoRestrictionRepository>>>,
    Extension(team_service): Extension<Option<Arc<TeamService>>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    payload: Result<Json<ScrapeRequestDto>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    // 提取器级拒绝（非法 JSON / 字段缺失）映射为统一包封，避免纯文本响应
    let Json(payload) = match payload {
        Ok(parsed) => parsed,
        Err(ref rej) => return json_rejection_response(rej),
    };
    let team_id = auth_state.team_id;

    if let Some(response) = preflight_scrape(
        &payload,
        rate_limiting_service.as_ref(),
        &auth_state,
        geo_restriction_repo.as_ref(),
        team_service.as_ref(),
        addr.ip(),
    )
    .await
    {
        return response;
    }

    let task = build_scrape_task(&payload, team_id, auth_state.api_key_id);

    let sync_wait_ms = payload.sync_wait_ms.unwrap_or(0);

    enqueue_scrape(
        queue.as_ref(),
        task,
        task_repository.as_ref(),
        rate_limiting_service.as_ref(),
        team_id,
        &payload,
        sync_wait_ms,
    )
    .await
}

/// 构造 scrape 任务。
fn build_scrape_task(
    payload: &ScrapeRequestDto,
    team_id: crate::domain::id::Id,
    api_key_id: crate::domain::id::Id,
) -> Task {
    Task::new(
        crate::domain::id::Id::generate(),
        TaskType::Scrape,
        team_id,
        api_key_id,
        payload.url.clone(),
        serde_json::to_value(payload).unwrap_or_default(),
    )
}

/// scrape 前置校验与计费：sync_wait 范围 → 限流 → SSRF → 地理限制 → 配额扣减。
///
/// 返回 `Some(response)` 表示请求必须中止。
async fn preflight_scrape(
    payload: &ScrapeRequestDto,
    rate_limiting_service: &dyn RateLimitingService,
    auth_state: &AuthState,
    geo_restriction_repo: Option<&Arc<dyn GeoRestrictionRepository>>,
    team_service: Option<&Arc<TeamService>>,
    client_ip: std::net::IpAddr,
) -> Option<Response> {
    let team_id = auth_state.team_id;
    let api_key_id = auth_state.api_key_id;

    // 验证 sync_wait_ms 范围
    if let Some(ms) = payload.sync_wait_ms {
        if ms > MAX_SYNC_WAIT_MS {
            return Some(errors::unprocessable_entity(format!(
                "sync_wait_ms must be <= {}",
                MAX_SYNC_WAIT_MS
            )));
        }
    }

    // 1. 检查限流（架构限流必须在 SSRF 之前，避免恶意请求触发异步 DNS 解析消耗资源）
    // 性能直接传 `crate::domain::id::Id`（实现 Display），由 helper 内部按需 to_string，
    // 消除 handler 中的中间变量分配。
    if let Err(response) = check_rate_limit(rate_limiting_service, api_key_id, "/v1/scrape").await {
        return Some(response);
    }

    // 2. SSRF 验证 - 使用完整的异步 DNS 验证（目标 URL / proxy / webhook）
    if let Some(response) = check_scrape_ssrf_targets(payload, team_id, api_key_id).await {
        return Some(response);
    }

    // 2.7 地理限制检查（scrape 入口此前缺失，与 extract/crawl 对齐）
    if let Some(response) =
        enforce_scrape_geo_restrictions(geo_restriction_repo, team_service, team_id, client_ip)
            .await
    {
        return Some(response);
    }

    CreditFlow::new(rate_limiting_service, team_id)
        .deduct(CreditCharge::Scrape { url: &payload.url })
        .await
}

/// 入队并构造成功 / 失败响应（失败时补偿退还已扣配额）。
async fn enqueue_scrape(
    queue: &dyn TaskQueue,
    task: Task,
    task_repository: &dyn TaskRepository,
    rate_limiting_service: &dyn RateLimitingService,
    team_id: crate::domain::id::Id,
    payload: &ScrapeRequestDto,
    sync_wait_ms: u32,
) -> Response {
    match queue.enqueue(task.clone()).await {
        Ok(_) => respond_scrape_enqueued(task_repository, task, team_id, sync_wait_ms).await,
        Err(e) => {
            error!(
                "Failed to enqueue task for team {}: {}. Payload: {:?}",
                team_id, e, payload
            );
            // 补偿：入队失败时退还已扣的 1 credit，避免客户积分静默丢失
            CreditFlow::new(rate_limiting_service, team_id)
                .refund(CreditCharge::Scrape { url: &payload.url }, Some(task.id))
                .await;
            errors::internal_server_error(e.to_string())
        }
    }
}

/// 2. SSRF 防护 (CWE-918)：目标 URL、options.proxy、任务级回调 URL 依次校验。
///
/// webhook URL 会由 webhook 服务在任务完成后主动 POST，
/// 若指向内网则形成以平台为跳板的 SSRF（投递侧另有 sender 级守卫兜底）。
async fn check_scrape_ssrf_targets(
    payload: &ScrapeRequestDto,
    team_id: crate::domain::id::Id,
    api_key_id: crate::domain::id::Id,
) -> Option<Response> {
    if let Some(response) = check_ssrf_url(&payload.url, team_id, api_key_id).await {
        return Some(response);
    }

    // 2.5 SSRF 防护 (CWE-918)：验证 options.proxy 不指向内部网络
    if let Some(ref options) = payload.options {
        if let Some(ref proxy_url) = options.proxy {
            if let Some(response) = check_ssrf_url(proxy_url, team_id, api_key_id).await {
                return Some(response);
            }
        }
    }

    // 2.6 SSRF 防护 (CWE-918)：任务级回调 URL 与目标 URL 同等对待。
    if let Some(ref webhook_url) = payload.webhook {
        if let Some(response) = check_ssrf_url(webhook_url, team_id, api_key_id).await {
            return Some(response);
        }
    }

    None
}

/// 2.7 地理限制检查。
///
/// geo repo / team_service 以 Option 注入——teams-off（Extension 未装配）时为 None，
/// 跳过检查，行为与单租户降级一致。
async fn enforce_scrape_geo_restrictions(
    geo_restriction_repo: Option<&Arc<dyn GeoRestrictionRepository>>,
    team_service: Option<&Arc<TeamService>>,
    team_id: crate::domain::id::Id,
    client_ip: std::net::IpAddr,
) -> Option<Response> {
    let (geo_repo, team_service) = match (geo_restriction_repo, team_service) {
        (Some(geo_repo), Some(team_service)) => (geo_repo, team_service),
        _ => return None,
    };

    let client_ip = client_ip.to_string();
    let restrictions = match geo_repo.get_team_restrictions(team_id).await {
        Ok(r) => r,
        Err(e) => {
            error!("Failed to get team restrictions: {:?}", e);
            return Some(errors::internal_server_error(
                "Failed to validate geographic access",
            ));
        }
    };

    match team_service
        .validate_geographic_restriction(team_id, &client_ip, &restrictions)
        .await
    {
        Ok(crate::domain::services::team_service::GeoRestrictionResult::Allowed) => {
            if let Err(e) = geo_repo
                .log_geo_restriction_action(
                    team_id,
                    &client_ip,
                    "",
                    "ALLOWED",
                    "Scrape request - geographic restriction check passed",
                )
                .await
            {
                error!("Failed to log geographic restriction action: {:?}", e);
            }
            None
        }
        Ok(crate::domain::services::team_service::GeoRestrictionResult::Denied(reason)) => {
            if let Err(e) = geo_repo
                .log_geo_restriction_action(team_id, &client_ip, "", "DENIED", &reason)
                .await
            {
                error!("Failed to log geographic restriction action: {:?}", e);
            }
            Some(errors::forbidden(reason))
        }
        Err(e) => {
            error!("Geographic restriction validation error: {:?}", e);
            Some(errors::internal_server_error(
                "Failed to validate geographic access",
            ))
        }
    }
}

/// 入队成功后按 sync_wait_ms 等待任务状态并构造 201/202 响应。
async fn respond_scrape_enqueued(
    task_repository: &dyn TaskRepository,
    task: Task,
    team_id: crate::domain::id::Id,
    sync_wait_ms: u32,
) -> Response {
    // 使用公共函数处理同步等待
    let wait_result =
        handle_sync_wait_and_get_status(task_repository, &[task.id], team_id, sync_wait_ms)
            .await
            .unwrap_or({
                crate::presentation::handlers::task_queries::SyncWaitResult {
                    waited_time_ms: 0,
                    is_timeout: false,
                }
            });

    let response = ScrapeResponseDto {
        id: task.id,
        url: task.url,
        credits_used: 1,
        // 入队响应不携带捕获数据（任务尚未执行）
        captured_responses: None,
    };

    let status_code = sync_wait_status_code(sync_wait_ms, wait_result.is_timeout);

    success_response(status_code, response)
}

/// Cancel a scrape task by ID.
///
/// # Arguments
///
/// * `id` - UUID of the task to cancel.
/// * `repository` - Task repository.
/// * `auth_state` - Authenticated caller state.
/// * `locale` / `bundle` - i18n resources.
///
/// # Errors
///
/// Returns 403 if the caller does not own the task, 404 if not found.
pub async fn cancel_scrape(
    Path(id): Path<crate::domain::id::Id>,
    Extension(repository): Extension<Arc<dyn TaskRepository>>,
    Extension(auth_state): Extension<AuthState>,
    Extension(locale): Extension<Locale>,
    Extension(bundle): Extension<Arc<I18nBundle>>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    match repository.find_by_id(id).await {
        Ok(Some(task)) => {
            if task.team_id != team_id {
                return errors_locale::forbidden(&locale, &bundle, "api-access-denied");
            }

            match repository.mark_cancelled(id).await {
                Ok(_) => {
                    let response = CancelScrapeResponseDto {
                        message: crate::i18n::t(&locale, &bundle, "api-scrape-cancelled"),
                    };
                    (StatusCode::OK, Json(ApiResponse::success(response))).into_response()
                }
                Err(e) => {
                    error!("Failed to cancel task {}: {}", id, e);
                    errors_locale::internal_server_error(&locale, &bundle, "api-internal-error")
                }
            }
        }
        Ok(None) => errors_locale::not_found(&locale, &bundle, "api-task-not-found"),
        Err(e) => {
            error!("Failed to get task {} for cancellation: {}", id, e);
            errors_locale::internal_server_error(&locale, &bundle, "api-internal-error")
        }
    }
}
