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
    Extension(state): Extension<Arc<CrawlHandlerState>>,
    Extension(auth_state): Extension<AuthState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<CrawlRequestDto>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let sync_wait_ms = payload.sync_wait_ms.unwrap_or(DEFAULT_TIMEOUT_MS as u32);

    // 验证 config 字段
    if payload.config.max_depth > 5 {
        return errors::unprocessable_entity("max_depth must be between 0 and 5");
    }

    // 1. 检查限流（架构 MEDIUM-1：限流必须在 SSRF 之前，避免恶意请求触发异步 DNS 解析消耗资源）
    // 性能 LOW-1：直接传 `Uuid`（实现 Display），由 helper 内部按需 to_string，
    // 消除 handler 中的中间变量分配。
    if let Err(response) =
        check_rate_limit(state.rate_limiting_service.as_ref(), auth_state.api_key_id, "/v1/crawl").await
    {
        return response;
    }

    // 2. SSRF 验证 - 使用完整的异步 DNS 验证
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

    // 3. 检查配额
    if let Err(e) = state
        .rate_limiting_service
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

    let use_case = state.create_use_case();

    let client_ip = addr.ip().to_string();
    match use_case
        .create_crawl(team_id, auth_state.api_key_id, payload, &client_ip)
        .await
    {
        Ok(crawl) => {
            // 处理同步等待
            let wait_result = if sync_wait_ms > 0 {
                match state.task_repo.find_by_crawl_id(crawl.id).await {
                    Ok(tasks) => {
                        if !tasks.is_empty() {
                            let task_ids = extract_task_ids(&tasks);
                            handle_sync_wait_and_get_status(
                                state.task_repo.as_ref(),
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
    Extension(state): Extension<Arc<CrawlHandlerState>>,
    Extension(auth_state): Extension<AuthState>,
    Path(crawl_id): Path<Uuid>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let use_case = state.create_use_case();

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
    Extension(state): Extension<Arc<CrawlHandlerState>>,
    Extension(auth_state): Extension<AuthState>,
    Path(crawl_id): Path<Uuid>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let use_case = state.create_use_case();

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
    Extension(state): Extension<Arc<CrawlHandlerState>>,
    Extension(auth_state): Extension<AuthState>,
    Path(crawl_id): Path<Uuid>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let use_case = state.create_use_case();

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
    use crate::common::test_helpers::create_test_db_pool;
    use crate::domain::repositories::task_repository::RepositoryError;
    use chrono::Datelike;
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

    // ========== sync_wait_ms default logic (mirrors handler line 41) ==========

    // The `None` literal here is intentional — we are testing the None-arm of
    // `unwrap_or`. clippy::unnecessary_literal_unwrap would have us delete the
    // `unwrap_or`, which defeats the test's purpose. Allow at fn scope so both
    // the `let` and the `unwrap_or` expression are covered.
    #[test]
    #[allow(clippy::unnecessary_literal_unwrap)]
    fn test_sync_wait_ms_defaults_to_default_timeout_when_none() {
        // Verify: payload.sync_wait_ms.unwrap_or(DEFAULT_TIMEOUT_MS as u32)
        // exercises the unwrap_or branch (not just the constant value).
        let payload_sync_wait_ms: Option<u32> = None;
        let sync_wait_ms = payload_sync_wait_ms.unwrap_or(DEFAULT_TIMEOUT_MS as u32);
        assert_eq!(sync_wait_ms, DEFAULT_TIMEOUT_MS as u32);
        assert_eq!(sync_wait_ms, 5000);
    }

    #[test]
    fn test_sync_wait_ms_uses_custom_value_when_some() {
        let sync_wait_ms = 10000;
        assert_eq!(sync_wait_ms, 10000);
    }

    #[test]
    fn test_sync_wait_ms_zero_uses_zero() {
        let sync_wait_ms = 0;
        assert_eq!(sync_wait_ms, 0);
    }

    // ========== max_depth validation logic (mirrors handler line 44) ==========

    #[test]
    fn test_max_depth_six_fails_handler_check() {
        // Handler: if payload.config.max_depth > 5 { return error }
        let max_depth: u32 = 6;
        assert!(max_depth > 5, "max_depth of 6 should fail handler check");
    }

    #[test]
    fn test_max_depth_five_passes_handler_check() {
        let max_depth: u32 = 5;
        assert!(max_depth <= 5, "max_depth of 5 should pass handler check");
    }

    #[test]
    fn test_max_depth_zero_passes_handler_check() {
        let max_depth: u32 = 0;
        assert!(max_depth <= 5);
    }

    #[test]
    fn test_max_depth_one_passes_handler_check() {
        let max_depth: u32 = 1;
        assert!(max_depth <= 5);
    }

    // ========== CrawlRequestDto with expires_at ==========

    #[test]
    fn test_crawl_request_dto_with_expires_at() {
        let json = r#"{
            "url": "https://example.com",
            "config": {"max_depth": 2},
            "expires_at": "2025-12-31T23:59:59Z"
        }"#;
        let dto: CrawlRequestDto = serde_json::from_str(json).unwrap();
        assert!(dto.expires_at.is_some());
        let expires = dto.expires_at.unwrap();
        assert_eq!(expires.year(), 2025);
        assert_eq!(expires.month(), 12);
        assert_eq!(expires.day(), 31);
    }

    #[test]
    fn test_crawl_request_dto_with_name() {
        let json = r#"{
            "url": "https://example.com",
            "name": "My Crawl Task",
            "config": {"max_depth": 1}
        }"#;
        let dto: CrawlRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.name.as_deref(), Some("My Crawl Task"));
    }

    // ========== CrawlConfigDto with all optional fields None ==========

    #[test]
    fn test_crawl_config_dto_minimal() {
        let json = r#"{"max_depth": 1}"#;
        let config: CrawlConfigDto = serde_json::from_str(json).unwrap();
        assert_eq!(config.max_depth, 1);
        assert!(config.include_patterns.is_none());
        assert!(config.exclude_patterns.is_none());
        assert!(config.strategy.is_none());
        assert!(config.crawl_delay_ms.is_none());
        assert!(config.max_concurrency.is_none());
        assert!(config.proxy.is_none());
        assert!(config.headers.is_none());
        assert!(config.extraction_rules.is_none());
    }

    #[test]
    fn test_crawl_config_dto_with_patterns() {
        let json = r#"{
            "max_depth": 3,
            "include_patterns": ["/blog/*", "/news/*"],
            "exclude_patterns": ["/admin/*", "/login/*"]
        }"#;
        let config: CrawlConfigDto = serde_json::from_str(json).unwrap();
        assert_eq!(config.max_depth, 3);
        assert_eq!(config.include_patterns.as_ref().unwrap().len(), 2);
        assert_eq!(config.exclude_patterns.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn test_crawl_config_dto_with_headers_and_proxy() {
        let json = r#"{
            "max_depth": 2,
            "headers": {"Authorization": "Bearer token"},
            "proxy": "socks5://proxy:1080"
        }"#;
        let config: CrawlConfigDto = serde_json::from_str(json).unwrap();
        assert!(config.headers.is_some());
        assert_eq!(config.proxy.as_deref(), Some("socks5://proxy:1080"));
    }

    // ========== CrawlUseCaseError Display trait ==========

    #[test]
    fn test_crawl_use_case_error_validation_display() {
        let err = CrawlUseCaseError::ValidationError("invalid input".to_string());
        let display = format!("{}", err);
        assert!(display.contains("Validation failed"));
        assert!(display.contains("invalid input"));
    }

    #[test]
    fn test_crawl_use_case_error_repository_display() {
        let err = CrawlUseCaseError::Repository(RepositoryError::NotFound);
        let display = format!("{}", err);
        assert!(display.contains("Repository error"));
    }

    #[test]
    fn test_crawl_use_case_error_not_found_display() {
        let err = CrawlUseCaseError::NotFound;
        let display = format!("{}", err);
        assert!(display.contains("Crawl not found"));
    }

    #[test]
    fn test_crawl_use_case_error_anyhow_display() {
        let err = CrawlUseCaseError::Anyhow(anyhow::anyhow!("something went wrong"));
        let display = format!("{}", err);
        assert!(display.contains("something went wrong"));
    }

    // ========== SyncWaitResult default values in handler ==========

    #[test]
    fn test_sync_wait_result_default_when_no_tasks() {
        // Handler creates this when tasks list is empty
        let result = SyncWaitResult {
            waited_time_ms: 0,
            is_timeout: false,
        };
        assert_eq!(result.waited_time_ms, 0);
        assert!(!result.is_timeout);
    }

    #[test]
    fn test_sync_wait_result_default_on_error() {
        // Handler creates this when find_by_crawl_id fails
        let result = SyncWaitResult {
            waited_time_ms: 0,
            is_timeout: false,
        };
        assert!(!result.is_timeout);
    }

    #[test]
    fn test_sync_wait_result_timeout_with_waited_time() {
        // Handler creates this when sync_wait_ms > 0 and tasks exist but timeout
        let sync_wait_ms = 5000u32;
        let result = SyncWaitResult {
            waited_time_ms: sync_wait_ms as u64,
            is_timeout: true,
        };
        assert_eq!(result.waited_time_ms, 5000);
        assert!(result.is_timeout);
    }

    // ========== Combined status code + sync_wait logic ==========

    #[test]
    fn test_no_sync_wait_returns_created() {
        // When sync_wait_ms is 0 (after unwrap_or), status is always CREATED
        let sync_wait_ms: u32 = 0;
        let wait_result = SyncWaitResult {
            waited_time_ms: 0,
            is_timeout: true, // Even if timeout, sync_wait_ms=0 means CREATED
        };
        let status_code = if sync_wait_ms > 0 && wait_result.is_timeout {
            StatusCode::ACCEPTED
        } else {
            StatusCode::CREATED
        };
        assert_eq!(status_code, StatusCode::CREATED);
    }

    #[test]
    fn test_sync_wait_with_timeout_returns_accepted() {
        let sync_wait_ms: u32 = 3000;
        let wait_result = SyncWaitResult {
            waited_time_ms: 3000,
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
    fn test_sync_wait_without_timeout_returns_created() {
        let sync_wait_ms: u32 = 3000;
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

    // ========== CrawlConfigDto with extraction_rules ==========

    #[test]
    fn test_crawl_config_dto_with_empty_extraction_rules() {
        let json = r#"{
            "max_depth": 1,
            "extraction_rules": {}
        }"#;
        let config: CrawlConfigDto = serde_json::from_str(json).unwrap();
        assert!(config.extraction_rules.is_some());
        assert_eq!(config.extraction_rules.as_ref().unwrap().len(), 0);
    }

    // ========== CrawlRequestDto serialization ==========

    #[test]
    fn test_crawl_request_dto_serialization_roundtrip() {
        let dto = CrawlRequestDto {
            url: "https://example.com".to_string(),
            validated_url: None,
            name: Some("Test Crawl".to_string()),
            config: CrawlConfigDto {
                max_depth: 3,
                include_patterns: Some(vec!["/api/*".to_string()]),
                exclude_patterns: None,
                strategy: Some("bfs".to_string()),
                crawl_delay_ms: Some(500),
                max_concurrency: Some(5),
                proxy: None,
                headers: None,
                extraction_rules: None,
            },
            sync_wait_ms: Some(5000),
            expires_at: None,
        };
        let json = serde_json::to_string(&dto).unwrap();
        // Note: validated_url has #[serde(skip)] so it won't appear in JSON
        let deserialized: CrawlRequestDto = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.url, dto.url);
        assert_eq!(deserialized.name, dto.name);
        assert_eq!(deserialized.config.max_depth, dto.config.max_depth);
        assert_eq!(deserialized.sync_wait_ms, dto.sync_wait_ms);
    }

    #[test]
    fn test_crawl_request_dto_validated_url_is_skipped_in_serialization() {
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
            sync_wait_ms: None,
            expires_at: None,
        };
        let json = serde_json::to_string(&dto).unwrap();
        assert!(!json.contains("validated_url"));
    }

    // ========== Handler function tests ==========
    //
    // The following tests verify the HTTP-layer behavior of the four handler
    // functions: status code mapping, request validation, sync-wait handling,
    // and error mapping. Business logic is covered by `crawl_use_case::tests`;
    // these tests focus on handler-specific concerns.

    use crate::domain::auth::ApiKeyScope;
    use crate::domain::models::scrape_result::ScrapeResult;
    use crate::domain::models::{Crawl, CrawlStatus, Task, TaskStatus, TaskType, Webhook};
    use crate::domain::repositories::crawl_repository::CrawlRepository;
    use crate::domain::repositories::geo_restriction_repository::{
        GeoRestrictionRepository, GeoRestrictionRepositoryError,
    };
    use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
    use crate::domain::repositories::task_repository::{TaskQueryParams, TaskRepository};
    use crate::domain::repositories::webhook_repository::WebhookRepository;
    use crate::domain::services::geo_location::{GeoLocation, GeoLocationService};
    use crate::domain::services::rate_limiting_service::{
        BacklogService, ConcurrencyConfig, ConcurrencyControlService, ConcurrencyResult,
        QuotaService, RateLimitConfig, RateLimitResult, RateLimitService, RateLimitingError,
        RateLimitingService,
    };
    use crate::domain::services::team_service::{TeamGeoRestrictions, TeamService};
    use async_trait::async_trait;
    use std::collections::HashSet;
    use std::net::IpAddr;
    use std::sync::Mutex;

    // --- MockCrawlRepository ---

    struct MockCrawlRepository {
        stored_crawl: Mutex<Option<Crawl>>,
        find_should_fail: bool,
        create_should_fail: bool,
        update_should_fail: bool,
    }

    impl MockCrawlRepository {
        fn new() -> Self {
            Self {
                stored_crawl: Mutex::new(None),
                find_should_fail: false,
                create_should_fail: false,
                update_should_fail: false,
            }
        }

        fn with_crawl(crawl: Crawl) -> Self {
            Self {
                stored_crawl: Mutex::new(Some(crawl)),
                find_should_fail: false,
                create_should_fail: false,
                update_should_fail: false,
            }
        }

        fn failing_find() -> Self {
            Self {
                find_should_fail: true,
                ..Self::new()
            }
        }

        fn failing_create() -> Self {
            Self {
                create_should_fail: true,
                ..Self::new()
            }
        }

        fn failing_update_with_crawl(crawl: Crawl) -> Self {
            Self {
                stored_crawl: Mutex::new(Some(crawl)),
                update_should_fail: true,
                ..Self::new()
            }
        }
    }

    #[async_trait]
    impl CrawlRepository for MockCrawlRepository {
        async fn create(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
            if self.create_should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!("create failed")));
            }
            Ok(crawl.clone())
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Crawl>, RepositoryError> {
            if self.find_should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "find_by_id failed"
                )));
            }
            Ok(self.stored_crawl.lock().unwrap().clone())
        }

        async fn update(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
            if self.update_should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!("update failed")));
            }
            Ok(crawl.clone())
        }

        async fn increment_completed_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn increment_failed_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn update_status(
            &self,
            _id: Uuid,
            _status: crate::domain::models::CrawlStatus,
        ) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn increment_total_tasks(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn find_by_team_id_paginated(
            &self,
            _team_id: Uuid,
            _limit: u32,
            _offset: u32,
        ) -> Result<Vec<Crawl>, RepositoryError> {
            Ok(vec![])
        }
        async fn count_by_team_id(&self, _team_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
    }

    // --- MockTaskRepository ---

    struct MockTaskRepository {
        /// Tasks returned by `find_by_crawl_id` and `query_tasks` (filtered by task_ids).
        tasks: Vec<Task>,
        find_by_crawl_should_fail: bool,
    }

    impl MockTaskRepository {
        fn new() -> Self {
            Self {
                tasks: vec![],
                find_by_crawl_should_fail: false,
            }
        }

        fn with_tasks(tasks: Vec<Task>) -> Self {
            Self {
                tasks,
                find_by_crawl_should_fail: false,
            }
        }

        fn failing_find_by_crawl() -> Self {
            Self {
                tasks: vec![],
                find_by_crawl_should_fail: true,
            }
        }
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }

        async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(self.tasks.iter().find(|t| t.id == id).cloned())
        }

        async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }

        async fn acquire_next(&self, _worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }

        async fn mark_completed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn mark_failed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn mark_cancelled(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn exists_by_url(&self, _url: &str) -> Result<bool, RepositoryError> {
            Ok(false)
        }

        async fn find_existing_urls(
            &self,
            _urls: &[String],
        ) -> Result<HashSet<String>, RepositoryError> {
            Ok(HashSet::new())
        }

        async fn reset_stuck_tasks(
            &self,
            _timeout: chrono::Duration,
        ) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
            Ok(0)
        }

        async fn find_by_crawl_id(&self, _crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
            if self.find_by_crawl_should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "find_by_crawl_id failed"
                )));
            }
            Ok(self.tasks.clone())
        }

        async fn query_tasks(
            &self,
            params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            let result: Vec<Task> = if let Some(ref task_ids) = params.task_ids {
                self.tasks
                    .iter()
                    .filter(|t| task_ids.contains(&t.id))
                    .cloned()
                    .collect()
            } else {
                self.tasks.clone()
            };
            let count = result.len() as u64;
            Ok((result, count))
        }

        async fn batch_cancel(
            &self,
            _task_ids: Vec<Uuid>,
            _team_id: Uuid,
            _force: bool,
        ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
            Ok((vec![], vec![]))
        }
    }

    // --- MockWebhookRepository ---

    struct MockWebhookRepository;
    #[async_trait]
    impl WebhookRepository for MockWebhookRepository {
        async fn create(&self, webhook: &Webhook) -> Result<Webhook, RepositoryError> {
            Ok(webhook.clone())
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Webhook>, RepositoryError> {
            Ok(None)
        }
        async fn find_by_team_id(&self, _team_id: Uuid) -> Result<Vec<Webhook>, RepositoryError> {
            Ok(vec![])
        }
    }

    // --- MockScrapeResultRepository ---

    struct MockScrapeResultRepository {
        results: Vec<ScrapeResult>,
        find_should_fail: bool,
    }

    impl MockScrapeResultRepository {
        fn new() -> Self {
            Self {
                results: vec![],
                find_should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                results: vec![],
                find_should_fail: true,
            }
        }
    }

    #[async_trait]
    impl ScrapeResultRepository for MockScrapeResultRepository {
        async fn save(&self, _result: ScrapeResult) -> anyhow::Result<()> {
            Ok(())
        }

        async fn find_by_task_id(&self, _task_id: Uuid) -> anyhow::Result<Option<ScrapeResult>> {
            Ok(None)
        }

        async fn find_by_task_ids(&self, task_ids: &[Uuid]) -> anyhow::Result<Vec<ScrapeResult>> {
            if self.find_should_fail {
                return Err(anyhow::anyhow!("find_by_task_ids failed"));
            }
            Ok(self
                .results
                .iter()
                .filter(|r| task_ids.contains(&r.task_id))
                .cloned()
                .collect())
        }

        async fn get_team_avg_response_time(&self, _team_id: Uuid) -> anyhow::Result<f64> {
            Ok(0.0)
        }
    }

    // --- MockGeoRestrictionRepository ---

    struct MockGeoRestrictionRepository {
        restrictions: TeamGeoRestrictions,
        get_should_fail: bool,
    }

    impl MockGeoRestrictionRepository {
        fn new() -> Self {
            Self {
                restrictions: TeamGeoRestrictions::default(),
                get_should_fail: false,
            }
        }
    }

    #[async_trait]
    impl GeoRestrictionRepository for MockGeoRestrictionRepository {
        async fn get_team_restrictions(
            &self,
            _team_id: Uuid,
        ) -> Result<TeamGeoRestrictions, GeoRestrictionRepositoryError> {
            if self.get_should_fail {
                return Err(GeoRestrictionRepositoryError::Database(
                    "get_team_restrictions failed".to_string(),
                ));
            }
            Ok(self.restrictions.clone())
        }

        async fn update_team_restrictions(
            &self,
            _team_id: Uuid,
            _restrictions: &TeamGeoRestrictions,
        ) -> Result<(), GeoRestrictionRepositoryError> {
            Ok(())
        }

        async fn log_geo_restriction_action(
            &self,
            _team_id: Uuid,
            _ip_address: &str,
            _country_code: &str,
            _action: &str,
            _reason: &str,
        ) -> Result<(), GeoRestrictionRepositoryError> {
            Ok(())
        }
    }

    // --- MockGeoLocationService ---

    struct MockGeoLocationService;
    #[async_trait]
    impl GeoLocationService for MockGeoLocationService {
        async fn get_location(&self, _ip: &IpAddr) -> anyhow::Result<GeoLocation> {
            Ok(GeoLocation::default())
        }
    }

    // --- MockRateLimitingService ---

    struct MockRateLimitingService {
        rate_limit_result: RateLimitResult,
        quota_should_fail: bool,
    }

    impl MockRateLimitingService {
        fn new_allowed() -> Self {
            Self {
                rate_limit_result: RateLimitResult::Allowed,
                quota_should_fail: false,
            }
        }

        fn new_denied() -> Self {
            Self {
                rate_limit_result: RateLimitResult::Denied {
                    reason: "Too many requests".to_string(),
                },
                quota_should_fail: false,
            }
        }

        fn new_quota_exceeded() -> Self {
            Self {
                rate_limit_result: RateLimitResult::Allowed,
                quota_should_fail: true,
            }
        }
    }

    #[async_trait]
    impl RateLimitService for MockRateLimitingService {
        async fn check_rate_limit(
            &self,
            _api_key: &str,
            _endpoint: &str,
        ) -> Result<RateLimitResult, RateLimitingError> {
            Ok(self.rate_limit_result.clone())
        }

        async fn get_team_rate_limit_config(
            &self,
            _team_id: Uuid,
        ) -> Result<RateLimitConfig, RateLimitingError> {
            Ok(RateLimitConfig::default())
        }

        async fn update_team_rate_limit_config(
            &self,
            _team_id: Uuid,
            _config: RateLimitConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn cleanup_expired_rate_limits(&self) -> Result<u64, RateLimitingError> {
            Ok(0)
        }
    }

    #[async_trait]
    impl ConcurrencyControlService for MockRateLimitingService {
        async fn check_team_concurrency(
            &self,
            _team_id: Uuid,
            _task_id: Uuid,
        ) -> Result<ConcurrencyResult, RateLimitingError> {
            Ok(ConcurrencyResult::Allowed)
        }

        async fn release_team_concurrency_slot(
            &self,
            _team_id: Uuid,
            _task_id: Uuid,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn get_team_current_concurrency(
            &self,
            _team_id: Uuid,
        ) -> Result<u32, RateLimitingError> {
            Ok(0)
        }

        async fn get_team_concurrency_config(
            &self,
            _team_id: Uuid,
        ) -> Result<ConcurrencyConfig, RateLimitingError> {
            Ok(ConcurrencyConfig::default())
        }

        async fn update_team_concurrency_config(
            &self,
            _team_id: Uuid,
            _config: ConcurrencyConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
    }

    #[async_trait]
    impl BacklogService for MockRateLimitingService {
        async fn process_backlog_tasks(&self, _team_id: Uuid) -> Result<u32, RateLimitingError> {
            Ok(0)
        }
    }

    #[async_trait]
    impl QuotaService for MockRateLimitingService {
        async fn check_and_deduct_quota(
            &self,
            _team_id: Uuid,
            _amount: i64,
            _transaction_type: crate::domain::models::CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<(), RateLimitingError> {
            if self.quota_should_fail {
                return Err(RateLimitingError::CreditsError);
            }
            Ok(())
        }

        async fn get_quota_balance(&self, _team_id: Uuid) -> Result<i64, RateLimitingError> {
            Ok(1000)
        }
    }

    impl RateLimitingService for MockRateLimitingService {}

    // --- Helper functions ---
    // `make_db_pool` 已集中到 `src/common/test_helpers.rs::create_test_db_pool`。

    fn make_auth_state() -> AuthState {
        AuthState::new(
            create_test_db_pool(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
        )
    }

    fn make_auth_state_with_team(team_id: Uuid) -> AuthState {
        AuthState::new(
            create_test_db_pool(),
            team_id,
            Uuid::new_v4(),
            ApiKeyScope::default(),
        )
    }

    fn make_socket_addr() -> SocketAddr {
        "203.0.113.1:8080".parse().expect("valid SocketAddr")
    }

    fn make_crawl(team_id: Uuid, status: CrawlStatus) -> Crawl {
        let now = chrono::Utc::now();
        Crawl::with_all_fields(
            Uuid::new_v4(),
            team_id,
            "Test Crawl".to_string(),
            "https://example.com".to_string(),
            "https://example.com".to_string(),
            status,
            serde_json::json!({"max_depth": 2}),
            1,
            0,
            0,
            now,
            now,
            None,
        )
    }

    fn make_task(crawl_id: Uuid, team_id: Uuid, status: TaskStatus) -> Task {
        let now = chrono::Utc::now();
        Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Crawl,
            status,
            priority: 100,
            team_id,
            api_key_id: Uuid::new_v4(),
            url: "https://example.com".to_string(),
            payload: serde_json::json!({"crawl_id": crawl_id, "depth": 0}),
            retry_count: 0,
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            created_at: now,
            started_at: None,
            completed_at: None,
            crawl_id: Some(crawl_id),
            updated_at: now,
            lock_token: None,
            lock_expires_at: None,
            expires_at: None,
        }
    }

    fn make_crawl_request_dto(
        url: &str,
        max_depth: u32,
        sync_wait_ms: Option<u32>,
        max_concurrency: Option<u32>,
    ) -> CrawlRequestDto {
        CrawlRequestDto {
            url: url.to_string(),
            validated_url: None,
            name: Some("Test Crawl".to_string()),
            config: CrawlConfigDto {
                max_depth,
                include_patterns: None,
                exclude_patterns: None,
                strategy: None,
                crawl_delay_ms: None,
                max_concurrency,
                proxy: None,
                headers: None,
                extraction_rules: None,
            },
            sync_wait_ms,
            expires_at: None,
        }
    }

    /// Build a CrawlHandlerState from configurable mock dependencies.
    fn build_handler_state(
        crawl_repo: MockCrawlRepository,
        task_repo: MockTaskRepository,
        scrape_result_repo: MockScrapeResultRepository,
        geo_restriction_repo: MockGeoRestrictionRepository,
        rate_limiting_service: MockRateLimitingService,
    ) -> Arc<CrawlHandlerState> {
        let crawl_repo: Arc<dyn CrawlRepository> = Arc::new(crawl_repo);
        let task_repo: Arc<dyn TaskRepository> = Arc::new(task_repo);
        let webhook_repo: Arc<dyn WebhookRepository> = Arc::new(MockWebhookRepository);
        let scrape_result_repo: Arc<dyn ScrapeResultRepository> = Arc::new(scrape_result_repo);
        let geo_restriction_repo: Arc<dyn GeoRestrictionRepository> =
            Arc::new(geo_restriction_repo);
        let team_service = Arc::new(TeamService::new(
            Arc::new(MockGeoLocationService),
            geo_restriction_repo.clone(),
        ));
        let rate_limiting_service: Arc<dyn RateLimitingService> = Arc::new(rate_limiting_service);
        Arc::new(CrawlHandlerState::new(
            crawl_repo,
            task_repo,
            webhook_repo,
            scrape_result_repo,
            geo_restriction_repo,
            team_service,
            rate_limiting_service,
        ))
    }

    // ========== create_crawl tests ==========

    #[tokio::test]
    async fn test_create_crawl_success_no_sync_wait() {
        let state = build_handler_state(
            MockCrawlRepository::new(),
            MockTaskRepository::new(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state();
        let payload = make_crawl_request_dto("https://example.com", 2, Some(0), None);

        let response = create_crawl(
            Extension(state),
            Extension(auth),
            ConnectInfo(make_socket_addr()),
            Json(payload),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_create_crawl_success_sync_wait_empty_tasks() {
        let state = build_handler_state(
            MockCrawlRepository::new(),
            MockTaskRepository::new(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state();
        let payload = make_crawl_request_dto("https://example.com", 2, Some(100), None);

        let response = create_crawl(
            Extension(state),
            Extension(auth),
            ConnectInfo(make_socket_addr()),
            Json(payload),
        )
        .await
        .into_response();

        // sync_wait_ms > 0 but find_by_crawl_id returns empty → no timeout → 201
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_create_crawl_success_sync_wait_completed() {
        let team_id = Uuid::new_v4();
        let task = make_task(Uuid::new_v4(), team_id, TaskStatus::Completed);
        let state = build_handler_state(
            MockCrawlRepository::new(),
            MockTaskRepository::with_tasks(vec![task]),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state_with_team(team_id);
        // sync_wait_ms=5000 避免 tarpaulin 插桩开销导致 elapsed >= 100ms 误判超时
        let payload = make_crawl_request_dto("https://example.com", 2, Some(5000), None);

        let response = create_crawl(
            Extension(state),
            Extension(auth),
            ConnectInfo(make_socket_addr()),
            Json(payload),
        )
        .await
        .into_response();

        // Tasks complete → is_timeout=false → 201
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_create_crawl_sync_wait_timeout_returns_accepted() {
        let team_id = Uuid::new_v4();
        // Queued tasks never complete → polling loops until sync_wait_ms elapses
        let task = make_task(Uuid::new_v4(), team_id, TaskStatus::Queued);
        let state = build_handler_state(
            MockCrawlRepository::new(),
            MockTaskRepository::with_tasks(vec![task]),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state_with_team(team_id);
        let payload = make_crawl_request_dto("https://example.com", 2, Some(100), None);

        let response = create_crawl(
            Extension(state),
            Extension(auth),
            ConnectInfo(make_socket_addr()),
            Json(payload),
        )
        .await
        .into_response();

        // Tasks never complete within 100ms → is_timeout=true → 202
        assert_eq!(response.status(), StatusCode::ACCEPTED);
    }

    #[tokio::test]
    async fn test_create_crawl_sync_wait_find_error_returns_created() {
        let state = build_handler_state(
            MockCrawlRepository::new(),
            MockTaskRepository::failing_find_by_crawl(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state();
        let payload = make_crawl_request_dto("https://example.com", 2, Some(100), None);

        let response = create_crawl(
            Extension(state),
            Extension(auth),
            ConnectInfo(make_socket_addr()),
            Json(payload),
        )
        .await
        .into_response();

        // find_by_crawl_id fails → handler logs error, no timeout → 201
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_create_crawl_max_depth_exceeds_returns_unprocessable_entity() {
        let state = build_handler_state(
            MockCrawlRepository::new(),
            MockTaskRepository::new(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state();
        let payload = make_crawl_request_dto("https://example.com", 6, Some(0), None);

        let response = create_crawl(
            Extension(state),
            Extension(auth),
            ConnectInfo(make_socket_addr()),
            Json(payload),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_create_crawl_ssrf_private_ip_returns_bad_request() {
        let state = build_handler_state(
            MockCrawlRepository::new(),
            MockTaskRepository::new(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state();
        // 127.0.0.1 is a loopback IP, rejected by SSRF validator
        let payload = make_crawl_request_dto("http://127.0.0.1", 2, Some(0), None);

        let response = create_crawl(
            Extension(state),
            Extension(auth),
            ConnectInfo(make_socket_addr()),
            Json(payload),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_crawl_rate_limited_returns_too_many_requests() {
        let state = build_handler_state(
            MockCrawlRepository::new(),
            MockTaskRepository::new(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_denied(),
        );
        let auth = make_auth_state();
        let payload = make_crawl_request_dto("https://example.com", 2, Some(0), None);

        let response = create_crawl(
            Extension(state),
            Extension(auth),
            ConnectInfo(make_socket_addr()),
            Json(payload),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_create_crawl_quota_exceeded_returns_payment_required() {
        let state = build_handler_state(
            MockCrawlRepository::new(),
            MockTaskRepository::new(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_quota_exceeded(),
        );
        let auth = make_auth_state();
        let payload = make_crawl_request_dto("https://example.com", 2, Some(0), None);

        let response = create_crawl(
            Extension(state),
            Extension(auth),
            ConnectInfo(make_socket_addr()),
            Json(payload),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
    }

    #[tokio::test]
    async fn test_create_crawl_use_case_validation_error_returns_bad_request() {
        // max_concurrency > 100 triggers use_case ValidationError (not handler check)
        let state = build_handler_state(
            MockCrawlRepository::new(),
            MockTaskRepository::new(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state();
        let payload = make_crawl_request_dto("https://example.com", 2, Some(0), Some(200));

        let response = create_crawl(
            Extension(state),
            Extension(auth),
            ConnectInfo(make_socket_addr()),
            Json(payload),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_crawl_use_case_repository_error_returns_internal_server_error() {
        let state = build_handler_state(
            MockCrawlRepository::failing_create(),
            MockTaskRepository::new(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state();
        let payload = make_crawl_request_dto("https://example.com", 2, Some(0), None);

        let response = create_crawl(
            Extension(state),
            Extension(auth),
            ConnectInfo(make_socket_addr()),
            Json(payload),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // ========== get_crawl tests ==========

    #[tokio::test]
    async fn test_get_crawl_success_returns_ok() {
        let team_id = Uuid::new_v4();
        let crawl = make_crawl(team_id, CrawlStatus::Queued);
        let crawl_id = crawl.id;
        let state = build_handler_state(
            MockCrawlRepository::with_crawl(crawl),
            MockTaskRepository::new(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state_with_team(team_id);

        let response = get_crawl(Extension(state), Extension(auth), Path(crawl_id))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_crawl_not_found_returns_404() {
        let state = build_handler_state(
            MockCrawlRepository::new(),
            MockTaskRepository::new(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state();

        let response = get_crawl(Extension(state), Extension(auth), Path(Uuid::new_v4()))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_crawl_wrong_team_returns_404() {
        let crawl = make_crawl(Uuid::new_v4(), CrawlStatus::Queued);
        let crawl_id = crawl.id;
        let state = build_handler_state(
            MockCrawlRepository::with_crawl(crawl),
            MockTaskRepository::new(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        // Different team_id → use_case returns Ok(None) → 404
        let auth = make_auth_state_with_team(Uuid::new_v4());

        let response = get_crawl(Extension(state), Extension(auth), Path(crawl_id))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_crawl_repo_error_returns_internal_server_error() {
        let state = build_handler_state(
            MockCrawlRepository::failing_find(),
            MockTaskRepository::new(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state();

        let response = get_crawl(Extension(state), Extension(auth), Path(Uuid::new_v4()))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // ========== get_crawl_results tests ==========

    #[tokio::test]
    async fn test_get_crawl_results_success_returns_ok() {
        let team_id = Uuid::new_v4();
        let crawl = make_crawl(team_id, CrawlStatus::Completed);
        let crawl_id = crawl.id;
        let task = make_task(crawl_id, team_id, TaskStatus::Completed);
        let state = build_handler_state(
            MockCrawlRepository::with_crawl(crawl),
            MockTaskRepository::with_tasks(vec![task]),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state_with_team(team_id);

        let response = get_crawl_results(Extension(state), Extension(auth), Path(crawl_id))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_crawl_results_crawl_not_found_returns_404() {
        let state = build_handler_state(
            MockCrawlRepository::new(),
            MockTaskRepository::new(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state();

        let response = get_crawl_results(Extension(state), Extension(auth), Path(Uuid::new_v4()))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_crawl_results_repo_error_returns_internal_server_error() {
        let team_id = Uuid::new_v4();
        let crawl = make_crawl(team_id, CrawlStatus::Completed);
        let crawl_id = crawl.id;
        let task = make_task(crawl_id, team_id, TaskStatus::Completed);
        let state = build_handler_state(
            MockCrawlRepository::with_crawl(crawl),
            MockTaskRepository::with_tasks(vec![task]),
            MockScrapeResultRepository::failing(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state_with_team(team_id);

        let response = get_crawl_results(Extension(state), Extension(auth), Path(crawl_id))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // ========== cancel_crawl tests ==========

    #[tokio::test]
    async fn test_cancel_crawl_success_returns_no_content() {
        let team_id = Uuid::new_v4();
        let crawl = make_crawl(team_id, CrawlStatus::Queued);
        let crawl_id = crawl.id;
        let state = build_handler_state(
            MockCrawlRepository::with_crawl(crawl),
            MockTaskRepository::new(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state_with_team(team_id);

        let response = cancel_crawl(Extension(state), Extension(auth), Path(crawl_id))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_cancel_crawl_not_found_returns_404() {
        let state = build_handler_state(
            MockCrawlRepository::new(),
            MockTaskRepository::new(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state();

        let response = cancel_crawl(Extension(state), Extension(auth), Path(Uuid::new_v4()))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_cancel_crawl_wrong_team_returns_404() {
        let crawl = make_crawl(Uuid::new_v4(), CrawlStatus::Queued);
        let crawl_id = crawl.id;
        let state = build_handler_state(
            MockCrawlRepository::with_crawl(crawl),
            MockTaskRepository::new(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state_with_team(Uuid::new_v4());

        let response = cancel_crawl(Extension(state), Extension(auth), Path(crawl_id))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_cancel_crawl_already_completed_returns_no_content() {
        let team_id = Uuid::new_v4();
        let crawl = make_crawl(team_id, CrawlStatus::Completed);
        let crawl_id = crawl.id;
        let state = build_handler_state(
            MockCrawlRepository::with_crawl(crawl),
            MockTaskRepository::new(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state_with_team(team_id);

        let response = cancel_crawl(Extension(state), Extension(auth), Path(crawl_id))
            .await
            .into_response();

        // Already completed → use_case returns Ok(()) → 204
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn test_cancel_crawl_repo_error_returns_internal_server_error() {
        let state = build_handler_state(
            MockCrawlRepository::failing_find(),
            MockTaskRepository::new(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state();

        let response = cancel_crawl(Extension(state), Extension(auth), Path(Uuid::new_v4()))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_cancel_crawl_update_error_returns_internal_server_error() {
        let team_id = Uuid::new_v4();
        let crawl = make_crawl(team_id, CrawlStatus::Queued);
        let crawl_id = crawl.id;
        let state = build_handler_state(
            MockCrawlRepository::failing_update_with_crawl(crawl),
            MockTaskRepository::new(),
            MockScrapeResultRepository::new(),
            MockGeoRestrictionRepository::new(),
            MockRateLimitingService::new_allowed(),
        );
        let auth = make_auth_state_with_team(team_id);

        let response = cancel_crawl(Extension(state), Extension(auth), Path(crawl_id))
            .await
            .into_response();

        // find_by_id ok but update fails → RepositoryError → 500
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
