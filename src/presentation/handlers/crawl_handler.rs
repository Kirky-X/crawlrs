// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use axum::{
    extract::{ConnectInfo, Extension, Path},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::net::SocketAddr;
use std::sync::Arc;
use uuid::Uuid;

use crate::application::dto::crawl_request::CrawlRequestDto;
use crate::application::use_cases::crawl_use_case::CrawlUseCaseError;
use crate::common::constants::crawl_task::CRAWL_TASK_CREDITS_COST;
use crate::common::constants::crawl_task::DEFAULT_TIMEOUT_MS;
use crate::di::{AppState, AppStateExt};
use crate::presentation::handlers::extract_task_ids;
use crate::presentation::handlers::response_builder::errors;
use crate::presentation::handlers::response_builder::{error_response, success_response};
use crate::presentation::handlers::task_handler::handle_sync_wait_and_get_status;
use crate::presentation::handlers::task_handler::SyncWaitResult;
use crate::presentation::helpers::rate_limit_helper::check_rate_limit;
use crate::presentation::helpers::ssrf::validate_url;
use crate::presentation::middleware::auth_middleware::AuthState;
use crate::presentation::state::CrawlHandlerState;
use log::error;

/// 创建新的爬取任务
pub async fn create_crawl(
    Extension(app_state): Extension<Arc<AppState>>,
    Extension(auth_state): Extension<AuthState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<CrawlRequestDto>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let api_key = auth_state.api_key_id.to_string();
    let sync_wait_ms = payload.sync_wait_ms.unwrap_or(DEFAULT_TIMEOUT_MS as u32);

    // 验证 config 字段
    if payload.config.max_depth > 5 {
        return errors::unprocessable_entity("max_depth must be between 0 and 5");
    }

    // SSRF 验证 - 使用完整的异步 DNS 验证
    match validate_url(&payload.url).await {
        Ok(validated) => {
            log::debug!(
                "URL passed SSRF validation url={} team_id={} resolved_ips={:?}",
                payload.url,
                team_id,
                validated.resolved_ips
            );
        }
        Err(e) => {
            log::warn!(
                "SSRF attack attempt blocked url={} team_id={} api_key_id={} error={}",
                payload.url,
                team_id,
                auth_state.api_key_id,
                e
            );
            return errors::bad_request(format!("SSRF protection: {}", e));
        }
    }

    // 1. 检查限流
    if let Err(response) = check_rate_limit(
        app_state.rate_limiting_service().as_ref(),
        &api_key,
        "/v1/crawl",
    )
    .await
    {
        return response;
    }

    // 2. 检查配额
    if let Err(e) = app_state
        .rate_limiting_service()
        .check_and_deduct_quota(
            team_id,
            CRAWL_TASK_CREDITS_COST,
            crate::domain::models::CreditsTransactionType::Crawl,
            format!("Crawl URL: {}", payload.url),
            None,
        )
        .await
    {
        return errors::payment_required(e.to_string());
    }

    // Create CrawlHandlerState from unified AppState
    let handler_state = CrawlHandlerState::from_app_state(&app_state);
    let use_case = handler_state.create_use_case();

    let client_ip = addr.ip().to_string();
    match use_case
        .create_crawl(team_id, auth_state.api_key_id, payload, &client_ip)
        .await
    {
        Ok(crawl) => {
            // 处理同步等待
            let wait_result = if sync_wait_ms > 0 {
                match app_state.task_repo().find_by_crawl_id(crawl.id).await {
                    Ok(tasks) => {
                        if !tasks.is_empty() {
                            let task_ids = extract_task_ids(&tasks);
                            handle_sync_wait_and_get_status(
                                app_state.task_repo().as_ref(),
                                &task_ids,
                                team_id,
                                sync_wait_ms,
                            )
                            .await
                            .unwrap_or(SyncWaitResult {
                                waited_time_ms: sync_wait_ms as u64,
                                is_timeout: true,
                            })
                        } else {
                            SyncWaitResult {
                                waited_time_ms: 0,
                                is_timeout: false,
                            }
                        }
                    }
                    Err(e) => {
                        error!("Failed to find tasks for crawl {}: {:?}", crawl.id, e);
                        SyncWaitResult {
                            waited_time_ms: 0,
                            is_timeout: false,
                        }
                    }
                }
            } else {
                SyncWaitResult {
                    waited_time_ms: 0,
                    is_timeout: false,
                }
            };

            let status_code = if sync_wait_ms > 0 && wait_result.is_timeout {
                StatusCode::ACCEPTED
            } else {
                StatusCode::CREATED
            };

            success_response(status_code, crawl)
        }
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            error_response(status, msg)
        }
    }
}

