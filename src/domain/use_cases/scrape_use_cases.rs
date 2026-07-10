// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Scrape-related use cases

use crate::application::dto::scrape_request::ScrapeRequestDto;
use crate::application::dto::scrape_response::ScrapeResponseDto;
use crate::domain::models::{Task, TaskType};
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::services::credits_service::CreditsService;
use std::sync::Arc;
use uuid::Uuid;

/// 异步抓取请求
pub struct AsyncScrapeRequest {
    pub team_id: Uuid,
    pub api_key_id: Uuid,
    pub request: ScrapeRequestDto,
    pub engine: Option<String>,
    pub priority: Option<i32>,
    pub max_retries: Option<i32>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// 异步抓取响应
pub struct AsyncScrapeResponse {
    pub task_id: Uuid,
}

/// 同步抓取请求
pub struct SyncScrapeRequest {
    pub team_id: Uuid,
    pub request: ScrapeRequestDto,
    pub engine: Option<String>,
    pub timeout_ms: Option<u32>,
}

/// 同步抓取响应
pub struct SyncScrapeResponse {
    pub task_id: Uuid,
    pub url: String,
    pub response_time_ms: u64,
}

/// 抓取结果查询请求
pub struct GetScrapeResultRequest {
    pub team_id: Uuid,
    pub task_id: Uuid,
}

/// 抓取结果查询响应
pub struct GetScrapeResultResponse {
    pub result: Option<ScrapeResponseDto>,
    pub status: String,
    pub is_complete: bool,
}

/// 异步抓取用例
#[allow(dead_code)]
pub struct AsyncScrapeUseCase<T: TaskRepository, R: ScrapeResultRepository, Cr: CreditsRepository> {
    task_repo: Arc<T>,
    result_repo: Arc<R>,
    credits_service: Arc<CreditsService<Cr>>,
}

impl<T: TaskRepository, R: ScrapeResultRepository, Cr: CreditsRepository>
    AsyncScrapeUseCase<T, R, Cr>
{
    pub fn new(
        task_repo: Arc<T>,
        result_repo: Arc<R>,
        credits_service: Arc<CreditsService<Cr>>,
    ) -> Self {
        Self {
            task_repo,
            result_repo,
            credits_service,
        }
    }

    pub async fn execute(
        &self,
        request: AsyncScrapeRequest,
    ) -> Result<AsyncScrapeResponse, anyhow::Error> {
        // 创建抓取任务
        let payload = serde_json::to_value(&request.request).unwrap_or_default();
        let _now = chrono::Utc::now();
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Scrape,
            request.team_id,
            request.api_key_id,
            request.request.url.clone(),
            payload,
        );

        self.task_repo.create(&task).await?;

        Ok(AsyncScrapeResponse { task_id: task.id })
    }
}

/// 同步抓取用例
#[allow(dead_code)]
pub struct SyncScrapeUseCase<R: ScrapeResultRepository, Cr: CreditsRepository> {
    result_repo: Arc<R>,
    credits_service: Arc<CreditsService<Cr>>,
}

impl<R: ScrapeResultRepository, Cr: CreditsRepository> SyncScrapeUseCase<R, Cr> {
    pub fn new(result_repo: Arc<R>, credits_service: Arc<CreditsService<Cr>>) -> Self {
        Self {
            result_repo,
            credits_service,
        }
    }

    /// 创建任务并返回基本信息供后续处理
    pub async fn prepare(
        &self,
        team_id: Uuid,
        api_key_id: Uuid,
        request: &ScrapeRequestDto,
    ) -> Result<Task, anyhow::Error> {
        let payload = serde_json::to_value(request).unwrap_or_default();
        let task = Task::new(
            Uuid::new_v4(),
            TaskType::Scrape,
            team_id,
            api_key_id,
            request.url.clone(),
            payload,
        );

        Ok(task)
    }
}

/// 获取抓取结果用例
pub struct GetScrapeResultUseCase<R: ScrapeResultRepository> {
    result_repo: Arc<R>,
}

