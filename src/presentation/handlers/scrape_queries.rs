// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Scrape query handlers — GET scrape status operations.

use axum::{
    extract::{Extension, Path},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use log::error;
use std::sync::Arc;

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
    Path(id): Path<crate::domain::id::Id>,
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
            let result_data = fetch_result_dto(result_repository.as_ref(), &task).await;
            // 先取错误信息，避免下方移动 task.url 后仍借用 task
            let error = failure_error_message(&task, &locale, &bundle);

            let response = ScrapeStatusResponseDto {
                id: task.id,
                status: task.status.to_string(),
                url: task.url,
                created_at: task.created_at.naive_utc(),
                completed_at: task.completed_at.map(|dt| dt.naive_utc()),
                result: result_data,
                metadata: task.payload.get("metadata").cloned(),
                error,
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

/// 已完成任务的结果 DTO：读取抓取结果、脱敏敏感 header。
///
/// 非 Completed 任务、结果缺失或读取失败均返回 `None`（失败路径记录 error 日志）。
async fn fetch_result_dto(
    result_repository: &dyn ScrapeResultRepository,
    task: &crate::domain::models::Task,
) -> Option<ScrapeResultDto> {
    if task.status != TaskStatus::Completed {
        return None;
    }
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
            // redact sensitive headers before returning to client
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
}

/// 失败任务的错误信息：优先 payload.error，缺失时回退 i18n 通用失败文案。
fn failure_error_message(
    task: &crate::domain::models::Task,
    locale: &Locale,
    bundle: &I18nBundle,
) -> Option<String> {
    if task.status == TaskStatus::Failed {
        task.payload
            .get("error")
            .and_then(|e| e.as_str())
            .map(|s| s.to_string())
            .or(Some(crate::i18n::t(locale, bundle, "api-task-failed")))
    } else {
        None
    }
}
