// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

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
use crate::di::{AppState, AppStateExt};
use crate::presentation::handlers::extract_task_ids;
use crate::presentation::handlers::response_builder::errors;
use crate::presentation::handlers::response_builder::{error_response, success_response};
use crate::presentation::handlers::task_handler::handle_sync_wait_and_get_status;
use crate::presentation::handlers::task_handler::SyncWaitResult;
use crate::presentation::helpers::rate_limit_helper::check_rate_limit;
use crate::presentation::helpers::ssrf::validate_url;
use crate::presentation::middleware::auth_middleware::AuthState;
use crate::presentation::state::CrawlHandlerState;
use tracing::error;

/// 创建新的爬取任务
pub async fn create_crawl(
    Extension(app_state): Extension<Arc<AppState>>,
    Extension(auth_state): Extension<AuthState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<CrawlRequestDto>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let api_key = auth_state.api_key_id.to_string();
    let sync_wait_ms = payload.sync_wait_ms.unwrap_or(DEFAULT_TIMEOUT_MS as u32);

    // 验证 config 字段
    if payload.config.max_depth > 5 {
        return errors::unprocessable_entity("max_depth must be between 0 and 5");
    }

    // SSRF 验证 - 使用完整的异步 DNS 验证
    match validate_url(&payload.url).await {
        Ok(validated) => {
            tracing::debug!(
                target: "security",
                url = %payload.url,
                team_id = %team_id,
                resolved_ips = ?validated.resolved_ips,
                "URL passed SSRF validation"
            );
        }
        Err(e) => {
            tracing::warn!(
                target: "security_audit",
                url = %payload.url,
                team_id = %team_id,
                api_key_id = %auth_state.api_key_id,
                error = %e,
                "SSRF attack attempt blocked"
            );
            return errors::bad_request(&format!("SSRF protection: {}", e));
        }
    }

    // 1. 检查限流
    if let Err(response) = check_rate_limit(
        app_state.rate_limiting_service().as_ref(),
        &api_key,
        "/v1/crawl",
    )
    .await
    {
        return response;
    }

    // 2. 检查配额
    if let Err(e) = app_state
        .rate_limiting_service()
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

    // Create CrawlHandlerState from unified AppState
    let handler_state = CrawlHandlerState::from_app_state(&app_state);
    let use_case = handler_state.create_use_case();

    let client_ip = addr.ip().to_string();
    match use_case
        .create_crawl(team_id, auth_state.api_key_id, payload, &client_ip)
        .await
    {
        Ok(crawl) => {
            // 处理同步等待
            let wait_result = if sync_wait_ms > 0 {
                match app_state.task_repo().find_by_crawl_id(crawl.id).await {
                    Ok(tasks) => {
                        if !tasks.is_empty() {
                            let task_ids = extract_task_ids(&tasks);
                            handle_sync_wait_and_get_status(
                                app_state.task_repo().as_ref(),
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
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            error_response(status, msg)
        }
    }
}

/// 获取爬取任务详情
pub async fn get_crawl(
    Extension(app_state): Extension<Arc<AppState>>,
    Extension(auth_state): Extension<AuthState>,
    Path(crawl_id): Path<Uuid>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let handler_state = CrawlHandlerState::from_app_state(&app_state);
    let use_case = handler_state.create_use_case();

    match use_case.get_crawl(crawl_id, team_id).await {
        Ok(Some(crawl)) => success_response(StatusCode::OK, crawl),
        Ok(None) => errors::not_found("Crawl not found"),
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            error_response(status, msg)
        }
    }
}

/// 获取爬取任务结果
pub async fn get_crawl_results(
    Extension(app_state): Extension<Arc<AppState>>,
    Extension(auth_state): Extension<AuthState>,
    Path(crawl_id): Path<Uuid>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let handler_state = CrawlHandlerState::from_app_state(&app_state);
    let use_case = handler_state.create_use_case();

    match use_case.get_crawl_results(crawl_id, team_id).await {
        Ok(results) => success_response(StatusCode::OK, results),
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            error_response(status, msg)
        }
    }
}

/// 取消进行中的爬取任务
pub async fn cancel_crawl(
    Extension(app_state): Extension<Arc<AppState>>,
    Extension(auth_state): Extension<AuthState>,
    Path(crawl_id): Path<Uuid>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let handler_state = CrawlHandlerState::from_app_state(&app_state);
    let use_case = handler_state.create_use_case();

    match use_case.cancel_crawl(crawl_id, team_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            error_response(status, msg)
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
