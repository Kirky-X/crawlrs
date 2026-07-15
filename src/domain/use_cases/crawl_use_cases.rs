// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl-related use cases

use crate::application::dto::crawl_request::CrawlRequestDto;
use crate::domain::models::Crawl;
use crate::domain::models::{Task, TaskStatus, TaskType};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::services::credits_service::CreditsService;
use std::sync::Arc;
use uuid::Uuid;

/// 异步爬取请求
pub struct AsyncCrawlRequest {
    pub team_id: Uuid,
    pub api_key_id: Uuid,
    pub request: CrawlRequestDto,
    pub priority: Option<i32>,
    pub max_retries: Option<i32>,
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

/// 异步爬取响应
pub struct AsyncCrawlResponse {
    pub crawl_id: Uuid,
    pub root_task_id: Uuid,
    pub estimated_pages: u32,
}

/// 同步爬取请求
pub struct SyncCrawlRequest {
    pub team_id: Uuid,
    pub api_key_id: Uuid,
    pub request: CrawlRequestDto,
    pub timeout_ms: Option<u32>,
}

/// 同步爬取响应
pub struct SyncCrawlResponse {
    pub crawl_id: Uuid,
    pub tasks: Vec<Task>,
    pub total_pages: u32,
    pub completed_pages: u32,
    pub response_time_ms: u64,
}

/// 爬取状态查询请求
pub struct GetCrawlStatusRequest {
    pub team_id: Uuid,
    pub crawl_id: Uuid,
}

/// 爬取状态响应
pub struct GetCrawlStatusResponse {
    pub crawl: Option<Crawl>,
    pub total_tasks: u64,
    pub completed_tasks: u64,
    pub failed_tasks: u64,
    pub pending_tasks: u64,
    pub progress_percentage: f64,
}

/// 异步爬取用例
#[allow(dead_code)]
pub struct AsyncCrawlUseCase<C: CrawlRepository, T: TaskRepository, R: CreditsRepository> {
    crawl_repo: Arc<C>,
    task_repo: Arc<T>,
    credits_service: Arc<CreditsService<R>>,
}

impl<C: CrawlRepository, T: TaskRepository, R: CreditsRepository> AsyncCrawlUseCase<C, T, R> {
    pub fn new(
        crawl_repo: Arc<C>,
        task_repo: Arc<T>,
        credits_service: Arc<CreditsService<R>>,
    ) -> Self {
        Self {
            crawl_repo,
            task_repo,
            credits_service,
        }
    }

    pub async fn execute(
        &self,
        request: AsyncCrawlRequest,
    ) -> Result<AsyncCrawlResponse, anyhow::Error> {
        // 创建爬取任务
        let _now = chrono::Utc::now();
        let crawl = Crawl::new(
            Uuid::new_v4(),
            request.team_id,
            request.request.name.unwrap_or_default(),
            request.request.url.clone(),
            request.request.url.clone(),
            serde_json::to_value(&request.request.config).unwrap_or_default(),
        );
        self.crawl_repo.create(&crawl).await?;

        // 创建根任务
        let payload = serde_json::json!({
            "crawl_id": crawl.id,
            "config": request.request.config,
        });
        let now = chrono::Utc::now();
        let root_task = Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Crawl,
            status: TaskStatus::Queued,
            priority: 0,
            team_id: request.team_id,
            api_key_id: request.api_key_id,
            url: request.request.url.clone(),
            payload,
            retry_count: 0,
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            expires_at: None,
            created_at: now,
            started_at: None,
            completed_at: None,
            crawl_id: Some(crawl.id),
            updated_at: now,
            lock_token: None,
            lock_expires_at: None,
        };
        self.task_repo.create(&root_task).await?;

        // 计算预估页面数
        let config = &request.request.config;
        let max_depth = config
            .max_depth
            .min(crate::application::dto::crawl_request::MAX_CRAWL_DEPTH);
        let estimated_pages = calculate_estimated_pages(max_depth);

        Ok(AsyncCrawlResponse {
            crawl_id: crawl.id,
            root_task_id: root_task.id,
            estimated_pages,
        })
    }
}

/// 同步爬取用例
#[allow(dead_code)]
pub struct SyncCrawlUseCase<C: CrawlRepository, T: TaskRepository, R: CreditsRepository> {
    crawl_repo: Arc<C>,
    task_repo: Arc<T>,
    credits_service: Arc<CreditsService<R>>,
}

impl<C: CrawlRepository, T: TaskRepository, R: CreditsRepository> SyncCrawlUseCase<C, T, R> {
    pub fn new(
        crawl_repo: Arc<C>,
        task_repo: Arc<T>,
        credits_service: Arc<CreditsService<R>>,
    ) -> Self {
        Self {
            crawl_repo,
            task_repo,
            credits_service,
        }
    }

