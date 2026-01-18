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
    application::dto::{scrape_request::ScrapeRequestDto, scrape_response::ScrapeResponseDto},
    config::settings::Settings,
    domain::models::task::{Task, TaskType},
    domain::repositories::{
        scrape_result_repository::ScrapeResultRepository, task_repository::TaskRepository,
    },
    domain::services::rate_limiting_service::{RateLimitResult, RateLimitingService},
    infrastructure::{
        cache::redis_client::RedisClient,
        repositories::{
            scrape_result_repo_impl::ScrapeResultRepositoryImpl, task_repo_impl::TaskRepositoryImpl,
        },
    },
    presentation::handlers::response_builder::errors,
    presentation::handlers::response_builder::{access_denied, success_response},
    presentation::handlers::task_handler::wait_for_tasks_completion,
    presentation::helpers::ssrf_helper::is_internal_url,
    presentation::middleware::auth_middleware::AuthState,
    queue::task_queue::TaskQueue,
};

#[allow(clippy::too_many_arguments)]
pub async fn create_scrape(
    Extension(queue): Extension<Arc<dyn TaskQueue>>,
    Extension(_redis_client): Extension<Arc<RedisClient>>,
    Extension(_settings): Extension<Arc<Settings>>,
    Extension(task_repository): Extension<Arc<TaskRepositoryImpl>>,
    Extension(rate_limiting_service): Extension<Arc<dyn RateLimitingService>>,
    Extension(auth_state): Extension<AuthState>,
    Json(payload): Json<ScrapeRequestDto>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let api_key = auth_state.api_key_id.to_string();

    // 验证 sync_wait_ms 范围
    if let Some(ms) = payload.sync_wait_ms {
        if ms > 30000 {
            return errors::unprocessable_entity("sync_wait_ms must be <= 30000");
        }
    }

    if is_internal_url(&payload.url) {
        tracing::error!("SSRF Protection: Blocking internal URL {}", payload.url);
        return errors::bad_request("SSRF protection: Internal URLs are not allowed");
    }

    // 1. 检查限流
    match rate_limiting_service
        .check_rate_limit(&api_key, "/v1/scrape")
        .await
    {
        Ok(RateLimitResult::Denied { reason }) => {
            return errors::too_many_requests(format!("Rate limit exceeded: {}", reason));
        }
        Ok(RateLimitResult::RetryAfter {
            retry_after_seconds,
        }) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
                    "success": false,
                    "error": "Rate limit exceeded, please retry later",
                    "retry_after_seconds": retry_after_seconds
                })),
            )
                .into_response();
        }
        Err(e) => {
            error!("Rate limiting service error: {}", e);
            // 降级：如果限流服务出错，允许继续（或根据策略拒绝）
        }
        _ => {}
    }

    // 2. 检查配额
    if let Err(e) = rate_limiting_service
        .check_and_deduct_quota(
            team_id,
            1,
            crate::domain::models::credits::CreditsTransactionType::Scrape,
            format!("Scrape URL: {}", payload.url),
            None,
        )
        .await
    {
        error!("Quota check failed for team {}: {}", team_id, e);
        return errors::payment_required(e.to_string());
    }

    let task = Task::new(
        TaskType::Scrape,
        team_id,
        payload.url.clone(),
        serde_json::to_value(&payload).unwrap_or_default(),
    );

    match queue.enqueue(task.clone()).await {
        Ok(_) => {
            // 处理同步等待逻辑
            let sync_wait_ms = payload.sync_wait_ms.unwrap_or(5000);
            let mut waited_time_ms = 0u64;

            if sync_wait_ms > 0 {
                let wait_start = std::time::Instant::now();

                // 调用智能轮询等待函数
                match wait_for_tasks_completion(
                    task_repository.as_ref(),
                    &[task.id],
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
                        // 即使等待失败，也返回已创建的任务信息
                    }
                }
            }

            let response = ScrapeResponseDto {
                success: true,
                id: task.id,
                url: task.url,
                credits_used: 1,
            };

            // 根据同步等待结果设置响应状态
            let status_code = if sync_wait_ms > 0 {
                if waited_time_ms >= sync_wait_ms as u64 {
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
    Extension(repository): Extension<Arc<TaskRepositoryImpl>>,
    Extension(auth_state): Extension<AuthState>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    match repository.find_by_id(id).await {
        Ok(Some(task)) => {
            if task.team_id != team_id {
                return access_denied();
            }

            // Update task status to cancelled
            match repository.mark_cancelled(id).await {
                Ok(_) => (
                    StatusCode::NO_CONTENT,
                    Json(serde_json::json!({
                        "success": true
                    })),
                )
                    .into_response(),
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
    Extension(task_repository): Extension<Arc<TaskRepositoryImpl>>,
    Extension(result_repository): Extension<Arc<ScrapeResultRepositoryImpl>>,
    Extension(auth_state): Extension<AuthState>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    match task_repository.find_by_id(id).await {
        Ok(Some(task)) => {
            if task.team_id != team_id {
                return access_denied();
            }

            // Fetch scrape result if task is completed
            let mut result_data = None;
            if task.status == crate::domain::models::task::TaskStatus::Completed {
                match result_repository.find_by_task_id(task.id).await {
                    Ok(Some(result)) => {
                        result_data = Some(serde_json::json!({
                            "content": result.content,
                            "status_code": result.status_code,
                            "content_type": result.content_type,
                            "response_time_ms": result.response_time_ms,
                            "headers": result.headers,
                            "meta_data": result.meta_data,
                            "screenshot": result.screenshot,
                            "created_at": result.created_at
                        }));
                    }
                    Ok(None) => {
                        error!("No scrape result found for completed task {}", task.id);
                    }
                    Err(e) => {
                        error!("Failed to fetch scrape result for task {}: {}", task.id, e);
                    }
                }
            }

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "id": task.id,
                    "status": task.status,
                    "url": task.url,
                    "created_at": task.created_at,
                    "completed_at": task.completed_at,
                    "result": result_data,
                    "metadata": task.payload.get("metadata"), // Include metadata from task payload
                    "error": if task.status == crate::domain::models::task::TaskStatus::Failed {
                        task.payload.get("error").and_then(|e| e.as_str()).or(Some("Task failed"))
                    } else {
                        None
                    }
                })),
            )
                .into_response()
        }
        Ok(None) => errors::not_found("Task not found"),
        Err(e) => {
            error!("Failed to get task status {}: {}", id, e);
            errors::internal_server_error("Internal server error")
        }
    }
}
