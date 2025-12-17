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

use crate::{
    application::dto::{scrape_request::ScrapeRequestDto, scrape_response::ScrapeResponseDto},
    config::settings::Settings,
    domain::models::task::{Task, TaskType},
    domain::repositories::task_repository::TaskRepository,
    infrastructure::{
        cache::redis_client::RedisClient, repositories::task_repo_impl::TaskRepositoryImpl,
    },
    queue::task_queue::{PostgresTaskQueue, TaskQueue},
};

pub async fn create_scrape(
    Extension(queue): Extension<Arc<PostgresTaskQueue<TaskRepositoryImpl>>>,
    Extension(_redis_client): Extension<RedisClient>,
    Extension(_settings): Extension<Arc<Settings>>,
    Json(payload): Json<ScrapeRequestDto>,
) -> impl IntoResponse {
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

pub async fn get_scrape_status(
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

            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "success": true,
                    "id": task.id,
                    "status": task.status,
                    "url": task.url,
                    "created_at": task.created_at,
                    "completed_at": task.completed_at,
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
