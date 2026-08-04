// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl command handlers — POST/DELETE crawl mutation operations.

use axum::{
    extract::{ConnectInfo, Extension, Path},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::net::SocketAddr;
use std::sync::Arc;
use uuid::Uuid;

use crate::application::dto::crawl_request::CrawlRequestDto;
use crate::application::use_cases::crawl_use_case::CrawlUseCaseError;
use crate::common::constants::crawl_task::CRAWL_TASK_CREDITS_COST;
use crate::common::constants::crawl_task::DEFAULT_TIMEOUT_MS;
use crate::i18n::{I18nBundle, Locale};
use crate::presentation::handlers::extract_task_ids;
use crate::presentation::handlers::response_builder::errors;
use crate::presentation::handlers::response_builder::{
    error_response, errors_locale, success_response,
};
use crate::presentation::handlers::task_handler::handle_sync_wait_and_get_status;
use crate::presentation::handlers::task_handler::SyncWaitResult;
use crate::presentation::helpers::rate_limit_helper::check_rate_limit;
use crate::presentation::helpers::ssrf::validate_url;
use crate::presentation::middleware::auth_middleware::AuthState;
use crate::presentation::state::CrawlHandlerState;
use log::error;

/// Create a new crawl task.
///
/// # Arguments
///
/// * `state` - Shared crawl handler state (use-case factory).
/// * `auth_state` - Authenticated caller state.
/// * `locale` / `bundle` - i18n resources for error messages.
/// * `addr` - Client socket address (logging).
/// * `payload` - Crawl request DTO.
///
/// # Errors
///
/// Returns 400 for validation errors, 402 for insufficient credits,
/// 500 for internal errors.
pub async fn create_crawl(
    Extension(state): Extension<Arc<CrawlHandlerState>>,
    Extension(auth_state): Extension<AuthState>,
    Extension(locale): Extension<Locale>,
    Extension(bundle): Extension<Arc<I18nBundle>>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<CrawlRequestDto>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let sync_wait_ms = payload.sync_wait_ms.unwrap_or(DEFAULT_TIMEOUT_MS as u32);

    // 验证 config 字段
    if payload.config.max_depth > 5 {
        return errors::unprocessable_entity("max_depth must be between 0 and 5");
    }

    // 1. 检查限流（架构 MEDIUM-1：限流必须在 SSRF 之前，避免恶意请求触发异步 DNS 解析消耗资源）
    if let Err(response) = check_rate_limit(
        state.rate_limiting_service.as_ref(),
        auth_state.api_key_id,
        "/v1/crawl",
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

    // 2.5 SSRF 防护 (CWE-918)：验证 config.proxy 不指向内部网络
    if let Some(ref proxy_url) = payload.config.proxy {
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

    // 3. 检查配额
    if let Err(e) = state
        .rate_limiting_service
        .check_and_deduct_quota(
            team_id,
            CRAWL_TASK_CREDITS_COST,
            crate::domain::models::CreditsTransactionType::Crawl,
            format!("Crawl URL: {}", payload.url),
            None,
        )
        .await
    {
        return errors::payment_required(e.to_string());
    }

    let use_case = state.create_use_case();

    let client_ip = addr.ip().to_string();
    match use_case
        .create_crawl(team_id, auth_state.api_key_id, payload, &client_ip)
        .await
    {
        Ok(crawl) => {
            // 处理同步等待
            let wait_result = if sync_wait_ms > 0 {
                match state.task_repo.find_by_crawl_id(crawl.id).await {
                    Ok(tasks) => {
                        if !tasks.is_empty() {
                            let task_ids = extract_task_ids(&tasks);
                            handle_sync_wait_and_get_status(
                                state.task_repo.as_ref(),
                                &task_ids,
                                team_id,
                                sync_wait_ms,
                            )
                            .await
                            .unwrap_or(SyncWaitResult {
                                waited_time_ms: sync_wait_ms as u64,
                                is_timeout: true,
                            })
                        } else {
                            SyncWaitResult {
                                waited_time_ms: 0,
                                is_timeout: false,
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to find tasks for crawl {}: {:?}", crawl.id, e);
                        SyncWaitResult {
                            waited_time_ms: 0,
                            is_timeout: false,
                        }
                    }
                }
            } else {
                SyncWaitResult {
                    waited_time_ms: 0,
                    is_timeout: false,
                }
            };

            let status_code = if sync_wait_ms > 0 && wait_result.is_timeout {
                StatusCode::ACCEPTED
            } else {
                StatusCode::CREATED
            };

            success_response(status_code, crawl)
        }
        Err(e) => match e {
            CrawlUseCaseError::NotFound => {
                errors_locale::not_found(&locale, &bundle, "api-crawl-not-found")
            }
            e => {
                let (status, msg): (StatusCode, String) = e.into();
                error_response(status, msg)
            }
        },
    }
}

/// Cancel an in-progress crawl task.
///
/// # Arguments
///
/// * `state` - Shared crawl handler state.
/// * `auth_state` - Authenticated caller state.
/// * `locale` / `bundle` - i18n resources.
/// * `crawl_id` - ID of the crawl to cancel.
///
/// # Errors
///
/// Returns 403 if the caller does not own the crawl, 404 if not found,
/// 500 for internal errors.
pub async fn cancel_crawl(
    Extension(state): Extension<Arc<CrawlHandlerState>>,
    Extension(auth_state): Extension<AuthState>,
    Extension(locale): Extension<Locale>,
    Extension(bundle): Extension<Arc<I18nBundle>>,
    Path(crawl_id): Path<Uuid>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let use_case = state.create_use_case();

    match use_case.cancel_crawl(crawl_id, team_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => match e {
            CrawlUseCaseError::NotFound => {
                errors_locale::not_found(&locale, &bundle, "api-crawl-not-found")
            }
            e => {
                let (status, msg): (StatusCode, String) = e.into();
                error_response(status, msg)
            }
        },
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
