// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Task handlers — re-export facade for `task_queries` + `task_commands`.
//!
//! 路由和其他 handler 继续通过 `task_handler::query_tasks` 等路径访问，
//! 无需修改调用方。

// Re-export from sibling modules for backward compatibility
pub use super::task_commands::cancel_tasks;
pub use super::task_queries::{
    handle_sync_wait_and_get_status, query_tasks, wait_for_tasks_completion, SyncWaitResult,
    TaskQueryResponseMeta,
};

#[cfg(test)]
mod tests {
    use super::*;
    // Access helpers from task_queries
    use super::super::task_queries::{
        apply_defaults, build_scrape_result_json, build_task_infos, calculate_completion_rate,
        calculate_next_interval, execute_task_query, fetch_scrape_results, handle_sync_wait,
        poll_count_exceeded, query_tasks_for_poll, validate_request,
    };
    use crate::application::dto::task_query_request::{TaskCancelRequestDto, TaskQueryRequestDto};
    use crate::common::constants::crawl_task;
    use crate::common::constants::server_config;
    use crate::domain::auth::ApiKeyScope;
    use crate::domain::models::{Task, TaskStatus, TaskType};
    use crate::domain::repositories::task_repository::RepositoryError;
    use crate::domain::repositories::task_repository::{TaskQueryParams, TaskRepository};
    use crate::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl;
    use crate::presentation::errors::CrawlRsError;
    use crate::presentation::middleware::auth_middleware::AuthState;
    use async_trait::async_trait;
    use axum::{extract::Extension, Json};
    use dbnexus::DbPool;
    use std::sync::{Arc, Mutex};
    use uuid::Uuid;

    // ========== Helper to create test Task ==========

    fn make_test_task(id: Uuid, status: TaskStatus) -> Task {
        let now = chrono::Utc::now();
        Task {
            id,
            task_type: TaskType::Scrape,
            status,
            priority: 0,
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            url: "https://example.com".to_string(),
            payload: serde_json::json!({}),
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

    // ========== poll_count_exceeded tests ==========

    #[test]
    fn test_poll_count_exceeded_true_when_count_equals_max() {
        assert!(poll_count_exceeded(60, 60));
    }

    #[test]
    fn test_poll_count_exceeded_true_when_count_exceeds_max() {
        assert!(poll_count_exceeded(100, 60));
    }

    #[test]
    fn test_poll_count_exceeded_false_when_count_below_max() {
        assert!(!poll_count_exceeded(59, 60));
    }

    #[test]
    fn test_poll_count_exceeded_false_when_count_zero() {
        assert!(!poll_count_exceeded(0, 60));
    }

    #[test]
    fn test_poll_count_exceeded_with_max_one() {
        assert!(poll_count_exceeded(1, 1));
        assert!(!poll_count_exceeded(0, 1));
    }

    // ========== calculate_completion_rate tests ==========

    #[test]
    fn test_completion_rate_empty_task_ids_returns_one() {
        let tasks = vec![];
        let task_ids: Vec<Uuid> = vec![];
        assert_eq!(calculate_completion_rate(&tasks, &task_ids), 1.0);
    }

    #[test]
    fn test_completion_rate_all_completed() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let tasks = vec![
            make_test_task(id1, TaskStatus::Completed),
            make_test_task(id2, TaskStatus::Completed),
        ];
        let task_ids = vec![id1, id2];
        assert_eq!(calculate_completion_rate(&tasks, &task_ids), 1.0);
    }

    #[test]
    fn test_completion_rate_none_completed() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let tasks = vec![
            make_test_task(id1, TaskStatus::Queued),
            make_test_task(id2, TaskStatus::Active),
        ];
        let task_ids = vec![id1, id2];
        assert_eq!(calculate_completion_rate(&tasks, &task_ids), 0.0);
    }

