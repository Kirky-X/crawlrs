// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use axum::{
    extract::{Extension, Json, Path},
    http::StatusCode,
    response::IntoResponse,
};
use std::sync::Arc;
use tracing::error;
use url::Url;
use uuid::Uuid;
use validator::Validate;

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
    presentation::handlers::task_handler::wait_for_tasks_completion,
    queue::task_queue::TaskQueue,
};

pub async fn create_scrape(
    Extension(queue): Extension<Arc<dyn TaskQueue>>,
    Extension(_redis_client): Extension<RedisClient>,
    Extension(_settings): Extension<Arc<Settings>>,
    Extension(task_repository): Extension<Arc<TaskRepositoryImpl>>,
    Extension(rate_limiting_service): Extension<Arc<dyn RateLimitingService>>,
    Extension(team_id): Extension<Uuid>,
    Json(payload): Json<ScrapeRequestDto>,
) -> impl IntoResponse {
    if let Err(e) = payload.validate() {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            Json(serde_json::json!({
                "success": false,
                "error": e.to_string()
            })),
        )
            .into_response();
    }

    if is_internal_url(&payload.url) {
        // Allow disabling SSRF protection via environment variable (for testing)
        if std::env::var("CRAWLRS_DISABLE_SSRF_PROTECTION").unwrap_or_default() != "true" {
            return (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({
                    "success": false,
                    "error": "SSRF protection: Internal URLs are not allowed"
                })),
            )
                .into_response();
        }
    }

    // 1. 检查限流
    match rate_limiting_service
        .check_rate_limit("default_api_key", "/v1/scrape")
        .await
    {
        Ok(RateLimitResult::Denied { reason }) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(serde_json::json!({
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
    match rate_limiting_service
        .check_and_deduct_quota(
            team_id,
            1, // 每个抓取消耗 1 Credit
            crate::domain::models::credits::CreditsTransactionType::Scrape,
            format!("Scrape URL: {}", payload.url),
            None, // 任务尚未创建
        )
        .await
    {
        Ok(_) => {}
        Err(e) => {
            error!("Quota check failed for team {}: {}", team_id, e);
            return (
                StatusCode::PAYMENT_REQUIRED,
                Json(serde_json::json!({
                    "success": false,
                    "error": e.to_string()
                })),
            )
                .into_response();
        }
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

            (status_code, Json(response)).into_response()
        }
        Err(e) => {
            error!(
                "Failed to enqueue task for team {}: {}. Payload: {:?}",
                team_id, e, payload
            );
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "success": false,
                    "error": e.to_string()
                })),
            )
                .into_response()
        }
    }
}

pub async fn cancel_scrape(
    Path(id): Path<Uuid>,
    Extension(repository): Extension<Arc<TaskRepositoryImpl>>,
    Extension(team_id): Extension<Uuid>,
) -> impl IntoResponse {
    match repository.find_by_id(id).await {
        Ok(Some(task)) => {
            if task.team_id != team_id {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({
                        "success": false,
                        "error": "Access denied"
                    })),
                )
                    .into_response();
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
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(serde_json::json!({
                            "success": false,
                            "error": "Internal server error"
                        })),
                    )
                        .into_response()
                }
            }
        }
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "success": false,
                "error": "Task not found"
            })),
        )
            .into_response(),
        Err(e) => {
            error!("Failed to get task {} for cancellation: {}", id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "success": false,
                    "error": "Internal server error"
                })),
            )
                .into_response()
        }
    }
}

fn is_internal_url(url_str: &str) -> bool {
    if let Ok(url) = Url::parse(url_str) {
        if let Some(host) = url.host_str() {
            if host == "localhost" || host == "127.0.0.1" || host == "::1" {
                return true;
            }
            // Basic private IP check (simplified)
            if host.starts_with("192.168.")
                || host.starts_with("10.")
                || host.starts_with("169.254.")
            {
                return true;
            }
            if host.starts_with("172.") {
                // Check 172.16.0.0/12
                if let Some(second_octet) = host.split('.').nth(1) {
                    if let Ok(num) = second_octet.parse::<u8>() {
                        if (16..=31).contains(&num) {
                            return true;
                        }
                    }
                }
            }
        }
    }
    false
}

pub async fn get_scrape_status(
    Path(id): Path<Uuid>,
    Extension(task_repository): Extension<Arc<TaskRepositoryImpl>>,
    Extension(result_repository): Extension<Arc<ScrapeResultRepositoryImpl>>,
    Extension(team_id): Extension<Uuid>,
) -> impl IntoResponse {
    match task_repository.find_by_id(id).await {
        Ok(Some(task)) => {
            if task.team_id != team_id {
                return (
                    StatusCode::FORBIDDEN,
                    Json(serde_json::json!({
                        "success": false,
                        "error": "Access denied"
                    })),
                )
                    .into_response();
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
        Ok(None) => (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "success": false,
                "error": "Task not found"
            })),
        )
            .into_response(),
        Err(e) => {
            error!("Failed to get task status {}: {}", id, e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "success": false,
                    "error": "Internal server error"
                })),
            )
                .into_response()
        }
    }
}
