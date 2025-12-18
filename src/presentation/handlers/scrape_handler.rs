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
use uuid::Uuid;
use validator::Validate;
use url::Url;

use crate::{
    application::dto::{scrape_request::ScrapeRequestDto, scrape_response::ScrapeResponseDto},
    config::settings::Settings,
    domain::models::task::{Task, TaskType},
    domain::repositories::{task_repository::TaskRepository, scrape_result_repository::ScrapeResultRepository},
    infrastructure::{
        cache::redis_client::RedisClient, 
        repositories::{task_repo_impl::TaskRepositoryImpl, scrape_result_repo_impl::ScrapeResultRepositoryImpl},
    },
    queue::task_queue::TaskQueue,
};

pub async fn create_scrape(
    Extension(queue): Extension<Arc<dyn TaskQueue>>,
    Extension(_redis_client): Extension<RedisClient>,
    Extension(_settings): Extension<Arc<Settings>>,
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
        return (
            StatusCode::BAD_REQUEST,
            Json(serde_json::json!({
                "success": false,
                "error": "SSRF protection: Internal URLs are not allowed"
            })),
        )
        .into_response();
    }

    // Note: Concurrency limit is now enforced at the Worker level (Execution time)
    // rather than at Submission time. This allows tasks to be queued (Backlog)
    // and retried later if the team's concurrency limit is reached.
    let team_id = Uuid::nil(); // TODO: Get from auth

    let task = Task::new(
        TaskType::Scrape,
        team_id,
        payload.url.clone(),
        serde_json::to_value(&payload).unwrap_or_default(),
    );

    match queue.enqueue(task.clone()).await {
        Ok(_) => {
            let response = ScrapeResponseDto {
                success: true,
                id: task.id,
                url: task.url,
            };
            (StatusCode::CREATED, Json(response)).into_response()
        }
        Err(e) => {
            error!("Failed to enqueue task for team {}: {}", team_id, e);
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
) -> impl IntoResponse {
    let team_id = Uuid::nil(); // TODO: Get from auth
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
            if host.starts_with("192.168.") || host.starts_with("10.") {
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
) -> impl IntoResponse {
    let team_id = Uuid::nil(); // TODO: Get from auth
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
                        Some("Task failed") // Ideally we should store the error message in the task
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