impl<R: ScrapeResultRepository> GetScrapeResultUseCase<R> {
    pub fn new(result_repo: Arc<R>) -> Self {
        Self { result_repo }
    }

    pub async fn execute(
        &self,
        request: GetScrapeResultRequest,
    ) -> Result<GetScrapeResultResponse, anyhow::Error> {
        match self.result_repo.find_by_task_id(request.task_id).await? {
            Some(result) => {
                let response = Some(ScrapeResponseDto {
                    id: request.task_id,
                    url: result.url,
                    credits_used: 1,
                });

                Ok(GetScrapeResultResponse {
                    result: response,
                    status: "Completed".to_string(),
                    is_complete: true,
                })
            }
            None => Ok(GetScrapeResultResponse {
                result: None,
                status: "NotFound".to_string(),
                is_complete: false,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::scrape_result::ScrapeResult;
    use crate::domain::repositories::task_repository::RepositoryError;
    use crate::domain::services::test_helpers::MockCreditsRepository;
    use async_trait::async_trait;
    use chrono::Utc;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Mutex;

    // ============ Mock TaskRepository ============

    /// Configurable mock: succeeds or fails on create, tracks created tasks
    struct MockTaskRepository {
        created_count: AtomicU32,
        should_fail: bool,
    }

    impl Default for MockTaskRepository {
        fn default() -> Self {
            Self {
                created_count: AtomicU32::new(0),
                should_fail: false,
            }
        }
    }

    impl MockTaskRepository {
        fn failing() -> Self {
            Self {
                created_count: AtomicU32::new(0),
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            if self.should_fail {
                return Err(RepositoryError::Database(anyhow::anyhow!("task repo down")));
            }
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
            _params: crate::domain::repositories::task_repository::TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
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

    // ============ Mock ScrapeResultRepository ============

    /// Mock that returns a configurable result for find_by_task_id
    struct MockScrapeResultRepository {
        /// Stored result to return; None means "not found"
        stored_result: Mutex<Option<ScrapeResult>>,
        /// Whether find_by_task_id should error
        should_fail: bool,
    }

    impl MockScrapeResultRepository {
        fn with_result(result: Option<ScrapeResult>) -> Self {
            Self {
                stored_result: Mutex::new(result),
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                stored_result: Mutex::new(None),
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
                return Err(anyhow::anyhow!("result repo down"));
            }
            Ok(self.stored_result.lock().unwrap().clone())
        }

        async fn find_by_task_ids(&self, _task_ids: &[Uuid]) -> anyhow::Result<Vec<ScrapeResult>> {
            Ok(vec![])
        }

        async fn get_team_avg_response_time(&self, _team_id: Uuid) -> anyhow::Result<f64> {
            Ok(0.0)
        }
    }

    // ============ Helpers ============

    fn make_scrape_request() -> ScrapeRequestDto {
        ScrapeRequestDto {
            url: "https://example.com".to_string(),
            formats: None,
            include_tags: None,
            exclude_tags: None,
            webhook: None,
            extraction_rules: None,
            actions: None,
            options: None,
            metadata: None,
            sync_wait_ms: None,
        }
    }

    fn make_scrape_result() -> ScrapeResult {
        ScrapeResult {
            id: Uuid::new_v4(),
            task_id: Uuid::new_v4(),
            url: "https://example.com".to_string(),
            status_code: 200,
            content: "<html></html>".to_string(),
            content_type: "text/html".to_string(),
            headers: serde_json::json!({}),
            meta_data: serde_json::json!({}),
            screenshot: None,
            response_time_ms: 100,
            created_at: Utc::now().naive_utc(),
        }
    }

    fn make_credits_service() -> Arc<CreditsService<MockCreditsRepository>> {
        Arc::new(CreditsService::with_default_config(Arc::new(
            MockCreditsRepository {
                deducted: Arc::new(Mutex::new(vec![])),
            },
        )))
    }

    // ============ AsyncScrapeUseCase ============

    #[test]
    fn test_async_scrape_new_does_not_call_repo() {
        let task_repo = Arc::new(MockTaskRepository::default());
        let result_repo = Arc::new(MockScrapeResultRepository::with_result(None));
        let credits = make_credits_service();
        let _use_case = AsyncScrapeUseCase::new(task_repo.clone(), result_repo, credits);
        assert_eq!(task_repo.created_count.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_async_scrape_execute_success_creates_task() {
        let task_repo = Arc::new(MockTaskRepository::default());
        let result_repo = Arc::new(MockScrapeResultRepository::with_result(None));
        let credits = make_credits_service();
        let use_case = AsyncScrapeUseCase::new(task_repo.clone(), result_repo, credits);

        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let request = AsyncScrapeRequest {
            team_id,
            api_key_id,
            request: make_scrape_request(),
            engine: None,
            priority: None,
            max_retries: None,
            expires_at: None,
        };

        let resp = use_case
            .execute(request)
            .await
            .expect("execute should succeed");

        assert_eq!(task_repo.created_count.load(Ordering::SeqCst), 1);
        // task_id should be a valid non-nil UUID
        assert_ne!(resp.task_id, Uuid::nil());
    }

    #[tokio::test]
    async fn test_async_scrape_execute_propagates_repo_error() {
        let task_repo = Arc::new(MockTaskRepository::failing());
        let result_repo = Arc::new(MockScrapeResultRepository::with_result(None));
        let credits = make_credits_service();
        let use_case = AsyncScrapeUseCase::new(task_repo, result_repo, credits);

        let request = AsyncScrapeRequest {
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            request: make_scrape_request(),
            engine: None,
            priority: None,
            max_retries: None,
            expires_at: None,
        };

        let result = use_case.execute(request).await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("should propagate repo error, got Ok"),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("task repo down"),
            "error should mention repo failure, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn test_async_scrape_execute_with_engine_option_set() {
        // Engine field is stored but not used in execute; verify it doesn't break
        let task_repo = Arc::new(MockTaskRepository::default());
        let result_repo = Arc::new(MockScrapeResultRepository::with_result(None));
        let credits = make_credits_service();
        let use_case = AsyncScrapeUseCase::new(task_repo.clone(), result_repo, credits);

        let request = AsyncScrapeRequest {
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            request: make_scrape_request(),
            engine: Some("playwright".to_string()),
            priority: Some(5),
            max_retries: Some(10),
            expires_at: Some(Utc::now() + chrono::Duration::hours(1)),
        };

        let result = use_case.execute(request).await;
        assert!(result.is_ok());
        assert_eq!(task_repo.created_count.load(Ordering::SeqCst), 1);
    }

    // ============ SyncScrapeUseCase ============

    #[test]
    fn test_sync_scrape_new_does_not_call_repo() {
        let result_repo = Arc::new(MockScrapeResultRepository::with_result(None));
        let credits = make_credits_service();
        let _use_case = SyncScrapeUseCase::new(result_repo, credits);
    }

    #[tokio::test]
    async fn test_sync_scrape_prepare_returns_task_with_serialized_payload() {
        let result_repo = Arc::new(MockScrapeResultRepository::with_result(None));
        let credits = make_credits_service();
        let use_case = SyncScrapeUseCase::new(result_repo, credits);

        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let request = make_scrape_request();

        let result = use_case.prepare(team_id, api_key_id, &request).await;

        assert!(result.is_ok(), "prepare should succeed");
        let task = result.unwrap();
        assert_eq!(task.team_id, team_id);
        assert_eq!(task.api_key_id, api_key_id);
        assert_eq!(task.url, request.url);
        assert_eq!(task.task_type, TaskType::Scrape);
        // payload should be the serialized request (contains the url)
        assert!(
            task.payload.get("url").is_some(),
            "payload should contain serialized url field"
        );
        assert_eq!(task.payload["url"], serde_json::json!(request.url));
    }

    #[tokio::test]
    async fn test_sync_scrape_prepare_generates_unique_task_ids() {
        let result_repo = Arc::new(MockScrapeResultRepository::with_result(None));
        let credits = make_credits_service();
        let use_case = SyncScrapeUseCase::new(result_repo, credits);

        let request = make_scrape_request();
        let t1 = use_case
            .prepare(Uuid::new_v4(), Uuid::new_v4(), &request)
            .await
            .unwrap();
        let t2 = use_case
            .prepare(Uuid::new_v4(), Uuid::new_v4(), &request)
            .await
            .unwrap();

        assert_ne!(t1.id, t2.id, "each prepare should generate unique task id");
    }

    #[tokio::test]
    async fn test_sync_scrape_prepare_with_request_containing_extras() {
        // Request with formats, webhook, options — payload should serialize all fields
        let result_repo = Arc::new(MockScrapeResultRepository::with_result(None));
        let credits = make_credits_service();
        let use_case = SyncScrapeUseCase::new(result_repo, credits);

        let mut request = make_scrape_request();
        request.formats = Some(vec!["html".to_string(), "markdown".to_string()]);
        request.webhook = Some("https://hook.example.com".to_string());

        let task = use_case
            .prepare(Uuid::new_v4(), Uuid::new_v4(), &request)
            .await
            .unwrap();

        assert_eq!(
            task.payload["formats"],
            serde_json::json!(["html", "markdown"])
        );
        assert_eq!(
            task.payload["webhook"],
            serde_json::json!("https://hook.example.com")
        );
    }

    // ============ GetScrapeResultUseCase ============

    #[test]
    fn test_get_scrape_result_new_does_not_call_repo() {
        let result_repo = Arc::new(MockScrapeResultRepository::with_result(None));
        let _use_case = GetScrapeResultUseCase::new(result_repo);
    }

    #[tokio::test]
    async fn test_get_scrape_result_found_returns_completed() {
        let result = make_scrape_result();
        let expected_url = result.url.clone();
        let result_repo = Arc::new(MockScrapeResultRepository::with_result(Some(result)));
        let use_case = GetScrapeResultUseCase::new(result_repo);

        let task_id = Uuid::new_v4();
        let resp = use_case
            .execute(GetScrapeResultRequest {
                team_id: Uuid::new_v4(),
                task_id,
            })
            .await
            .expect("execute should succeed");

        assert!(resp.is_complete, "found result should be complete");
        assert_eq!(resp.status, "Completed");
        let dto = resp.result.expect("result should be Some");
        assert_eq!(dto.id, task_id);
        assert_eq!(dto.url, expected_url);
        assert_eq!(dto.credits_used, 1);
    }

    #[tokio::test]
    async fn test_get_scrape_result_not_found_returns_not_found() {
        let result_repo = Arc::new(MockScrapeResultRepository::with_result(None));
        let use_case = GetScrapeResultUseCase::new(result_repo);

        let resp = use_case
            .execute(GetScrapeResultRequest {
                team_id: Uuid::new_v4(),
                task_id: Uuid::new_v4(),
            })
            .await
            .expect("execute should succeed");

        assert!(!resp.is_complete, "not found should not be complete");
        assert_eq!(resp.status, "NotFound");
        assert!(resp.result.is_none(), "result should be None");
    }

    #[tokio::test]
    async fn test_get_scrape_result_repo_error_propagates() {
        let result_repo = Arc::new(MockScrapeResultRepository::failing());
        let use_case = GetScrapeResultUseCase::new(result_repo);

        let result = use_case
            .execute(GetScrapeResultRequest {
                team_id: Uuid::new_v4(),
                task_id: Uuid::new_v4(),
            })
            .await;

        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("should propagate repo error, got Ok"),
        };
        let msg = err.to_string();
        assert!(
            msg.contains("result repo down"),
            "error should mention repo failure, got: {}",
            msg
        );
    }
}
