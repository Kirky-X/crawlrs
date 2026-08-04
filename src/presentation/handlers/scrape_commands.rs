// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl command handlers — POST/DELETE scrape mutation operations.

use axum::{
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
    domain::models::{Task, TaskStatus, TaskType},
    domain::repositories::task_repository::TaskRepository,
    i18n::{I18nBundle, Locale},
    presentation::extractors::AppDeps,
    presentation::handlers::response_builder::{
        errors, errors_locale, success_response, ApiResponse,
    },
    presentation::handlers::task_handler::handle_sync_wait_and_get_status,
    presentation::helpers::rate_limit_helper::check_rate_limit,
    presentation::helpers::ssrf::validate_url,
    presentation::middleware::auth_middleware::AuthState,
};

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
    Json(payload): Json<ScrapeRequestDto>,
) -> impl IntoResponse {
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

    // 1. 检查限流（架构 MEDIUM-1：限流必须在 SSRF 之前，避免恶意请求触发异步 DNS 解析消耗资源）
    // 性能 LOW-1：直接传 `Uuid`（实现 Display），由 helper 内部按需 to_string，
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
    match validate_url(&payload.url).await {
        Ok(validated) => {
            log::trace!(
                "URL passed SSRF validation url={} team_id={} resolved_ips={:?}",
                payload.url,
                team_id,
                validated.resolved_ips
            );
        }
        Err(e) => {
            log::warn!(
                "SSRF attack attempt blocked url={} team_id={} api_key_id={} error={}",
                payload.url,
                team_id,
                auth_state.api_key_id,
                e
            );
            return errors::bad_request(format!("SSRF protection: {}", e));
        }
    }

    // 2.5 SSRF 防护 (CWE-918)：验证 options.proxy 不指向内部网络
    if let Some(ref options) = payload.options {
        if let Some(ref proxy_url) = options.proxy {
            if let Err(e) = validate_url(proxy_url).await {
                log::warn!(
                    "SSRF via proxy blocked proxy={} team_id={} api_key_id={} error={}",
                    proxy_url,
                    team_id,
                    auth_state.api_key_id,
                    e
                );
                return errors::bad_request(format!("SSRF protection: proxy URL rejected: {}", e));
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

    let now = chrono::Utc::now();
    let task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id,
        api_key_id: auth_state.api_key_id,
        url: payload.url.clone(),
        payload: serde_json::to_value(&payload).unwrap_or_default(),
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

            // 根据同步等待结果设置响应状态
            let status_code = if sync_wait_ms > 0 {
                if wait_result.is_timeout {
                    StatusCode::ACCEPTED // 同步等待超时
                } else {
                    StatusCode::CREATED // 同步等待完成
                }
            } else {
                StatusCode::CREATED // 异步模式
            };

            success_response(status_code, response)
        }
        Err(e) => {
            error!(
                "Failed to enqueue task for team {}: {}. Payload: {:?}",
                team_id, e, payload
            );
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
