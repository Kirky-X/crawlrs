// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Task-related use cases

use crate::domain::models::{Task, TaskStatus, TaskType};
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::services::credits_service::CreditsService;
use chrono::{DateTime, FixedOffset};
use std::sync::Arc;
use uuid::Uuid;

/// 创建任务请求
pub struct CreateTaskRequest {
    pub team_id: Uuid,
    pub api_key_id: Uuid,
    pub task_type: TaskType,
    pub url: String,
    pub name: Option<String>,
    pub config: Option<serde_json::Value>,
    pub priority: Option<i32>,
    pub max_retries: Option<i32>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// 创建任务响应
pub struct CreateTaskResponse {
    pub task: Task,
}

/// 查询任务请求
pub struct QueryTasksRequest {
    pub team_id: Uuid,
    pub task_ids: Option<Vec<Uuid>>,
    pub task_types: Option<Vec<TaskType>>,
    pub statuses: Option<Vec<TaskStatus>>,
    pub created_after: Option<DateTime<FixedOffset>>,
    pub created_before: Option<DateTime<FixedOffset>>,
    pub crawl_id: Option<Uuid>,
    pub limit: Option<u32>,
    pub offset: Option<u32>,
}

/// 查询任务响应
pub struct QueryTasksResponse {
    pub tasks: Vec<Task>,
    pub total: u64,
    pub has_more: bool,
}

/// 取消任务请求
pub struct CancelTasksRequest {
    pub team_id: Uuid,
    pub task_ids: Vec<Uuid>,
    pub force: Option<bool>,
}

/// 取消任务响应
pub struct CancelTasksResponse {
    pub cancelled: Vec<Uuid>,
    pub failed: Vec<(Uuid, String)>,
    pub total_cancelled: u64,
    pub total_failed: u64,
}

/// 创建任务用例
#[allow(dead_code)]
pub struct CreateTaskUseCase<T: TaskRepository, R: CreditsRepository> {
    task_repo: Arc<T>,
    credits_service: Arc<CreditsService<R>>,
}

impl<T: TaskRepository, R: CreditsRepository> CreateTaskUseCase<T, R> {
    pub fn new(task_repo: Arc<T>, credits_service: Arc<CreditsService<R>>) -> Self {
        Self {
            task_repo,
            credits_service,
        }
    }

    pub async fn execute(
        &self,
        request: CreateTaskRequest,
    ) -> Result<CreateTaskResponse, anyhow::Error> {
        // 创建任务
        let payload = request.config.unwrap_or_else(|| serde_json::json!({}));
        let mut task = Task::new(
            Uuid::new_v4(),
            request.task_type,
            request.team_id,
            request.api_key_id,
            request.url,
            payload,
        );

        // 设置可选参数
        if let Some(priority) = request.priority {
            task.priority = priority;
        }
        if let Some(max_retries) = request.max_retries {
            task.max_retries = max_retries;
        }
        if let Some(expires_at) = request.expires_at {
            task.expires_at = Some(expires_at);
        }

        self.task_repo.create(&task).await?;

        Ok(CreateTaskResponse { task })
    }
}

/// 查询任务用例
pub struct QueryTasksUseCase<T: TaskRepository> {
    task_repo: Arc<T>,
}

impl<T: TaskRepository> QueryTasksUseCase<T> {
    pub fn new(task_repo: Arc<T>) -> Self {
        Self { task_repo }
    }

