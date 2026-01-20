// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use axum::{
    extract::{Extension, Json},
    http::StatusCode,
    response::IntoResponse,
};
use serde_json::json;
use std::sync::Arc;

use crate::{
    application::dto::search_request::SearchRequestDto,
    domain::{
        repositories::{
            task_repository::TaskRepository,
        },
        services::rate_limiting_service::{RateLimitResult, RateLimitingService},
        services::search_service::{SearchServiceError, SearchServiceTrait},
    },
    presentation::handlers::task_handler::wait_for_tasks_completion,
    presentation::middleware::auth_middleware::AuthState,
};
use tracing::error;

/// 处理搜索请求
pub async fn search(
    Extension(search_service): Extension<Arc<dyn SearchServiceTrait>>,
    Extension(task_repo): Extension<Arc<dyn TaskRepository>>,
    Extension(rate_limiting_service): Extension<Arc<dyn RateLimitingService>>,
    Extension(auth_state): Extension<AuthState>,
    Json(payload): Json<SearchRequestDto>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let api_key = auth_state.api_key_id.to_string();
    // 1. 检查限流
    match rate_limiting_service
        .check_rate_limit(&api_key, "/v1/search")
        .await
    {
        Ok(RateLimitResult::Denied { reason }) => {
            return (
                StatusCode::TOO_MANY_REQUESTS,
                Json(json!({
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
                Json(json!({
                    "success": false,
                    "error": "Rate limit exceeded, please retry later",
                    "retry_after_seconds": retry_after_seconds
                })),
            )
                .into_response();
        }
        Err(e) => {
            error!("Rate limiting service error: {}", e);
        }
        _ => {}
    }

    // 2. 检查配额 (SearchService 内部已经处理了配额检查)

    let sync_wait_ms = payload.sync_wait_ms.unwrap_or(5000);

    // 使用注入的SearchService
    match search_service.search(team_id, payload).await {
        Ok(response) => {
            // 如果启用了爬取结果并且有crawl_id，则等待任务完成
            if sync_wait_ms > 0 {
                if let Some(crawl_id) = response.crawl_id {
                    match task_repo.find_by_crawl_id(crawl_id).await {
                        Ok(tasks) => {
                            if !tasks.is_empty() {
                                let task_ids: Vec<uuid::Uuid> =
                                    tasks.iter().map(|task| task.id).collect();
                                match wait_for_tasks_completion(
                                    task_repo.as_ref(),
                                    &task_ids,
                                    team_id,
                                    sync_wait_ms,
                                    1000,
                                )
                                .await
                                {
                                    Ok(_) => {
                                        // 等待成功，可以返回响应
                                    }
                                    Err(e) => {
                                        error!("Failed to wait for task completion: {:?}", e);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            error!("Failed to find tasks for crawl {}: {:?}", crawl_id, e);
                        }
                    }
                }
            }

            (StatusCode::OK, Json(response)).into_response()
        }
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            (status, Json(json!({ "error": msg }))).into_response()
        }
    }
}

impl From<SearchServiceError> for (StatusCode, String) {
    fn from(err: SearchServiceError) -> Self {
        match err {
            SearchServiceError::ValidationError(details) => (StatusCode::BAD_REQUEST, details),
            SearchServiceError::Repository(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            SearchServiceError::CreditsRepository(e) => {
                (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
            }
            SearchServiceError::InsufficientCredits {
                available,
                required,
            } => {
                let details = format!(
                    "Insufficient credits: available {}, required {}",
                    available, required
                );
                (StatusCode::PAYMENT_REQUIRED, details)
            }
            SearchServiceError::SearchEngine(e) => (StatusCode::INTERNAL_SERVER_ERROR, e),
        }
    }
}
