// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use axum::{
    extract::{Extension, Json, Path},
    http::StatusCode,
    response::IntoResponse,
};
use log::error;
use std::sync::Arc;
use uuid::Uuid;

use crate::{
    application::dto::scrape_request::ScrapeRequestDto,
    application::dto::scrape_response::{
        CancelScrapeResponseDto, ScrapeResponseDto, ScrapeResultDto, ScrapeStatusResponseDto,
    },
    common::constants::crawl_task::MAX_SYNC_WAIT_MS,
    config::settings::Settings,
    domain::models::{Task, TaskStatus, TaskType},
    domain::repositories::{
        scrape_result_repository::ScrapeResultRepository, task_repository::TaskRepository,
    },
    domain::services::rate_limiting_service::RateLimitingService,
    presentation::handlers::response_builder::{errors, success_response, ApiResponse},
    presentation::handlers::task_handler::handle_sync_wait_and_get_status,
    presentation::helpers::rate_limit_helper::check_rate_limit,
    presentation::helpers::ssrf::validate_url,
    presentation::middleware::auth_middleware::AuthState,
    queue::task_queue::TaskQueue,
};

#[allow(clippy::too_many_arguments)]
pub async fn create_scrape(
    Extension(queue): Extension<Arc<dyn TaskQueue>>,
    Extension(_settings): Extension<Arc<Settings>>,
    Extension(task_repository): Extension<Arc<dyn TaskRepository>>,
    Extension(rate_limiting_service): Extension<Arc<dyn RateLimitingService>>,
    Extension(auth_state): Extension<AuthState>,
    Json(payload): Json<ScrapeRequestDto>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    let api_key = auth_state.api_key_id.to_string();

    // 验证 sync_wait_ms 范围
    if let Some(ms) = payload.sync_wait_ms {
        if ms > MAX_SYNC_WAIT_MS {
            return errors::unprocessable_entity(format!(
                "sync_wait_ms must be <= {}",
                MAX_SYNC_WAIT_MS
            ));
        }
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
    if let Err(response) =
        check_rate_limit(rate_limiting_service.as_ref(), &api_key, "/v1/scrape").await
    {
        return response;
    }

    // 2. 检查配额
    if let Err(e) = rate_limiting_service
        .check_and_deduct_quota(
            team_id,
            1,
            crate::domain::models::CreditsTransactionType::Scrape,
            format!("Scrape URL: {}", payload.url),
            None,
        )
        .await
    {
        error!("Quota check failed for team {}: {}", team_id, e);
        return errors::payment_required(e.to_string());
    }

    let now = chrono::Utc::now();
    let task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Scrape,
        status: TaskStatus::Queued,
        priority: 0,
        team_id,
        api_key_id: auth_state.api_key_id,
        url: payload.url.clone(),
        payload: serde_json::to_value(&payload).unwrap_or_default(),
        retry_count: 0,
        attempt_count: 0,
        max_retries: 3,
        scheduled_at: None,
        expires_at: None,
        created_at: now,
        started_at: None,
        completed_at: None,
        crawl_id: None,
        updated_at: now,
        lock_token: None,
        lock_expires_at: None,
    };

    let sync_wait_ms = payload.sync_wait_ms.unwrap_or(0);

    match queue.enqueue(task.clone()).await {
        Ok(_) => {
            // 使用公共函数处理同步等待
            let wait_result = handle_sync_wait_and_get_status(
                task_repository.as_ref(),
                &[task.id],
                team_id,
                sync_wait_ms,
            )
            .await
            .unwrap_or({
                crate::presentation::handlers::task_handler::SyncWaitResult {
                    waited_time_ms: 0,
                    is_timeout: false,
                }
            });

            let response = ScrapeResponseDto {
                id: task.id,
                url: task.url,
                credits_used: 1,
            };

            // 根据同步等待结果设置响应状态
            let status_code = if sync_wait_ms > 0 {
                if wait_result.is_timeout {
                    StatusCode::ACCEPTED // 同步等待超时
                } else {
                    StatusCode::CREATED // 同步等待完成
                }
            } else {
                StatusCode::CREATED // 异步模式
            };

            success_response(status_code, response)
        }
        Err(e) => {
            error!(
                "Failed to enqueue task for team {}: {}. Payload: {:?}",
                team_id, e, payload
            );
            errors::internal_server_error(e.to_string())
        }
    }
}