    pub async fn execute(
        &self,
        request: QueryTasksRequest,
    ) -> Result<QueryTasksResponse, anyhow::Error> {
        let params = crate::domain::repositories::task_repository::TaskQueryParams {
            team_id: request.team_id,
            task_ids: request.task_ids,
            task_types: request.task_types,
            statuses: request.statuses,
            created_after: request
                .created_after
                .map(|dt| dt.with_timezone(&chrono::Utc)),
            created_before: request
                .created_before
                .map(|dt| dt.with_timezone(&chrono::Utc)),
            crawl_id: request.crawl_id,
            limit: request.limit.unwrap_or(100),
            offset: request.offset.unwrap_or(0),
            cursor: None,
            cursor_id: None,
        };

        let (tasks, total) = self.task_repo.query_tasks(params).await?;

        let has_more = (u64::from(request.offset.unwrap_or(0)) + tasks.len() as u64) < total;

        Ok(QueryTasksResponse {
            tasks,
            total,
            has_more,
        })
    }
}

/// 取消任务用例
#[allow(dead_code)]
pub struct CancelTasksUseCase<T: TaskRepository, R: CreditsRepository> {
    task_repo: Arc<T>,
    credits_service: Arc<CreditsService<R>>,
}

impl<T: TaskRepository, R: CreditsRepository> CancelTasksUseCase<T, R> {
    pub fn new(task_repo: Arc<T>, credits_service: Arc<CreditsService<R>>) -> Self {
        Self {
            task_repo,
            credits_service,
        }
    }

