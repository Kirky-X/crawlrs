// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Scrape query handlers — GET scrape status operations.

use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use log::error;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    application::dto::scrape_response::{ScrapeResultDto, ScrapeStatusResponseDto},
    domain::models::TaskStatus,
    domain::repositories::{
        scrape_result_repository::ScrapeResultRepository, task_repository::TaskRepository,
    },
    i18n::{I18nBundle, Locale},
    presentation::handlers::response_builder::{errors_locale, ApiResponse},
    presentation::middleware::auth_middleware::AuthState,
};

/// Retrieve scrape status and results by task ID.
///
/// # Arguments
///
/// * `id` - UUID of the scrape task.
/// * `task_repository` - Task repository.
/// * `result_repository` - Scrape result repository.
/// * `auth_state` - Authenticated caller state.
/// * `locale` / `bundle` - i18n resources.
///
/// # Errors
///
/// Returns 403 if the caller does not own the task, 404 if not found.
pub async fn get_scrape_status(
    Path(id): Path<Uuid>,
    Extension(task_repository): Extension<Arc<dyn TaskRepository>>,
    Extension(result_repository): Extension<Arc<dyn ScrapeResultRepository>>,
    Extension(auth_state): Extension<AuthState>,
    Extension(locale): Extension<Locale>,
    Extension(bundle): Extension<Arc<I18nBundle>>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    match task_repository.find_by_id(id).await {
        Ok(Some(task)) => {
            if task.team_id != team_id {
                return errors_locale::forbidden(&locale, &bundle, "api-access-denied");
            }

            // Fetch scrape result if task is completed
            let result_data = if task.status == TaskStatus::Completed {
                match result_repository.find_by_task_id(task.id).await {
                    Ok(Some(result)) => {
                        let mut dto = ScrapeResultDto {
                            content: result.content,
                            status_code: result.status_code as u16,
                            content_type: Some(result.content_type),
                            response_time_ms: result.response_time_ms,
                            headers: Some(result.headers),
                            meta_data: Some(result.meta_data),
                            screenshot: result.screenshot,
                            created_at: result.created_at.naive_utc(),
                        };
                        // T010: redact sensitive headers before returning to client
                        dto.filter_sensitive_headers();
                        Some(dto)
                    }
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
                status: task.status.to_string(),
                url: task.url,
                created_at: task.created_at.naive_utc(),
                completed_at: task.completed_at.map(|dt| dt.naive_utc()),
                result: result_data,
                metadata: task.payload.get("metadata").cloned(),
                error: if task.status == TaskStatus::Failed {
                    task.payload
                        .get("error")
                        .and_then(|e| e.as_str())
                        .map(|s| s.to_string())
                        .or(Some(crate::i18n::t(&locale, &bundle, "api-task-failed")))
                } else {
                    None
                },
            };

            (StatusCode::OK, Json(ApiResponse::success(response))).into_response()
        }
        Ok(None) => errors_locale::not_found(&locale, &bundle, "api-task-not-found"),
        Err(e) => {
            error!("Failed to get task status {}: {}", id, e);
            errors_locale::internal_server_error(&locale, &bundle, "api-internal-error")
        }
    }
}