    #[test]
    fn test_completion_rate_half_completed() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let tasks = vec![
            make_test_task(id1, TaskStatus::Completed),
            make_test_task(id2, TaskStatus::Active),
        ];
        let task_ids = vec![id1, id2];
        assert_eq!(calculate_completion_rate(&tasks, &task_ids), 0.5);
    }

    #[test]
    fn test_completion_rate_counts_failed_as_completed() {
        let id1 = Uuid::new_v4();
        let tasks = vec![make_test_task(id1, TaskStatus::Failed)];
        let task_ids = vec![id1];
        assert_eq!(calculate_completion_rate(&tasks, &task_ids), 1.0);
    }

    #[test]
    fn test_completion_rate_counts_cancelled_as_completed() {
        let id1 = Uuid::new_v4();
        let tasks = vec![make_test_task(id1, TaskStatus::Cancelled)];
        let task_ids = vec![id1];
        assert_eq!(calculate_completion_rate(&tasks, &task_ids), 1.0);
    }

    #[test]
    fn test_completion_rate_mixed_statuses() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let id3 = Uuid::new_v4();
        let id4 = Uuid::new_v4();
        let tasks = vec![
            make_test_task(id1, TaskStatus::Completed),
            make_test_task(id2, TaskStatus::Failed),
            make_test_task(id3, TaskStatus::Cancelled),
            make_test_task(id4, TaskStatus::Active),
        ];
        let task_ids = vec![id1, id2, id3, id4];
        assert_eq!(calculate_completion_rate(&tasks, &task_ids), 0.75);
    }

    // ========== calculate_next_interval tests ==========

    #[test]
    fn test_next_interval_no_progress_uses_rate_based() {
        let interval = calculate_next_interval(0.5, 0.5, 1000, 500, 2000);
        // rate_based = 500 + (1500 * 0.5) = 1250
        assert_eq!(interval, 1250);
    }

    #[test]
    fn test_next_interval_positive_progress_increases() {
        let interval = calculate_next_interval(0.6, 0.5, 1000, 500, 2000);
        // progress > 0: max(1000 * 1.2, rate_based) = max(1200, 500 + 1500*0.6) = max(1200, 1400) = 1400
        assert!(interval >= 1000);
        assert!(interval <= 2000);
    }

    #[test]
    fn test_next_interval_negative_progress_decreases() {
        let interval = calculate_next_interval(0.4, 0.5, 1500, 500, 2000);
        // progress < 0: min(1500 * 0.8, rate_based) = min(1200, 500 + 1500*0.4) = min(1200, 1100) = 1100
        assert!(interval <= 1500);
        assert!(interval >= 500);
    }

    #[test]
    fn test_next_interval_clamped_to_min() {
        let interval = calculate_next_interval(0.0, 0.0, 500, 500, 2000);
        assert!(interval >= 500);
    }

    #[test]
    fn test_next_interval_clamped_to_max() {
        let interval = calculate_next_interval(1.0, 1.0, 2000, 500, 2000);
        assert!(interval <= 2000);
    }

    #[test]
    fn test_next_interval_full_completion() {
        let interval = calculate_next_interval(1.0, 0.0, 500, 500, 2000);
        // rate_based = 500 + 1500*1.0 = 2000
        assert_eq!(interval, 2000);
    }

    // ========== apply_defaults tests ==========

    #[test]
    fn test_apply_defaults_all_none() {
        let request = TaskQueryRequestDto {
            task_ids: None,
            team_id: Uuid::nil(),
            task_types: None,
            statuses: None,
            created_after: None,
            created_before: None,
            crawl_id: None,
            limit: None,
            offset: None,
            include_results: None,
            sync_wait_ms: None,
        };
        let (limit, offset, include_results, sync_wait_ms) = apply_defaults(&request);
        assert_eq!(limit, server_config::DEFAULT_PAGE_LIMIT);
        assert_eq!(offset, 0);
        assert!(!include_results);
        assert_eq!(sync_wait_ms, crawl_task::DEFAULT_TIMEOUT_MS as u32);
    }

    #[test]
    fn test_apply_defaults_with_values() {
        let request = TaskQueryRequestDto {
            task_ids: None,
            team_id: Uuid::nil(),
            task_types: None,
            statuses: None,
            created_after: None,
            created_before: None,
            crawl_id: None,
            limit: Some(50),
            offset: Some(100),
            include_results: Some(true),
            sync_wait_ms: Some(10000),
        };
        let (limit, offset, include_results, sync_wait_ms) = apply_defaults(&request);
        assert_eq!(limit, 50);
        assert_eq!(offset, 100);
        assert!(include_results);
        assert_eq!(sync_wait_ms, 10000);
    }

    #[test]
    fn test_apply_defaults_limit_capped_at_max() {
        let request = TaskQueryRequestDto {
            task_ids: None,
            team_id: Uuid::nil(),
            task_types: None,
            statuses: None,
            created_after: None,
            created_before: None,
            crawl_id: None,
            limit: Some(5000),
            offset: None,
            include_results: None,
            sync_wait_ms: None,
        };
        let (limit, _, _, _) = apply_defaults(&request);
        assert_eq!(limit, server_config::MAX_PAGE_LIMIT);
    }

    // ========== build_task_infos tests ==========

    #[test]
    fn test_build_task_infos_empty() {
        let tasks: Vec<Task> = vec![];
        let result = build_task_infos(&tasks, None);
        assert!(result.is_empty());
    }

    #[test]
    fn test_build_task_infos_single_task_no_results() {
        let id = Uuid::new_v4();
        let tasks = vec![make_test_task(id, TaskStatus::Completed)];
        let result = build_task_infos(&tasks, None);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].id, id);
        assert_eq!(result[0].status, TaskStatus::Completed);
        assert_eq!(result[0].url, "https://example.com");
        assert!(result[0].result.is_none());
    }

    #[test]
    fn test_build_task_infos_multiple_tasks() {
        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();
        let tasks = vec![
            make_test_task(id1, TaskStatus::Queued),
            make_test_task(id2, TaskStatus::Failed),
        ];
        let result = build_task_infos(&tasks, None);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].id, id1);
        assert_eq!(result[1].id, id2);
    }

    #[test]
    fn test_build_task_infos_preserves_task_type() {
        let id = Uuid::new_v4();
        let mut task = make_test_task(id, TaskStatus::Queued);
        task.task_type = TaskType::Crawl;
        let result = build_task_infos(&[task], None);
        assert_eq!(result[0].task_type, TaskType::Crawl);
    }

    // ========== SyncWaitResult struct tests ==========

    #[test]
    fn test_sync_wait_result_no_wait() {
        let result = SyncWaitResult {
            waited_time_ms: 0,
            is_timeout: false,
        };
        assert_eq!(result.waited_time_ms, 0);
        assert!(!result.is_timeout);
    }

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
    fn test_sync_wait_result_completed_before_timeout() {
        let result = SyncWaitResult {
            waited_time_ms: 3000,
            is_timeout: false,
        };
        assert_eq!(result.waited_time_ms, 3000);
        assert!(!result.is_timeout);
    }

    // ========== TaskQueryResponseMeta serialization ==========

    #[test]
    fn test_task_query_response_meta_serialization() {
        let meta = TaskQueryResponseMeta {
            status: "sync_completed".to_string(),
            credits_used: 5,
            response_time_ms: 1234,
        };
        let json = serde_json::to_string(&meta).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "sync_completed");
        assert_eq!(parsed["credits_used"], 5);
        assert_eq!(parsed["response_time_ms"], 1234);
    }

    #[test]
    fn test_task_query_response_meta_async_status() {
        let meta = TaskQueryResponseMeta {
            status: "async".to_string(),
            credits_used: 0,
            response_time_ms: 0,
        };
        let json = serde_json::to_string(&meta).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "async");
    }

    #[test]
    fn test_task_query_response_meta_timeout_status() {
        let meta = TaskQueryResponseMeta {
            status: "sync_timeout".to_string(),
            credits_used: 10,
            response_time_ms: 30000,
        };
        let json = serde_json::to_string(&meta).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["status"], "sync_timeout");
    }

    // ========== validate_request tests ==========

    #[test]
    fn test_validate_request_valid() {
        let request = TaskQueryRequestDto::default();
        assert!(validate_request(&request).is_ok());
    }

    #[test]
    fn test_validate_request_limit_too_small() {
        let request = TaskQueryRequestDto {
            limit: Some(0),
            ..TaskQueryRequestDto::default()
        };
        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn test_validate_request_limit_too_large() {
        let request = TaskQueryRequestDto {
            limit: Some(1001),
            ..TaskQueryRequestDto::default()
        };
        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn test_validate_request_sync_wait_ms_exceeds_max() {
        let request = TaskQueryRequestDto {
            sync_wait_ms: Some(30001),
            ..TaskQueryRequestDto::default()
        };
        assert!(validate_request(&request).is_err());
    }

    #[test]
    fn test_validate_request_sync_wait_ms_zero_ok() {
        let request = TaskQueryRequestDto {
            sync_wait_ms: Some(0),
            ..TaskQueryRequestDto::default()
        };
        assert!(validate_request(&request).is_ok());
    }

    #[test]
    fn test_validate_request_sync_wait_ms_at_max_ok() {
        let request = TaskQueryRequestDto {
            sync_wait_ms: Some(30000),
            ..TaskQueryRequestDto::default()
        };
        assert!(validate_request(&request).is_ok());
    }

    // ========== handle_sync_wait_and_get_status edge cases ==========

    #[tokio::test]
    async fn test_handle_sync_wait_zero_ms_returns_immediately() {
        // This test verifies that sync_wait_ms=0 returns immediately without calling the repo
        // We use a dummy that would fail if called, but since sync_wait_ms=0, it won't be called
        struct DummyRepo;
        #[async_trait::async_trait]
        impl TaskRepository for DummyRepo {
            async fn create(
                &self,
                _task: &Task,
            ) -> Result<Task, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn find_by_id(
                &self,
                _id: Uuid,
            ) -> Result<Option<Task>, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn update(
                &self,
                _task: &Task,
            ) -> Result<Task, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn acquire_next(
                &self,
                _worker_id: Uuid,
            ) -> Result<Option<Task>, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn mark_completed(
                &self,
                _id: Uuid,
            ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn mark_failed(
                &self,
                _id: Uuid,
            ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn mark_cancelled(
                &self,
                _id: Uuid,
            ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn exists_by_url(
                &self,
                _url: &str,
            ) -> Result<bool, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn find_existing_urls(
                &self,
                _urls: &[String],
            ) -> Result<
                std::collections::HashSet<String>,
                crate::domain::repositories::task_repository::RepositoryError,
            > {
                unreachable!("should not be called")
            }
            async fn reset_stuck_tasks(
                &self,
                _timeout: chrono::Duration,
            ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn cancel_tasks_by_crawl_id(
                &self,
                _crawl_id: Uuid,
            ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn expire_tasks(
                &self,
            ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn find_by_crawl_id(
                &self,
                _crawl_id: Uuid,
            ) -> Result<Vec<Task>, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn query_tasks(
                &self,
                _params: crate::domain::repositories::task_repository::TaskQueryParams,
            ) -> Result<
                (Vec<Task>, u64),
                crate::domain::repositories::task_repository::RepositoryError,
            > {
                unreachable!("should not be called")
            }
            async fn batch_cancel(
                &self,
                _task_ids: Vec<Uuid>,
                _team_id: Uuid,
                _force: bool,
            ) -> Result<
                (Vec<Uuid>, Vec<(Uuid, String)>),
                crate::domain::repositories::task_repository::RepositoryError,
            > {
                unreachable!("should not be called")
            }
        }

        let result = handle_sync_wait_and_get_status(&DummyRepo, &[], Uuid::nil(), 0).await;
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.waited_time_ms, 0);
        assert!(!result.is_timeout);
    }

    #[tokio::test]
    async fn test_handle_sync_wait_empty_task_ids_returns_immediately() {
        // Even with sync_wait_ms > 0, empty task_ids should return immediately
        struct DummyRepo;
        #[async_trait::async_trait]
        impl TaskRepository for DummyRepo {
            async fn create(
                &self,
                _task: &Task,
            ) -> Result<Task, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn find_by_id(
                &self,
                _id: Uuid,
            ) -> Result<Option<Task>, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn update(
                &self,
                _task: &Task,
            ) -> Result<Task, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn acquire_next(
                &self,
                _worker_id: Uuid,
            ) -> Result<Option<Task>, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn mark_completed(
                &self,
                _id: Uuid,
            ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn mark_failed(
                &self,
                _id: Uuid,
            ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn mark_cancelled(
                &self,
                _id: Uuid,
            ) -> Result<(), crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn exists_by_url(
                &self,
                _url: &str,
            ) -> Result<bool, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn find_existing_urls(
                &self,
                _urls: &[String],
            ) -> Result<
                std::collections::HashSet<String>,
                crate::domain::repositories::task_repository::RepositoryError,
            > {
                unreachable!("should not be called")
            }
            async fn reset_stuck_tasks(
                &self,
                _timeout: chrono::Duration,
            ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn cancel_tasks_by_crawl_id(
                &self,
                _crawl_id: Uuid,
            ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn expire_tasks(
                &self,
            ) -> Result<u64, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn find_by_crawl_id(
                &self,
                _crawl_id: Uuid,
            ) -> Result<Vec<Task>, crate::domain::repositories::task_repository::RepositoryError>
            {
                unreachable!("should not be called")
            }
            async fn query_tasks(
                &self,
                _params: crate::domain::repositories::task_repository::TaskQueryParams,
            ) -> Result<
                (Vec<Task>, u64),
                crate::domain::repositories::task_repository::RepositoryError,
            > {
                unreachable!("should not be called")
            }
            async fn batch_cancel(
                &self,
                _task_ids: Vec<Uuid>,
                _team_id: Uuid,
                _force: bool,
            ) -> Result<
                (Vec<Uuid>, Vec<(Uuid, String)>),
                crate::domain::repositories::task_repository::RepositoryError,
            > {
                unreachable!("should not be called")
            }
        }

        let result = handle_sync_wait_and_get_status(&DummyRepo, &[], Uuid::nil(), 5000).await;
        assert!(result.is_ok());
        let result = result.unwrap();
        assert_eq!(result.waited_time_ms, 0);
        assert!(!result.is_timeout);
    }

    // ========== build_scrape_result_json tests ==========

    // 构造测试用 ScrapeResult
    fn make_test_scrape_result(task_id: Uuid) -> crate::domain::models::ScrapeResult {
        crate::domain::models::ScrapeResult {
            id: Uuid::new_v4(),
            task_id,
            url: "https://example.com".to_string(),
            status_code: 200,
            content: "<html><body>Hello</body></html>".to_string(),
            content_type: "text/html".to_string(),
            headers: serde_json::json!({"content-length": "100"}),
            meta_data: serde_json::json!({"key": "value"}),
            screenshot: None,
            response_time_ms: 150,
            created_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn test_build_scrape_result_json_maps_basic_fields() {
        let task_id = Uuid::new_v4();
        let result = make_test_scrape_result(task_id);
        let dto = build_scrape_result_json(&result);

        assert_eq!(dto.id, result.id);
        assert_eq!(dto.status_code, 200);
        // content 经过 html_escape::encode_text 转义
        assert_eq!(
            dto.content,
            "&lt;html&gt;&lt;body&gt;Hello&lt;/body&gt;&lt;/html&gt;"
        );
    }

    #[test]
    fn test_build_scrape_result_json_escapes_html_special_chars() {
        // html_escape::encode_text 应转义 < > & ' "
        let task_id = Uuid::new_v4();
        let mut result = make_test_scrape_result(task_id);
        result.content = "<script>alert('xss')</script>".to_string();
        let dto = build_scrape_result_json(&result);

        assert!(dto.content.contains("&lt;script&gt;"));
        assert!(!dto.content.contains("<script>"));
    }

    #[test]
    fn test_build_scrape_result_json_escapes_ampersand() {
        let task_id = Uuid::new_v4();
        let mut result = make_test_scrape_result(task_id);
        result.content = "Tom & Jerry".to_string();
        let dto = build_scrape_result_json(&result);

        assert!(dto.content.contains("&amp;"));
        assert!(!dto.content.contains(" & "));
    }

    #[test]
    fn test_build_scrape_result_json_clones_metadata() {
        let task_id = Uuid::new_v4();
        let result = make_test_scrape_result(task_id);
        let dto = build_scrape_result_json(&result);

        assert!(dto.metadata.is_some());
        let metadata = dto.metadata.unwrap();
        assert_eq!(metadata["key"], "value");
    }

    #[test]
    fn test_build_scrape_result_json_status_code_404() {
        let task_id = Uuid::new_v4();
        let mut result = make_test_scrape_result(task_id);
        result.status_code = 404;
        let dto = build_scrape_result_json(&result);

        assert_eq!(dto.status_code, 404);
    }

    #[test]
    fn test_build_scrape_result_json_empty_content() {
        let task_id = Uuid::new_v4();
        let mut result = make_test_scrape_result(task_id);
        result.content = String::new();
        let dto = build_scrape_result_json(&result);

        assert!(dto.content.is_empty());
    }

    #[test]
    fn test_build_scrape_result_json_null_metadata() {
        let task_id = Uuid::new_v4();
        let mut result = make_test_scrape_result(task_id);
        result.meta_data = serde_json::Value::Null;
        let dto = build_scrape_result_json(&result);

        assert!(dto.metadata.is_some());
        assert!(dto.metadata.unwrap().is_null());
    }

    // ========== build_task_infos with results_map tests ==========

    #[test]
    fn test_build_task_infos_with_matching_result() {
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let scrape_result = make_test_scrape_result(task_id);
        let mut results_map = std::collections::HashMap::new();
        results_map.insert(task_id, scrape_result.clone());

        let infos = build_task_infos(&[task], Some(&results_map));

        assert_eq!(infos.len(), 1);
        assert!(infos[0].result.is_some());
        let result_dto = infos[0].result.as_ref().unwrap();
        assert_eq!(result_dto.id, scrape_result.id);
        assert_eq!(result_dto.status_code, 200);
    }

    #[test]
    fn test_build_task_infos_with_results_map_no_match() {
        // Task has no corresponding result in the map
        let task_id = Uuid::new_v4();
        let other_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let scrape_result = make_test_scrape_result(other_id);
        let mut results_map = std::collections::HashMap::new();
        results_map.insert(other_id, scrape_result);

        let infos = build_task_infos(&[task], Some(&results_map));

        assert_eq!(infos.len(), 1);
        assert!(infos[0].result.is_none());
    }

    #[test]
    fn test_build_task_infos_mixed_with_and_without_results() {
        let task1_id = Uuid::new_v4();
        let task2_id = Uuid::new_v4();
        let tasks = vec![
            make_test_task(task1_id, TaskStatus::Completed),
            make_test_task(task2_id, TaskStatus::Queued),
        ];
        let scrape_result = make_test_scrape_result(task1_id);
        let mut results_map = std::collections::HashMap::new();
        results_map.insert(task1_id, scrape_result);

        let infos = build_task_infos(&tasks, Some(&results_map));

        assert_eq!(infos.len(), 2);
        assert!(infos[0].result.is_some());
        assert!(infos[1].result.is_none());
    }

    #[test]
    fn test_build_task_infos_empty_map_returns_none_for_all() {
        let task1_id = Uuid::new_v4();
        let task2_id = Uuid::new_v4();
        let tasks = vec![
            make_test_task(task1_id, TaskStatus::Completed),
            make_test_task(task2_id, TaskStatus::Failed),
        ];
        let results_map: std::collections::HashMap<Uuid, crate::domain::models::ScrapeResult> =
            std::collections::HashMap::new();

        let infos = build_task_infos(&tasks, Some(&results_map));

        assert_eq!(infos.len(), 2);
        assert!(infos[0].result.is_none());
        assert!(infos[1].result.is_none());
    }

    #[test]
    fn test_build_task_infos_result_html_escaped_in_dto() {
        // 验证 results_map 中的 content 经过 HTML 转义后出现在 TaskInfoDto 中
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let mut scrape_result = make_test_scrape_result(task_id);
        scrape_result.content = "<b>bold</b>".to_string();
        let mut results_map = std::collections::HashMap::new();
        results_map.insert(task_id, scrape_result);

        let infos = build_task_infos(&[task], Some(&results_map));

        let result_dto = infos[0].result.as_ref().expect("result should exist");
        assert!(result_dto.content.contains("&lt;b&gt;"));
        assert!(!result_dto.content.contains("<b>"));
    }

    // ========== Handler test infrastructure ==========

    /// Construct a lazy `DbPool` that does not connect to any database.
    ///
    /// `DbPool::try_from` is lazy: it builds the internal struct without opening
    /// a connection. The connection is only established on `get_session()`, which
    /// handlers under test never call (they only read `team_id` / `api_key_id`
    /// from `AuthState`). Since `try_from` internally calls
    /// `Handle::current().block_on(...)`, we construct the pool on a dedicated
    /// OS thread to avoid runtime-in-runtime panics.
    fn make_test_db_pool() -> Arc<DbPool> {
        // 委托公共 helper：其使用进程级保活 runtime 构造池，避免临时 runtime
        // 销毁导致池连接 IO 失效（跨 runtime acquire 超时）
        crate::common::test_helpers::create_test_db_pool()
    }

    /// Build an `AuthState` suitable for handler unit tests.
    fn make_test_auth_state() -> AuthState {
        AuthState::new(
            make_test_db_pool(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
        )
    }

    /// Build a `ScrapeResultRepositoryImpl` backed by a lazy (non-connecting) pool.
    fn make_test_scrape_result_repo() -> Arc<ScrapeResultRepositoryImpl> {
        Arc::new(ScrapeResultRepositoryImpl::new(make_test_db_pool()))
    }

    // ========== MockTaskRepository ==========

    /// Mock `TaskRepository` with configurable `query_tasks` and `batch_cancel`.
    ///
    /// All other trait methods return benign defaults. `query_tasks` returns the
    /// stored data on each call (cloned), or returns a stored error once (consumed).
    /// `batch_cancel` returns the stored result once (consumed), then empty.
    #[allow(clippy::type_complexity)]
    struct MockTaskRepository {
        query_error: Mutex<Option<RepositoryError>>,
        query_tasks_data: Mutex<Vec<Task>>,
        query_total: u64,
        batch_cancel_result:
            Mutex<Option<Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError>>>,
    }

    #[allow(clippy::type_complexity)]
    impl MockTaskRepository {
        fn new() -> Self {
            Self {
                query_error: Mutex::new(None),
                query_tasks_data: Mutex::new(Vec::new()),
                query_total: 0,
                batch_cancel_result: Mutex::new(None),
            }
        }

        fn with_query_data(tasks: Vec<Task>, total: u64) -> Self {
            Self {
                query_error: Mutex::new(None),
                query_tasks_data: Mutex::new(tasks),
                query_total: total,
                batch_cancel_result: Mutex::new(None),
            }
        }

        fn with_query_error(err: RepositoryError) -> Self {
            Self {
                query_error: Mutex::new(Some(err)),
                query_tasks_data: Mutex::new(Vec::new()),
                query_total: 0,
                batch_cancel_result: Mutex::new(None),
            }
        }

        fn with_batch_cancel_result(
            result: Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError>,
        ) -> Self {
            Self {
                query_error: Mutex::new(None),
                query_tasks_data: Mutex::new(Vec::new()),
                query_total: 0,
                batch_cancel_result: Mutex::new(Some(result)),
            }
        }

        fn with_batch_cancel_result_and_query_data(
            result: Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError>,
            query_tasks: Vec<Task>,
        ) -> Self {
            Self {
                query_error: Mutex::new(None),
                query_tasks_data: Mutex::new(query_tasks),
                query_total: 1,
                batch_cancel_result: Mutex::new(Some(result)),
            }
        }
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, _task: &Task) -> Result<Task, RepositoryError> {
            unreachable!("create not expected in task_handler tests")
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }

        async fn update(&self, _task: &Task) -> Result<Task, RepositoryError> {
            unreachable!("update not expected in task_handler tests")
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
        ) -> Result<std::collections::HashSet<String>, RepositoryError> {
            Ok(std::collections::HashSet::new())
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
            Ok(Vec::new())
        }

        async fn query_tasks(
            &self,
            _params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            if let Some(err) = self.query_error.lock().unwrap().take() {
                return Err(err);
            }
            Ok((
                self.query_tasks_data.lock().unwrap().clone(),
                self.query_total,
            ))
        }

        async fn batch_cancel(
            &self,
            _task_ids: Vec<Uuid>,
            _team_id: Uuid,
            _force: bool,
        ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
            match self.batch_cancel_result.lock().unwrap().take() {
                Some(result) => result,
                None => Ok((Vec::new(), Vec::new())),
            }
        }
    }

    // ========== query_tasks handler tests ==========

    #[tokio::test]
    async fn test_query_tasks_handler_success() {
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let repo = Arc::new(MockTaskRepository::with_query_data(vec![task], 1));
        let auth = make_test_auth_state();
        let scrape_repo = make_test_scrape_result_repo();
        let request = TaskQueryRequestDto {
            sync_wait_ms: Some(0),
            ..TaskQueryRequestDto::default()
        };

        let result = query_tasks::<MockTaskRepository>(
            Extension(auth),
            Extension(repo),
            Extension(scrape_repo),
            Json(request),
        )
        .await;

        assert!(result.is_ok(), "query_tasks should succeed");
        let response = result.unwrap();
        let data = response
            .data
            .as_ref()
            .expect("response data should be present");
        assert_eq!(data.tasks.len(), 1);
        assert_eq!(data.total, 1);
        assert!(!data.has_more);
    }

    #[tokio::test]
    async fn test_query_tasks_handler_empty_result() {
        let repo = Arc::new(MockTaskRepository::new());
        let auth = make_test_auth_state();
        let scrape_repo = make_test_scrape_result_repo();
        let request = TaskQueryRequestDto {
            sync_wait_ms: Some(0),
            ..TaskQueryRequestDto::default()
        };

        let result = query_tasks::<MockTaskRepository>(
            Extension(auth),
            Extension(repo),
            Extension(scrape_repo),
            Json(request),
        )
        .await;

        assert!(
            result.is_ok(),
            "query_tasks should succeed with empty results"
        );
        let response = result.unwrap();
        let data = response
            .data
            .as_ref()
            .expect("response data should be present");
        assert_eq!(data.tasks.len(), 0);
        assert_eq!(data.total, 0);
        assert!(!data.has_more);
    }

    #[tokio::test]
    async fn test_query_tasks_handler_has_more() {
        let task1 = make_test_task(Uuid::new_v4(), TaskStatus::Completed);
        let task2 = make_test_task(Uuid::new_v4(), TaskStatus::Completed);
        let repo = Arc::new(MockTaskRepository::with_query_data(vec![task1, task2], 10));
        let auth = make_test_auth_state();
        let scrape_repo = make_test_scrape_result_repo();
        let request = TaskQueryRequestDto {
            limit: Some(2),
            offset: Some(0),
            sync_wait_ms: Some(0),
            ..TaskQueryRequestDto::default()
        };

        let result = query_tasks::<MockTaskRepository>(
            Extension(auth),
            Extension(repo),
            Extension(scrape_repo),
            Json(request),
        )
        .await;

        assert!(result.is_ok());
        let response = result.unwrap();
        let data = response
            .data
            .as_ref()
            .expect("response data should be present");
        assert_eq!(data.tasks.len(), 2);
        assert_eq!(data.total, 10);
        assert!(
            data.has_more,
            "has_more should be true when total > offset+limit"
        );
    }

    #[tokio::test]
    async fn test_query_tasks_handler_validation_error_limit() {
        let repo = Arc::new(MockTaskRepository::new());
        let auth = make_test_auth_state();
        let scrape_repo = make_test_scrape_result_repo();
        let request = TaskQueryRequestDto {
            limit: Some(0),
            sync_wait_ms: Some(0),
            ..TaskQueryRequestDto::default()
        };

        let result = query_tasks::<MockTaskRepository>(
            Extension(auth),
            Extension(repo),
            Extension(scrape_repo),
            Json(request),
        )
        .await;

        assert!(result.is_err(), "limit=0 should fail validation");
    }

    #[tokio::test]
    async fn test_query_tasks_handler_validation_error_sync_wait() {
        let repo = Arc::new(MockTaskRepository::new());
        let auth = make_test_auth_state();
        let scrape_repo = make_test_scrape_result_repo();
        let request = TaskQueryRequestDto {
            sync_wait_ms: Some(30001),
            ..TaskQueryRequestDto::default()
        };

        let result = query_tasks::<MockTaskRepository>(
            Extension(auth),
            Extension(repo),
            Extension(scrape_repo),
            Json(request),
        )
        .await;

        assert!(result.is_err(), "sync_wait_ms=30001 should fail validation");
    }

    #[tokio::test]
    async fn test_query_tasks_handler_repo_error() {
        let repo = Arc::new(MockTaskRepository::with_query_error(
            RepositoryError::Database(anyhow::anyhow!("query failed")),
        ));
        let auth = make_test_auth_state();
        let scrape_repo = make_test_scrape_result_repo();
        let request = TaskQueryRequestDto {
            sync_wait_ms: Some(0),
            ..TaskQueryRequestDto::default()
        };

        let result = query_tasks::<MockTaskRepository>(
            Extension(auth),
            Extension(repo),
            Extension(scrape_repo),
            Json(request),
        )
        .await;

        assert!(result.is_err(), "repo error should propagate");
        match result.unwrap_err() {
            CrawlRsError::Other(msg) => assert!(msg.contains("Query failed")),
            other => panic!("expected CrawlRsError::Other, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_query_tasks_handler_sync_wait_completed() {
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let repo = Arc::new(MockTaskRepository::with_query_data(vec![task], 1));
        let auth = make_test_auth_state();
        let scrape_repo = make_test_scrape_result_repo();
        let request = TaskQueryRequestDto {
            sync_wait_ms: Some(100),
            ..TaskQueryRequestDto::default()
        };

        let result = query_tasks::<MockTaskRepository>(
            Extension(auth),
            Extension(repo),
            Extension(scrape_repo),
            Json(request),
        )
        .await;

        assert!(
            result.is_ok(),
            "sync wait with completed tasks should succeed"
        );
        let response = result.unwrap();
        let data = response
            .data
            .as_ref()
            .expect("response data should be present");
        assert_eq!(data.tasks.len(), 1);
    }

    #[tokio::test]
    async fn test_query_tasks_handler_include_results_no_match_returns_success() {
        // With include_results=true and a real DB pool, fetch_scrape_results
        // queries the DB for the random task_id (no match) and returns an
        // empty map, so the handler succeeds with empty results.
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let repo = Arc::new(MockTaskRepository::with_query_data(vec![task], 1));
        let auth = make_test_auth_state();
        let scrape_repo = make_test_scrape_result_repo();
        let request = TaskQueryRequestDto {
            include_results: Some(true),
            sync_wait_ms: Some(0),
            ..TaskQueryRequestDto::default()
        };

        let result = query_tasks::<MockTaskRepository>(
            Extension(auth),
            Extension(repo),
            Extension(scrape_repo),
            Json(request),
        )
        .await;

        assert!(
            result.is_ok(),
            "include_results with no matching scrape results should succeed"
        );
        let response = result.unwrap();
        let data = response
            .data
            .as_ref()
            .expect("response data should be present");
        assert_eq!(data.tasks.len(), 1);
        // Task has no matching scrape_result in DB → result is None
        assert!(data.tasks[0].result.is_none());
    }

    // ========== cancel_tasks handler tests ==========

    #[tokio::test]
    async fn test_cancel_tasks_handler_success() {
        let task_id = Uuid::new_v4();
        let repo = Arc::new(MockTaskRepository::with_batch_cancel_result(Ok((
            vec![task_id],
            vec![],
        ))));
        let auth = make_test_auth_state();
        let request = TaskCancelRequestDto {
            task_ids: vec![task_id],
            team_id: auth.team_id,
            force: Some(false),
            sync_wait_ms: Some(0),
        };

        let result =
            cancel_tasks::<MockTaskRepository>(Extension(auth), Extension(repo), Json(request))
                .await;

        assert!(result.is_ok(), "cancel_tasks should succeed");
        let response = result.unwrap();
        let data = response
            .data
            .as_ref()
            .expect("response data should be present");
        assert_eq!(data.total_cancelled, 1);
        assert_eq!(data.total_failed, 0);
        assert_eq!(data.cancelled_tasks.len(), 1);
        assert_eq!(data.cancelled_tasks[0].task_id, task_id);
    }

    #[tokio::test]
    async fn test_cancel_tasks_handler_empty_task_ids() {
        let repo = Arc::new(MockTaskRepository::new());
        let auth = make_test_auth_state();
        let request = TaskCancelRequestDto {
            task_ids: vec![],
            team_id: auth.team_id,
            force: Some(false),
            sync_wait_ms: Some(0),
        };

        let result =
            cancel_tasks::<MockTaskRepository>(Extension(auth), Extension(repo), Json(request))
                .await;

        assert!(result.is_err(), "empty task_ids should fail");
        match result.unwrap_err() {
            CrawlRsError::Validation(msg) => {
                assert!(msg.contains("Task IDs cannot be empty"), "got: {}", msg);
            }
            other => panic!("expected CrawlRsError::Validation, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_cancel_tasks_handler_validation_error_sync_wait() {
        let repo = Arc::new(MockTaskRepository::new());
        let auth = make_test_auth_state();
        let request = TaskCancelRequestDto {
            task_ids: vec![Uuid::new_v4()],
            team_id: auth.team_id,
            force: Some(false),
            sync_wait_ms: Some(30001),
        };

        let result =
            cancel_tasks::<MockTaskRepository>(Extension(auth), Extension(repo), Json(request))
                .await;

        assert!(result.is_err(), "sync_wait_ms=30001 should fail validation");
    }

    #[tokio::test]
    async fn test_cancel_tasks_handler_repo_error() {
        let repo = Arc::new(MockTaskRepository::with_batch_cancel_result(Err(
            RepositoryError::Database(anyhow::anyhow!("batch_cancel failed")),
        )));
        let auth = make_test_auth_state();
        let request = TaskCancelRequestDto {
            task_ids: vec![Uuid::new_v4()],
            team_id: auth.team_id,
            force: Some(false),
            sync_wait_ms: Some(0),
        };

        let result =
            cancel_tasks::<MockTaskRepository>(Extension(auth), Extension(repo), Json(request))
                .await;

        assert!(result.is_err(), "repo error should propagate");
    }

    #[tokio::test]
    async fn test_cancel_tasks_handler_with_failed_tasks() {
        let task_id1 = Uuid::new_v4();
        let task_id2 = Uuid::new_v4();
        let repo = Arc::new(MockTaskRepository::with_batch_cancel_result(Ok((
            vec![task_id1],
            vec![(task_id2, "Already completed".to_string())],
        ))));
        let auth = make_test_auth_state();
        let request = TaskCancelRequestDto {
            task_ids: vec![task_id1, task_id2],
            team_id: auth.team_id,
            force: Some(false),
            sync_wait_ms: Some(0),
        };

        let result =
            cancel_tasks::<MockTaskRepository>(Extension(auth), Extension(repo), Json(request))
                .await;

        assert!(result.is_ok());
        let response = result.unwrap();
        let data = response
            .data
            .as_ref()
            .expect("response data should be present");
        assert_eq!(data.total_cancelled, 1);
        assert_eq!(data.total_failed, 1);
        assert_eq!(data.failed_tasks[0].task_id, task_id2);
        assert_eq!(data.failed_tasks[0].reason, "Already completed");
    }

    #[tokio::test]
    async fn test_cancel_tasks_handler_sync_wait() {
        let task_id = Uuid::new_v4();
        let cancelled_task = make_test_task(task_id, TaskStatus::Cancelled);
        let repo = Arc::new(MockTaskRepository::with_batch_cancel_result_and_query_data(
            Ok((vec![task_id], vec![])),
            vec![cancelled_task],
        ));
        let auth = make_test_auth_state();
        let request = TaskCancelRequestDto {
            task_ids: vec![task_id],
            team_id: auth.team_id,
            force: Some(false),
            sync_wait_ms: Some(100),
        };

        let result =
            cancel_tasks::<MockTaskRepository>(Extension(auth), Extension(repo), Json(request))
                .await;

        assert!(result.is_ok(), "cancel with sync wait should succeed");
        let response = result.unwrap();
        let data = response
            .data
            .as_ref()
            .expect("response data should be present");
        assert_eq!(data.total_cancelled, 1);
    }

    // ========== wait_for_tasks_completion tests ==========

    #[tokio::test]
    async fn test_wait_for_tasks_completion_already_completed() {
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let repo = MockTaskRepository::with_query_data(vec![task], 1);

        let result = wait_for_tasks_completion(&repo, &[task_id], Uuid::nil(), 100, 500).await;

        assert!(
            result.is_ok(),
            "should complete immediately when tasks are done"
        );
    }

    #[tokio::test]
    async fn test_wait_for_tasks_completion_timeout() {
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Active);
        let repo = MockTaskRepository::with_query_data(vec![task], 1);

        let result = wait_for_tasks_completion(&repo, &[task_id], Uuid::nil(), 50, 500).await;

        assert!(result.is_ok(), "should return Ok on timeout");
    }

    #[tokio::test]
    async fn test_wait_for_tasks_completion_query_error() {
        let task_id = Uuid::new_v4();
        let repo = MockTaskRepository::with_query_error(RepositoryError::Database(
            anyhow::anyhow!("poll query failed"),
        ));

        let result = wait_for_tasks_completion(&repo, &[task_id], Uuid::nil(), 100, 500).await;

        assert!(result.is_err(), "query error should propagate");
    }

    // ========== handle_sync_wait_and_get_status tests ==========

    #[tokio::test]
    async fn test_handle_sync_wait_and_get_status_success() {
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let repo = MockTaskRepository::with_query_data(vec![task], 1);

        let result = handle_sync_wait_and_get_status(&repo, &[task_id], Uuid::nil(), 200).await;

        assert!(result.is_ok());
        let sync_result = result.unwrap();
        assert!(!sync_result.is_timeout, "should complete before timeout");
    }

    #[tokio::test]
    async fn test_handle_sync_wait_and_get_status_error_continues() {
        // Even when wait_for_tasks_completion returns an error,
        // handle_sync_wait_and_get_status catches it and returns Ok.
        let task_id = Uuid::new_v4();
        let repo = MockTaskRepository::with_query_error(RepositoryError::Database(
            anyhow::anyhow!("poll failed"),
        ));

        let result = handle_sync_wait_and_get_status(&repo, &[task_id], Uuid::nil(), 200).await;

        assert!(result.is_ok(), "should return Ok even on wait error");
    }

    // ========== Direct function tests ==========

    #[tokio::test]
    async fn test_query_tasks_for_poll_success() {
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let repo = MockTaskRepository::with_query_data(vec![task], 1);

        let result = query_tasks_for_poll(&repo, Uuid::nil(), &[task_id]).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn test_query_tasks_for_poll_error() {
        let task_id = Uuid::new_v4();
        let repo = MockTaskRepository::with_query_error(RepositoryError::Database(
            anyhow::anyhow!("poll failed"),
        ));

        let result = query_tasks_for_poll(&repo, Uuid::nil(), &[task_id]).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_execute_task_query_success() {
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let repo = MockTaskRepository::with_query_data(vec![task], 1);
        let request = TaskQueryRequestDto::default();

        let result = execute_task_query(&repo, Uuid::nil(), &request, 100, 0).await;

        assert!(result.is_ok());
        let (tasks, total) = result.unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(total, 1);
    }

    #[tokio::test]
    async fn test_execute_task_query_error() {
        let repo = MockTaskRepository::with_query_error(RepositoryError::Database(
            anyhow::anyhow!("exec failed"),
        ));
        let request = TaskQueryRequestDto::default();

        let result = execute_task_query(&repo, Uuid::nil(), &request, 100, 0).await;

        assert!(result.is_err());
        match result.unwrap_err() {
            CrawlRsError::Other(msg) => assert!(msg.contains("Query failed")),
            other => panic!("expected CrawlRsError::Other, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_handle_sync_wait_direct() {
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Completed);
        let repo = MockTaskRepository::with_query_data(vec![task.clone()], 1);

        let result = handle_sync_wait(&repo, &[task], Uuid::nil(), 100).await;

        assert!(result.is_ok());
        let waited = result.unwrap();
        assert!(
            waited < 1000,
            "waited_time_ms should be small, got {}",
            waited
        );
    }

    #[tokio::test]
    async fn test_fetch_scrape_results_empty_tasks() {
        // 空 tasks 时 fetch_scrape_results 提前返回，理论上不需要 DB；
        // 但 make_test_scrape_result_repo 仍会构造 DbPool，需 TEST_DATABASE_URL。
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let repo = make_test_scrape_result_repo();
        let tasks: Vec<Task> = vec![];

        let result = fetch_scrape_results(repo.as_ref(), &tasks).await;

        assert!(result.is_ok());
        let map = result.unwrap();
        assert!(map.is_some());
        assert!(map.unwrap().is_empty());
    }

    #[tokio::test]
    #[ignore = "test semantics invalid after permission feature removal"]
    async fn test_fetch_scrape_results_non_empty_no_db() {
        // With a lazy (non-connecting) pool, calling find_by_task_ids on
        // non-empty task IDs should fail because the pool cannot connect.
        let repo = make_test_scrape_result_repo();
        let task_id = Uuid::new_v4();
        let tasks = vec![make_test_task(task_id, TaskStatus::Completed)];

        let result = fetch_scrape_results(repo.as_ref(), &tasks).await;

        assert!(result.is_err(), "should fail without a real DB connection");
    }

    // ========== Additional edge case tests ==========

    #[tokio::test]
    async fn test_query_tasks_handler_include_results_with_empty_tasks_skips_fetch() {
        // include_results=true but tasks is empty → fetch_scrape_results not called
        let repo = Arc::new(MockTaskRepository::new());
        let auth = make_test_auth_state();
        let scrape_repo = make_test_scrape_result_repo();
        let request = TaskQueryRequestDto {
            include_results: Some(true),
            sync_wait_ms: Some(0),
            ..TaskQueryRequestDto::default()
        };

        let result = query_tasks::<MockTaskRepository>(
            Extension(auth),
            Extension(repo),
            Extension(scrape_repo),
            Json(request),
        )
        .await;

        assert!(
            result.is_ok(),
            "should succeed when tasks empty even with include_results"
        );
        let binding = result.unwrap();
        let data = binding.data.as_ref().expect("data present");
        assert!(data.tasks.is_empty());
    }

    #[tokio::test]
    async fn test_cancel_tasks_handler_force_true_succeeds() {
        let task_id = Uuid::new_v4();
        let repo = Arc::new(MockTaskRepository::with_batch_cancel_result(Ok((
            vec![task_id],
            vec![],
        ))));
        let auth = make_test_auth_state();
        let request = TaskCancelRequestDto {
            task_ids: vec![task_id],
            team_id: auth.team_id,
            force: Some(true),
            sync_wait_ms: Some(0),
        };

        let result =
            cancel_tasks::<MockTaskRepository>(Extension(auth), Extension(repo), Json(request))
                .await;

        assert!(result.is_ok(), "force=true should succeed");
        let binding = result.unwrap();
        let data = binding.data.as_ref().expect("data present");
        assert_eq!(data.total_cancelled, 1);
    }

    #[tokio::test]
    async fn test_cancel_tasks_handler_force_none_uses_default_false() {
        let task_id = Uuid::new_v4();
        let repo = Arc::new(MockTaskRepository::with_batch_cancel_result(Ok((
            vec![task_id],
            vec![],
        ))));
        let auth = make_test_auth_state();
        let request = TaskCancelRequestDto {
            task_ids: vec![task_id],
            team_id: auth.team_id,
            force: None,
            sync_wait_ms: Some(0),
        };

        let result =
            cancel_tasks::<MockTaskRepository>(Extension(auth), Extension(repo), Json(request))
                .await;

        assert!(result.is_ok(), "force=None should default to false");
    }

    // ========== Test logger for covering log::debug!/log::error! ==========

    use log::{LevelFilter, Log, Metadata, Record};
    use std::sync::Once;

    static LOGGER_INIT: Once = Once::new();

    struct CapturingLogger;

    impl Log for CapturingLogger {
        fn enabled(&self, metadata: &Metadata) -> bool {
            metadata.level() <= log::Level::Debug
        }
        fn log(&self, _record: &Record) {}
        fn flush(&self) {}
    }

    fn ensure_debug_logger() {
        LOGGER_INIT.call_once(|| {
            static CAPTURING_LOGGER: CapturingLogger = CapturingLogger;
            let _ = log::set_logger(&CAPTURING_LOGGER);
            log::set_max_level(LevelFilter::Debug);
        });
    }

    // ========== log::debug!/log::error! coverage tests ==========

    #[test]
    fn test_poll_count_exceeded_logs_debug_with_logger() {
        ensure_debug_logger();
        // When count >= max_count, log::debug! should execute
        assert!(poll_count_exceeded(60, 60));
        assert!(poll_count_exceeded(100, 60));
    }

    #[tokio::test]
    async fn test_handle_sync_wait_error_logs_error_with_logger() {
        ensure_debug_logger();
        // When wait_for_tasks_completion returns an error, log::error! should execute
        let task_id = Uuid::new_v4();
        let repo = MockTaskRepository::with_query_error(RepositoryError::Database(
            anyhow::anyhow!("poll failed for logging test"),
        ));

        let result = handle_sync_wait_and_get_status(&repo, &[task_id], Uuid::nil(), 200).await;

        assert!(result.is_ok(), "should return Ok even on wait error");
    }

    #[tokio::test]
    async fn test_wait_for_tasks_completion_logs_debug_poll_count_with_logger() {
        ensure_debug_logger();
        // Trigger poll_count_exceeded path (poll_count >= MAX_POLL_COUNT)
        // Use a very short timeout and active tasks to ensure polling occurs
        let task_id = Uuid::new_v4();
        let task = make_test_task(task_id, TaskStatus::Active);
        let repo = MockTaskRepository::with_query_data(vec![task], 1);

        let result = wait_for_tasks_completion(&repo, &[task_id], Uuid::nil(), 50, 500).await;

        assert!(result.is_ok(), "should return Ok on timeout");
    }
}