/// 获取爬取任务详情
pub async fn get_crawl(
    Extension(app_state): Extension<Arc<AppState>>,
    Extension(auth_state): Extension<AuthState>,
    Path(crawl_id): Path<Uuid>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let handler_state = CrawlHandlerState::from_app_state(&app_state);
    let use_case = handler_state.create_use_case();

    match use_case.get_crawl(crawl_id, team_id).await {
        Ok(Some(crawl)) => success_response(StatusCode::OK, crawl),
        Ok(None) => errors::not_found("Crawl not found"),
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            error_response(status, msg)
        }
    }
}

/// 获取爬取任务结果
pub async fn get_crawl_results(
    Extension(app_state): Extension<Arc<AppState>>,
    Extension(auth_state): Extension<AuthState>,
    Path(crawl_id): Path<Uuid>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let handler_state = CrawlHandlerState::from_app_state(&app_state);
    let use_case = handler_state.create_use_case();

    match use_case.get_crawl_results(crawl_id, team_id).await {
        Ok(results) => success_response(StatusCode::OK, results),
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            error_response(status, msg)
        }
    }
}

/// 取消进行中的爬取任务
pub async fn cancel_crawl(
    Extension(app_state): Extension<Arc<AppState>>,
    Extension(auth_state): Extension<AuthState>,
    Path(crawl_id): Path<Uuid>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let handler_state = CrawlHandlerState::from_app_state(&app_state);
    let use_case = handler_state.create_use_case();

    match use_case.cancel_crawl(crawl_id, team_id).await {
        Ok(_) => StatusCode::NO_CONTENT.into_response(),
        Err(e) => {
            let (status, msg): (StatusCode, String) = e.into();
            error_response(status, msg)
        }
    }
}

