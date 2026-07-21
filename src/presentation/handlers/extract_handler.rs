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
use crate::presentation::helpers::ssrf::validate_url;
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

    // SSRF 防护 (CWE-918)：对每个 URL 执行完整的异步 DNS 验证，
    // 与 scrape_handler / crawl_handler 保持一致，在入队前拦截恶意 URL。
    for url in &payload.urls {
        if let Err(e) = validate_url(url).await {
            log::warn!(
                "SSRF attack attempt blocked url={} team_id={} api_key_id={} error={}",
                url,
                team_id,
                auth_state.api_key_id,
                e
            );
            return error_response(StatusCode::BAD_REQUEST, format!("SSRF protection: {}", e));
        }
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

    // ========== Handler tests (calling extract function directly) ==========

    use crate::domain::auth::ApiKeyScope;
    use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError;
    use crate::domain::repositories::task_repository::{RepositoryError, TaskQueryParams};
    use crate::domain::services::geo_location::{GeoLocation, GeoLocationService};
    use crate::domain::services::team_service::TeamGeoRestrictions;
    use crate::queue::task_queue::QueueError;
    use async_trait::async_trait;
    use axum::body::to_bytes;
    use dbnexus::DbPool;
    use std::collections::HashSet;
    use std::net::IpAddr;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    // ============ MockTaskQueue ============

    struct MockTaskQueue {
        enqueue_should_fail: bool,
        enqueue_count: AtomicU32,
    }

    impl MockTaskQueue {
        fn succeeding() -> Self {
            Self {
                enqueue_should_fail: false,
                enqueue_count: AtomicU32::new(0),
            }
        }

        fn failing() -> Self {
            Self {
                enqueue_should_fail: true,
                enqueue_count: AtomicU32::new(0),
            }
        }
    }

    #[async_trait]
    impl TaskQueue for MockTaskQueue {
        async fn enqueue(&self, task: Task) -> Result<Task, QueueError> {
            self.enqueue_count.fetch_add(1, Ordering::SeqCst);
            if self.enqueue_should_fail {
                return Err(QueueError::Repository(RepositoryError::Database(
                    anyhow::anyhow!("queue enqueue down"),
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

    // ============ MockTaskRepository ============

    struct MockTaskRepository {
        created_count: AtomicU32,
        should_fail_query: bool,
        completed_task: Mutex<Option<Task>>,
    }

    impl MockTaskRepository {
        fn succeeding() -> Self {
            Self {
                created_count: AtomicU32::new(0),
                should_fail_query: false,
                completed_task: Mutex::new(None),
            }
        }

        fn failing_query() -> Self {
            Self {
                created_count: AtomicU32::new(0),
                should_fail_query: true,
                completed_task: Mutex::new(None),
            }
        }

        fn with_completed_task(task: Task) -> Self {
            Self {
                created_count: AtomicU32::new(0),
                should_fail_query: false,
                completed_task: Mutex::new(Some(task)),
            }
        }
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            self.created_count.fetch_add(1, Ordering::SeqCst);
            Ok(task.clone())
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
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
            Ok(vec![])
        }

        async fn query_tasks(
            &self,
            _params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            if self.should_fail_query {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "query_tasks down"
                )));
            }
            let guard = self.completed_task.lock().unwrap();
            if let Some(ref task) = *guard {
                return Ok((vec![task.clone()], 1));
            }
            Ok((vec![], 0))
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

    // ============ MockGeoRestrictionRepository ============

    struct MockGeoRestrictionRepository {
        restrictions: Mutex<TeamGeoRestrictions>,
        get_should_fail: bool,
        log_should_fail: bool,
        log_count: AtomicU32,
    }

    impl MockGeoRestrictionRepository {
        fn with_restrictions(restrictions: TeamGeoRestrictions) -> Self {
            Self {
                restrictions: Mutex::new(restrictions),
                get_should_fail: false,
                log_should_fail: false,
                log_count: AtomicU32::new(0),
            }
        }

        fn failing_get() -> Self {
            Self {
                restrictions: Mutex::new(TeamGeoRestrictions::default()),
                get_should_fail: true,
                log_should_fail: false,
                log_count: AtomicU32::new(0),
            }
        }

        fn with_failing_log(restrictions: TeamGeoRestrictions) -> Self {
            Self {
                restrictions: Mutex::new(restrictions),
                get_should_fail: false,
                log_should_fail: true,
                log_count: AtomicU32::new(0),
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
                    "geo restriction repo down".to_string(),
                ));
            }
            Ok(self.restrictions.lock().unwrap().clone())
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
            self.log_count.fetch_add(1, Ordering::SeqCst);
            if self.log_should_fail {
                return Err(GeoRestrictionRepositoryError::Other(
                    "log action down".to_string(),
                ));
            }
            Ok(())
        }
    }

    // ============ MockGeoLocationService ============

    struct MockGeoLocationService {
        should_fail: bool,
        country_code: String,
    }

    impl MockGeoLocationService {
        fn succeeding_with_country(code: &str) -> Self {
            Self {
                should_fail: false,
                country_code: code.to_string(),
            }
        }

        fn failing() -> Self {
            Self {
                should_fail: true,
                country_code: "US".to_string(),
            }
        }
    }

    #[async_trait]
    impl GeoLocationService for MockGeoLocationService {
        async fn get_location(&self, _ip: &IpAddr) -> anyhow::Result<GeoLocation> {
            if self.should_fail {
                return Err(anyhow::anyhow!("geolocation service down"));
            }
            Ok(GeoLocation {
                country_code: self.country_code.clone(),
                ..GeoLocation::default()
            })
        }
    }

    // ============ Helpers ============

    /// Construct a lazy `DbPool` that does not connect to any database.
    /// Reuses the pattern from webhook_handler tests: spawns a dedicated OS
    /// thread with its own runtime to avoid "Cannot start a runtime from
    /// within a runtime" panic when `try_from` calls `block_on`.
    fn make_test_db_pool() -> Arc<DbPool> {
        std::thread::scope(|s| {
            let handle = s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build tokio runtime for DbPool construction");
                let _guard = rt.enter();
                let url = std::env::var("TEST_DATABASE_URL")
                    .expect("TEST_DATABASE_URL must be set; no hardcoded fallback");
                rt.block_on(async {
                    let cfg = dbnexus::DbConfig {
                        url,
                        ..Default::default()
                    };
                    DbPool::with_config(cfg).await
                })
                .expect("failed to create DbPool for test")
            });
            Arc::new(handle.join().expect("DbPool construction thread panicked"))
        })
    }

    fn make_test_auth_state() -> AuthState {
        AuthState::new(
            make_test_db_pool(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
        )
    }

    fn make_test_settings() -> Arc<Settings> {
        Arc::new(Settings::default())
    }

    fn make_addr() -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], 8080))
    }

    fn make_valid_payload() -> ExtractRequestDto {
        ExtractRequestDto {
            urls: vec!["https://example.com".to_string()],
            prompt: Some("Extract title".to_string()),
            schema: None,
            model: None,
            rules: None,
            sync_wait_ms: Some(0),
        }
    }

    /// Build a TeamService with the given geo location service and a default
    /// (disabled) geo restriction repo for its internal use.
    fn make_team_service(geo_loc_service: Arc<dyn GeoLocationService>) -> Arc<TeamService> {
        let geo_repo: Arc<dyn GeoRestrictionRepository> = Arc::new(
            MockGeoRestrictionRepository::with_restrictions(TeamGeoRestrictions::default()),
        );
        Arc::new(TeamService::new(geo_loc_service, geo_repo))
    }

    /// Helper to extract status code and JSON body from an IntoResponse.
    async fn assert_response(
        response: axum::response::Response,
        expected_status: StatusCode,
    ) -> serde_json::Value {
        assert_eq!(response.status(), expected_status);
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("failed to read response body");
        serde_json::from_slice(&bytes).expect("response body is not valid JSON")
    }

    // ============ Handler test cases ============

    #[tokio::test]
    async fn test_extract_empty_urls_returns_bad_request() {
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::succeeding());
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_restrictions(
            TeamGeoRestrictions::default(),
        ));
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));
        let payload = ExtractRequestDto {
            urls: vec![],
            prompt: Some("test".to_string()),
            schema: None,
            model: None,
            rules: None,
            sync_wait_ms: None,
        };

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(payload),
        )
        .await
        .into_response();

        let json = assert_response(response, StatusCode::BAD_REQUEST).await;
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn test_extract_no_extraction_method_returns_bad_request() {
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::succeeding());
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_restrictions(
            TeamGeoRestrictions::default(),
        ));
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));
        let payload = ExtractRequestDto {
            urls: vec!["https://example.com".to_string()],
            prompt: None,
            schema: None,
            model: None,
            rules: None,
            sync_wait_ms: None,
        };

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(payload),
        )
        .await
        .into_response();

        let json = assert_response(response, StatusCode::BAD_REQUEST).await;
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn test_extract_geo_repo_error_returns_internal_error() {
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::succeeding());
        let geo_repo = Arc::new(MockGeoRestrictionRepository::failing_get());
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(make_valid_payload()),
        )
        .await
        .into_response();

        let json = assert_response(response, StatusCode::INTERNAL_SERVER_ERROR).await;
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn test_extract_sync_wait_ms_exceeds_max_returns_bad_request() {
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::succeeding());
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_restrictions(
            TeamGeoRestrictions::default(),
        ));
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));
        let payload = ExtractRequestDto {
            urls: vec!["https://example.com".to_string()],
            prompt: Some("test".to_string()),
            schema: None,
            model: None,
            rules: None,
            sync_wait_ms: Some(crawl_task::MAX_SYNC_WAIT_MS + 1),
        };

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(payload),
        )
        .await
        .into_response();

        let json = assert_response(response, StatusCode::BAD_REQUEST).await;
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn test_extract_geo_denied_returns_forbidden() {
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::succeeding());
        // Enable geo restrictions and block "US"
        let restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: true,
            allowed_countries: None,
            blocked_countries: Some(vec!["US".to_string()]),
            ip_whitelist: None,
            domain_blacklist: None,
        };
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_restrictions(
            restrictions,
        ));
        // GeoLocation service returns "US" which is in blocked_countries
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(make_valid_payload()),
        )
        .await
        .into_response();

        let json = assert_response(response, StatusCode::FORBIDDEN).await;
        assert_eq!(json["success"], false);
        assert!(
            json["error"]["message"]
                .as_str()
                .unwrap_or("")
                .contains("geographic restrictions"),
            "error message should mention geographic restrictions"
        );
    }

    #[tokio::test]
    async fn test_extract_geo_validation_error_returns_internal_error() {
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::succeeding());
        // Enable geo restrictions so that validate_geographic_restriction
        // calls the (failing) geolocation service.
        let restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: true,
            allowed_countries: None,
            blocked_countries: Some(vec!["CN".to_string()]),
            ip_whitelist: None,
            domain_blacklist: None,
        };
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_restrictions(
            restrictions,
        ));
        let team_service = make_team_service(Arc::new(MockGeoLocationService::failing()));

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(make_valid_payload()),
        )
        .await
        .into_response();

        let json = assert_response(response, StatusCode::INTERNAL_SERVER_ERROR).await;
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn test_extract_success_returns_created() {
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::succeeding());
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_restrictions(
            TeamGeoRestrictions::default(),
        ));
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));
        // sync_wait_ms = 0 → no waiting → CREATED
        let payload = make_valid_payload();

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(payload),
        )
        .await
        .into_response();

        let json = assert_response(response, StatusCode::CREATED).await;
        assert_eq!(json["success"], true);
        assert!(json["data"]["id"].is_string());
        assert_eq!(json["data"]["status"], "pending");
    }

    #[tokio::test]
    async fn test_extract_with_sync_wait_returns_accepted() {
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::succeeding());
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_restrictions(
            TeamGeoRestrictions::default(),
        ));
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));
        // sync_wait_ms = 1 → wait_for_tasks_completion runs, times out → ACCEPTED
        let payload = ExtractRequestDto {
            urls: vec!["https://example.com".to_string()],
            prompt: Some("test".to_string()),
            schema: None,
            model: None,
            rules: None,
            sync_wait_ms: Some(1),
        };

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(payload),
        )
        .await
        .into_response();

        let json = assert_response(response, StatusCode::ACCEPTED).await;
        assert_eq!(json["success"], true);
        assert!(json["data"]["id"].is_string());
        assert_eq!(json["data"]["status"], "pending");
    }

    #[tokio::test]
    async fn test_extract_enqueue_failure_returns_internal_error() {
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::failing());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::succeeding());
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_restrictions(
            TeamGeoRestrictions::default(),
        ));
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(make_valid_payload()),
        )
        .await
        .into_response();

        let json = assert_response(response, StatusCode::INTERNAL_SERVER_ERROR).await;
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn test_extract_allowed_country_passes_geo_check() {
        // Enable geo restrictions but the client's country is in allowed list
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::succeeding());
        let restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: true,
            allowed_countries: Some(vec!["US".to_string()]),
            blocked_countries: None,
            ip_whitelist: None,
            domain_blacklist: None,
        };
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_restrictions(
            restrictions,
        ));
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(make_valid_payload()),
        )
        .await
        .into_response();

        let json = assert_response(response, StatusCode::CREATED).await;
        assert_eq!(json["success"], true);
    }

    #[tokio::test]
    async fn test_extract_ip_whitelist_bypasses_country_check() {
        // IP in whitelist should be allowed regardless of country rules
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::succeeding());
        let restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: true,
            allowed_countries: Some(vec!["CN".to_string()]),
            blocked_countries: None,
            ip_whitelist: Some(vec!["127.0.0.0/8".to_string()]),
            domain_blacklist: None,
        };
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_restrictions(
            restrictions,
        ));
        // Even though geo service returns "US" (not in allowed "CN"),
        // the IP whitelist 127.0.0.0/8 should allow 127.0.0.1 first.
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(make_valid_payload()),
        )
        .await
        .into_response();

        let json = assert_response(response, StatusCode::CREATED).await;
        assert_eq!(json["success"], true);
    }

    #[tokio::test]
    async fn test_extract_allowed_with_failing_log() {
        // Allowed path where log_geo_restriction_action returns Err — covers
        // the error! branch on the Allowed arm (line 107).
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::succeeding());
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_failing_log(
            TeamGeoRestrictions::default(),
        ));
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo.clone()),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(make_valid_payload()),
        )
        .await
        .into_response();

        // Log failure is non-fatal: handler still returns CREATED
        let json = assert_response(response, StatusCode::CREATED).await;
        assert_eq!(json["success"], true);
        // Confirm the log path was actually exercised
        assert_eq!(geo_repo.log_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_extract_denied_with_failing_log() {
        // Denied path where log_geo_restriction_action returns Err — covers
        // the error! branch on the Denied arm (line 122).
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::succeeding());
        let restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: true,
            allowed_countries: None,
            blocked_countries: Some(vec!["US".to_string()]),
            ip_whitelist: None,
            domain_blacklist: None,
        };
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_failing_log(restrictions));
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo.clone()),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(make_valid_payload()),
        )
        .await
        .into_response();

        // Denied path returns FORBIDDEN even when logging fails
        let json = assert_response(response, StatusCode::FORBIDDEN).await;
        assert_eq!(json["success"], false);
        assert_eq!(geo_repo.log_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_extract_sync_wait_with_query_failure() {
        // sync_wait_ms > 0 but query_tasks fails → wait_for_tasks_completion
        // returns Err — covers the error! branch on the wait arm (lines 192-193).
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::failing_query());
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_restrictions(
            TeamGeoRestrictions::default(),
        ));
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));
        let payload = ExtractRequestDto {
            urls: vec!["https://example.com".to_string()],
            prompt: Some("test".to_string()),
            schema: None,
            model: None,
            rules: None,
            // Use a generous timeout: the query fails immediately on the first
            // poll, so the loop exits via Err before any sleeping. A larger
            // value avoids timing races where the loop condition is already
            // false before the first iteration under tarpaulin instrumentation.
            sync_wait_ms: Some(1000),
        };

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(payload),
        )
        .await
        .into_response();

        // Even when wait_for_tasks_completion fails, the handler returns the
        // created task. waited_time_ms stays 0, so 0 >= 1000 is false → CREATED.
        let json = assert_response(response, StatusCode::CREATED).await;
        assert_eq!(json["success"], true);
        assert!(json["data"]["id"].is_string());
    }

    #[tokio::test]
    async fn test_extract_sync_wait_completes_returns_created() {
        // sync_wait_ms > 0 and wait_for_tasks_completion returns Ok quickly
        // (task is already Completed on first poll) → waited_time_ms <
        // sync_wait_ms → CREATED (not ACCEPTED). Covers the Ok arm where
        // waited_time_ms < sync_wait_ms (handler lines 189-190, 205-209).
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let completed_task = Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Extract,
            status: TaskStatus::Completed,
            priority: 0,
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            url: "https://example.com".to_string(),
            payload: serde_json::Value::Null,
            retry_count: 0,
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            expires_at: None,
            created_at: chrono::Utc::now(),
            started_at: None,
            completed_at: None,
            crawl_id: None,
            updated_at: chrono::Utc::now(),
            lock_token: None,
            lock_expires_at: None,
        };
        let task_repo: Arc<dyn TaskRepository> =
            Arc::new(MockTaskRepository::with_completed_task(completed_task));
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_restrictions(
            TeamGeoRestrictions::default(),
        ));
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));
        let payload = ExtractRequestDto {
            urls: vec!["https://example.com".to_string()],
            prompt: Some("test".to_string()),
            schema: None,
            model: None,
            rules: None,
            sync_wait_ms: Some(500),
        };

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(payload),
        )
        .await
        .into_response();

        // Task completes on first poll; waited_time_ms < 500 → CREATED
        let json = assert_response(response, StatusCode::CREATED).await;
        assert_eq!(json["success"], true);
        assert!(json["data"]["id"].is_string());
        assert_eq!(json["data"]["status"], "pending");
    }

    // ========== CapturingLogger for covering log::error! format args ==========
    //
    // The log crate's `error!` macro only evaluates its format arguments when
    // `max_level() >= Error` AND a logger is installed. Without a logger, the
    // `enabled()` check returns false and the format args (including `{:?}` of
    // error values) are never evaluated, leaving the `error!` line partially
    // uncovered. We install a no-op CapturingLogger at Error level so the
    // format args are evaluated (and thus counted as covered).

    use log::{LevelFilter, Log, Metadata, Record};
    use std::sync::Once;

    static LOGGER_INIT: Once = Once::new();

    struct CapturingLogger;

    impl Log for CapturingLogger {
        fn enabled(&self, metadata: &Metadata) -> bool {
            metadata.level() <= log::Level::Error
        }
        fn log(&self, _record: &Record) {}
        fn flush(&self) {}
    }

    /// Install a global error-level logger so `log::error!` format arguments
    /// in the extract handler are evaluated and counted as covered.
    fn ensure_error_logger() {
        LOGGER_INIT.call_once(|| {
            static CAPTURING_LOGGER: CapturingLogger = CapturingLogger;
            let _ = log::set_logger(&CAPTURING_LOGGER);
            log::set_max_level(LevelFilter::Error);
        });
    }

    // ========== Log-evaluated handler tests ==========
    // These tests call `ensure_error_logger()` so that `log::error!` format
    // args are evaluated, covering the error! lines (72, 107, 122, 131, 193)
    // and the surrounding error_response lines in the handler.

    #[tokio::test]
    async fn test_extract_no_extraction_method_log_evaluated() {
        // Covers error_response lines 60-61 (Either prompt, schema, or rules).
        ensure_error_logger();
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::succeeding());
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_restrictions(
            TeamGeoRestrictions::default(),
        ));
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));
        let payload = ExtractRequestDto {
            urls: vec!["https://example.com".to_string()],
            prompt: None,
            schema: None,
            model: None,
            rules: None,
            sync_wait_ms: None,
        };

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(payload),
        )
        .await
        .into_response();

        let json = assert_response(response, StatusCode::BAD_REQUEST).await;
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn test_extract_geo_repo_error_log_evaluated() {
        // Covers error! line 72 + error_response lines 74-75.
        ensure_error_logger();
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::succeeding());
        let geo_repo = Arc::new(MockGeoRestrictionRepository::failing_get());
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(make_valid_payload()),
        )
        .await
        .into_response();

        let json = assert_response(response, StatusCode::INTERNAL_SERVER_ERROR).await;
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn test_extract_sync_wait_ms_exceeds_max_log_evaluated() {
        // Covers error_response line 84 (sync_wait_ms must be <= MAX).
        ensure_error_logger();
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::succeeding());
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_restrictions(
            TeamGeoRestrictions::default(),
        ));
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));
        let payload = ExtractRequestDto {
            urls: vec!["https://example.com".to_string()],
            prompt: Some("test".to_string()),
            schema: None,
            model: None,
            rules: None,
            sync_wait_ms: Some(crawl_task::MAX_SYNC_WAIT_MS + 1),
        };

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(payload),
        )
        .await
        .into_response();

        let json = assert_response(response, StatusCode::BAD_REQUEST).await;
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn test_extract_geo_denied_log_evaluated() {
        // Covers error_response line 126 (Access denied due to geographic
        // restrictions).
        ensure_error_logger();
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::succeeding());
        let restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: true,
            allowed_countries: None,
            blocked_countries: Some(vec!["US".to_string()]),
            ip_whitelist: None,
            domain_blacklist: None,
        };
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_restrictions(
            restrictions,
        ));
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(make_valid_payload()),
        )
        .await
        .into_response();

        let json = assert_response(response, StatusCode::FORBIDDEN).await;
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn test_extract_geo_validation_error_log_evaluated() {
        // Covers error! line 131 + error_response lines 133-134.
        ensure_error_logger();
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::succeeding());
        let restrictions = TeamGeoRestrictions {
            enable_geo_restrictions: true,
            allowed_countries: None,
            blocked_countries: Some(vec!["CN".to_string()]),
            ip_whitelist: None,
            domain_blacklist: None,
        };
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_restrictions(
            restrictions,
        ));
        let team_service = make_team_service(Arc::new(MockGeoLocationService::failing()));

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(make_valid_payload()),
        )
        .await
        .into_response();

        let json = assert_response(response, StatusCode::INTERNAL_SERVER_ERROR).await;
        assert_eq!(json["success"], false);
    }

    #[tokio::test]
    async fn test_extract_sync_wait_log_evaluated() {
        // Covers line 185 (BASE_POLL_INTERVAL_MS argument) + error! line 193
        // when wait_for_tasks_completion fails. Uses a failing query repo so
        // the wait path exercises the Err arm.
        ensure_error_logger();
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::succeeding());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::failing_query());
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_restrictions(
            TeamGeoRestrictions::default(),
        ));
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));
        let payload = ExtractRequestDto {
            urls: vec!["https://example.com".to_string()],
            prompt: Some("test".to_string()),
            schema: None,
            model: None,
            rules: None,
            sync_wait_ms: Some(1000),
        };

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(payload),
        )
        .await
        .into_response();

        let json = assert_response(response, StatusCode::CREATED).await;
        assert_eq!(json["success"], true);
    }

    #[tokio::test]
    async fn test_extract_enqueue_failure_log_evaluated() {
        // Covers error_response line 213 (enqueue failure path).
        ensure_error_logger();
        let queue: Arc<dyn TaskQueue> = Arc::new(MockTaskQueue::failing());
        let task_repo: Arc<dyn TaskRepository> = Arc::new(MockTaskRepository::succeeding());
        let geo_repo = Arc::new(MockGeoRestrictionRepository::with_restrictions(
            TeamGeoRestrictions::default(),
        ));
        let team_service = make_team_service(Arc::new(
            MockGeoLocationService::succeeding_with_country("US"),
        ));

        let response = extract::<MockGeoRestrictionRepository>(
            Extension(queue),
            Extension(make_test_settings()),
            Extension(task_repo),
            Extension(geo_repo),
            Extension(team_service),
            Extension(make_test_auth_state()),
            ConnectInfo(make_addr()),
            Json(make_valid_payload()),
        )
        .await
        .into_response();

        let json = assert_response(response, StatusCode::INTERNAL_SERVER_ERROR).await;
        assert_eq!(json["success"], false);
    }
}