    pub async fn execute(
        &self,
        request: SyncCrawlRequest,
    ) -> Result<SyncCrawlResponse, anyhow::Error> {
        let start_time = std::time::Instant::now();

        // 创建爬取任务
        let _now = chrono::Utc::now();
        let crawl = Crawl::new(
            Uuid::new_v4(),
            request.team_id,
            request.request.name.unwrap_or_default(),
            request.request.url.clone(),
            request.request.url.clone(),
            serde_json::to_value(&request.request.config).unwrap_or_default(),
        );
        self.crawl_repo.create(&crawl).await?;

        // 创建根任务
        let payload = serde_json::json!({
            "crawl_id": crawl.id,
            "config": request.request.config,
        });
        let now = chrono::Utc::now();
        let root_task = Task {
            id: Uuid::new_v4(),
            task_type: TaskType::Crawl,
            status: TaskStatus::Queued,
            priority: 0,
            team_id: request.team_id,
            api_key_id: request.api_key_id,
            url: request.request.url.clone(),
            payload,
            retry_count: 0,
            attempt_count: 0,
            max_retries: 3,
            scheduled_at: None,
            expires_at: None,
            created_at: now,
            started_at: None,
            completed_at: None,
            crawl_id: Some(crawl.id),
            updated_at: now,
            lock_token: None,
            lock_expires_at: None,
        };
        self.task_repo.create(&root_task).await?;

        let response_time_ms = start_time.elapsed().as_millis() as u64;

        Ok(SyncCrawlResponse {
            crawl_id: crawl.id,
            tasks: vec![root_task],
            total_pages: 1,
            completed_pages: 0,
            response_time_ms,
        })
    }
}

/// 获取爬取状态用例
pub struct GetCrawlStatusUseCase<C: CrawlRepository, T: TaskRepository> {
    crawl_repo: Arc<C>,
    task_repo: Arc<T>,
}

impl<C: CrawlRepository, T: TaskRepository> GetCrawlStatusUseCase<C, T> {
    pub fn new(crawl_repo: Arc<C>, task_repo: Arc<T>) -> Self {
        Self {
            crawl_repo,
            task_repo,
        }
    }

    pub async fn execute(
        &self,
        request: GetCrawlStatusRequest,
    ) -> Result<GetCrawlStatusResponse, anyhow::Error> {
        match self.crawl_repo.find_by_id(request.crawl_id).await? {
            Some(crawl) => {
                if crawl.team_id != request.team_id {
                    return Err(anyhow::anyhow!("Crawl not found"));
                }

                let tasks = self.task_repo.find_by_crawl_id(request.crawl_id).await?;
                let total_tasks = tasks.len() as u64;
                let completed_tasks = tasks
                    .iter()
                    .filter(|t| t.status == TaskStatus::Completed)
                    .count() as u64;
                let failed_tasks = tasks
                    .iter()
                    .filter(|t| t.status == TaskStatus::Failed)
                    .count() as u64;
                let pending_tasks = total_tasks - completed_tasks - failed_tasks;
                let progress_percentage = if total_tasks > 0 {
                    (completed_tasks as f64 / total_tasks as f64) * 100.0
                } else {
                    0.0
                };

                Ok(GetCrawlStatusResponse {
                    crawl: Some(crawl),
                    total_tasks,
                    completed_tasks,
                    failed_tasks,
                    pending_tasks,
                    progress_percentage,
                })
            }
            None => Ok(GetCrawlStatusResponse {
                crawl: None,
                total_tasks: 0,
                completed_tasks: 0,
                failed_tasks: 0,
                pending_tasks: 0,
                progress_percentage: 0.0,
            }),
        }
    }
}