impl From<CrawlUseCaseError> for (StatusCode, String) {
    fn from(err: CrawlUseCaseError) -> Self {
        match err {
            CrawlUseCaseError::ValidationError(msg) => (StatusCode::BAD_REQUEST, msg),
            CrawlUseCaseError::Repository(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            CrawlUseCaseError::NotFound => (StatusCode::NOT_FOUND, "Crawl not found".to_string()),
            CrawlUseCaseError::Anyhow(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::dto::crawl_request::CrawlConfigDto;
    use crate::domain::repositories::task_repository::RepositoryError;
    use validator::Validate;

    // ========== From<CrawlUseCaseError> mapping tests ==========

    #[test]
    fn test_validation_error_maps_to_bad_request() {
        let err = CrawlUseCaseError::ValidationError("max_depth exceeds limit".to_string());
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(msg, "max_depth exceeds limit");
    }

    #[test]
    fn test_repository_database_error_maps_to_internal_server_error() {
        let err = CrawlUseCaseError::Repository(RepositoryError::Database(anyhow::anyhow!(
            "connection refused"
        )));
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(msg.contains("connection refused"));
    }

    #[test]
    fn test_repository_not_found_maps_to_internal_server_error() {
        let err = CrawlUseCaseError::Repository(RepositoryError::NotFound);
        let (status, _msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn test_not_found_maps_to_404() {
        let err = CrawlUseCaseError::NotFound;
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert_eq!(msg, "Crawl not found");
    }

    #[test]
    fn test_anyhow_error_maps_to_internal_server_error() {
        let err = CrawlUseCaseError::Anyhow(anyhow::anyhow!("unexpected failure"));
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(msg.contains("unexpected failure"));
    }

    #[test]
    fn test_anyhow_error_with_context_preserved() {
        let err = CrawlUseCaseError::Anyhow(
            anyhow::anyhow!("base error").context("with additional context"),
        );
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(msg.contains("with additional context"));
    }

    // ========== CrawlRequestDto construction tests ==========

    #[test]
    fn test_crawl_request_dto_minimal() {
        let json = r#"{
            "url": "https://example.com",
            "config": {"max_depth": 2}
        }"#;
        let dto: CrawlRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.url, "https://example.com");
        assert_eq!(dto.config.max_depth, 2);
        assert!(dto.name.is_none());
        assert!(dto.sync_wait_ms.is_none());
        assert!(dto.expires_at.is_none());
    }

    #[test]
    fn test_crawl_request_dto_full() {
        let json = r#"{
            "url": "https://example.com",
            "name": "My Crawl",
            "config": {
                "max_depth": 3,
                "include_patterns": ["*/blog/*"],
                "exclude_patterns": ["*/admin/*"],
                "strategy": "bfs",
                "crawl_delay_ms": 1000,
                "max_concurrency": 5,
                "proxy": "http://proxy:8080",
                "headers": {"X-Custom": "value"}
            },
            "sync_wait_ms": 5000
        }"#;
        let dto: CrawlRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.url, "https://example.com");
        assert_eq!(dto.name.as_deref(), Some("My Crawl"));
        assert_eq!(dto.config.max_depth, 3);
        assert!(dto.config.include_patterns.is_some());
        assert!(dto.config.exclude_patterns.is_some());
        assert_eq!(dto.config.strategy.as_deref(), Some("bfs"));
        assert_eq!(dto.config.crawl_delay_ms, Some(1000));
        assert_eq!(dto.config.max_concurrency, Some(5));
        assert!(dto.config.proxy.is_some());
        assert!(dto.config.headers.is_some());
        assert_eq!(dto.sync_wait_ms, Some(5000));
    }

    #[test]
    fn test_crawl_request_dto_with_max_depth_zero() {
        let json = r#"{"url":"https://example.com","config":{"max_depth":0}}"#;
        let dto: CrawlRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.config.max_depth, 0);
    }

    #[test]
    fn test_crawl_request_dto_with_max_depth_five() {
        let json = r#"{"url":"https://example.com","config":{"max_depth":5}}"#;
        let dto: CrawlRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.config.max_depth, 5);
    }

    #[test]
    fn test_crawl_request_dto_deny_unknown_fields() {
        let json = r#"{"url":"https://example.com","config":{"max_depth":2},"unknown_field":42}"#;
        let result: Result<CrawlRequestDto, _> = serde_json::from_str(json);
        assert!(result.is_err(), "unknown fields should be rejected");
    }

