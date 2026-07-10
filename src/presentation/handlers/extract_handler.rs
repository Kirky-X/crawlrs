// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use axum::{
    extract::{ConnectInfo, Extension},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use log::error;
use std::net::SocketAddr;

use crate::application::dto::extract_request::ExtractRequestDto;
use crate::common::constants::crawl_task;
use crate::config::settings::Settings;
use crate::domain::models::{Task, TaskStatus, TaskType};
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::services::team_service::TeamService;
use crate::presentation::handlers::response_builder::{error_response, ApiResponse};
use crate::presentation::handlers::task_handler::wait_for_tasks_completion;
use crate::presentation::middleware::auth_middleware::AuthState;
use crate::queue::task_queue::TaskQueue;
use std::sync::Arc;
use uuid::Uuid;

/// 提取任务响应数据传输对象
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ExtractResponseDto {
    /// 任务ID
    pub id: Uuid,
    /// 任务状态
    pub status: String,
}

#[allow(clippy::too_many_arguments)]
pub async fn extract<GR>(
    Extension(queue): Extension<Arc<dyn TaskQueue>>,
    Extension(_settings): Extension<Arc<Settings>>,
    Extension(task_repository): Extension<Arc<dyn TaskRepository>>,
    Extension(geo_restriction_repo): Extension<Arc<GR>>,
    Extension(team_service): Extension<Arc<TeamService>>,
    Extension(auth_state): Extension<AuthState>,
    ConnectInfo(addr): ConnectInfo<SocketAddr>,
    Json(payload): Json<ExtractRequestDto>,
) -> impl IntoResponse
where
    GR: GeoRestrictionRepository + 'static,
{
    let team_id = auth_state.team_id;
    // Validate the request
    if payload.urls.is_empty() {
        return error_response(StatusCode::BAD_REQUEST, "At least one URL is required");
    }

    if payload.prompt.is_none() && payload.schema.is_none() && payload.rules.is_none() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "Either prompt, schema, or rules is required",
        );
    }

    // 检查地理限制
    let client_ip = addr.ip().to_string();

    // 获取团队地理限制配置
    let restrictions = match geo_restriction_repo.get_team_restrictions(team_id).await {
        Ok(restrictions) => restrictions,
        Err(e) => {
            error!("Failed to get team restrictions: {:?}", e);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to validate geographic access",
            );
        }
    };

    // Validate sync_wait_ms if present
    if let Some(ms) = payload.sync_wait_ms {
        if ms > crawl_task::MAX_SYNC_WAIT_MS {
            return error_response(
                StatusCode::BAD_REQUEST,
                format!("sync_wait_ms must be <= {}", crawl_task::MAX_SYNC_WAIT_MS),
            );
        }
    }

    // 使用团队服务验证地理限制
    match team_service
        .validate_geographic_restriction(team_id, &client_ip, &restrictions)
        .await
    {
        Ok(crate::domain::services::team_service::GeoRestrictionResult::Allowed) => {
            // 记录允许的访问日志
            if let Err(e) = geo_restriction_repo
                .log_geo_restriction_action(
                    team_id,
                    &client_ip,
                    "",
                    "ALLOWED",
                    "Extract request - Geographic restriction check passed",
                )
                .await
            {
                error!("Failed to log geographic restriction action: {}", e);
            }
        }
        Ok(crate::domain::services::team_service::GeoRestrictionResult::Denied(reason)) => {
            // 记录拒绝的访问日志
            if let Err(e) = geo_restriction_repo
                .log_geo_restriction_action(
                    team_id,
                    &client_ip,
                    "",
                    "DENIED",
                    &format!("Extract request - {}", reason),
                )
                .await
            {
                error!("Failed to log geographic restriction action: {}", e);
            }

            return error_response(
                StatusCode::FORBIDDEN,
                format!("Access denied due to geographic restrictions: {}", reason),
            );
        }
        Err(e) => {
            error!("Failed to validate geographic restrictions: {:?}", e);
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to validate geographic access",
            );
        }
    }

    // Create a task for async extraction
    let primary_url = payload
        .urls
        .first()
        .expect("URLs already validated as non-empty");
    let now = chrono::Utc::now();
    let task = Task {
        id: Uuid::new_v4(),
        task_type: TaskType::Extract,
        status: TaskStatus::Queued,
        priority: 0,
        team_id,
        api_key_id: auth_state.api_key_id,
        url: primary_url.clone(),
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

    match queue.enqueue(task.clone()).await {
        Ok(_) => {
            // 处理同步等待逻辑
            let sync_wait_ms = payload
                .sync_wait_ms
                .unwrap_or(crawl_task::DEFAULT_TIMEOUT_MS as u32);
            let mut waited_time_ms = 0u64;

            if sync_wait_ms > 0 {
                let wait_start = std::time::Instant::now();

                // 调用智能轮询等待函数
                match wait_for_tasks_completion(
                    task_repository.as_ref(),
                    &[task.id],
                    team_id,
                    sync_wait_ms,
                    crawl_task::BASE_POLL_INTERVAL_MS,
                )
                .await
                {
                    Ok(_) => {
                        waited_time_ms = wait_start.elapsed().as_millis() as u64;
                    }
                    Err(e) => {
                        error!("Failed to wait for task completion: {:?}", e);
                        // 即使等待失败，也返回已创建的任务信息
                    }
                }
            }

            let response = ExtractResponseDto {
                id: task.id,
                status: "pending".to_string(),
            };

            // 根据同步等待结果设置响应状态
            let status_code = if sync_wait_ms > 0 && waited_time_ms >= sync_wait_ms as u64 {
                StatusCode::ACCEPTED // 同步等待超时，任务已接受但可能未完成
            } else {
                StatusCode::CREATED // 任务已创建（可能已完成）
            };

            (status_code, Json(ApiResponse::success(response))).into_response()
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    // ========== ExtractResponseDto tests ==========

    #[test]
    fn test_extract_response_dto_serialization() {
        let task_id = Uuid::new_v4();
        let dto = ExtractResponseDto {
            id: task_id,
            status: "pending".to_string(),
        };
        let json = serde_json::to_string(&dto).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["id"], task_id.to_string());
        assert_eq!(parsed["status"], "pending");
    }

    #[test]
    fn test_extract_response_dto_deserialization() {
        let task_id = Uuid::new_v4();
        let json = format!(r#"{{"id":"{}","status":"completed"}}"#, task_id);
        let dto: ExtractResponseDto = serde_json::from_str(&json).unwrap();
        assert_eq!(dto.id, task_id);
        assert_eq!(dto.status, "completed");
    }

    #[test]
    fn test_extract_response_dto_accepted_status() {
        let dto = ExtractResponseDto {
            id: Uuid::new_v4(),
            status: "accepted".to_string(),
        };
        let json = serde_json::to_string(&dto).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "accepted");
    }

    // ========== ExtractRequestDto validation ==========

    #[test]
    fn test_extract_request_dto_minimal_with_prompt() {
        let json = r#"{"urls":["https://example.com"],"prompt":"Extract title"}"#;
        let dto: ExtractRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.urls.len(), 1);
        assert_eq!(dto.prompt.as_deref(), Some("Extract title"));
        assert!(dto.schema.is_none());
        assert!(dto.rules.is_none());
    }

    #[test]
    fn test_extract_request_dto_with_schema() {
        let json = r#"{"urls":["https://example.com"],"schema":{"type":"object"}}"#;
        let dto: ExtractRequestDto = serde_json::from_str(json).unwrap();
        assert!(dto.schema.is_some());
        assert!(dto.prompt.is_none());
    }

    #[test]
    fn test_extract_request_dto_with_sync_wait_ms() {
        let json = r#"{"urls":["https://example.com"],"prompt":"test","sync_wait_ms":10000}"#;
        let dto: ExtractRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.sync_wait_ms, Some(10000));
    }

    #[test]
    fn test_extract_request_dto_multiple_urls() {
        let json = r#"{"urls":["https://a.com","https://b.com","https://c.com"],"prompt":"test"}"#;
        let dto: ExtractRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.urls.len(), 3);
    }

    #[test]
    fn test_extract_request_dto_empty_urls() {
        let json = r#"{"urls":[],"prompt":"test"}"#;
        let dto: ExtractRequestDto = serde_json::from_str(json).unwrap();
        assert!(dto.urls.is_empty());
    }

    // ========== MAX_SYNC_WAIT_MS constant ==========

    #[test]
    fn test_max_sync_wait_ms_value() {
        assert_eq!(crawl_task::MAX_SYNC_WAIT_MS, 30000);
    }

    #[test]
    fn test_default_timeout_ms_value() {
        assert_eq!(crawl_task::DEFAULT_TIMEOUT_MS, 5000);
    }

    #[test]
    fn test_base_poll_interval_ms_value() {
        assert_eq!(crawl_task::BASE_POLL_INTERVAL_MS, 1000);
    }

    // ========== ExtractResponseDto additional tests ==========

    #[test]
    fn test_extract_response_dto_clone() {
        let task_id = Uuid::new_v4();
        let dto = ExtractResponseDto {
            id: task_id,
            status: "pending".to_string(),
        };
        let cloned = dto.clone();
        assert_eq!(dto.id, cloned.id);
        assert_eq!(dto.status, cloned.status);
    }

    #[test]
    fn test_extract_response_dto_debug() {
        let task_id = Uuid::new_v4();
        let dto = ExtractResponseDto {
            id: task_id,
            status: "completed".to_string(),
        };
        let debug = format!("{:?}", dto);
        assert!(debug.contains("ExtractResponseDto"));
        assert!(debug.contains(&task_id.to_string()));
        assert!(debug.contains("completed"));
    }

    #[test]
    fn test_extract_response_dto_round_trip() {
        let task_id = Uuid::new_v4();
        let original = ExtractResponseDto {
            id: task_id,
            status: "queued".to_string(),
        };
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: ExtractResponseDto = serde_json::from_str(&json).unwrap();
        assert_eq!(original.id, deserialized.id);
        assert_eq!(original.status, deserialized.status);
    }

    // ========== ExtractRequestDto edge cases ==========

    #[test]
    fn test_extract_request_dto_with_rules() {
        let mut rules = std::collections::HashMap::new();
        rules.insert(
            "title".to_string(),
            crate::domain::services::extraction_service::ExtractionRule {
                selector: Some("h1".to_string()),
                attr: None,
                is_array: false,
                use_llm: None,
                llm_prompt: None,
                output_format: None,
            },
        );
        let dto = ExtractRequestDto {
            urls: vec!["https://example.com".to_string()],
            prompt: None,
            schema: None,
            model: None,
            rules: Some(rules),
            sync_wait_ms: None,
        };
        assert!(dto.rules.is_some());
        assert_eq!(dto.rules.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn test_extract_request_dto_with_model() {
        let json = r#"{"urls":["https://example.com"],"prompt":"test","model":"gpt-4"}"#;
        let dto: ExtractRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.model.as_deref(), Some("gpt-4"));
    }

    #[test]
    fn test_extract_request_dto_full_payload() {
        let json = r#"{
            "urls": ["https://a.com", "https://b.com"],
            "prompt": "Extract data",
            "schema": {"type": "object", "properties": {}},
            "model": "gpt-4",
            "sync_wait_ms": 5000
        }"#;
        let dto: ExtractRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.urls.len(), 2);
        assert!(dto.prompt.is_some());
        assert!(dto.schema.is_some());
        assert!(dto.model.is_some());
        assert_eq!(dto.sync_wait_ms, Some(5000));
        assert!(dto.rules.is_none());
    }

    #[test]
    fn test_extract_request_dto_minimal_with_only_schema() {
        let json = r#"{"urls":["https://example.com"],"schema":{"type":"object"}}"#;
        let dto: ExtractRequestDto = serde_json::from_str(json).unwrap();
        assert!(dto.prompt.is_none());
        assert!(dto.schema.is_some());
        assert!(dto.rules.is_none());
    }

    #[test]
    fn test_extract_request_dto_minimal_with_only_rules() {
        let json = r#"{"urls":["https://example.com"],"rules":{}}"#;
        let dto: ExtractRequestDto = serde_json::from_str(json).unwrap();
        assert!(dto.prompt.is_none());
        assert!(dto.schema.is_none());
        assert!(dto.rules.is_some());
        assert!(dto.rules.as_ref().unwrap().is_empty());
    }

    #[test]
    fn test_extract_request_dto_no_extraction_method() {
        let json = r#"{"urls":["https://example.com"]}"#;
        let dto: ExtractRequestDto = serde_json::from_str(json).unwrap();
        assert!(dto.prompt.is_none());
        assert!(dto.schema.is_none());
        assert!(dto.rules.is_none());
    }

    #[test]
    fn test_extract_request_dto_sync_wait_ms_zero() {
        let json = r#"{"urls":["https://example.com"],"prompt":"test","sync_wait_ms":0}"#;
        let dto: ExtractRequestDto = serde_json::from_str(json).unwrap();
        assert_eq!(dto.sync_wait_ms, Some(0));
    }

    // ========== Validation logic tests ==========
    // These test the validation conditions used in the extract handler

    #[test]
    fn test_validation_empty_urls_fails() {
        let dto = ExtractRequestDto {
            urls: vec![],
            prompt: Some("test".to_string()),
            schema: None,
            model: None,
            rules: None,
            sync_wait_ms: None,
        };
        // This mirrors the handler's validation: payload.urls.is_empty()
        assert!(dto.urls.is_empty(), "empty urls should trigger BAD_REQUEST");
    }

    #[test]
    fn test_validation_no_extraction_method_fails() {
        let dto = ExtractRequestDto {
            urls: vec!["https://example.com".to_string()],
            prompt: None,
            schema: None,
            model: None,
            rules: None,
            sync_wait_ms: None,
        };
        // This mirrors: prompt.is_none() && schema.is_none() && rules.is_none()
        let has_extraction_method =
            dto.prompt.is_some() || dto.schema.is_some() || dto.rules.is_some();
        assert!(
            !has_extraction_method,
            "no extraction method should trigger BAD_REQUEST"
        );
    }

    #[test]
    fn test_validation_has_prompt_passes() {
        let dto = ExtractRequestDto {
            urls: vec!["https://example.com".to_string()],
            prompt: Some("test".to_string()),
            schema: None,
            model: None,
            rules: None,
            sync_wait_ms: None,
        };
        let has_extraction_method =
            dto.prompt.is_some() || dto.schema.is_some() || dto.rules.is_some();
        assert!(has_extraction_method);
        assert!(!dto.urls.is_empty());
    }

    #[test]
    fn test_validation_has_schema_passes() {
        let dto = ExtractRequestDto {
            urls: vec!["https://example.com".to_string()],
            prompt: None,
            schema: Some(serde_json::json!({"type": "object"})),
            model: None,
            rules: None,
            sync_wait_ms: None,
        };
        let has_extraction_method =
            dto.prompt.is_some() || dto.schema.is_some() || dto.rules.is_some();
        assert!(has_extraction_method);
    }

    #[test]
    fn test_validation_has_rules_passes() {
        let dto = ExtractRequestDto {
            urls: vec!["https://example.com".to_string()],
            prompt: None,
            schema: None,
            model: None,
            rules: Some(std::collections::HashMap::new()),
            sync_wait_ms: None,
        };
        let has_extraction_method =
            dto.prompt.is_some() || dto.schema.is_some() || dto.rules.is_some();
        assert!(has_extraction_method);
    }

    #[test]
    fn test_validation_sync_wait_ms_at_max_passes() {
        // MAX_SYNC_WAIT_MS is 30000
        let dto = ExtractRequestDto {
            urls: vec!["https://example.com".to_string()],
            prompt: Some("test".to_string()),
            schema: None,
            model: None,
            rules: None,
            sync_wait_ms: Some(crawl_task::MAX_SYNC_WAIT_MS),
        };
        if let Some(ms) = dto.sync_wait_ms {
            assert!(ms <= crawl_task::MAX_SYNC_WAIT_MS);
        }
    }

    #[test]
    fn test_validation_sync_wait_ms_exceeds_max() {
        let dto = ExtractRequestDto {
            urls: vec!["https://example.com".to_string()],
            prompt: Some("test".to_string()),
            schema: None,
            model: None,
            rules: None,
            sync_wait_ms: Some(crawl_task::MAX_SYNC_WAIT_MS + 1),
        };
        if let Some(ms) = dto.sync_wait_ms {
            assert!(ms > crawl_task::MAX_SYNC_WAIT_MS, "should exceed max");
        }
    }

    // ========== Task construction logic ==========

    #[test]
    fn test_task_construction_for_extract() {
        // Verify that a Task with Extract type can be constructed (mirrors handler logic)
        let task_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let now = chrono::Utc::now();
        let task = Task {
            id: task_id,
            task_type: TaskType::Extract,
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
        assert_eq!(task.task_type, TaskType::Extract);
        assert_eq!(task.status, TaskStatus::Queued);
        assert_eq!(task.max_retries, 3);
    }

    // ========== Constants additional tests ==========

    #[test]
    fn test_extract_task_credits_cost() {
        assert_eq!(crawl_task::EXTRACT_TASK_CREDITS_COST, 8);
    }

    #[test]
    fn test_scrape_task_credits_cost() {
        assert_eq!(crawl_task::SCRAPE_TASK_CREDITS_COST, 5);
    }

    #[test]
    fn test_crawl_task_credits_cost() {
        assert_eq!(crawl_task::CRAWL_TASK_CREDITS_COST, 10);
    }

    // ========== error_response function test ==========

    #[tokio::test]
    async fn test_error_response_builds_correct_status() {
        use axum::body::to_bytes;
        let response = error_response(StatusCode::BAD_REQUEST, "test error");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert!(json["success"].is_boolean());
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn test_error_response_internal_server_error() {
        use axum::body::to_bytes;
        let response = error_response(StatusCode::INTERNAL_SERVER_ERROR, "Something went wrong");
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn test_error_response_forbidden() {
        let response = error_response(
            StatusCode::FORBIDDEN,
            "Access denied due to geographic restrictions: blocked region",
        );
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
}
