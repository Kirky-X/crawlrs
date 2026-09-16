// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl command handlers — POST/DELETE scrape mutation operations.

use axum::{
    extract::ConnectInfo,
    extract::{Extension, Json, Path},
    http::StatusCode,
    response::IntoResponse,
};
use log::error;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    application::dto::scrape_request::ScrapeRequestDto,
    application::dto::scrape_response::{CancelScrapeResponseDto, ScrapeResponseDto},
    common::constants::crawl_task::MAX_SYNC_WAIT_MS,
    domain::models::{Task, TaskType},
    domain::repositories::geo_restriction_repository::GeoRestrictionRepository,
    domain::repositories::task_repository::TaskRepository,
    domain::services::team_service::TeamService,
    i18n::{I18nBundle, Locale},
    presentation::extractors::AppDeps,
    presentation::handlers::response_builder::{
        errors, errors_locale, json_rejection_response, success_response, ApiResponse,
    },
    presentation::handlers::task_handler::handle_sync_wait_and_get_status,
    presentation::handlers::{check_ssrf_url, sync_wait_status_code},
    presentation::helpers::rate_limit_helper::check_rate_limit,
    presentation::middleware::auth_middleware::AuthState,
};
use std::net::SocketAddr;

/// Create a new scrape task.
///
/// # Arguments
///
/// * `deps` - Aggregated application dependencies (`AppDeps`).
/// * `payload` - Scrape request DTO.
///
/// # Errors
///
/// Returns 422 if `sync_wait_ms` exceeds the maximum, 400 for SSRF
/// violations, 402 for insufficient credits, 500 for enqueue failure.
pub async fn create_scrape(
    AppDeps {
        queue,
        settings: _settings,
        task_repo: task_repository,
        rate_limiting_service,
        auth_state,
    }: AppDeps,
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

    // 验证 sync_wait_ms 范围
    if let Some(ms) = payload.sync_wait_ms {
        if ms > MAX_SYNC_WAIT_MS {
            return errors::unprocessable_entity(format!(
                "sync_wait_ms must be <= {}",
                MAX_SYNC_WAIT_MS
            ));
        }
    }

    // 1. 检查限流（架构限流必须在 SSRF 之前，避免恶意请求触发异步 DNS 解析消耗资源）
    // 性能直接传 `Uuid`（实现 Display），由 helper 内部按需 to_string，
    // 消除 handler 中的中间变量分配。
    if let Err(response) = check_rate_limit(
        rate_limiting_service.as_ref(),
        auth_state.api_key_id,
        "/v1/scrape",
    )
    .await
    {
        return response;
    }

    // 2. SSRF 验证 - 使用完整的异步 DNS 验证
    if let Some(response) = check_ssrf_url(&payload.url, team_id, auth_state.api_key_id).await {
        return response;
    }

    // 2.5 SSRF 防护 (CWE-918)：验证 options.proxy 不指向内部网络
    if let Some(ref options) = payload.options {
        if let Some(ref proxy_url) = options.proxy {
            if let Some(response) = check_ssrf_url(proxy_url, team_id, auth_state.api_key_id).await
            {
                return response;
            }
        }
    }

    // 2.6 SSRF 防护 (CWE-918)：任务级回调 URL 与目标 URL 同等对待。
    // webhook URL 会由 webhook 服务在任务完成后主动 POST，
    // 若指向内网则形成以平台为跳板的 SSRF（投递侧另有 sender 级守卫兜底）。
    if let Some(ref webhook_url) = payload.webhook {
        if let Some(response) = check_ssrf_url(webhook_url, team_id, auth_state.api_key_id).await {
            return response;
        }
    }

    // 2.7 地理限制检查（scrape 入口此前缺失，与 extract/crawl 对齐）
    if let (Some(geo_repo), Some(team_service)) =
        (geo_restriction_repo.as_ref(), team_service.as_ref())
    {
        let client_ip = addr.ip().to_string();
        let restrictions = match geo_repo.get_team_restrictions(team_id).await {
            Ok(r) => r,
            Err(e) => {
                error!("Failed to get team restrictions: {:?}", e);
                return errors::internal_server_error("Failed to validate geographic access");
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
            }
            Ok(crate::domain::services::team_service::GeoRestrictionResult::Denied(reason)) => {
                if let Err(e) = geo_repo
                    .log_geo_restriction_action(team_id, &client_ip, "", "DENIED", &reason)
                    .await
                {
                    error!("Failed to log geographic restriction action: {:?}", e);
                }
                return errors::forbidden(reason);
            }
            Err(e) => {
                error!("Geographic restriction validation error: {:?}", e);
                return errors::internal_server_error("Failed to validate geographic access");
            }
        }
    }

    // 3. 检查配额
    if let Err(e) = rate_limiting_service
        .check_and_deduct_quota(
            team_id,
            1,
            crate::domain::models::CreditsTransactionType::Scrape,
            format!("Scrape URL: {}", payload.url),
            None,
        )
        .await
    {
        error!("Quota check failed for team {}: {}", team_id, e);
        return errors::payment_required(e.to_string());
    }

    let task = Task::new(
        Uuid::new_v4(),
        TaskType::Scrape,
        team_id,
        auth_state.api_key_id,
        payload.url.clone(),
        serde_json::to_value(&payload).unwrap_or_default(),
    );

    let sync_wait_ms = payload.sync_wait_ms.unwrap_or(0);

    match queue.enqueue(task.clone()).await {
        Ok(_) => {
            // 使用公共函数处理同步等待
            let wait_result = handle_sync_wait_and_get_status(
                task_repository.as_ref(),
                &[task.id],
                team_id,
                sync_wait_ms,
            )
            .await
            .unwrap_or({
                crate::presentation::handlers::task_handler::SyncWaitResult {
                    waited_time_ms: 0,
                    is_timeout: false,
                }
            });

            let response = ScrapeResponseDto {
                id: task.id,
                url: task.url,
                credits_used: 1,
            };

            let status_code = sync_wait_status_code(sync_wait_ms, wait_result.is_timeout);

            success_response(status_code, response)
        }
        Err(e) => {
            error!(
                "Failed to enqueue task for team {}: {}. Payload: {:?}",
                team_id, e, payload
            );
            // 补偿：入队失败时退还已扣的 1 credit，避免客户积分静默丢失
            if let Err(refund_err) = rate_limiting_service
                .refund_quota(
                    team_id,
                    1,
                    format!("Refund: enqueue failed for {}", payload.url),
                    Some(task.id),
                )
                .await
            {
                error!(
                    "CRITICAL: Failed to refund 1 credit for team {} after enqueue failure: {}",
                    team_id, refund_err
                );
            }
            errors::internal_server_error(e.to_string())
        }
    }
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
    Path(id): Path<Uuid>,
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

            // Update task status to cancelled
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
