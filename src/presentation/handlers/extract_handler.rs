// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use axum::{http::StatusCode, response::IntoResponse, Extension, Json};
use serde_json::json;
use uuid::Uuid;

use crate::application::dto::extract_request::ExtractRequestDto;
use crate::config::settings::Settings;
use crate::domain::models::task::{Task, TaskType};
use crate::queue::task_queue::TaskQueue;
use std::sync::Arc;

pub async fn extract(
    Extension(queue): Extension<Arc<dyn TaskQueue>>,
    Extension(_settings): Extension<Arc<Settings>>,
    Json(payload): Json<ExtractRequestDto>,
) -> impl IntoResponse {
    // Validate the request
    if payload.urls.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "At least one URL is required"
            })),
        )
            .into_response();
    }

    if payload.prompt.is_none() && payload.schema.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "Either prompt or schema is required"
            })),
        )
            .into_response();
    }

    // Create a task for async extraction
    let team_id = Uuid::nil(); // TODO: Get from auth
    let task = Task::new(
        TaskType::Extract,
        team_id,
        payload.urls.first().unwrap().clone(), // Use first URL as primary
        serde_json::to_value(&payload).unwrap_or_default(),
    );

    match queue.enqueue(task.clone()).await {
        Ok(_) => {
            let response = json!({
                "success": true,
                "id": task.id,
                "status": "pending"
            });
            (StatusCode::ACCEPTED, Json(response)).into_response()
        }
        Err(e) => {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": e.to_string()
                })),
            )
                .into_response()
        }
    }
}
