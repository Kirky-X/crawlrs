// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use axum::{
    extract::{Extension, Json, Path},
    http::StatusCode,
    response::IntoResponse,
};
use std::sync::Arc;
use tracing::{error, warn};
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
    Extension(team_id): Extension<Uuid>,
    Extension(redis_client): Extension<RedisClient>,
    Extension(settings): Extension<Arc<Settings>>,
    Json(payload): Json<ScrapeRequestDto>,
) -> impl IntoResponse {
    // Parse request body happens in the extractor, so if it fails, it's handled by Axum's default rejection handler.
    // However, since we are doing manual concurrency management before parsing (in my thought process, but actually here Json is extracted FIRST),
    // we need to be careful. Axum extracts arguments in order.
    // If Json parsing fails, this handler is never called, so we don't increment the counter.
    // The previous implementation assumed we were manually parsing body, but here we use Json extractor.
    // So the counter increment happens AFTER successful parsing.

    let team_concurrency_key = format!("team:{}:active_jobs", team_id);
    let team_concurrency_limit_key = format!("team:{}:concurrency_limit", team_id);

    // Increment active jobs counter
    let current_active_jobs = match redis_client.incr(&team_concurrency_key).await {
        Ok(val) => val,
        Err(e) => {
            error!(
                "Failed to increment active jobs for team {}: {}",
                team_id, e
            );
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "success": false,
                    "error": "Failed to process request due to internal error."
                })),
            )
                .into_response();
        }
    };

    // Get concurrency limit (default to 10 if not set)
    let concurrency_limit: i64 = match redis_client.get(&team_concurrency_limit_key).await {
        Ok(Some(limit_str)) => limit_str
            .parse()
            .unwrap_or(settings.concurrency.default_team_limit),
        Ok(None) => settings.concurrency.default_team_limit, // Default limit from settings
        Err(e) => {
            error!(
                "Failed to get concurrency limit for team {}: {}",
                team_id, e
            );
            // If we can't get the limit, we should probably err on the side of caution
            // and decrement the counter to avoid false positives.
            if let Err(decr_err) = redis_client.decr(&team_concurrency_key).await {
                error!(
                    "Failed to decrement active jobs after limit check error: {}",
                    decr_err
                );
            }
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({
                    "success": false,
                    "error": "Failed to process request due to internal error."
                })),
            )
                .into_response();
        }
    };

    if current_active_jobs > concurrency_limit {
        warn!(
            "Team {} exceeded concurrency limit. Current: {}, Limit: {}",
            team_id, current_active_jobs, concurrency_limit
        );
        // Decrement the counter as this request is rejected
        if let Err(e) = redis_client.decr(&team_concurrency_key).await {
            error!(
                "Failed to decrement active jobs for team {} after exceeding limit: {}",
                team_id, e
            );
        }
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(serde_json::json!({
                "success": false,
                "error": "Too many concurrent scrape requests for this team."
            })),
        )
            .into_response();
    }

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
            // If enqueuing fails, decrement the counter
            if let Err(decr_err) = redis_client.decr(&team_concurrency_key).await {
                error!(
                    "Failed to decrement active jobs after enqueue failure: {}",
                    decr_err
                );
            }
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