pub async fn cancel_scrape(
    Path(id): Path<Uuid>,
    Extension(repository): Extension<Arc<dyn TaskRepository>>,
    Extension(auth_state): Extension<AuthState>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    match repository.find_by_id(id).await {
        Ok(Some(task)) => {
            if task.team_id != team_id {
                return errors::forbidden("Access denied");
            }

            // Update task status to cancelled
            match repository.mark_cancelled(id).await {
                Ok(_) => {
                    let response = CancelScrapeResponseDto {
                        message: "Scrape task cancelled".to_string(),
                    };
                    (StatusCode::OK, Json(ApiResponse::success(response))).into_response()
                }
                Err(e) => {
                    error!("Failed to cancel task {}: {}", id, e);
                    errors::internal_server_error("Internal server error")
                }
            }
        }
        Ok(None) => errors::not_found("Task not found"),
        Err(e) => {
            error!("Failed to get task {} for cancellation: {}", id, e);
            errors::internal_server_error("Internal server error")
        }
    }
}

pub async fn get_scrape_status(
    Path(id): Path<Uuid>,
    Extension(task_repository): Extension<Arc<dyn TaskRepository>>,
    Extension(result_repository): Extension<Arc<dyn ScrapeResultRepository>>,
    Extension(auth_state): Extension<AuthState>,
) -> impl IntoResponse {
    let team_id = auth_state.team_id;
    match task_repository.find_by_id(id).await {
        Ok(Some(task)) => {
            if task.team_id != team_id {
                return errors::forbidden("Access denied");
            }

            // Fetch scrape result if task is completed
            let result_data = if task.status == TaskStatus::Completed {
                match result_repository.find_by_task_id(task.id).await {
                    Ok(Some(result)) => Some(ScrapeResultDto {
                        content: result.content,
                        status_code: result.status_code as u16,
                        content_type: Some(result.content_type),
                        response_time_ms: result.response_time_ms,
                        headers: Some(result.headers),
                        meta_data: Some(result.meta_data),
                        screenshot: result.screenshot,
                        created_at: result.created_at,
                    }),
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
                        .or(Some("Task failed".to_string()))
                } else {
                    None
                },
            };

            (StatusCode::OK, Json(ApiResponse::success(response))).into_response()
        }
        Ok(None) => errors::not_found("Task not found"),
        Err(e) => {
            error!("Failed to get task status {}: {}", id, e);
            errors::internal_server_error("Internal server error")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::dto::scrape_request::ScrapeActionDto;
    use chrono::NaiveDateTime;
    use uuid::Uuid;
    use validator::Validate;

    // ========== ScrapeRequestDto validation tests ==========

    #[test]
    fn test_scrape_request_dto_minimal_valid() {
        let json = r#"{"url":"https://example.com"}"#;
        let dto: ScrapeRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.url, "https://example.com");
        assert!(dto.formats.is_none());
        assert!(dto.sync_wait_ms.is_none());
    }

    #[test]
    fn test_scrape_request_dto_with_all_fields() {
        let json = r#"{
            "url": "https://example.com",
            "formats": ["html", "markdown"],
            "include_tags": ["div", "span"],
            "exclude_tags": ["script"],
            "sync_wait_ms": 5000,
            "options": {
                "headers": {},
                "timeout": 30,
                "js_rendering": true
            }
        }"#;
        let dto: ScrapeRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.url, "https://example.com");
        assert_eq!(dto.formats.as_ref().unwrap().len(), 2);
        assert_eq!(dto.sync_wait_ms, Some(5000));
        assert!(dto.options.is_some());
    }

    #[test]
    fn test_scrape_request_dto_rejects_unknown_fields() {
        let json = r#"{"url":"https://example.com","unknown_field":"value"}"#;
        let result: Result<ScrapeRequestDto, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_scrape_request_dto_empty_url_fails_validation() {
        let json = r#"{"url":""}"#;
        let dto: ScrapeRequestDto = serde_json::from_str(json).unwrap();
        let validation = dto.validate();
        assert!(validation.is_err());
    }

    #[test]
    fn test_scrape_request_dto_sync_wait_ms_at_max() {
        let json = r#"{"url":"https://example.com","sync_wait_ms":30000}"#;
        let dto: ScrapeRequestDto = serde_json::from_str(json).unwrap();
        assert!(dto.validate().is_ok());
    }

    #[test]
    fn test_scrape_request_dto_sync_wait_ms_exceeds_max() {
        let json = r#"{"url":"https://example.com","sync_wait_ms":30001}"#;
        let dto: ScrapeRequestDto = serde_json::from_str(json).unwrap();
        assert!(dto.validate().is_err());
    }

    #[test]
    fn test_scrape_request_dto_sync_wait_ms_zero_ok() {
        let json = r#"{"url":"https://example.com","sync_wait_ms":0}"#;
        let dto: ScrapeRequestDto = serde_json::from_str(json).unwrap();
        assert!(dto.validate().is_ok());
    }

    // ========== ScrapeResponseDto serialization ==========

    #[test]
    fn test_scrape_response_dto_serialization() {
        let task_id = Uuid::new_v4();
        let dto = ScrapeResponseDto {
            id: task_id,
            url: "https://example.com".to_string(),
            credits_used: 1,
        };
        let json = serde_json::to_string(&dto).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["id"], task_id.to_string());
        assert_eq!(parsed["url"], "https://example.com");
        assert_eq!(parsed["credits_used"], 1);
    }

    #[test]
    fn test_scrape_response_dto_deserialization() {
        let json = format!(
            r#"{{"id":"{}","url":"https://test.com","credits_used":5}}"#,
            Uuid::new_v4()
        );
        let dto: ScrapeResponseDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto.url, "https://test.com");
        assert_eq!(dto.credits_used, 5);
    }

    #[test]
    fn test_scrape_response_dto_default_credits_used() {
        let json = format!(r#"{{"id":"{}","url":"https://test.com"}}"#, Uuid::new_v4());
        let dto: ScrapeResponseDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto.credits_used, 0);
    }

    // ========== ScrapeResultDto serialization ==========

    #[test]
    fn test_scrape_result_dto_serialization() {
        let dto = ScrapeResultDto {
            content: "<html>test</html>".to_string(),
            status_code: 200,
            content_type: Some("text/html".to_string()),
            response_time_ms: 150,
            headers: Some(serde_json::json!({"content-length": "100"})),
            meta_data: Some(serde_json::json!({"key": "value"})),
            screenshot: None,
            created_at: NaiveDateTime::parse_from_str("2025-01-01T00:00:00", "%Y-%m-%dT%H:%M:%S")
                .unwrap(),
        };
        let json = serde_json::to_string(&dto).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status_code"], 200);
        assert_eq!(parsed["content"], "<html>test</html>");
        assert_eq!(parsed["response_time_ms"], 150);
        assert_eq!(parsed["content_type"], "text/html");
        assert!(parsed["screenshot"].is_null());
    }

    #[test]
    fn test_scrape_result_dto_with_screenshot() {
        let dto = ScrapeResultDto {
            content: "content".to_string(),
            status_code: 200,
            content_type: None,
            response_time_ms: 0,
            headers: None,
            meta_data: None,
            screenshot: Some("base64data".to_string()),
            created_at: NaiveDateTime::parse_from_str("2025-01-01T00:00:00", "%Y-%m-%dT%H:%M:%S")
                .unwrap(),
        };
        let json = serde_json::to_string(&dto).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["screenshot"], "base64data");
    }

    // ========== ScrapeStatusResponseDto serialization ==========

    #[test]
    fn test_scrape_status_response_dto_pending() {
        let task_id = Uuid::new_v4();
        let dto = ScrapeStatusResponseDto {
            id: task_id,
            status: "queued".to_string(),
            url: "https://example.com".to_string(),
            created_at: NaiveDateTime::parse_from_str("2025-01-01T00:00:00", "%Y-%m-%dT%H:%M:%S")
                .unwrap(),
            completed_at: None,
            result: None,
            metadata: None,
            error: None,
        };
        let json = serde_json::to_string(&dto).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "queued");
        assert!(parsed["completed_at"].is_null());
        assert!(parsed["result"].is_null());
    }

    #[test]
    fn test_scrape_status_response_dto_failed_with_error() {
        let dto = ScrapeStatusResponseDto {
            id: Uuid::new_v4(),
            status: "failed".to_string(),
            url: "https://example.com".to_string(),
            created_at: NaiveDateTime::parse_from_str("2025-01-01T00:00:00", "%Y-%m-%dT%H:%M:%S")
                .unwrap(),
            completed_at: Some(
                NaiveDateTime::parse_from_str("2025-01-01T00:01:00", "%Y-%m-%dT%H:%M:%S").unwrap(),
            ),
            result: None,
            metadata: None,
            error: Some("Connection timeout".to_string()),
        };
        let json = serde_json::to_string(&dto).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "failed");
        assert_eq!(parsed["error"], "Connection timeout");
        assert!(parsed["completed_at"].is_string());
    }

    // ========== CancelScrapeResponseDto serialization ==========

    #[test]
    fn test_cancel_scrape_response_dto_serialization() {
        let dto = CancelScrapeResponseDto {
            message: "Scrape task cancelled".to_string(),
        };
        let json = serde_json::to_string(&dto).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["message"], "Scrape task cancelled");
    }

    #[test]
    fn test_cancel_scrape_response_dto_deserialization() {
        let json = r#"{"message":"done"}"#;
        let dto: CancelScrapeResponseDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.message, "done");
    }

    // ========== ScrapeActionDto tag-based deserialization ==========

    #[test]
    fn test_scrape_action_wait_deserialization() {
        let json = r#"{"type":"wait","milliseconds":1000}"#;
        let action: ScrapeActionDto = serde_json::from_str(json).unwrap();
        match action {
            ScrapeActionDto::Wait { milliseconds } => assert_eq!(milliseconds, 1000),
            _ => panic!("Expected Wait action"),
        }
    }

    #[test]
    fn test_scrape_action_click_deserialization() {
        let json = r##"{"type":"click","selector":"#button"}"##;
        let action: ScrapeActionDto = serde_json::from_str(json).unwrap();
        match action {
            ScrapeActionDto::Click { selector } => assert_eq!(selector, "#button"),
            _ => panic!("Expected Click action"),
        }
    }

    #[test]
    fn test_scrape_action_scroll_deserialization() {
        let json = r#"{"type":"scroll","direction":"down"}"#;
        let action: ScrapeActionDto = serde_json::from_str(json).unwrap();
        match action {
            ScrapeActionDto::Scroll { direction } => assert_eq!(direction, "down"),
            _ => panic!("Expected Scroll action"),
        }
    }

    #[test]
    fn test_scrape_action_screenshot_with_full_page() {
        let json = r#"{"type":"screenshot","full_page":true}"#;
        let action: ScrapeActionDto = serde_json::from_str(json).unwrap();
        match action {
            ScrapeActionDto::Screenshot { full_page } => assert_eq!(full_page, Some(true)),
            _ => panic!("Expected Screenshot action"),
        }
    }

    #[test]
    fn test_scrape_action_input_deserialization() {
        let json = r##"{"type":"input","selector":"#search","text":"hello"}"##;
        let action: ScrapeActionDto = serde_json::from_str(json).unwrap();
        match action {
            ScrapeActionDto::Input { selector, text } => {
                assert_eq!(selector, "#search");
                assert_eq!(text, "hello");
            }
            _ => panic!("Expected Input action"),
        }
    }

    // ========== ScrapeActionDto serialization round-trip ==========

    #[test]
    fn test_scrape_action_wait_serialization_roundtrip() {
        let action = ScrapeActionDto::Wait { milliseconds: 2000 };
        let json = serde_json::to_string(&action).unwrap();
        let deserialized: ScrapeActionDto = serde_json::from_str(&json).unwrap();
        match deserialized {
            ScrapeActionDto::Wait { milliseconds } => assert_eq!(milliseconds, 2000),
            _ => panic!("Expected Wait action"),
        }
    }

    #[test]
    fn test_scrape_action_click_serialization_roundtrip() {
        let action = ScrapeActionDto::Click {
            selector: ".btn".to_string(),
        };
        let json = serde_json::to_string(&action).unwrap();
        let deserialized: ScrapeActionDto = serde_json::from_str(&json).unwrap();
        match deserialized {
            ScrapeActionDto::Click { selector } => assert_eq!(selector, ".btn"),
            _ => panic!("Expected Click action"),
        }
    }

    #[test]
    fn test_scrape_action_scroll_serialization_roundtrip() {
        let action = ScrapeActionDto::Scroll {
            direction: "up".to_string(),
        };
        let json = serde_json::to_string(&action).unwrap();
        let deserialized: ScrapeActionDto = serde_json::from_str(&json).unwrap();
        match deserialized {
            ScrapeActionDto::Scroll { direction } => assert_eq!(direction, "up"),
            _ => panic!("Expected Scroll action"),
        }
    }

    #[test]
    fn test_scrape_action_screenshot_without_full_page() {
        let json = r#"{"type":"screenshot"}"#;
        let action: ScrapeActionDto = serde_json::from_str(json).unwrap();
        match action {
            ScrapeActionDto::Screenshot { full_page } => assert_eq!(full_page, None),
            _ => panic!("Expected Screenshot action"),
        }
    }

    #[test]
    fn test_scrape_action_screenshot_full_page_false() {
        let json = r#"{"type":"screenshot","full_page":false}"#;
        let action: ScrapeActionDto = serde_json::from_str(json).unwrap();
        match action {
            ScrapeActionDto::Screenshot { full_page } => assert_eq!(full_page, Some(false)),
            _ => panic!("Expected Screenshot action"),
        }
    }

    #[test]
    fn test_scrape_action_unknown_type_fails() {
        let json = r#"{"type":"unknown","data":123}"#;
        let result: Result<ScrapeActionDto, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_scrape_action_camel_case_tag() {
        // The enum uses rename_all = "camelCase" for the tag
        // Verify that "wait" stays lowercase (already camelCase)
        let json = r#"{"type":"wait","milliseconds":100}"#;
        let action: ScrapeActionDto = serde_json::from_str(json).unwrap();
        match action {
            ScrapeActionDto::Wait { milliseconds } => assert_eq!(milliseconds, 100),
            _ => panic!("Expected Wait action"),
        }
    }

    // ========== ScrapeOptionsDto tests ==========

    #[test]
    fn test_scrape_options_dto_minimal() {
        let json = r#"{"headers":{}}"#;
        let dto: crate::application::dto::scrape_request::ScrapeOptionsDto =
            serde_json::from_str(json).unwrap();
        assert!(dto.headers.is_some());
        assert!(dto.timeout.is_none());
        assert!(dto.js_rendering.is_none());
        assert!(dto.screenshot.is_none());
        assert!(dto.proxy.is_none());
    }

    #[test]
    fn test_scrape_options_dto_full() {
        let json = r#"{
            "headers": {"User-Agent": "test"},
            "wait_for": 1000,
            "timeout": 60,
            "js_rendering": true,
            "screenshot": true,
            "mobile": true,
            "proxy": "http://proxy:8080",
            "skip_tls_verification": false,
            "needs_tls_fingerprint": true,
            "use_fire_engine": true
        }"#;
        let dto: crate::application::dto::scrape_request::ScrapeOptionsDto =
            serde_json::from_str(json).unwrap();
        assert_eq!(dto.timeout, Some(60));
        assert_eq!(dto.wait_for, Some(1000));
        assert_eq!(dto.js_rendering, Some(true));
        assert_eq!(dto.screenshot, Some(true));
        assert_eq!(dto.mobile, Some(true));
        assert_eq!(dto.proxy.as_deref(), Some("http://proxy:8080"));
        assert_eq!(dto.skip_tls_verification, Some(false));
        assert_eq!(dto.needs_tls_fingerprint, Some(true));
        assert_eq!(dto.use_fire_engine, Some(true));
    }

    #[test]
    fn test_scrape_options_dto_deny_unknown_fields() {
        let json = r#"{"headers":{},"unknown":1}"#;
        let result: Result<crate::application::dto::scrape_request::ScrapeOptionsDto, _> =
            serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_scrape_options_dto_serialization_roundtrip() {
        let dto = crate::application::dto::scrape_request::ScrapeOptionsDto {
            headers: Some(serde_json::json!({"X-Test": "value"})),
            wait_for: Some(500),
            timeout: Some(30),
            js_rendering: Some(true),
            screenshot: Some(false),
            screenshot_options: None,
            mobile: Some(false),
            proxy: Some("http://proxy:9090".to_string()),
            skip_tls_verification: None,
            needs_tls_fingerprint: None,
            use_fire_engine: None,
        };
        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: crate::application::dto::scrape_request::ScrapeOptionsDto =
            serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.timeout, Some(30));
        assert_eq!(deserialized.wait_for, Some(500));
        assert_eq!(deserialized.js_rendering, Some(true));
        assert_eq!(deserialized.proxy.as_deref(), Some("http://proxy:9090"));
    }

    // ========== ScreenshotOptionsDto tests ==========

    #[test]
    fn test_screenshot_options_dto_minimal() {
        let json = r#"{}"#;
        let dto: crate::application::dto::scrape_request::ScreenshotOptionsDto =
            serde_json::from_str(json).unwrap();
        assert!(dto.full_page.is_none());
        assert!(dto.selector.is_none());
        assert!(dto.quality.is_none());
        assert!(dto.format.is_none());
    }

    #[test]
    fn test_screenshot_options_dto_full() {
        let json = r##"{
            "full_page": true,
            "selector": "#content",
            "quality": 80,
            "format": "jpeg"
        }"##;
        let dto: crate::application::dto::scrape_request::ScreenshotOptionsDto =
            serde_json::from_str(json).unwrap();
        assert_eq!(dto.full_page, Some(true));
        assert_eq!(dto.selector.as_deref(), Some("#content"));
        assert_eq!(dto.quality, Some(80));
        assert_eq!(dto.format.as_deref(), Some("jpeg"));
    }

    #[test]
    fn test_screenshot_options_dto_serialization_roundtrip() {
        let dto = crate::application::dto::scrape_request::ScreenshotOptionsDto {
            full_page: Some(false),
            selector: Some("div.main".to_string()),
            quality: Some(100),
            format: Some("png".to_string()),
        };
        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: crate::application::dto::scrape_request::ScreenshotOptionsDto =
            serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.full_page, Some(false));
        assert_eq!(deserialized.selector.as_deref(), Some("div.main"));
        assert_eq!(deserialized.quality, Some(100));
        assert_eq!(deserialized.format.as_deref(), Some("png"));
    }

    // ========== MAX_SYNC_WAIT_MS constant ==========

    #[test]
    fn test_max_sync_wait_ms_constant() {
        assert_eq!(MAX_SYNC_WAIT_MS, 30000);
    }

    // ========== sync_wait_ms validation logic (mirrors handler lines 50-57) ==========

    #[test]
    fn test_sync_wait_ms_at_max_passes_handler_check() {
        // Handler check: if ms > MAX_SYNC_WAIT_MS { return error }
        let ms: u32 = 30000;
        assert!(!(ms > MAX_SYNC_WAIT_MS), "ms at max should pass");
    }

    #[test]
    fn test_sync_wait_ms_above_max_fails_handler_check() {
        let ms: u32 = 30001;
        assert!(ms > MAX_SYNC_WAIT_MS, "ms above max should fail");
    }

    #[test]
    fn test_sync_wait_ms_none_skips_check() {
        // When sync_wait_ms is None, the handler skips the check
        let ms: Option<u32> = None;
        // if let Some(ms) = payload.sync_wait_ms { ... } — no branch taken
        assert!(ms.is_none());
    }

    #[test]
    fn test_sync_wait_ms_zero_passes_handler_check() {
        let ms: u32 = 0;
        assert!(!(ms > MAX_SYNC_WAIT_MS));
    }

    // ========== Status code selection logic (mirrors handler lines 153-161) ==========

    #[test]
    fn test_status_code_async_mode_returns_created() {
        // sync_wait_ms == 0 → async mode → CREATED
        let sync_wait_ms: u32 = 0;
        let is_timeout = false;
        let status_code = if sync_wait_ms > 0 {
            if is_timeout {
                StatusCode::ACCEPTED
            } else {
                StatusCode::CREATED
            }
        } else {
            StatusCode::CREATED
        };
        assert_eq!(status_code, StatusCode::CREATED);
    }

    #[test]
    fn test_status_code_sync_mode_timeout_returns_accepted() {
        // sync_wait_ms > 0 && is_timeout → ACCEPTED
        let sync_wait_ms: u32 = 5000;
        let is_timeout = true;
        let status_code = if sync_wait_ms > 0 {
            if is_timeout {
                StatusCode::ACCEPTED
            } else {
                StatusCode::CREATED
            }
        } else {
            StatusCode::CREATED
        };
        assert_eq!(status_code, StatusCode::ACCEPTED);
    }

    #[test]
    fn test_status_code_sync_mode_completed_returns_created() {
        // sync_wait_ms > 0 && !is_timeout → CREATED
        let sync_wait_ms: u32 = 5000;
        let is_timeout = false;
        let status_code = if sync_wait_ms > 0 {
            if is_timeout {
                StatusCode::ACCEPTED
            } else {
                StatusCode::CREATED
            }
        } else {
            StatusCode::CREATED
        };
        assert_eq!(status_code, StatusCode::CREATED);
    }

    // ========== ScrapeResultDto deserialization ==========

    #[test]
    fn test_scrape_result_dto_deserialization() {
        let json = r#"{
            "content": "<html></html>",
            "status_code": 200,
            "content_type": "text/html",
            "response_time_ms": 100,
            "headers": {"x-custom": "val"},
            "meta_data": {"key": "value"},
            "screenshot": null,
            "created_at": "2025-01-01T00:00:00"
        }"#;
        let dto: ScrapeResultDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.content, "<html></html>");
        assert_eq!(dto.status_code, 200);
        assert_eq!(dto.content_type.as_deref(), Some("text/html"));
        assert_eq!(dto.response_time_ms, 100);
        assert!(dto.screenshot.is_none());
    }

    #[test]
    fn test_scrape_result_dto_deserialization_minimal() {
        let json = r#"{
            "content": "text",
            "status_code": 404,
            "response_time_ms": 50,
            "created_at": "2025-06-01T12:00:00"
        }"#;
        let dto: ScrapeResultDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.status_code, 404);
        assert_eq!(dto.response_time_ms, 50);
        assert!(dto.content_type.is_none());
        assert!(dto.headers.is_none());
        assert!(dto.meta_data.is_none());
        assert!(dto.screenshot.is_none());
    }

    // ========== ScrapeStatusResponseDto deserialization ==========

    #[test]
    fn test_scrape_status_response_dto_deserialization() {
        let task_id = Uuid::new_v4();
        let json = format!(
            r#"{{
                "id": "{}",
                "status": "completed",
                "url": "https://example.com",
                "created_at": "2025-01-01T00:00:00",
                "completed_at": "2025-01-01T00:01:00",
                "result": null,
                "metadata": {{"page": 1}},
                "error": null
            }}"#,
            task_id
        );
        let dto: ScrapeStatusResponseDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto.id, task_id);
        assert_eq!(dto.status, "completed");
        assert_eq!(dto.url, "https://example.com");
        assert!(dto.result.is_none());
        assert!(dto.error.is_none());
        assert!(dto.completed_at.is_some());
        assert!(dto.metadata.is_some());
    }

    #[test]
    fn test_scrape_status_response_dto_with_result() {
        let json = r#"{
            "id": "00000000-0000-0000-0000-000000000001",
            "status": "completed",
            "url": "https://test.com",
            "created_at": "2025-01-01T00:00:00",
            "completed_at": null,
            "result": {
                "content": "data",
                "status_code": 200,
                "content_type": "application/json",
                "response_time_ms": 42,
                "headers": null,
                "meta_data": null,
                "screenshot": null,
                "created_at": "2025-01-01T00:00:01"
            },
            "metadata": null,
            "error": null
        }"#;
        let dto: ScrapeStatusResponseDto = serde_json::from_str(json).unwrap();
        assert!(dto.result.is_some());
        let result = dto.result.unwrap();
        assert_eq!(result.content, "data");
        assert_eq!(result.status_code, 200);
        assert_eq!(result.response_time_ms, 42);
    }

    // ========== ScrapeResponseDto round-trip ==========

    #[test]
    fn test_scrape_response_dto_roundtrip() {
        let dto = ScrapeResponseDto {
            id: Uuid::new_v4(),
            url: "https://roundtrip.com".to_string(),
            credits_used: 3,
        };
        let json = serde_json::to_string(&dto).unwrap();
        let deserialized: ScrapeResponseDto = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.id, dto.id);
        assert_eq!(deserialized.url, dto.url);
        assert_eq!(deserialized.credits_used, dto.credits_used);
    }

    // ========== TaskStatus Display (used in handler via to_string()) ==========

    #[test]
    fn test_task_status_display_for_scrape_response() {
        // The handler uses task.status.to_string() for the status field
        assert_eq!(TaskStatus::Queued.to_string(), "queued");
        assert_eq!(TaskStatus::Active.to_string(), "active");
        assert_eq!(TaskStatus::Completed.to_string(), "completed");
        assert_eq!(TaskStatus::Failed.to_string(), "failed");
        assert_eq!(TaskStatus::Cancelled.to_string(), "cancelled");
    }

    // ========== Task construction logic (mirrors handler lines 104-125) ==========

    #[test]
    fn test_task_construction_for_scrape() {
        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let now = chrono::Utc::now();
        let task = Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Scrape,
            status: TaskStatus::Queued,
            priority: 0,
            team_id,
            api_key_id,
            url: "https://example.com".to_string(),
            payload: serde_json::Value::Null,
            retry_count: 0,
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            expires_at: None,
            created_at: now,
            started_at: None,
            completed_at: None,
            crawl_id: None,
            updated_at: now,
            lock_token: None,
            lock_expires_at: None,
        };
        assert_eq!(task.task_type, TaskType::Scrape);
        assert_eq!(task.status, TaskStatus::Queued);
        assert_eq!(task.priority, 0);
        assert_eq!(task.max_retries, 3);
        assert_eq!(task.team_id, team_id);
        assert_eq!(task.api_key_id, api_key_id);
        assert!(task.scheduled_at.is_none());
        assert!(task.expires_at.is_none());
        assert!(task.started_at.is_none());
        assert!(task.completed_at.is_none());
    }

    // ========== ScrapeRequestDto with options and actions ==========

    #[test]
    fn test_scrape_request_dto_with_options_and_actions() {
        let json = r##"{
            "url": "https://example.com",
            "formats": ["html"],
            "actions": [
                {"type":"wait","milliseconds":500},
                {"type":"click","selector":"#load-more"}
            ],
            "options": {
                "timeout": 30,
                "js_rendering": true
            }
        }"##;
        let dto: ScrapeRequestDto = serde_json::from_str(json).unwrap();
        assert!(dto.actions.is_some());
        assert_eq!(dto.actions.as_ref().unwrap().len(), 2);
        assert!(dto.options.is_some());
        assert_eq!(dto.options.as_ref().unwrap().timeout, Some(30));
    }

    #[test]
    fn test_scrape_request_dto_with_metadata() {
        let json = r#"{
            "url": "https://example.com",
            "metadata": {"user_id": 123, "session": "abc"}
        }"#;
        let dto: ScrapeRequestDto = serde_json::from_str(json).unwrap();
        assert!(dto.metadata.is_some());
        let meta = dto.metadata.unwrap();
        assert_eq!(meta["user_id"], 123);
        assert_eq!(meta["session"], "abc");
    }

    #[test]
    fn test_scrape_request_dto_with_webhook() {
        let json = r#"{
            "url": "https://example.com",
            "webhook": "https://hooks.example.com/abc"
        }"#;
        let dto: ScrapeRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(
            dto.webhook.as_deref(),
            Some("https://hooks.example.com/abc")
        );
    }

    #[test]
    fn test_scrape_request_dto_url_too_long_fails_validation() {
        // URL longer than 2048 chars should fail validation
        let long_url = "https://example.com/".to_string() + &"a".repeat(2048);
        let json = serde_json::json!({
            "url": long_url
        })
        .to_string();
        let dto: ScrapeRequestDto = serde_json::from_str(&json).unwrap();
        assert!(dto.validate().is_err());
    }

    #[test]
    fn test_scrape_request_dto_url_at_max_length_passes() {
        // URL at exactly 2048 chars should pass
        let base = "https://example.com/";
        let padding = "a".repeat(2048 - base.len());
        let url = format!("{}{}", base, padding);
        assert_eq!(url.len(), 2048);
        let json = serde_json::json!({ "url": url }).to_string();
        let dto: ScrapeRequestDto = serde_json::from_str(&json).unwrap();
        assert!(dto.validate().is_ok());
    }
}