/// 计算预估页面数量
fn calculate_estimated_pages(max_depth: u32) -> u32 {
    // 简单的估算：假设每个页面平均有 10 个链接，第一层有 1 个页面
    if max_depth == 0 {
        return 1;
    }
    let mut pages = 1;
    let mut current_level_pages = 1;
    for _ in 1..=max_depth {
        current_level_pages *= 10;
        pages += current_level_pages;
        if pages > 10000 {
            return 10000; // 限制最大预估页面数
        }
    }
    pages
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::dto::crawl_request::{
        CrawlConfigDto, CrawlRequestDto, MAX_CRAWL_DEPTH,
    };
    use crate::domain::models::CrawlStatus;
    use crate::domain::repositories::task_repository::{RepositoryError, TaskQueryParams};
    use crate::domain::services::test_helpers::MockCreditsRepository;
    use async_trait::async_trait;
    use std::collections::{HashMap, HashSet};
    use std::sync::atomic::{AtomicU32, AtomicU8, Ordering};
    use std::sync::{Arc, Mutex};

    // ============ MockCrawlRepository ============

    /// Mock CrawlRepository with configurable behavior.
    /// Defaults to all-success; stores crawls for find_by_id.
    #[derive(Default)]
    struct MockCrawlRepository {
        crawls: Mutex<HashMap<Uuid, Crawl>>,
        create_count: AtomicU32,
        create_should_fail: AtomicU8,
        find_should_fail: AtomicU8,
    }

    impl MockCrawlRepository {
        fn with_crawl(crawl: Crawl) -> Self {
            let mut crawls = HashMap::new();
            crawls.insert(crawl.id, crawl);
            Self {
                crawls: Mutex::new(crawls),
                ..Default::default()
            }
        }

        fn set_create_fail(&self) {
            self.create_should_fail.store(1, Ordering::SeqCst);
        }

        fn set_find_fail(&self) {
            self.find_should_fail.store(1, Ordering::SeqCst);
        }

        fn create_count(&self) -> u32 {
            self.create_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl CrawlRepository for MockCrawlRepository {
        async fn create(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
            self.create_count.fetch_add(1, Ordering::SeqCst);
            if self.create_should_fail.load(Ordering::SeqCst) == 1 {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "crawl create failed"
                )));
            }
            Ok(crawl.clone())
        }

        async fn find_by_id(&self, id: Uuid) -> Result<Option<Crawl>, RepositoryError> {
            if self.find_should_fail.load(Ordering::SeqCst) == 1 {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "find_by_id error"
                )));
            }
            Ok(self.crawls.lock().unwrap().get(&id).cloned())
        }

        async fn update(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
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
            _status: CrawlStatus,
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

    // ============ MockTaskRepository (minimal for crawl use cases) ============

    /// Minimal MockTaskRepository for crawl use cases.
    /// Only `create` and `find_by_crawl_id` have meaningful behavior.
    #[derive(Default)]
    struct MockTaskRepository {
        crawl_tasks: Mutex<HashMap<Uuid, Vec<Task>>>,
        create_count: AtomicU32,
        create_should_fail: AtomicU8,
        find_by_crawl_id_should_fail: AtomicU8,
    }

    impl MockTaskRepository {
        fn with_crawl_tasks(crawl_id: Uuid, tasks: Vec<Task>) -> Self {
            let mut crawl_tasks = HashMap::new();
            crawl_tasks.insert(crawl_id, tasks);
            Self {
                crawl_tasks: Mutex::new(crawl_tasks),
                ..Default::default()
            }
        }

        fn set_create_fail(&self) {
            self.create_should_fail.store(1, Ordering::SeqCst);
        }

        fn set_find_by_crawl_id_fail(&self) {
            self.find_by_crawl_id_should_fail.store(1, Ordering::SeqCst);
        }

        fn create_count(&self) -> u32 {
            self.create_count.load(Ordering::SeqCst)
        }
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            self.create_count.fetch_add(1, Ordering::SeqCst);
            if self.create_should_fail.load(Ordering::SeqCst) == 1 {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "task create failed"
                )));
            }
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

        async fn find_by_crawl_id(&self, crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
            if self.find_by_crawl_id_should_fail.load(Ordering::SeqCst) == 1 {
                return Err(RepositoryError::Database(anyhow::anyhow!(
                    "find_by_crawl_id error"
                )));
            }
            Ok(self
                .crawl_tasks
                .lock()
                .unwrap()
                .get(&crawl_id)
                .cloned()
                .unwrap_or_default())
        }

        async fn query_tasks(
            &self,
            _params: TaskQueryParams,
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

    // ============ Helpers ============

    fn make_credits_service() -> Arc<CreditsService<MockCreditsRepository>> {
        Arc::new(CreditsService::with_default_config(Arc::new(
            MockCreditsRepository {
                deducted: Arc::new(Mutex::new(vec![])),
            },
        )))
    }

    fn make_crawl_config(max_depth: u32) -> CrawlConfigDto {
        CrawlConfigDto {
            max_depth,
            include_patterns: None,
            exclude_patterns: None,
            strategy: None,
            crawl_delay_ms: None,
            max_concurrency: None,
            proxy: None,
            headers: None,
            extraction_rules: None,
        }
    }

    fn make_crawl_request(max_depth: u32, name: Option<&str>) -> CrawlRequestDto {
        CrawlRequestDto {
            url: "https://example.com".to_string(),
            validated_url: None,
            name: name.map(|s| s.to_string()),
            config: make_crawl_config(max_depth),
            sync_wait_ms: None,
            expires_at: None,
        }
    }

    fn make_task_with_status(status: TaskStatus) -> Task {
        let mut task = Task::new(
            Uuid::new_v4(),
            TaskType::Crawl,
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        );
        task.status = status;
        task
    }

    fn make_async_crawl_request(max_depth: u32, name: Option<&str>) -> AsyncCrawlRequest {
        AsyncCrawlRequest {
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            request: make_crawl_request(max_depth, name),
            priority: None,
            max_retries: None,
            expires_at: None,
        }
    }

    fn make_sync_crawl_request(max_depth: u32, name: Option<&str>) -> SyncCrawlRequest {
        SyncCrawlRequest {
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            request: make_crawl_request(max_depth, name),
            timeout_ms: None,
        }
    }

    // ============ AsyncCrawlUseCase tests ============

    #[test]
    fn test_async_crawl_new_does_not_call_repo() {
        let crawl_repo = Arc::new(MockCrawlRepository::default());
        let task_repo = Arc::new(MockTaskRepository::default());
        let credits = make_credits_service();
        let _use_case = AsyncCrawlUseCase::new(crawl_repo.clone(), task_repo.clone(), credits);
        assert_eq!(crawl_repo.create_count(), 0);
        assert_eq!(task_repo.create_count(), 0);
    }

    #[tokio::test]
    async fn test_async_crawl_execute_success_with_name() {
        let crawl_repo = Arc::new(MockCrawlRepository::default());
        let task_repo = Arc::new(MockTaskRepository::default());
        let credits = make_credits_service();
        let use_case = AsyncCrawlUseCase::new(crawl_repo.clone(), task_repo.clone(), credits);

        let request = make_async_crawl_request(2, Some("my crawl"));

        let resp = use_case
            .execute(request)
            .await
            .expect("execute should succeed");

        assert_eq!(
            crawl_repo.create_count(),
            1,
            "crawl_repo.create should be called once"
        );
        assert_eq!(
            task_repo.create_count(),
            1,
            "task_repo.create should be called once"
        );
        // depth=2 -> 1 + 10 + 100 = 111
        assert_eq!(resp.estimated_pages, 111);
        assert_ne!(resp.crawl_id, Uuid::nil());
        assert_ne!(resp.root_task_id, Uuid::nil());
        assert_ne!(resp.crawl_id, resp.root_task_id);
    }

    #[tokio::test]
    async fn test_async_crawl_execute_success_without_name_uses_default() {
        let crawl_repo = Arc::new(MockCrawlRepository::default());
        let task_repo = Arc::new(MockTaskRepository::default());
        let credits = make_credits_service();
        let use_case = AsyncCrawlUseCase::new(crawl_repo.clone(), task_repo.clone(), credits);

        let request = make_async_crawl_request(1, None);

        let resp = use_case
            .execute(request)
            .await
            .expect("execute should succeed with name=None");

        assert_eq!(crawl_repo.create_count(), 1);
        assert_eq!(task_repo.create_count(), 1);
        // depth=1 -> 1 + 10 = 11
        assert_eq!(resp.estimated_pages, 11);
    }

    #[tokio::test]
    async fn test_async_crawl_execute_propagates_crawl_repo_error() {
        let crawl_repo = Arc::new(MockCrawlRepository::default());
        crawl_repo.set_create_fail();
        let task_repo = Arc::new(MockTaskRepository::default());
        let credits = make_credits_service();
        let use_case = AsyncCrawlUseCase::new(crawl_repo, task_repo.clone(), credits);

        let request = make_async_crawl_request(1, None);

        let result = use_case.execute(request).await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("should propagate crawl_repo error, got Ok"),
        };
        assert!(
            err.to_string().contains("crawl create failed"),
            "error should mention crawl create failure, got: {}",
            err
        );
        assert_eq!(
            task_repo.create_count(),
            0,
            "task_repo.create should not be called when crawl_repo fails"
        );
    }

    #[tokio::test]
    async fn test_async_crawl_execute_propagates_task_repo_error() {
        let crawl_repo = Arc::new(MockCrawlRepository::default());
        let task_repo = Arc::new(MockTaskRepository::default());
        task_repo.set_create_fail();
        let credits = make_credits_service();
        let use_case = AsyncCrawlUseCase::new(crawl_repo.clone(), task_repo, credits);

        let request = make_async_crawl_request(1, None);

        let result = use_case.execute(request).await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("should propagate task_repo error, got Ok"),
        };
        assert!(
            err.to_string().contains("task create failed"),
            "error should mention task create failure, got: {}",
            err
        );
        assert_eq!(
            crawl_repo.create_count(),
            1,
            "crawl_repo.create should have been called before task_repo fails"
        );
    }

    #[tokio::test]
    async fn test_async_crawl_execute_estimated_pages_depth_0() {
        let crawl_repo = Arc::new(MockCrawlRepository::default());
        let task_repo = Arc::new(MockTaskRepository::default());
        let credits = make_credits_service();
        let use_case = AsyncCrawlUseCase::new(crawl_repo, task_repo, credits);

        let request = make_async_crawl_request(0, None);

        let resp = use_case
            .execute(request)
            .await
            .expect("execute should succeed");

        // depth=0 -> 1 page
        assert_eq!(resp.estimated_pages, 1);
    }

    #[tokio::test]
    async fn test_async_crawl_execute_estimated_pages_depth_4_capped_at_10000() {
        let crawl_repo = Arc::new(MockCrawlRepository::default());
        let task_repo = Arc::new(MockTaskRepository::default());
        let credits = make_credits_service();
        let use_case = AsyncCrawlUseCase::new(crawl_repo, task_repo, credits);

        let request = make_async_crawl_request(4, None);

        let resp = use_case
            .execute(request)
            .await
            .expect("execute should succeed");

        // depth=4 -> 1+10+100+1000+10000 = 11111 > 10000 -> capped at 10000
        assert_eq!(resp.estimated_pages, 10000);
    }

    #[tokio::test]
    async fn test_async_crawl_execute_estimated_pages_capped_at_max_depth() {
        let crawl_repo = Arc::new(MockCrawlRepository::default());
        let task_repo = Arc::new(MockTaskRepository::default());
        let credits = make_credits_service();
        let use_case = AsyncCrawlUseCase::new(crawl_repo, task_repo, credits);

        // max_depth=200 exceeds MAX_CRAWL_DEPTH (100), should be clamped to 100, then capped at 10000
        let request = make_async_crawl_request(200, None);

        let resp = use_case
            .execute(request)
            .await
            .expect("execute should succeed");

        assert_eq!(resp.estimated_pages, 10000);
        // Sanity: MAX_CRAWL_DEPTH is 100
        assert_eq!(MAX_CRAWL_DEPTH, 100);
    }

    // ============ SyncCrawlUseCase tests ============

    #[test]
    fn test_sync_crawl_new_does_not_call_repo() {
        let crawl_repo = Arc::new(MockCrawlRepository::default());
        let task_repo = Arc::new(MockTaskRepository::default());
        let credits = make_credits_service();
        let _use_case = SyncCrawlUseCase::new(crawl_repo.clone(), task_repo.clone(), credits);
        assert_eq!(crawl_repo.create_count(), 0);
        assert_eq!(task_repo.create_count(), 0);
    }

    #[tokio::test]
    async fn test_sync_crawl_execute_success_returns_response() {
        let crawl_repo = Arc::new(MockCrawlRepository::default());
        let task_repo = Arc::new(MockTaskRepository::default());
        let credits = make_credits_service();
        let use_case = SyncCrawlUseCase::new(crawl_repo.clone(), task_repo.clone(), credits);

        let request = make_sync_crawl_request(2, Some("sync crawl"));

        let resp = use_case
            .execute(request)
            .await
            .expect("execute should succeed");

        assert_eq!(crawl_repo.create_count(), 1);
        assert_eq!(task_repo.create_count(), 1);
        assert_eq!(
            resp.total_pages, 1,
            "sync crawl returns 1 root task as total_pages"
        );
        assert_eq!(resp.completed_pages, 0);
        assert_eq!(resp.tasks.len(), 1, "response should contain the root task");
        assert_ne!(resp.crawl_id, Uuid::nil());
    }

    #[tokio::test]
    async fn test_sync_crawl_execute_propagates_crawl_repo_error() {
        let crawl_repo = Arc::new(MockCrawlRepository::default());
        crawl_repo.set_create_fail();
        let task_repo = Arc::new(MockTaskRepository::default());
        let credits = make_credits_service();
        let use_case = SyncCrawlUseCase::new(crawl_repo, task_repo.clone(), credits);

        let request = make_sync_crawl_request(1, None);

        let result = use_case.execute(request).await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("should propagate crawl_repo error, got Ok"),
        };
        assert!(
            err.to_string().contains("crawl create failed"),
            "error should mention crawl create failure, got: {}",
            err
        );
        assert_eq!(task_repo.create_count(), 0);
    }

    #[tokio::test]
    async fn test_sync_crawl_execute_propagates_task_repo_error() {
        let crawl_repo = Arc::new(MockCrawlRepository::default());
        let task_repo = Arc::new(MockTaskRepository::default());
        task_repo.set_create_fail();
        let credits = make_credits_service();
        let use_case = SyncCrawlUseCase::new(crawl_repo.clone(), task_repo, credits);

        let request = make_sync_crawl_request(1, None);

        let result = use_case.execute(request).await;
        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("should propagate task_repo error, got Ok"),
        };
        assert!(
            err.to_string().contains("task create failed"),
            "error should mention task create failure, got: {}",
            err
        );
        assert_eq!(crawl_repo.create_count(), 1);
    }

    #[tokio::test]
    async fn test_sync_crawl_execute_returns_root_task_with_correct_fields() {
        let crawl_repo = Arc::new(MockCrawlRepository::default());
        let task_repo = Arc::new(MockTaskRepository::default());
        let credits = make_credits_service();
        let use_case = SyncCrawlUseCase::new(crawl_repo, task_repo, credits);

        let request = make_sync_crawl_request(3, None);

        let resp = use_case
            .execute(request)
            .await
            .expect("execute should succeed");

        let task = &resp.tasks[0];
        assert_eq!(
            task.task_type,
            TaskType::Crawl,
            "root task should be Crawl type"
        );
        assert_eq!(
            task.status,
            TaskStatus::Queued,
            "root task should start as Queued"
        );
        assert_eq!(task.priority, 0, "root task priority should be 0");
        assert_eq!(task.max_retries, 3, "root task max_retries should be 3");
        assert_eq!(task.retry_count, 0);
        assert_eq!(task.attempt_count, 0);
        assert_eq!(
            task.crawl_id,
            Some(resp.crawl_id),
            "root task crawl_id should match response crawl_id"
        );
        assert_eq!(task.url, "https://example.com");
        // payload should contain crawl_id and config
        assert!(
            task.payload.get("crawl_id").is_some(),
            "payload should contain crawl_id"
        );
        assert!(
            task.payload.get("config").is_some(),
            "payload should contain config"
        );
    }

    // ============ GetCrawlStatusUseCase tests ============

    #[test]
    fn test_get_crawl_status_new_does_not_call_repo() {
        let crawl_repo = Arc::new(MockCrawlRepository::default());
        let task_repo = Arc::new(MockTaskRepository::default());
        let _use_case = GetCrawlStatusUseCase::new(crawl_repo, task_repo);
    }

    #[tokio::test]
    async fn test_get_crawl_status_crawl_found_with_tasks() {
        let team_id = Uuid::new_v4();
        let crawl = Crawl::new(
            Uuid::new_v4(),
            team_id,
            "test".to_string(),
            "https://example.com".to_string(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        );
        let crawl_id = crawl.id;
        let crawl_repo = Arc::new(MockCrawlRepository::with_crawl(crawl));

        let tasks = vec![
            make_task_with_status(TaskStatus::Completed),
            make_task_with_status(TaskStatus::Completed),
            make_task_with_status(TaskStatus::Failed),
            make_task_with_status(TaskStatus::Queued),
        ];
        let task_repo = Arc::new(MockTaskRepository::with_crawl_tasks(crawl_id, tasks));

        let use_case = GetCrawlStatusUseCase::new(crawl_repo, task_repo);
        let resp = use_case
            .execute(GetCrawlStatusRequest { team_id, crawl_id })
            .await
            .expect("execute should succeed");

        assert!(resp.crawl.is_some(), "crawl should be found");
        assert_eq!(resp.crawl.unwrap().id, crawl_id);
        assert_eq!(resp.total_tasks, 4);
        assert_eq!(resp.completed_tasks, 2);
        assert_eq!(resp.failed_tasks, 1);
        assert_eq!(
            resp.pending_tasks, 1,
            "pending = total - completed - failed = 4-2-1 = 1"
        );
        // progress = 2/4 * 100 = 50.0
        assert!(
            (resp.progress_percentage - 50.0).abs() < f64::EPSILON,
            "progress should be 50%, got {}",
            resp.progress_percentage
        );
    }

    #[tokio::test]
    async fn test_get_crawl_status_crawl_not_found_returns_empty() {
        let crawl_repo = Arc::new(MockCrawlRepository::default());
        let task_repo = Arc::new(MockTaskRepository::default());
        let use_case = GetCrawlStatusUseCase::new(crawl_repo, task_repo);

        let resp = use_case
            .execute(GetCrawlStatusRequest {
                team_id: Uuid::new_v4(),
                crawl_id: Uuid::new_v4(),
            })
            .await
            .expect("execute should succeed");

        assert!(resp.crawl.is_none(), "crawl should be None when not found");
        assert_eq!(resp.total_tasks, 0);
        assert_eq!(resp.completed_tasks, 0);
        assert_eq!(resp.failed_tasks, 0);
        assert_eq!(resp.pending_tasks, 0);
        assert_eq!(resp.progress_percentage, 0.0);
    }

    #[tokio::test]
    async fn test_get_crawl_status_wrong_team_returns_error() {
        let crawl = Crawl::new(
            Uuid::new_v4(),
            Uuid::new_v4(), // different team
            "test".to_string(),
            "https://example.com".to_string(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        );
        let crawl_id = crawl.id;
        let crawl_repo = Arc::new(MockCrawlRepository::with_crawl(crawl));
        let task_repo = Arc::new(MockTaskRepository::default());
        let use_case = GetCrawlStatusUseCase::new(crawl_repo, task_repo);

        let result = use_case
            .execute(GetCrawlStatusRequest {
                team_id: Uuid::new_v4(), // different team
                crawl_id,
            })
            .await;

        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("should error on wrong team, got Ok"),
        };
        assert!(
            err.to_string().contains("Crawl not found"),
            "error should say Crawl not found for wrong team, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_get_crawl_status_no_tasks_returns_zero_progress() {
        let team_id = Uuid::new_v4();
        let crawl = Crawl::new(
            Uuid::new_v4(),
            team_id,
            "test".to_string(),
            "https://example.com".to_string(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        );
        let crawl_id = crawl.id;
        let crawl_repo = Arc::new(MockCrawlRepository::with_crawl(crawl));
        let task_repo = Arc::new(MockTaskRepository::default()); // no tasks

        let use_case = GetCrawlStatusUseCase::new(crawl_repo, task_repo);
        let resp = use_case
            .execute(GetCrawlStatusRequest { team_id, crawl_id })
            .await
            .expect("execute should succeed");

        assert!(resp.crawl.is_some());
        assert_eq!(resp.total_tasks, 0);
        assert_eq!(resp.completed_tasks, 0);
        assert_eq!(resp.failed_tasks, 0);
        assert_eq!(resp.pending_tasks, 0);
        // total_tasks=0 -> progress = 0.0 (per source: if total_tasks > 0 ... else 0.0)
        assert_eq!(resp.progress_percentage, 0.0);
    }

    #[tokio::test]
    async fn test_get_crawl_status_all_completed_100_percent() {
        let team_id = Uuid::new_v4();
        let crawl = Crawl::new(
            Uuid::new_v4(),
            team_id,
            "test".to_string(),
            "https://example.com".to_string(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        );
        let crawl_id = crawl.id;
        let crawl_repo = Arc::new(MockCrawlRepository::with_crawl(crawl));

        let tasks = vec![
            make_task_with_status(TaskStatus::Completed),
            make_task_with_status(TaskStatus::Completed),
            make_task_with_status(TaskStatus::Completed),
        ];
        let task_repo = Arc::new(MockTaskRepository::with_crawl_tasks(crawl_id, tasks));

        let use_case = GetCrawlStatusUseCase::new(crawl_repo, task_repo);
        let resp = use_case
            .execute(GetCrawlStatusRequest { team_id, crawl_id })
            .await
            .expect("execute should succeed");

        assert_eq!(resp.total_tasks, 3);
        assert_eq!(resp.completed_tasks, 3);
        assert_eq!(resp.failed_tasks, 0);
        assert_eq!(resp.pending_tasks, 0);
        assert!(
            (resp.progress_percentage - 100.0).abs() < f64::EPSILON,
            "progress should be 100%, got {}",
            resp.progress_percentage
        );
    }

    #[tokio::test]
    async fn test_get_crawl_status_propagates_find_by_id_error() {
        let crawl_repo = Arc::new(MockCrawlRepository::default());
        crawl_repo.set_find_fail();
        let task_repo = Arc::new(MockTaskRepository::default());
        let use_case = GetCrawlStatusUseCase::new(crawl_repo, task_repo);

        let result = use_case
            .execute(GetCrawlStatusRequest {
                team_id: Uuid::new_v4(),
                crawl_id: Uuid::new_v4(),
            })
            .await;

        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("should propagate find_by_id error, got Ok"),
        };
        assert!(
            err.to_string().contains("find_by_id error"),
            "error should mention find_by_id, got: {}",
            err
        );
    }

    #[tokio::test]
    async fn test_get_crawl_status_propagates_find_by_crawl_id_error() {
        let team_id = Uuid::new_v4();
        let crawl = Crawl::new(
            Uuid::new_v4(),
            team_id,
            "test".to_string(),
            "https://example.com".to_string(),
            "https://example.com".to_string(),
            serde_json::json!({}),
        );
        let crawl_id = crawl.id;
        let crawl_repo = Arc::new(MockCrawlRepository::with_crawl(crawl));
        let task_repo = Arc::new(MockTaskRepository::default());
        task_repo.set_find_by_crawl_id_fail();

        let use_case = GetCrawlStatusUseCase::new(crawl_repo, task_repo);

        let result = use_case
            .execute(GetCrawlStatusRequest { team_id, crawl_id })
            .await;

        let err = match result {
            Err(e) => e,
            Ok(_) => panic!("should propagate find_by_crawl_id error, got Ok"),
        };
        assert!(
            err.to_string().contains("find_by_crawl_id error"),
            "error should mention find_by_crawl_id, got: {}",
            err
        );
    }

    // ============ calculate_estimated_pages tests ============

    #[test]
    fn test_calculate_estimated_pages_depth_0_returns_1() {
        assert_eq!(
            calculate_estimated_pages(0),
            1,
            "depth=0 should return 1 page"
        );
    }

    #[test]
    fn test_calculate_estimated_pages_depth_1_returns_11() {
        // 1 (root) + 10 (level 1) = 11
        assert_eq!(calculate_estimated_pages(1), 11);
    }

    #[test]
    fn test_calculate_estimated_pages_depth_2_returns_111() {
        // 1 + 10 + 100 = 111
        assert_eq!(calculate_estimated_pages(2), 111);
    }

    #[test]
    fn test_calculate_estimated_pages_depth_3_returns_1111() {
        // 1 + 10 + 100 + 1000 = 1111
        assert_eq!(calculate_estimated_pages(3), 1111);
    }

    #[test]
    fn test_calculate_estimated_pages_depth_4_capped_at_10000() {
        // 1 + 10 + 100 + 1000 + 10000 = 11111 > 10000 -> capped
        assert_eq!(calculate_estimated_pages(4), 10000);
    }

    #[test]
    fn test_calculate_estimated_pages_large_depth_capped_at_10000() {
        // Very large depth should still cap at 10000
        assert_eq!(calculate_estimated_pages(100), 10000);
        assert_eq!(calculate_estimated_pages(1000), 10000);
        assert_eq!(calculate_estimated_pages(u32::MAX), 10000);
    }

    #[tokio::test]
    async fn test_sync_crawl_execute_validates_root_task_timestamps_and_lock_fields() {
        let crawl_repo = Arc::new(MockCrawlRepository::default());
        let task_repo = Arc::new(MockTaskRepository::default());
        let credits = make_credits_service();
        let use_case = SyncCrawlUseCase::new(crawl_repo, task_repo, credits);

        let resp = use_case
            .execute(make_sync_crawl_request(1, Some("ts test")))
            .await
            .expect("execute should succeed");

        assert_eq!(resp.total_pages, 1);
        assert_eq!(resp.completed_pages, 0);
        assert!(
            resp.response_time_ms < 5000,
            "response_time_ms should be small, got {}",
            resp.response_time_ms
        );
        let task = &resp.tasks[0];
        assert_eq!(task.created_at, task.updated_at);
        assert!(task.scheduled_at.is_none());
        assert!(task.started_at.is_none());
        assert!(task.completed_at.is_none());
        assert!(task.lock_token.is_none());
        assert!(task.lock_expires_at.is_none());
        assert!(task.expires_at.is_none());
    }

    #[tokio::test]
    async fn test_get_crawl_status_returns_crawl_with_correct_fields() {
        let team_id = Uuid::new_v4();
        let crawl = Crawl::new(
            Uuid::new_v4(),
            team_id,
            "test crawl".to_string(),
            "https://example.com".to_string(),
            "https://example.com".to_string(),
            serde_json::json!({"key": "value"}),
        );
        let crawl_id = crawl.id;
        let crawl_repo = Arc::new(MockCrawlRepository::with_crawl(crawl));
        let task_repo = Arc::new(MockTaskRepository::default());

        let resp = GetCrawlStatusUseCase::new(crawl_repo, task_repo)
            .execute(GetCrawlStatusRequest { team_id, crawl_id })
            .await
            .expect("execute should succeed");

        let returned = resp.crawl.expect("crawl should be Some");
        assert_eq!(returned.id, crawl_id);
        assert_eq!(returned.team_id, team_id);
        assert_eq!(returned.name, "test crawl");
    }
}
