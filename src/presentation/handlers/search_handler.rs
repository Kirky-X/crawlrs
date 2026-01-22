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
    application::dto::search_request::{SearchRequestDto, SearchResponseDto, SearchResultDto},
    common::constants::crawl_task,
    domain::{
        repositories::task_repository::TaskRepository,
        services::rate_limiting_service::{RateLimitResult, RateLimitingService},
        services::search_service::{SearchQuery, SearchServiceError, SearchServiceTrait},
    },
    presentation::handlers::response_builder::{error_response, success_response},
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
            return error_response(
                StatusCode::TOO_MANY_REQUESTS,
                format!("Rate limit exceeded: {}", reason),
            );
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

    let sync_wait_ms = payload
        .sync_wait_ms
        .unwrap_or(crawl_task::DEFAULT_TIMEOUT_MS as u32);

    // 将 DTO 转换为领域参数
    let search_query = SearchQuery {
        query: payload.query,
        limit: payload.limit,
        lang: payload.lang,
        country: payload.country,
        engine: payload.engine,
        sources: payload.sources,
        crawl_results: payload.crawl_results,
        crawl_config: payload.crawl_config.map(|c| {
            crate::domain::services::search_service::SearchCrawlConfig {
                max_depth: c.max_depth,
                include_patterns: c.include_patterns,
                exclude_patterns: c.exclude_patterns,
                strategy: c.strategy.unwrap_or_else(|| "bfs".to_string()),
                crawl_delay_ms: c.crawl_delay_ms,
                max_concurrency: c.max_concurrency.unwrap_or(10),
                headers: c.headers,
                proxy: c.proxy,
                extraction_rules: c.extraction_rules,
            }
        }),
    };

    // 使用注入的SearchService
    match search_service.search(team_id, search_query).await {
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
                                    crawl_task::BASE_POLL_INTERVAL_MS,
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

            // 将领域响应转换为 DTO
            let response_dto = SearchResponseDto {
                query: response.query,
                results: response
                    .results
                    .into_iter()
                    .map(|r| SearchResultDto {
                        title: r.title,
                        url: r.url,
                        description: r.description,
                        engine: Some(r.engine),
                    })
                    .collect(),
                crawl_id: response.crawl_id,
                credits_used: response.credits_used,
            };

            success_response(StatusCode::OK, response_dto)
        }
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            error_response(status, msg)
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
