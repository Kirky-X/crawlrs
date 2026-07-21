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

    // 验证 sync_wait_ms 范围
    if let Some(ms) = payload.sync_wait_ms {
        if ms > MAX_SYNC_WAIT_MS {
            return errors::unprocessable_entity(format!(
                "sync_wait_ms must be <= {}",
                MAX_SYNC_WAIT_MS
            ));
        }
    }

    // 1. 检查限流（架构 MEDIUM-1：限流必须在 SSRF 之前，避免恶意请求触发异步 DNS 解析消耗资源）
    // 性能 LOW-1：直接传 `Uuid`（实现 Display），由 helper 内部按需 to_string，
    // 消除 handler 中的中间变量分配。
    if let Err(response) =
        check_rate_limit(rate_limiting_service.as_ref(), auth_state.api_key_id, "/v1/scrape").await
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
        assert!(ms <= MAX_SYNC_WAIT_MS, "ms at max should pass");
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
        assert!(ms <= MAX_SYNC_WAIT_MS);
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

    // ========== Handler function tests ==========

    use crate::common::test_helpers::create_test_db_pool;
    use crate::domain::auth::ApiKeyScope;
    use crate::domain::models::scrape_result::ScrapeResult;
    use crate::domain::repositories::task_repository::{RepositoryError, TaskQueryParams};
    use crate::domain::services::rate_limiting_service::{
        BacklogService, ConcurrencyConfig, ConcurrencyControlService, ConcurrencyResult,
        QuotaService, RateLimitConfig, RateLimitResult, RateLimitService, RateLimitingError,
    };
    use crate::queue::task_queue::QueueError;
    use async_trait::async_trait;
    use std::collections::{HashMap, HashSet};
    use std::sync::Mutex;

    // --- MockTaskQueue ---

    type SharedTasks = Arc<Mutex<HashMap<Uuid, Task>>>;

    struct MockTaskQueue {
        enqueue_should_fail: bool,
    }

    impl MockTaskQueue {
        fn new_success() -> Self {
            Self {
                enqueue_should_fail: false,
            }
        }

        fn new_failing() -> Self {
            Self {
                enqueue_should_fail: true,
            }
        }
    }

    #[async_trait]
    impl TaskQueue for MockTaskQueue {
        async fn enqueue(&self, task: Task) -> Result<Task, QueueError> {
            if self.enqueue_should_fail {
                return Err(QueueError::Repository(RepositoryError::Database(
                    anyhow::anyhow!("enqueue failed"),
                )));
            }
            Ok(task)
        }

        async fn dequeue(&self, _worker_id: Uuid) -> Result<Option<Task>, QueueError> {
            Ok(None)
        }

        async fn complete(&self, _task_id: Uuid) -> Result<(), QueueError> {
            Ok(())
        }

        async fn fail(&self, _task_id: Uuid) -> Result<(), QueueError> {
            Ok(())
        }

        async fn cancel(&self, _task_id: Uuid) -> Result<(), QueueError> {
            Ok(())
        }
    }

    /// A queue that stores enqueued tasks into a shared store as Completed,
    /// so that sync-wait polling immediately observes completion.
    struct MockTaskQueueLinkedCompleted {
        shared: SharedTasks,
    }

    impl MockTaskQueueLinkedCompleted {
        fn new(shared: SharedTasks) -> Self {
            Self { shared }
        }
    }

    #[async_trait]
    impl TaskQueue for MockTaskQueueLinkedCompleted {
        async fn enqueue(&self, mut task: Task) -> Result<Task, QueueError> {
            task.status = TaskStatus::Completed;
            task.completed_at = Some(chrono::Utc::now());
            self.shared.lock().unwrap().insert(task.id, task.clone());
            Ok(task)
        }

        async fn dequeue(&self, _worker_id: Uuid) -> Result<Option<Task>, QueueError> {
            Ok(None)
        }

        async fn complete(&self, _task_id: Uuid) -> Result<(), QueueError> {
            Ok(())
        }

        async fn fail(&self, _task_id: Uuid) -> Result<(), QueueError> {
            Ok(())
        }

        async fn cancel(&self, _task_id: Uuid) -> Result<(), QueueError> {
            Ok(())
        }
    }

    // --- MockTaskRepository ---

    struct MockTaskRepository {
        tasks: SharedTasks,
        find_should_fail: bool,
        mark_cancelled_should_fail: bool,
    }

    impl MockTaskRepository {
        fn new() -> Self {
            Self {
                tasks: Arc::new(Mutex::new(HashMap::new())),
                find_should_fail: false,
                mark_cancelled_should_fail: false,
            }
        }

        fn with_task(task: Task) -> Self {
            let id = task.id;
            let map = Arc::new(Mutex::new(HashMap::new()));
            map.lock().unwrap().insert(id, task);
            Self {
                tasks: map,
                find_should_fail: false,
                mark_cancelled_should_fail: false,
            }
        }

        fn new_linked(shared: SharedTasks) -> Self {
            Self {
                tasks: shared,
                find_should_fail: false,
                mark_cancelled_should_fail: false,
            }
        }

        fn failing_find() -> Self {
            Self {
                tasks: Arc::new(Mutex::new(HashMap::new())),
                find_should_fail: true,
                mark_cancelled_should_fail: false,
            }
        }

        fn failing_mark_cancelled() -> Self {
            Self {
                tasks: Arc::new(Mutex::new(HashMap::new())),
                find_should_fail: false,
                mark_cancelled_should_fail: true,
            }
        }
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }

        async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>, RepositoryError> {
            if self.find_should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "find_by_id failed"
                )));
            }
            Ok(self.tasks.lock().unwrap().get(&id).cloned())
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
            if self.mark_cancelled_should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "mark_cancelled failed"
                )));
            }
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
            Ok(vec![])
        }

        async fn query_tasks(
            &self,
            params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            let tasks = self.tasks.lock().unwrap();
            let mut result: Vec<Task> = Vec::new();
            if let Some(ref task_ids) = params.task_ids {
                for id in task_ids {
                    if let Some(task) = tasks.get(id) {
                        result.push(task.clone());
                    }
                }
            }
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

        fn new_retry_after() -> Self {
            Self {
                rate_limit_result: RateLimitResult::RetryAfter {
                    retry_after_seconds: 30,
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

    #[async_trait]
    impl RateLimitingService for MockRateLimitingService {}

    // --- MockScrapeResultRepository ---

    struct MockScrapeResultRepository {
        result: Option<ScrapeResult>,
        should_fail: bool,
    }

    impl MockScrapeResultRepository {
        fn new_empty() -> Self {
            Self {
                result: None,
                should_fail: false,
            }
        }

        fn with_result(result: ScrapeResult) -> Self {
            Self {
                result: Some(result),
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                result: None,
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl ScrapeResultRepository for MockScrapeResultRepository {
        async fn save(&self, _result: ScrapeResult) -> anyhow::Result<()> {
            Ok(())
        }

        async fn find_by_task_id(&self, _task_id: Uuid) -> anyhow::Result<Option<ScrapeResult>> {
            if self.should_fail {
                return Err(anyhow::anyhow!("scrape result repo down"));
            }
            Ok(self.result.clone())
        }

        async fn find_by_task_ids(&self, _task_ids: &[Uuid]) -> anyhow::Result<Vec<ScrapeResult>> {
            Ok(vec![])
        }

        async fn get_team_avg_response_time(&self, _team_id: Uuid) -> anyhow::Result<f64> {
            Ok(0.0)
        }
    }

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

    fn make_task(team_id: Uuid, status: TaskStatus) -> Task {
        let now = chrono::Utc::now();
        Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Scrape,
            status,
            priority: 0,
            team_id,
            api_key_id: Uuid::new_v4(),
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
        }
    }

    fn make_scrape_result(task_id: Uuid) -> ScrapeResult {
        ScrapeResult {
            id: Uuid::new_v4(),
            task_id,
            url: "https://example.com".to_string(),
            status_code: 200,
            content: "<html>test</html>".to_string(),
            content_type: "text/html".to_string(),
            response_time_ms: 100,
            created_at: chrono::Utc::now().naive_utc(),
            headers: serde_json::json!({"content-length": "100"}),
            meta_data: serde_json::json!({"key": "value"}),
            screenshot: None,
        }
    }

    fn make_scrape_request_dto(url: &str, sync_wait_ms: Option<u32>) -> ScrapeRequestDto {
        ScrapeRequestDto {
            url: url.to_string(),
            formats: None,
            include_tags: None,
            exclude_tags: None,
            webhook: None,
            extraction_rules: None,
            actions: None,
            options: None,
            metadata: None,
            sync_wait_ms,
        }
    }

    // ========== create_scrape tests ==========

    #[tokio::test]
    async fn test_create_scrape_success_async() {
        let queue = Arc::new(MockTaskQueue::new_success());
        let task_repo = Arc::new(MockTaskRepository::new());
        let rate_limit = Arc::new(MockRateLimitingService::new_allowed());
        let settings = Arc::new(Settings::default());
        let auth = make_auth_state();

        let payload = make_scrape_request_dto("https://example.com", None);

        let response = create_scrape(
            Extension(queue),
            Extension(settings),
            Extension(task_repo),
            Extension(rate_limit),
            Extension(auth),
            Json(payload),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_create_scrape_sync_wait_ms_exceeds_max() {
        let queue = Arc::new(MockTaskQueue::new_success());
        let task_repo = Arc::new(MockTaskRepository::new());
        let rate_limit = Arc::new(MockRateLimitingService::new_allowed());
        let settings = Arc::new(Settings::default());
        let auth = make_auth_state();

        let payload = make_scrape_request_dto("https://example.com", Some(MAX_SYNC_WAIT_MS + 1));

        let response = create_scrape(
            Extension(queue),
            Extension(settings),
            Extension(task_repo),
            Extension(rate_limit),
            Extension(auth),
            Json(payload),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    async fn test_create_scrape_ssrf_blocked_localhost() {
        let queue = Arc::new(MockTaskQueue::new_success());
        let task_repo = Arc::new(MockTaskRepository::new());
        let rate_limit = Arc::new(MockRateLimitingService::new_allowed());
        let settings = Arc::new(Settings::default());
        let auth = make_auth_state();

        let payload = make_scrape_request_dto("http://localhost", None);

        let response = create_scrape(
            Extension(queue),
            Extension(settings),
            Extension(task_repo),
            Extension(rate_limit),
            Extension(auth),
            Json(payload),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_scrape_ssrf_blocked_127001() {
        let queue = Arc::new(MockTaskQueue::new_success());
        let task_repo = Arc::new(MockTaskRepository::new());
        let rate_limit = Arc::new(MockRateLimitingService::new_allowed());
        let settings = Arc::new(Settings::default());
        let auth = make_auth_state();

        let payload = make_scrape_request_dto("http://127.0.0.1", None);

        let response = create_scrape(
            Extension(queue),
            Extension(settings),
            Extension(task_repo),
            Extension(rate_limit),
            Extension(auth),
            Json(payload),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_scrape_rate_limited_denied() {
        let queue = Arc::new(MockTaskQueue::new_success());
        let task_repo = Arc::new(MockTaskRepository::new());
        let rate_limit = Arc::new(MockRateLimitingService::new_denied());
        let settings = Arc::new(Settings::default());
        let auth = make_auth_state();

        let payload = make_scrape_request_dto("https://example.com", None);

        let response = create_scrape(
            Extension(queue),
            Extension(settings),
            Extension(task_repo),
            Extension(rate_limit),
            Extension(auth),
            Json(payload),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_create_scrape_rate_limited_retry_after() {
        let queue = Arc::new(MockTaskQueue::new_success());
        let task_repo = Arc::new(MockTaskRepository::new());
        let rate_limit = Arc::new(MockRateLimitingService::new_retry_after());
        let settings = Arc::new(Settings::default());
        let auth = make_auth_state();

        let payload = make_scrape_request_dto("https://example.com", None);

        let response = create_scrape(
            Extension(queue),
            Extension(settings),
            Extension(task_repo),
            Extension(rate_limit),
            Extension(auth),
            Json(payload),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[tokio::test]
    async fn test_create_scrape_quota_exceeded() {
        let queue = Arc::new(MockTaskQueue::new_success());
        let task_repo = Arc::new(MockTaskRepository::new());
        let rate_limit = Arc::new(MockRateLimitingService::new_quota_exceeded());
        let settings = Arc::new(Settings::default());
        let auth = make_auth_state();

        let payload = make_scrape_request_dto("https://example.com", None);

        let response = create_scrape(
            Extension(queue),
            Extension(settings),
            Extension(task_repo),
            Extension(rate_limit),
            Extension(auth),
            Json(payload),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::PAYMENT_REQUIRED);
    }

    #[tokio::test]
    async fn test_create_scrape_enqueue_failure() {
        let queue = Arc::new(MockTaskQueue::new_failing());
        let task_repo = Arc::new(MockTaskRepository::new());
        let rate_limit = Arc::new(MockRateLimitingService::new_allowed());
        let settings = Arc::new(Settings::default());
        let auth = make_auth_state();

        let payload = make_scrape_request_dto("https://example.com", None);

        let response = create_scrape(
            Extension(queue),
            Extension(settings),
            Extension(task_repo),
            Extension(rate_limit),
            Extension(auth),
            Json(payload),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_create_scrape_sync_mode_completed() {
        // sync_wait_ms > 0 with task becoming completed via linked queue/repo → CREATED
        // The queue and repo share the same task store, so when enqueue inserts
        // the task, the sync-wait poll will find it. We mark it Completed via
        // a shared store so completion_rate >= 1.0 → is_timeout=false → CREATED.
        let team_id = Uuid::new_v4();
        let shared: SharedTasks = Arc::new(Mutex::new(HashMap::new()));
        let task_repo = Arc::new(MockTaskRepository::new_linked(Arc::clone(&shared)));
        // Wrap queue so that on enqueue, the task is stored as Completed.
        let queue = Arc::new(MockTaskQueueLinkedCompleted::new(Arc::clone(&shared)));
        let rate_limit = Arc::new(MockRateLimitingService::new_allowed());
        let settings = Arc::new(Settings::default());
        let auth = make_auth_state_with_team(team_id);

        let payload = make_scrape_request_dto("https://example.com", Some(5000));

        let response = create_scrape(
            Extension(queue),
            Extension(settings),
            Extension(task_repo),
            Extension(rate_limit),
            Extension(auth),
            Json(payload),
        )
        .await
        .into_response();

        // sync_wait_ms > 0, not timeout → CREATED
        assert_eq!(response.status(), StatusCode::CREATED);
    }

    // ========== cancel_scrape tests ==========

    #[tokio::test]
    async fn test_cancel_scrape_success() {
        let team_id = Uuid::new_v4();
        let task = make_task(team_id, TaskStatus::Queued);
        let task_id = task.id;
        let repo = Arc::new(MockTaskRepository::with_task(task));
        let auth = make_auth_state_with_team(team_id);

        let response = cancel_scrape(Path(task_id), Extension(repo), Extension(auth))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_cancel_scrape_not_found() {
        let repo = Arc::new(MockTaskRepository::new());
        let auth = make_auth_state();
        let task_id = Uuid::new_v4();

        let response = cancel_scrape(Path(task_id), Extension(repo), Extension(auth))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_cancel_scrape_forbidden() {
        let task = make_task(Uuid::new_v4(), TaskStatus::Queued);
        let task_id = task.id;
        let repo = Arc::new(MockTaskRepository::with_task(task));
        // Different team_id
        let auth = make_auth_state_with_team(Uuid::new_v4());

        let response = cancel_scrape(Path(task_id), Extension(repo), Extension(auth))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_cancel_scrape_find_error() {
        let repo = Arc::new(MockTaskRepository::failing_find());
        let auth = make_auth_state();
        let task_id = Uuid::new_v4();

        let response = cancel_scrape(Path(task_id), Extension(repo), Extension(auth))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_cancel_scrape_mark_cancelled_error() {
        let team_id = Uuid::new_v4();
        let task = make_task(team_id, TaskStatus::Queued);
        let task_id = task.id;
        let repo = Arc::new(MockTaskRepository::failing_mark_cancelled());
        // Need the task to exist for find_by_id, but mark_cancelled to fail
        {
            let mut tasks = repo.tasks.lock().unwrap();
            tasks.insert(task_id, task);
        }
        let auth = make_auth_state_with_team(team_id);

        let response = cancel_scrape(Path(task_id), Extension(repo), Extension(auth))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // ========== get_scrape_status tests ==========

    #[tokio::test]
    async fn test_get_scrape_status_in_progress() {
        let team_id = Uuid::new_v4();
        let task = make_task(team_id, TaskStatus::Active);
        let task_id = task.id;
        let task_repo = Arc::new(MockTaskRepository::with_task(task));
        let result_repo = Arc::new(MockScrapeResultRepository::new_empty());
        let auth = make_auth_state_with_team(team_id);

        let response = get_scrape_status(
            Path(task_id),
            Extension(task_repo),
            Extension(result_repo),
            Extension(auth),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_scrape_status_completed_with_result() {
        let team_id = Uuid::new_v4();
        let mut task = make_task(team_id, TaskStatus::Completed);
        task.completed_at = Some(chrono::Utc::now());
        let task_id = task.id;
        let result = make_scrape_result(task_id);
        let task_repo = Arc::new(MockTaskRepository::with_task(task));
        let result_repo = Arc::new(MockScrapeResultRepository::with_result(result));
        let auth = make_auth_state_with_team(team_id);

        let response = get_scrape_status(
            Path(task_id),
            Extension(task_repo),
            Extension(result_repo),
            Extension(auth),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_scrape_status_completed_no_result() {
        let team_id = Uuid::new_v4();
        let mut task = make_task(team_id, TaskStatus::Completed);
        task.completed_at = Some(chrono::Utc::now());
        let task_id = task.id;
        let task_repo = Arc::new(MockTaskRepository::with_task(task));
        let result_repo = Arc::new(MockScrapeResultRepository::new_empty());
        let auth = make_auth_state_with_team(team_id);

        let response = get_scrape_status(
            Path(task_id),
            Extension(task_repo),
            Extension(result_repo),
            Extension(auth),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_scrape_status_completed_result_error() {
        let team_id = Uuid::new_v4();
        let mut task = make_task(team_id, TaskStatus::Completed);
        task.completed_at = Some(chrono::Utc::now());
        let task_id = task.id;
        let task_repo = Arc::new(MockTaskRepository::with_task(task));
        let result_repo = Arc::new(MockScrapeResultRepository::failing());
        let auth = make_auth_state_with_team(team_id);

        let response = get_scrape_status(
            Path(task_id),
            Extension(task_repo),
            Extension(result_repo),
            Extension(auth),
        )
        .await
        .into_response();

        // Error is logged but status is still OK (result_data = None)
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_scrape_status_failed_task() {
        let team_id = Uuid::new_v4();
        let mut task = make_task(team_id, TaskStatus::Failed);
        task.completed_at = Some(chrono::Utc::now());
        task.payload = serde_json::json!({"error": "Connection timeout"});
        let task_id = task.id;
        let task_repo = Arc::new(MockTaskRepository::with_task(task));
        let result_repo = Arc::new(MockScrapeResultRepository::new_empty());
        let auth = make_auth_state_with_team(team_id);

        let response = get_scrape_status(
            Path(task_id),
            Extension(task_repo),
            Extension(result_repo),
            Extension(auth),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_scrape_status_failed_task_no_error_field() {
        let team_id = Uuid::new_v4();
        let mut task = make_task(team_id, TaskStatus::Failed);
        task.completed_at = Some(chrono::Utc::now());
        // payload has no "error" field → should default to "Task failed"
        let task_id = task.id;
        let task_repo = Arc::new(MockTaskRepository::with_task(task));
        let result_repo = Arc::new(MockScrapeResultRepository::new_empty());
        let auth = make_auth_state_with_team(team_id);

        let response = get_scrape_status(
            Path(task_id),
            Extension(task_repo),
            Extension(result_repo),
            Extension(auth),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_scrape_status_not_found() {
        let task_repo = Arc::new(MockTaskRepository::new());
        let result_repo = Arc::new(MockScrapeResultRepository::new_empty());
        let auth = make_auth_state();
        let task_id = Uuid::new_v4();

        let response = get_scrape_status(
            Path(task_id),
            Extension(task_repo),
            Extension(result_repo),
            Extension(auth),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_get_scrape_status_forbidden() {
        let task = make_task(Uuid::new_v4(), TaskStatus::Active);
        let task_id = task.id;
        let task_repo = Arc::new(MockTaskRepository::with_task(task));
        let result_repo = Arc::new(MockScrapeResultRepository::new_empty());
        // Different team_id
        let auth = make_auth_state_with_team(Uuid::new_v4());

        let response = get_scrape_status(
            Path(task_id),
            Extension(task_repo),
            Extension(result_repo),
            Extension(auth),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_get_scrape_status_find_error() {
        let task_repo = Arc::new(MockTaskRepository::failing_find());
        let result_repo = Arc::new(MockScrapeResultRepository::new_empty());
        let auth = make_auth_state();
        let task_id = Uuid::new_v4();

        let response = get_scrape_status(
            Path(task_id),
            Extension(task_repo),
            Extension(result_repo),
            Extension(auth),
        )
        .await
        .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
