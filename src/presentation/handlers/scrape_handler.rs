// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use axum::{
    extract::{Extension, Json, Path},
    http::StatusCode,
    response::IntoResponse,
};
use std::sync::Arc;
use tracing::error;
use uuid::Uuid;

use crate::{
    application::dto::scrape_request::ScrapeRequestDto,
    application::dto::scrape_response::{
        CancelScrapeResponseDto, ScrapeResponseDto, ScrapeResultDto, ScrapeStatusResponseDto,
    },
    common::constants::crawl_task::MAX_SYNC_WAIT_MS,
    config::settings::Settings,
    domain::models::{Task, TaskStatus, TaskType},
    domain::repositories::{
        scrape_result_repository::ScrapeResultRepository, task_repository::TaskRepository,
    },
    domain::services::rate_limiting_service::{RateLimitResult, RateLimitingService},
    infrastructure::cache::redis_client::RedisClient,
    presentation::handlers::response_builder::{errors, success_response, ApiResponse},
    presentation::handlers::task_handler::handle_sync_wait_and_get_status,
    presentation::helpers::ssrf::is_internal_url,
    presentation::middleware::auth_middleware::AuthState,
    queue::task_queue::TaskQueue,
};

#[allow(clippy::too_many_arguments)]
pub async fn create_scrape(
    Extension(queue): Extension<Arc<dyn TaskQueue>>,
    Extension(_redis_client): Extension<Arc<RedisClient>>,
    Extension(_settings): Extension<Arc<Settings>>,
    Extension(task_repository): Extension<Arc<dyn TaskRepository>>,
    Extension(rate_limiting_service): Extension<Arc<dyn RateLimitingService>>,
    Extension(auth_state): Extension<AuthState>,
    Json(payload): Json<ScrapeRequestDto>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let api_key = auth_state.api_key_id.to_string();

    // 验证 sync_wait_ms 范围
    if let Some(ms) = payload.sync_wait_ms {
        if ms > MAX_SYNC_WAIT_MS {
            return errors::unprocessable_entity(format!(
                "sync_wait_ms must be <= {}",
                MAX_SYNC_WAIT_MS
            ));
        }
    }

    if is_internal_url(&payload.url) {
        tracing::error!("SSRF Protection: Blocking internal URL {}", payload.url);
        return errors::bad_request("SSRF protection: Internal URLs are not allowed");
    }

    // 1. 检查限流
    if let Err(response) =
        check_rate_limit(rate_limiting_service.as_ref(), &api_key, "/v1/scrape").await
    {
        return response;
    }

    // 2. 检查配额
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

    let now = chrono::Utc::now().naive_utc();
    let task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape.to_string(),
        status: TaskStatus::Queued.to_string(),
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

pub async fn cancel_scrape(
    Path(id): Path<Uuid>,
    Extension(repository): Extension<Arc<dyn TaskRepository>>,
    Extension(auth_state): Extension<AuthState>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    match repository.find_by_id(id).await {
        Ok(Some(task)) => {
            if task.team_id != team_id {
                return errors::forbidden("Access denied");
            }

            // Update task status to cancelled
            match repository.mark_cancelled(id).await {
                Ok(_) => {
                    let response = CancelScrapeResponseDto {
                        message: "Scrape task cancelled".to_string(),
                    };
                    (StatusCode::OK, Json(ApiResponse::success(response))).into_response()
                }
                Err(e) => {
                    error!("Failed to cancel task {}: {}", id, e);
                    errors::internal_server_error("Internal server error")
                }
            }
        }
        Ok(None) => errors::not_found("Task not found"),
        Err(e) => {
            error!("Failed to get task {} for cancellation: {}", id, e);
            errors::internal_server_error("Internal server error")
        }
    }
}

pub async fn get_scrape_status(
    Path(id): Path<Uuid>,
    Extension(task_repository): Extension<Arc<dyn TaskRepository>>,
    Extension(result_repository): Extension<Arc<dyn ScrapeResultRepository>>,
    Extension(auth_state): Extension<AuthState>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    match task_repository.find_by_id(id).await {
        Ok(Some(task)) => {
            if task.team_id != team_id {
                return errors::forbidden("Access denied");
            }

            // Fetch scrape result if task is completed
            let result_data = if task.status == TaskStatus::Completed {
                match result_repository.find_by_task_id(task.id).await {
                    Ok(Some(result)) => Some(ScrapeResultDto {
                        content: result.content,
                        status_code: result.status_code,
                        content_type: result.content_type,
                        response_time_ms: result.response_time_ms,
                        headers: result.headers,
                        meta_data: result.meta_data,
                        screenshot: result.screenshot,
                        created_at: result.created_at,
                    }),
                    Ok(None) => {
                        error!("No scrape result found for completed task {}", task.id);
                        None
                    }
                    Err(e) => {
                        error!("Failed to fetch scrape result for task {}: {}", task.id, e);
                        None
                    }
                }
            } else {
                None
            };

            let response = ScrapeStatusResponseDto {
                id: task.id,
                status: task.status,
                url: task.url,
                created_at: task.created_at,
                completed_at: task.completed_at,
                result: result_data,
                metadata: task.payload.get("metadata").cloned(),
                error: if task.status == TaskStatus::Failed {
                    task.payload
                        .get("error")
                        .and_then(|e| e.as_str())
                        .map(|s| s.to_string())
                        .or(Some("Task failed".to_string()))
                } else {
                    None
                },
            };

            (StatusCode::OK, Json(ApiResponse::success(response))).into_response()
        }
        Ok(None) => errors::not_found("Task not found"),
        Err(e) => {
            error!("Failed to get task status {}: {}", id, e);
            errors::internal_server_error("Internal server error")
        }
    }
}