    pub async fn execute(
        &self,
        request: CancelTasksRequest,
    ) -> Result<CancelTasksResponse, anyhow::Error> {
        let mut cancelled = Vec::new();
        let mut failed = Vec::new();

        for task_id in &request.task_ids {
            match self.task_repo.find_by_id(*task_id).await {
                Ok(Some(task)) => {
                    if task.team_id != request.team_id {
                        failed.push((*task_id, "Task does not belong to team".to_string()));
                        continue;
                    }

                    if task.status == TaskStatus::Completed || task.status == TaskStatus::Failed {
                        failed.push((
                            *task_id,
                            format!("Cannot cancel task in status: {}", task.status),
                        ));
                        continue;
                    }

                    // 如果不是强制取消且任务正在执行中，则不允许取消
                    if !request.force.unwrap_or(false) && task.status == TaskStatus::Active {
                        failed.push((
                            *task_id,
                            "Task is running, use force=true to cancel".to_string(),
                        ));
                        continue;
                    }

                    // 更新任务状态为已取消
                    self.task_repo.mark_cancelled(*task_id).await?;
                    cancelled.push(*task_id);
                }
                Ok(None) => {
                    failed.push((*task_id, "Task not found".to_string()));
                }
                Err(e) => {
                    failed.push((*task_id, format!("Repository error: {:?}", e)));
                }
            }
        }

        let total_cancelled = cancelled.len() as u64;
        let total_failed = failed.len() as u64;

        Ok(CancelTasksResponse {
            cancelled,
            failed,
            total_cancelled,
            total_failed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::repositories::task_repository::{RepositoryError, TaskQueryParams};
    use crate::domain::services::test_helpers::MockCreditsRepository;
    use async_trait::async_trait;
    use chrono::{FixedOffset, Utc};
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};
    use std::sync::{Arc, Mutex};

    // ============ Configurable Mock TaskRepository ============

    /// Mock TaskRepository with configurable behavior via flags and stored tasks.
    /// Defaults to all-success behavior.
    #[derive(Default)]
    struct MockTaskRepository {
        /// Tasks returned by find_by_id (keyed by task id)
        tasks: Mutex<HashMap<Uuid, Task>>,
        /// Tasks returned by find_by_crawl_id (keyed by crawl id)
        crawl_tasks: Mutex<HashMap<Uuid, Vec<Task>>>,
        /// Tasks and total returned by query_tasks
        query_result: Mutex<Option<(Vec<Task>, u64)>>,
        create_count: AtomicU32,
        cancel_count: AtomicU32,
        /// 0 = success, 1 = fail
        create_should_fail: AtomicU8,
        query_should_fail: AtomicU8,
        cancel_should_fail: AtomicU8,
        /// 0 = success, 1 = error on find_by_id
        find_should_error: AtomicU8,
    }

    impl MockTaskRepository {
        fn with_task(task: Task) -> Self {
            let mut tasks = HashMap::new();
            tasks.insert(task.id, task);
            Self {
                tasks: Mutex::new(tasks),
                ..Default::default()
            }
        }

        fn with_tasks(task_list: Vec<Task>) -> Self {
            let tasks: HashMap<Uuid, Task> = task_list
                .into_iter()
                .map(|t| (t.id, t))
                .collect();
            Self {
                tasks: Mutex::new(tasks),
                ..Default::default()
            }
        }

        fn set_create_fail(&self) {
            self.create_should_fail.store(1, Ordering::SeqCst);
        }

        fn set_query_fail(&self) {
            self.query_should_fail.store(1, Ordering::SeqCst);
        }

        fn set_cancel_fail(&self) {
            self.cancel_should_fail.store(1, Ordering::SeqCst);
        }

        fn set_find_error(&self) {
            self.find_should_error.store(1, Ordering::SeqCst);
        }

        fn set_query_result(&self, tasks: Vec<Task>, total: u64) {
            *self.query_result.lock().unwrap() = Some((tasks, total));
        }

        fn create_count(&self) -> u32 {
            self.create_count.load(Ordering::SeqCst)
        }

        fn cancel_count(&self) -> u32 {
            self.cancel_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            self.create_count.fetch_add(1, Ordering::SeqCst);
            if self.create_should_fail.load(Ordering::SeqCst) == 1 {
                return Err(RepositoryError::Database(anyhow::anyhow!("create failed")));
            }
            Ok(task.clone())
        }

        async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>, RepositoryError> {
            if self.find_should_error.load(Ordering::SeqCst) == 1 {
                return Err(RepositoryError::Database(anyhow::anyhow!("find_by_id error")));
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
            self.cancel_count.fetch_add(1, Ordering::SeqCst);
            if self.cancel_should_fail.load(Ordering::SeqCst) == 1 {
                return Err(RepositoryError::Database(anyhow::anyhow!("cancel failed")));
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

        async fn find_by_crawl_id(&self, crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
            Ok(self.crawl_tasks.lock().unwrap().get(&crawl_id).cloned().unwrap_or_default())
        }

        async fn query_tasks(
            &self,
            _params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            if self.query_should_fail.load(Ordering::SeqCst) == 1 {
                return Err(RepositoryError::Database(anyhow::anyhow!("query failed")));
            }
            Ok(self
                .query_result
                .lock()
                .unwrap()
                .clone()
                .unwrap_or((vec![], 0)))
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

    // ============ Helpers ============

    fn make_task_with_status(status: TaskStatus, team_id: Uuid) -> Task {
        let mut task = Task::new(
            Uuid::new_v4(),
            TaskType::Scrape,
            team_id,
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        );
        task.status = status;
        task
    }

    fn make_credits_service() -> Arc<CreditsService<MockCreditsRepository>> {
        Arc::new(CreditsService::with_default_config(Arc::new(
            MockCreditsRepository {
                deducted: Arc::new(Mutex::new(vec![])),
            },
        )))
    }

    // ============ CreateTaskUseCase ============

    #[test]
    fn test_create_task_new_does_not_call_repo() {
        let repo = Arc::new(MockTaskRepository::default());
        let credits = make_credits_service();
        let _use_case = CreateTaskUseCase::new(repo.clone(), credits);
        assert_eq!(repo.create_count(), 0);
    }

    #[tokio::test]
    async fn test_create_task_execute_success_with_all_optional_fields() {
        let repo = Arc::new(MockTaskRepository::default());
        let credits = make_credits_service();
        let use_case = CreateTaskUseCase::new(repo.clone(), credits);

        let team_id = Uuid::new_v4();
        let expires = Utc::now() + chrono::Duration::days(1);
        let request = CreateTaskRequest {
            team_id,
            api_key_id: Uuid::new_v4(),
            task_type: TaskType::Crawl,
            url: "https://example.com".to_string(),
            name: Some("test task".to_string()),
            config: Some(serde_json::json!({"key": "value"})),
            priority: Some(10),
            max_retries: Some(5),
            expires_at: Some(expires),
        };

        let resp = use_case.execute(request).await.expect("execute should succeed");

        assert_eq!(repo.create_count(), 1);
        assert_eq!(resp.task.team_id, team_id);
        assert_eq!(resp.task.task_type, TaskType::Crawl);
        assert_eq!(resp.task.priority, 10, "priority should be set from request");
        assert_eq!(resp.task.max_retries, 5, "max_retries should be set from request");
        assert_eq!(resp.task.expires_at, Some(expires));
        assert_eq!(resp.task.payload, serde_json::json!({"key": "value"}));
    }

    #[tokio::test]
    async fn test_create_task_execute_with_no_optional_fields_uses_defaults() {
        let repo = Arc::new(MockTaskRepository::default());
        let credits = make_credits_service();
        let use_case = CreateTaskUseCase::new(repo.clone(), credits);

        let request = CreateTaskRequest {
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            task_type: TaskType::Scrape,
            url: "https://example.com".to_string(),
            name: None,
            config: None,
            priority: None,
            max_retries: None,
            expires_at: None,
        };

        let resp = use_case.execute(request).await.expect("execute should succeed");

        // Default priority from Task::new is 0, max_retries is 3
        assert_eq!(resp.task.priority, 0);
        assert_eq!(resp.task.max_retries, 3);
        assert!(resp.task.expires_at.is_none());
        // config None -> empty json object
        assert_eq!(resp.task.payload, serde_json::json!({}));
    }

    #[tokio::test]
    async fn test_create_task_execute_propagates_repo_error() {
        let repo = Arc::new(MockTaskRepository::default());
        repo.set_create_fail();
        let credits = make_credits_service();
        let use_case = CreateTaskUseCase::new(repo, credits);

        let request = CreateTaskRequest {
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            task_type: TaskType::Scrape,
            url: "https://example.com".to_string(),
            name: None,
            config: None,
            priority: None,
            max_retries: None,
            expires_at: None,
        };

        let result = use_case.execute(request).await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("should propagate repo error"),
        };
        assert!(
            err.to_string().contains("create failed"),
            "error should mention create failure, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_create_task_execute_with_extract_type() {
        let repo = Arc::new(MockTaskRepository::default());
        let credits = make_credits_service();
        let use_case = CreateTaskUseCase::new(repo.clone(), credits);

        let request = CreateTaskRequest {
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            task_type: TaskType::Extract,
            url: "https://example.com".to_string(),
            name: None,
            config: Some(serde_json::json!({"rules": []})),
            priority: None,
            max_retries: None,
            expires_at: None,
        };

        let resp = use_case.execute(request).await.expect("execute should succeed");
        assert_eq!(resp.task.task_type, TaskType::Extract);
        assert_eq!(resp.task.payload["rules"], serde_json::json!([]));
    }

    // ============ QueryTasksUseCase ============

    #[test]
    fn test_query_tasks_new_does_not_call_repo() {
        let repo = Arc::new(MockTaskRepository::default());
        let _use_case = QueryTasksUseCase::new(repo);
    }

    #[tokio::test]
    async fn test_query_tasks_execute_success_no_more() {
        let repo = Arc::new(MockTaskRepository::default());
        let tasks = vec![
            Task::new(Uuid::new_v4(), TaskType::Scrape, Uuid::new_v4(), Uuid::new_v4(), "u1".into(), serde_json::json!({})),
            Task::new(Uuid::new_v4(), TaskType::Crawl, Uuid::new_v4(), Uuid::new_v4(), "u2".into(), serde_json::json!({})),
        ];
        repo.set_query_result(tasks.clone(), 2);

        let use_case = QueryTasksUseCase::new(repo);
        let resp = use_case
            .execute(QueryTasksRequest {
                team_id: Uuid::new_v4(),
                task_ids: None,
                task_types: None,
                statuses: None,
                created_after: None,
                created_before: None,
                crawl_id: None,
                limit: Some(100),
                offset: Some(0),
            })
            .await
            .expect("execute should succeed");

        assert_eq!(resp.tasks.len(), 2);
        assert_eq!(resp.total, 2);
        assert!(!resp.has_more, "no more when offset+len >= total");
    }

    #[tokio::test]
    async fn test_query_tasks_execute_has_more_true() {
        let repo = Arc::new(MockTaskRepository::default());
        let tasks = vec![
            Task::new(Uuid::new_v4(), TaskType::Scrape, Uuid::new_v4(), Uuid::new_v4(), "u1".into(), serde_json::json!({})),
        ];
        // total=10, offset=0, len=1 -> has_more = (0+1) < 10 = true
        repo.set_query_result(tasks, 10);

        let use_case = QueryTasksUseCase::new(repo);
        let resp = use_case
            .execute(QueryTasksRequest {
                team_id: Uuid::new_v4(),
                task_ids: None,
                task_types: None,
                statuses: None,
                created_after: None,
                created_before: None,
                crawl_id: None,
                limit: Some(1),
                offset: Some(0),
            })
            .await
            .expect("execute should succeed");

        assert_eq!(resp.tasks.len(), 1);
        assert_eq!(resp.total, 10);
        assert!(resp.has_more, "has_more when offset+len < total");
    }

    #[tokio::test]
    async fn test_query_tasks_execute_has_more_boundary_exact_match() {
        let repo = Arc::new(MockTaskRepository::default());
        let tasks = vec![
            Task::new(Uuid::new_v4(), TaskType::Scrape, Uuid::new_v4(), Uuid::new_v4(), "u1".into(), serde_json::json!({})),
        ];
        // total=5, offset=4, len=1 -> has_more = (4+1) < 5 = false
        repo.set_query_result(tasks, 5);

        let use_case = QueryTasksUseCase::new(repo);
        let resp = use_case
            .execute(QueryTasksRequest {
                team_id: Uuid::new_v4(),
                task_ids: None,
                task_types: None,
                statuses: None,
                created_after: None,
                created_before: None,
                crawl_id: None,
                limit: None,
                offset: Some(4),
            })
            .await
            .expect("execute should succeed");

        assert!(!resp.has_more, "has_more false when offset+len == total");
    }

    #[tokio::test]
    async fn test_query_tasks_execute_with_default_limit_and_offset() {
        let repo = Arc::new(MockTaskRepository::default());
        repo.set_query_result(vec![], 0);

        let use_case = QueryTasksUseCase::new(repo.clone());
        let resp = use_case
            .execute(QueryTasksRequest {
                team_id: Uuid::new_v4(),
                task_ids: None,
                task_types: None,
                statuses: None,
                created_after: None,
                created_before: None,
                crawl_id: None,
                limit: None,
                offset: None,
            })
            .await
            .expect("execute should succeed");

        assert_eq!(resp.tasks.len(), 0);
        assert_eq!(resp.total, 0);
        assert!(!resp.has_more);
    }

    #[tokio::test]
    async fn test_query_tasks_execute_with_all_filters() {
        let repo = Arc::new(MockTaskRepository::default());
        let task = Task::new(Uuid::new_v4(), TaskType::Scrape, Uuid::new_v4(), Uuid::new_v4(), "u".into(), serde_json::json!({}));
        repo.set_query_result(vec![task], 1);

        let use_case = QueryTasksUseCase::new(repo);
        let tz = FixedOffset::east_opt(0).unwrap();
        let now = Utc::now();
        let after = now - chrono::Duration::days(1);
        let before = now + chrono::Duration::days(1);

        let resp = use_case
            .execute(QueryTasksRequest {
                team_id: Uuid::new_v4(),
                task_ids: Some(vec![Uuid::new_v4()]),
                task_types: Some(vec![TaskType::Scrape]),
                statuses: Some(vec![TaskStatus::Queued]),
                created_after: Some(after.with_timezone(&tz)),
                created_before: Some(before.with_timezone(&tz)),
                crawl_id: Some(Uuid::new_v4()),
                limit: Some(50),
                offset: Some(10),
            })
            .await
            .expect("execute should succeed");

        assert_eq!(resp.tasks.len(), 1);
        assert_eq!(resp.total, 1);
    }

    #[tokio::test]
    async fn test_query_tasks_execute_propagates_repo_error() {
        let repo = Arc::new(MockTaskRepository::default());
        repo.set_query_fail();
        let use_case = QueryTasksUseCase::new(repo);

        let result = use_case
            .execute(QueryTasksRequest {
                team_id: Uuid::new_v4(),
                task_ids: None,
                task_types: None,
                statuses: None,
                created_after: None,
                created_before: None,
                crawl_id: None,
                limit: None,
                offset: None,
            })
            .await;

        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("should propagate repo error"),
        };
        assert!(
            err.to_string().contains("query failed"),
            "error should mention query failure, got: {}",
            err
        );
    }

    // ============ CancelTasksUseCase ============

    #[test]
    fn test_cancel_tasks_new_does_not_call_repo() {
        let repo = Arc::new(MockTaskRepository::default());
        let credits = make_credits_service();
        let _use_case = CancelTasksUseCase::new(repo, credits);
    }

    #[tokio::test]
    async fn test_cancel_tasks_empty_list_returns_empty_response() {
        let repo = Arc::new(MockTaskRepository::default());
        let credits = make_credits_service();
        let use_case = CancelTasksUseCase::new(repo.clone(), credits);

        let resp = use_case
            .execute(CancelTasksRequest {
                team_id: Uuid::new_v4(),
                task_ids: vec![],
                force: None,
            })
            .await
            .expect("execute should succeed");

        assert_eq!(resp.total_cancelled, 0);
        assert_eq!(resp.total_failed, 0);
        assert!(resp.cancelled.is_empty());
        assert!(resp.failed.is_empty());
        assert_eq!(repo.cancel_count(), 0);
    }

    #[tokio::test]
    async fn test_cancel_tasks_queued_task_succeeds() {
        let team_id = Uuid::new_v4();
        let task = make_task_with_status(TaskStatus::Queued, team_id);
        let task_id = task.id;
        let repo = Arc::new(MockTaskRepository::with_task(task));
        let credits = make_credits_service();
        let use_case = CancelTasksUseCase::new(repo.clone(), credits);

        let resp = use_case
            .execute(CancelTasksRequest {
                team_id,
                task_ids: vec![task_id],
                force: None,
            })
            .await
            .expect("execute should succeed");

        assert_eq!(resp.total_cancelled, 1);
        assert_eq!(resp.total_failed, 0);
        assert_eq!(resp.cancelled, vec![task_id]);
        assert_eq!(repo.cancel_count(), 1);
    }

    #[tokio::test]
    async fn test_cancel_tasks_force_cancels_active_task() {
        let team_id = Uuid::new_v4();
        let task = make_task_with_status(TaskStatus::Active, team_id);
        let task_id = task.id;
        let repo = Arc::new(MockTaskRepository::with_task(task));
        let credits = make_credits_service();
        let use_case = CancelTasksUseCase::new(repo.clone(), credits);

        let resp = use_case
            .execute(CancelTasksRequest {
                team_id,
                task_ids: vec![task_id],
                force: Some(true),
            })
            .await
            .expect("execute should succeed");

        assert_eq!(resp.total_cancelled, 1);
        assert_eq!(resp.total_failed, 0);
        assert_eq!(repo.cancel_count(), 1);
    }

    #[tokio::test]
    async fn test_cancel_tasks_active_without_force_fails() {
        let team_id = Uuid::new_v4();
        let task = make_task_with_status(TaskStatus::Active, team_id);
        let task_id = task.id;
        let repo = Arc::new(MockTaskRepository::with_task(task));
        let credits = make_credits_service();
        let use_case = CancelTasksUseCase::new(repo.clone(), credits);

        let resp = use_case
            .execute(CancelTasksRequest {
                team_id,
                task_ids: vec![task_id],
                force: Some(false),
            })
            .await
            .expect("execute should succeed");

        assert_eq!(resp.total_cancelled, 0);
        assert_eq!(resp.total_failed, 1);
        assert!(resp.failed[0].1.contains("use force=true"));
        assert_eq!(repo.cancel_count(), 0);
    }

    #[tokio::test]
    async fn test_cancel_tasks_active_with_force_none_fails() {
        let team_id = Uuid::new_v4();
        let task = make_task_with_status(TaskStatus::Active, team_id);
        let task_id = task.id;
        let repo = Arc::new(MockTaskRepository::with_task(task));
        let credits = make_credits_service();
        let use_case = CancelTasksUseCase::new(repo, credits);

        let resp = use_case
            .execute(CancelTasksRequest {
                team_id,
                task_ids: vec![task_id],
                force: None,
            })
            .await
            .expect("execute should succeed");

        assert_eq!(resp.total_cancelled, 0);
        assert_eq!(resp.total_failed, 1);
        assert!(resp.failed[0].1.contains("use force=true"));
    }

    #[tokio::test]
    async fn test_cancel_tasks_completed_task_fails() {
        let team_id = Uuid::new_v4();
        let task = make_task_with_status(TaskStatus::Completed, team_id);
        let task_id = task.id;
        let repo = Arc::new(MockTaskRepository::with_task(task));
        let credits = make_credits_service();
        let use_case = CancelTasksUseCase::new(repo.clone(), credits);

        let resp = use_case
            .execute(CancelTasksRequest {
                team_id,
                task_ids: vec![task_id],
                force: Some(true),
            })
            .await
            .expect("execute should succeed");

        assert_eq!(resp.total_cancelled, 0);
        assert_eq!(resp.total_failed, 1);
        assert!(resp.failed[0].1.contains("Cannot cancel task in status"));
        assert_eq!(repo.cancel_count(), 0);
    }

    #[tokio::test]
    async fn test_cancel_tasks_failed_task_fails() {
        let team_id = Uuid::new_v4();
        let task = make_task_with_status(TaskStatus::Failed, team_id);
        let task_id = task.id;
        let repo = Arc::new(MockTaskRepository::with_task(task));
        let credits = make_credits_service();
        let use_case = CancelTasksUseCase::new(repo, credits);

        let resp = use_case
            .execute(CancelTasksRequest {
                team_id,
                task_ids: vec![task_id],
                force: Some(true),
            })
            .await
            .expect("execute should succeed");

        assert_eq!(resp.total_failed, 1);
        assert!(resp.failed[0].1.contains("Cannot cancel task in status"));
    }

    #[tokio::test]
    async fn test_cancel_tasks_wrong_team_fails() {
        let task = make_task_with_status(TaskStatus::Queued, Uuid::new_v4());
        let task_id = task.id;
        let repo = Arc::new(MockTaskRepository::with_task(task));
        let credits = make_credits_service();
        let use_case = CancelTasksUseCase::new(repo.clone(), credits);

        let resp = use_case
            .execute(CancelTasksRequest {
                team_id: Uuid::new_v4(), // different team
                task_ids: vec![task_id],
                force: Some(true),
            })
            .await
            .expect("execute should succeed");

        assert_eq!(resp.total_cancelled, 0);
        assert_eq!(resp.total_failed, 1);
        assert!(resp.failed[0].1.contains("does not belong to team"));
        assert_eq!(repo.cancel_count(), 0);
    }

    #[tokio::test]
    async fn test_cancel_tasks_not_found_fails() {
        let repo = Arc::new(MockTaskRepository::default());
        let credits = make_credits_service();
        let use_case = CancelTasksUseCase::new(repo, credits);

        let missing_id = Uuid::new_v4();
        let resp = use_case
            .execute(CancelTasksRequest {
                team_id: Uuid::new_v4(),
                task_ids: vec![missing_id],
                force: None,
            })
            .await
            .expect("execute should succeed");

        assert_eq!(resp.total_cancelled, 0);
        assert_eq!(resp.total_failed, 1);
        assert_eq!(resp.failed[0].0, missing_id);
        assert!(resp.failed[0].1.contains("Task not found"));
    }

    #[tokio::test]
    async fn test_cancel_tasks_find_by_id_error_fails() {
        let repo = Arc::new(MockTaskRepository::default());
        repo.set_find_error();
        let credits = make_credits_service();
        let use_case = CancelTasksUseCase::new(repo, credits);

        let task_id = Uuid::new_v4();
        let resp = use_case
            .execute(CancelTasksRequest {
                team_id: Uuid::new_v4(),
                task_ids: vec![task_id],
                force: None,
            })
            .await
            .expect("execute should succeed");

        assert_eq!(resp.total_failed, 1);
        assert!(resp.failed[0].1.contains("Repository error"));
    }

    #[tokio::test]
    async fn test_cancel_tasks_mark_cancelled_error_propagates() {
        let team_id = Uuid::new_v4();
        let task = make_task_with_status(TaskStatus::Queued, team_id);
        let task_id = task.id;
        let repo = Arc::new(MockTaskRepository::with_task(task));
        repo.set_cancel_fail();
        let credits = make_credits_service();
        let use_case = CancelTasksUseCase::new(repo, credits);

        let result = use_case
            .execute(CancelTasksRequest {
                team_id,
                task_ids: vec![task_id],
                force: None,
            })
            .await;

        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("should propagate mark_cancelled error"),
        };
        assert!(
            err.to_string().contains("cancel failed"),
            "error should mention cancel failure, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_cancel_tasks_mixed_success_and_failure() {
        let team_id = Uuid::new_v4();
        let ok_task = make_task_with_status(TaskStatus::Queued, team_id);
        let active_task = make_task_with_status(TaskStatus::Active, team_id);
        let completed_task = make_task_with_status(TaskStatus::Completed, team_id);
        let ok_id = ok_task.id;
        let active_id = active_task.id;
        let completed_id = completed_task.id;
        let missing_id = Uuid::new_v4();

        let repo = Arc::new(MockTaskRepository::with_tasks(vec![
            ok_task,
            active_task,
            completed_task,
        ]));
        let credits = make_credits_service();
        let use_case = CancelTasksUseCase::new(repo.clone(), credits);

        let resp = use_case
            .execute(CancelTasksRequest {
                team_id,
                task_ids: vec![ok_id, active_id, completed_id, missing_id],
                force: None,
            })
            .await
            .expect("execute should succeed");

        assert_eq!(resp.total_cancelled, 1, "only Queued task should cancel");
        assert_eq!(resp.total_failed, 3);
        assert_eq!(resp.cancelled, vec![ok_id]);
        assert_eq!(repo.cancel_count(), 1);
        // Verify failure reasons
        let reasons: Vec<String> = resp.failed.iter().map(|(_, r)| r.clone()).collect();
        assert!(reasons.iter().any(|r| r.contains("use force=true")));
        assert!(reasons.iter().any(|r| r.contains("Cannot cancel task in status")));
        assert!(reasons.iter().any(|r| r.contains("Task not found")));
    }
}