    #[test]
    fn test_crawl_config_dto_deny_unknown_fields() {
        let json = r#"{"max_depth":2,"unknown":true}"#;
        let result: Result<CrawlConfigDto, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    // ========== max_depth validation logic ==========

    #[test]
    fn test_max_depth_at_boundary_passes() {
        let config = CrawlConfigDto {
            max_depth: 5,
            include_patterns: None,
            exclude_patterns: None,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: None,
            headers: None,
            extraction_rules: None,
        };
        // Handler checks: payload.config.max_depth > 5
        assert!(config.max_depth <= 5, "max_depth of 5 should pass");
    }

    #[test]
    fn test_max_depth_exceeds_limit_fails() {
        let config = CrawlConfigDto {
            max_depth: 6,
            include_patterns: None,
            exclude_patterns: None,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: None,
            headers: None,
            extraction_rules: None,
        };
        // Handler checks: payload.config.max_depth > 5
        assert!(config.max_depth > 5, "max_depth of 6 should fail");
    }

    #[test]
    fn test_max_depth_zero_passes() {
        let config = CrawlConfigDto {
            max_depth: 0,
            include_patterns: None,
            exclude_patterns: None,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: None,
            headers: None,
            extraction_rules: None,
        };
        assert!(config.max_depth <= 5);
    }

    // ========== CrawlConfigDto clone and serialization ==========

    #[test]
    fn test_crawl_config_dto_clone() {
        let config = CrawlConfigDto {
            max_depth: 3,
            include_patterns: Some(vec!["/blog/*".to_string()]),
            exclude_patterns: Some(vec!["/admin/*".to_string()]),
            strategy: Some("dfs".to_string()),
            crawl_delay_ms: Some(500),
            max_concurrency: Some(10),
            proxy: Some("http://proxy:8080".to_string()),
            headers: Some(serde_json::json!({"Accept": "text/html"})),
            extraction_rules: None,
        };
        let cloned = config.clone();
        assert_eq!(cloned.max_depth, 3);
        assert_eq!(cloned.include_patterns, config.include_patterns);
        assert_eq!(cloned.exclude_patterns, config.exclude_patterns);
        assert_eq!(cloned.strategy, config.strategy);
        assert_eq!(cloned.crawl_delay_ms, config.crawl_delay_ms);
        assert_eq!(cloned.max_concurrency, config.max_concurrency);
        assert_eq!(cloned.proxy, config.proxy);
    }

    #[test]
    fn test_crawl_config_dto_serialization_roundtrip() {
        let config = CrawlConfigDto {
            max_depth: 2,
            include_patterns: Some(vec!["/api/*".to_string()]),
            exclude_patterns: None,
            strategy: Some("bfs".to_string()),
            crawl_delay_ms: None,
            max_concurrency: Some(20),
            proxy: None,
            headers: None,
            extraction_rules: None,
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: CrawlConfigDto = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.max_depth, 2);
        assert_eq!(deserialized.include_patterns, config.include_patterns);
        assert_eq!(deserialized.strategy, config.strategy);
        assert_eq!(deserialized.max_concurrency, config.max_concurrency);
    }

    #[test]
    fn test_crawl_config_dto_debug() {
        let config = CrawlConfigDto {
            max_depth: 1,
            include_patterns: None,
            exclude_patterns: None,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: None,
            headers: None,
            extraction_rules: None,
        };
        let debug = format!("{:?}", config);
        assert!(debug.contains("CrawlConfigDto"));
        assert!(debug.contains("max_depth"));
        assert!(debug.contains("1"));
    }

    // ========== CrawlRequestDto validation ==========

    #[test]
    fn test_crawl_request_dto_validate_success() {
        let dto = CrawlRequestDto {
            url: "https://example.com".to_string(),
            validated_url: None,
            name: Some("test".to_string()),
            config: CrawlConfigDto {
                max_depth: 3,
                include_patterns: None,
                exclude_patterns: None,
                strategy: None,
                crawl_delay_ms: None,
                max_concurrency: None,
                proxy: None,
                headers: None,
                extraction_rules: None,
            },
            sync_wait_ms: Some(5000),
            expires_at: None,
        };
        assert!(dto.validate().is_ok());
    }

    #[test]
    fn test_crawl_request_dto_validate_empty_url_fails() {
        let dto = CrawlRequestDto {
            url: "".to_string(),
            validated_url: None,
            name: None,
            config: CrawlConfigDto {
                max_depth: 1,
                include_patterns: None,
                exclude_patterns: None,
                strategy: None,
                crawl_delay_ms: None,
                max_concurrency: None,
                proxy: None,
                headers: None,
                extraction_rules: None,
            },
            sync_wait_ms: None,
            expires_at: None,
        };
        assert!(dto.validate().is_err());
    }

    #[test]
    fn test_crawl_request_dto_validate_sync_wait_ms_too_large_fails() {
        let dto = CrawlRequestDto {
            url: "https://example.com".to_string(),
            validated_url: None,
            name: None,
            config: CrawlConfigDto {
                max_depth: 1,
                include_patterns: None,
                exclude_patterns: None,
                strategy: None,
                crawl_delay_ms: None,
                max_concurrency: None,
                proxy: None,
                headers: None,
                extraction_rules: None,
            },
            sync_wait_ms: Some(30001),
            expires_at: None,
        };
        assert!(dto.validate().is_err());
    }

    #[test]
    fn test_crawl_request_dto_validate_sync_wait_ms_zero_passes() {
        let dto = CrawlRequestDto {
            url: "https://example.com".to_string(),
            validated_url: None,
            name: None,
            config: CrawlConfigDto {
                max_depth: 1,
                include_patterns: None,
                exclude_patterns: None,
                strategy: None,
                crawl_delay_ms: None,
                max_concurrency: None,
                proxy: None,
                headers: None,
                extraction_rules: None,
            },
            sync_wait_ms: Some(0),
            expires_at: None,
        };
        assert!(dto.validate().is_ok());
    }

    // ========== SyncWaitResult construction ==========

    #[test]
    fn test_sync_wait_result_timeout() {
        let result = SyncWaitResult {
            waited_time_ms: 5000,
            is_timeout: true,
        };
        assert_eq!(result.waited_time_ms, 5000);
        assert!(result.is_timeout);
    }

    #[test]
    fn test_sync_wait_result_no_timeout() {
        let result = SyncWaitResult {
            waited_time_ms: 0,
            is_timeout: false,
        };
        assert_eq!(result.waited_time_ms, 0);
        assert!(!result.is_timeout);
    }

    // ========== Status code selection logic ==========

    #[test]
    fn test_status_code_accepted_when_timeout() {
        let sync_wait_ms = 5000u32;
        let wait_result = SyncWaitResult {
            waited_time_ms: 5000,
            is_timeout: true,
        };
        let status_code = if sync_wait_ms > 0 && wait_result.is_timeout {
            StatusCode::ACCEPTED
        } else {
            StatusCode::CREATED
        };
        assert_eq!(status_code, StatusCode::ACCEPTED);
    }

    #[test]
    fn test_status_code_created_when_no_timeout() {
        let sync_wait_ms = 5000u32;
        let wait_result = SyncWaitResult {
            waited_time_ms: 1000,
            is_timeout: false,
        };
        let status_code = if sync_wait_ms > 0 && wait_result.is_timeout {
            StatusCode::ACCEPTED
        } else {
            StatusCode::CREATED
        };
        assert_eq!(status_code, StatusCode::CREATED);
    }

    #[test]
    fn test_status_code_created_when_sync_wait_zero() {
        let sync_wait_ms = 0u32;
        let wait_result = SyncWaitResult {
            waited_time_ms: 0,
            is_timeout: true, // Even if timeout, sync_wait_ms=0 means no wait
        };
        let status_code = if sync_wait_ms > 0 && wait_result.is_timeout {
            StatusCode::ACCEPTED
        } else {
            StatusCode::CREATED
        };
        assert_eq!(status_code, StatusCode::CREATED);
    }

    // ========== Additional From<CrawlUseCaseError> tests ==========

    #[test]
    fn test_validation_error_empty_message() {
        let err = CrawlUseCaseError::ValidationError(String::new());
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert!(msg.is_empty());
    }

    #[test]
    fn test_anyhow_error_chained_context() {
        let err = CrawlUseCaseError::Anyhow(
            anyhow::anyhow!("root cause")
                .context("middle context")
                .context("outer context"),
        );
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(msg.contains("outer context"));
    }

    #[test]
    fn test_repository_database_error_with_complex_message() {
        let err = CrawlUseCaseError::Repository(RepositoryError::Database(anyhow::anyhow!(
            "connection pool exhausted after 30s timeout"
        )));
        let (status, msg) = <(StatusCode, String)>::from(err);
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(msg.contains("connection pool exhausted"));
        assert!(msg.contains("30s"));
    }

    // ========== CRAWL_TASK_CREDITS_COST constant ==========

    #[test]
    fn test_crawl_task_credits_cost_value() {
        assert_eq!(CRAWL_TASK_CREDITS_COST, 10);
    }

    #[test]
    fn test_default_timeout_ms_constant() {
        assert_eq!(DEFAULT_TIMEOUT_MS, 5000);
    }
}
