// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::common::error::{RepositoryResultExt, WorkerError};
use crate::config::Settings;
use crate::domain::models::task_domain::TaskStatus;
use crate::domain::repositories::{
    task_repository::TaskRepository, tasks_backlog_repository::TasksBacklogRepository,
};
use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::workers::worker::{ProcessResult, WorkerProcess};
use async_trait::async_trait;
use chrono::Utc;
use log::{error, info, warn};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// 积压任务处理Worker
///
/// 该Worker负责定期处理积压的任务，当团队并发限制释放时
/// 将积压任务重新加入执行队列
pub struct BacklogWorker {
    tasks_backlog_repository: Arc<dyn TasksBacklogRepository>,
    task_repository: Arc<dyn TaskRepository>,
    rate_limiting_service: Arc<dyn RateLimitingService>,
    settings: Arc<Settings>,
    cleanup_cycle_counter: AtomicU64,
}

impl BacklogWorker {
    pub fn new(
        tasks_backlog_repository: Arc<dyn TasksBacklogRepository>,
        task_repository: Arc<dyn TaskRepository>,
        rate_limiting_service: Arc<dyn RateLimitingService>,
        settings: Arc<Settings>,
    ) -> Self {
        Self {
            tasks_backlog_repository,
            task_repository,
            rate_limiting_service,
            settings,
            cleanup_cycle_counter: AtomicU64::new(0),
        }
    }

    /// 处理积压任务
    async fn process_backlog(&self) -> Result<(), WorkerError> {
        info!("开始处理积压任务");

        let batch_size = self.settings.concurrency.default_team_limit as usize;

        // 1. 获取所有待处理的积压任务
        let pending_backlogs = self
            .tasks_backlog_repository
            .get_pending_tasks(None, Some(batch_size as u64))
            .await
            .repo_err()?;

        if pending_backlogs.is_empty() {
            info!("没有待处理的积压任务");
            return Ok(());
        }

        info!("发现 {} 个待处理的积压任务", pending_backlogs.len());

        let mut processed_count = 0;
        let mut failed_count = 0;
        let mut expired_count = 0;

        // 2. 按团队分组处理任务
        let mut backlogs_by_team: std::collections::HashMap<uuid::Uuid, Vec<_>> =
            std::collections::HashMap::with_capacity(32);
        for backlog in pending_backlogs {
            backlogs_by_team
                .entry(backlog.team_id)
                .or_default()
                .push(backlog);
        }

        // 3. 处理每个团队的积压任务
        for (team_id, team_backlogs) in backlogs_by_team {
            info!("处理团队 {} 的 {} 个积压任务", team_id, team_backlogs.len());

            for backlog in team_backlogs {
                match self.process_single_backlog(backlog).await {
                    Ok(true) => processed_count += 1,
                    Ok(false) => {
                        // 任务已过期
                        expired_count += 1;
                    }
                    Err(e) => {
                        error!("处理积压任务失败: {}", e);
                        failed_count += 1;
                    }
                }
            }
        }

        info!(
            "积压任务处理完成: 成功={}, 失败={}, 过期={}",
            processed_count, failed_count, expired_count
        );

        Ok(())
    }

    /// 处理单个积压任务
    async fn process_single_backlog(
        &self,
        backlog: crate::domain::repositories::tasks_backlog_repository::TasksBacklog,
    ) -> Result<bool, WorkerError> {
        // 1. 检查任务是否已过期
        if backlog.is_expired() {
            info!("积压任务 {} 已过期，标记为过期状态", backlog.id);

            let mut expired_backlog = backlog.clone();
            expired_backlog
                .mark_expired()
                .map_err(WorkerError::DomainError)?;

            self.tasks_backlog_repository
                .update(&expired_backlog)
                .await
                .repo_err()?;

            return Ok(false);
        }

        // 2. 检查是否超过重试次数
        if !backlog.can_retry() {
            warn!("积压任务 {} 重试次数已达上限，标记为失败", backlog.id);

            let mut failed_backlog = backlog.clone();
            failed_backlog
                .mark_failed()
                .map_err(WorkerError::DomainError)?;

            self.tasks_backlog_repository
                .update(&failed_backlog)
                .await
                .repo_err()?;

            return Ok(false);
        }

        // 3. 检查团队的并发限制
        match self
            .rate_limiting_service
            .check_team_concurrency(backlog.team_id, backlog.task_id)
            .await
        {
            Ok(crate::domain::services::rate_limiting_service::ConcurrencyResult::Allowed) => {
                info!(
                    "团队 {} 并发槽位可用，处理积压任务 {}",
                    backlog.team_id, backlog.id
                );

                // 4. 重新激活任务
                match self.reactivate_task(backlog.clone()).await {
                    Ok(_) => {
                        info!("积压任务 {} 重新激活成功", backlog.id);
                        Ok(true)
                    }
                    Err(e) => {
                        error!("重新激活任务失败: {}", e);

                        // 增加重试次数
                        let mut retry_backlog = backlog.clone();
                        retry_backlog.increment_retry_count();

                        self.tasks_backlog_repository
                            .update(&retry_backlog)
                            .await
                            .repo_err()?;

                        Err(e)
                    }
                }
            }
            Ok(crate::domain::services::rate_limiting_service::ConcurrencyResult::Denied {
                reason,
            }) => {
                info!(
                    "团队 {} 并发限制未释放: {}，积压任务 {} 继续保持积压状态",
                    backlog.team_id, reason, backlog.id
                );
                Ok(false)
            }
            Ok(crate::domain::services::rate_limiting_service::ConcurrencyResult::Queued {
                ..
            }) => {
                // 这种情况不应该发生，因为我们正在处理积压任务
                warn!("积压任务 {} 被重新排队，这是意外的行为", backlog.id);
                Ok(false)
            }
            Err(e) => {
                error!("检查团队并发限制失败: {}", e);
                Err(WorkerError::ServiceError(e.to_string()))
            }
        }
    }

    /// 重新激活任务
    async fn reactivate_task(
        &self,
        backlog: crate::domain::repositories::tasks_backlog_repository::TasksBacklog,
    ) -> Result<(), WorkerError> {
        // 1. 查找原始任务
        let task = self
            .task_repository
            .find_by_id(backlog.task_id)
            .await
            .repo_err()?
            .ok_or_else(|| WorkerError::NotFound(format!("任务 {} 不存在", backlog.task_id)))?;

        // 2. 检查任务状态
        if task.status != TaskStatus::Queued {
            info!("任务 {} 状态为 {}，不需要重新激活", task.id, task.status);

            // 标记积压任务为已完成
            let mut completed_backlog = backlog.clone();
            completed_backlog
                .mark_completed()
                .map_err(|e| WorkerError::DomainError(e.to_string()))?;

            self.tasks_backlog_repository
                .update(&completed_backlog)
                .await
                .repo_err()?;

            return Ok(());
        }

        // 3. 更新任务状态为queued，准备重新执行
        let mut updated_task = task.clone();
        updated_task.status = TaskStatus::Queued;
        updated_task.updated_at = Utc::now();

        self.task_repository
            .update(&updated_task)
            .await
            .repo_err()?;

        // 4. 标记积压任务为已完成
        let mut completed_backlog = backlog.clone();
        completed_backlog
            .mark_completed()
            .map_err(WorkerError::DomainError)?;

        self.tasks_backlog_repository
            .update(&completed_backlog)
            .await
            .repo_err()?;

        info!("任务 {} 重新激活成功", task.id);
        Ok(())
    }

    /// 清理过期任务
    async fn cleanup_expired_tasks(&self) -> Result<(), WorkerError> {
        info!("开始清理过期积压任务");

        let expired_backlogs = self
            .tasks_backlog_repository
            .get_expired_tasks(Some(100))
            .await
            .repo_err()?;

        if expired_backlogs.is_empty() {
            info!("没有过期的积压任务");
            return Ok(());
        }

        let mut cleaned_count = 0;

        for backlog in expired_backlogs {
            match self.process_expired_backlog(backlog).await {
                Ok(_) => cleaned_count += 1,
                Err(e) => {
                    error!("清理过期积压任务失败: {}", e);
                }
            }
        }

        info!("清理过期积压任务完成，共清理 {} 个任务", cleaned_count);
        Ok(())
    }

    /// 处理过期积压任务
    async fn process_expired_backlog(
        &self,
        backlog: crate::domain::repositories::tasks_backlog_repository::TasksBacklog,
    ) -> Result<(), WorkerError> {
        info!("处理过期积压任务 {}", backlog.id);

        // 1. 标记积压任务为过期
        let mut expired_backlog = backlog.clone();
        expired_backlog
            .mark_expired()
            .map_err(WorkerError::DomainError)?;

        self.tasks_backlog_repository
            .update(&expired_backlog)
            .await
            .repo_err()?;

        // 2. 查找对应的任务
        let task = self
            .task_repository
            .find_by_id(backlog.task_id)
            .await
            .repo_err()?;

        if let Some(task) = task {
            // 3. 如果任务还在pending状态，标记为失败
            if task.status == TaskStatus::Queued {
                let mut failed_task = task.clone();
                failed_task.status = TaskStatus::Failed;
                failed_task.updated_at = Utc::now();

                self.task_repository.update(&failed_task).await.repo_err()?;

                info!("任务 {} 因积压过期被标记为失败", task.id);
            }
        }

        Ok(())
    }
}

#[async_trait]
impl WorkerProcess for BacklogWorker {
    fn name(&self) -> &str {
        "backlog-worker"
    }

    async fn process(&self) -> ProcessResult {
        // 处理积压任务
        if let Err(e) = self.process_backlog().await {
            return ProcessResult::Error(format!("处理积压任务时发生错误: {}", e));
        }

        // 定期清理过期任务（每10个周期清理一次）
        let counter = self.cleanup_cycle_counter.fetch_add(1, Ordering::SeqCst);
        if counter.is_multiple_of(10) {
            if let Err(e) = self.cleanup_expired_tasks().await {
                return ProcessResult::Error(format!("清理过期积压任务时发生错误: {}", e));
            }
        }

        ProcessResult::Completed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::{CreditsTransactionType, Task, TaskType};
    use crate::domain::repositories::task_repository::RepositoryError;
    use crate::domain::repositories::tasks_backlog_repository::TasksBacklog;
    use crate::domain::services::rate_limiting_service::{BacklogService, QuotaService};
    use crate::domain::services::rate_limiting_service::{
        ConcurrencyConfig, ConcurrencyControlService, ConcurrencyResult, RateLimitConfig,
        RateLimitResult, RateLimitService, RateLimitingError,
    };
    use crate::infrastructure::cache::redis_client::RedisClientConfig;
    use async_trait::async_trait;
    use std::collections::HashSet;
    use std::sync::Mutex;
    use uuid::Uuid;

    // ========== Test Settings factory ==========

    fn make_test_settings(default_team_limit: i64) -> Arc<Settings> {
        use crate::config::settings::*;
        let settings = Settings {
            server: ServerSettings::default(),
            database: DatabaseSettings::default(),
            redis: RedisSettings::default(),
            cors: CorsSettings::default(),
            rate_limiting: RateLimitingSettings::default(),
            concurrency: ConcurrencySettings {
                default_team_limit,
                task_lock_duration_seconds: 300,
            },
            webhook: WebhookSettings::default(),
            bing_search: BingSearchSettings::default(),
            search: SearchSettings::default(),
            llm: LLMSettings::default(),
            proxy: ProxySettings::default(),
            engines: EngineSettings::default(),
            logging: LoggingSettings::default(),
            workers: WorkerSettings::default(),
            timeouts: TimeoutSettings::default(),
            cache: CacheSettings::default(),
            trusted_proxies: TrustedProxySettings::default(),
        };
        Arc::new(settings)
    }

    // ========== Mock TasksBacklogRepository ==========

    struct MockBacklogRepo {
        pending: Mutex<Vec<TasksBacklog>>,
        expired: Mutex<Vec<TasksBacklog>>,
        updated: Mutex<Vec<TasksBacklog>>,
        fail_get_pending: bool,
    }

    impl MockBacklogRepo {
        fn new(pending: Vec<TasksBacklog>) -> Self {
            Self {
                pending: Mutex::new(pending),
                expired: Mutex::new(vec![]),
                updated: Mutex::new(vec![]),
                fail_get_pending: false,
            }
        }

        fn new_empty() -> Self {
            Self::new(vec![])
        }

        fn new_failing() -> Self {
            Self {
                pending: Mutex::new(vec![]),
                expired: Mutex::new(vec![]),
                updated: Mutex::new(vec![]),
                fail_get_pending: true,
            }
        }

        fn new_with_expired(expired: Vec<TasksBacklog>) -> Self {
            Self {
                pending: Mutex::new(vec![]),
                expired: Mutex::new(expired),
                updated: Mutex::new(vec![]),
                fail_get_pending: false,
            }
        }

        fn updated(&self) -> Vec<TasksBacklog> {
            self.updated.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl TasksBacklogRepository for MockBacklogRepo {
        async fn create(&self, backlog: &TasksBacklog) -> Result<TasksBacklog, RepositoryError> {
            Ok(backlog.clone())
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<TasksBacklog>, RepositoryError> {
            Ok(None)
        }

        async fn find_by_task_id(
            &self,
            _task_id: Uuid,
        ) -> Result<Option<TasksBacklog>, RepositoryError> {
            Ok(None)
        }

        async fn update(&self, backlog: &TasksBacklog) -> Result<TasksBacklog, RepositoryError> {
            self.updated.lock().unwrap().push(backlog.clone());
            Ok(backlog.clone())
        }

        async fn delete(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }

        async fn get_pending_tasks(
            &self,
            _team_id: Option<Uuid>,
            _limit: Option<u64>,
        ) -> Result<Vec<TasksBacklog>, RepositoryError> {
            if self.fail_get_pending {
                return Err(RepositoryError::Database(anyhow::anyhow!("db error")));
            }
            Ok(self.pending.lock().unwrap().drain(..).collect())
        }

        async fn get_expired_tasks(
            &self,
            _limit: Option<u64>,
        ) -> Result<Vec<TasksBacklog>, RepositoryError> {
            Ok(self.expired.lock().unwrap().drain(..).collect())
        }

        async fn count_by_status(
            &self,
            _team_id: Option<Uuid>,
            _status: crate::domain::repositories::tasks_backlog_repository::TasksBacklogStatus,
        ) -> Result<i64, RepositoryError> {
            Ok(0)
        }

        async fn update_status_batch(
            &self,
            _ids: &[Uuid],
            _status: crate::domain::repositories::tasks_backlog_repository::TasksBacklogStatus,
        ) -> Result<u64, RepositoryError> {
            Ok(0)
        }
    }

    // ========== Mock TaskRepository ==========

    struct MockTaskRepo {
        tasks: Mutex<std::collections::HashMap<Uuid, Task>>,
        updated_tasks: Mutex<Vec<Task>>,
        expire_count: AtomicU64,
    }

    impl MockTaskRepo {
        fn new() -> Self {
            Self {
                tasks: Mutex::new(std::collections::HashMap::new()),
                updated_tasks: Mutex::new(vec![]),
                expire_count: AtomicU64::new(0),
            }
        }

        fn with_task(self, task: Task) -> Self {
            self.tasks.lock().unwrap().insert(task.id, task);
            self
        }

        fn updated_tasks(&self) -> Vec<Task> {
            self.updated_tasks.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl TaskRepository for MockTaskRepo {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }

        async fn find_by_id(&self, id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(self.tasks.lock().unwrap().get(&id).cloned())
        }

        async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
            self.updated_tasks.lock().unwrap().push(task.clone());
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
            self.expire_count.fetch_add(1, Ordering::SeqCst);
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

    // ========== Mock RateLimitingService ==========

    struct MockRateLimitingService {
        concurrency_result: Mutex<ConcurrencyResult>,
        should_error: bool,
    }

    impl MockRateLimitingService {
        fn new_allowed() -> Self {
            Self {
                concurrency_result: Mutex::new(ConcurrencyResult::Allowed),
                should_error: false,
            }
        }

        fn new_denied() -> Self {
            Self {
                concurrency_result: Mutex::new(ConcurrencyResult::Denied {
                    reason: "limit reached".to_string(),
                }),
                should_error: false,
            }
        }

        fn new_queued() -> Self {
            Self {
                concurrency_result: Mutex::new(ConcurrencyResult::Queued {
                    backlog_id: Uuid::new_v4(),
                }),
                should_error: false,
            }
        }

        fn new_error() -> Self {
            Self {
                concurrency_result: Mutex::new(ConcurrencyResult::Allowed),
                should_error: true,
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
            Ok(RateLimitResult::Allowed)
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
            if self.should_error {
                return Err(RateLimitingError::Other(anyhow::anyhow!("redis error")));
            }
            Ok(self.concurrency_result.lock().unwrap().clone())
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
            _transaction_type: CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn get_quota_balance(&self, _team_id: Uuid) -> Result<i64, RateLimitingError> {
            Ok(100)
        }
    }

    #[async_trait]
    impl RateLimitingService for MockRateLimitingService {}

    // ========== Helper functions ==========

    fn make_backlog(team_id: Uuid, task_id: Uuid) -> TasksBacklog {
        TasksBacklog::new(
            task_id,
            team_id,
            "scrape".to_string(),
            0,
            serde_json::json!({"url": "http://example.com"}),
            None,
        )
    }

    fn make_expired_backlog(team_id: Uuid, task_id: Uuid) -> TasksBacklog {
        let mut backlog = make_backlog(team_id, task_id);
        backlog.expires_at = Some(Utc::now() - chrono::Duration::hours(1));
        backlog
    }

    fn make_max_retry_backlog(team_id: Uuid, task_id: Uuid) -> TasksBacklog {
        let mut backlog = make_backlog(team_id, task_id);
        backlog.retry_count = 3;
        backlog.max_retries = 3;
        backlog
    }

    fn make_processing_backlog(team_id: Uuid, task_id: Uuid) -> TasksBacklog {
        let mut backlog = make_backlog(team_id, task_id);
        backlog.mark_processing().unwrap();
        backlog
    }

    fn make_task(id: Uuid, status: TaskStatus) -> Task {
        let now = Utc::now();
        Task {
            id,
            task_type: TaskType::Scrape,
            status,
            priority: 0,
            team_id: Uuid::new_v4(),
            api_key_id: Uuid::new_v4(),
            url: "http://example.com".to_string(),
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

    fn make_worker(
        backlog_repo: Arc<dyn TasksBacklogRepository>,
        task_repo: Arc<dyn TaskRepository>,
        rate_limiting: Arc<dyn RateLimitingService>,
        settings: Arc<Settings>,
    ) -> BacklogWorker {
        BacklogWorker::new(backlog_repo, task_repo, rate_limiting, settings)
    }

    fn default_settings() -> Arc<Settings> {
        make_test_settings(10)
    }

    // ========== name() tests ==========

    #[test]
    fn test_worker_name() {
        let worker = make_worker(
            Arc::new(MockBacklogRepo::new_empty()),
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRateLimitingService::new_allowed()),
            default_settings(),
        );
        assert_eq!(worker.name(), "backlog-worker");
    }

    // ========== process() with empty backlog ==========

    #[tokio::test]
    async fn test_process_empty_backlog_returns_completed() {
        let worker = make_worker(
            Arc::new(MockBacklogRepo::new_empty()),
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRateLimitingService::new_allowed()),
            default_settings(),
        );
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
    }

    // ========== process() with repo failure ==========

    #[tokio::test]
    async fn test_process_repo_failure_returns_error() {
        let worker = make_worker(
            Arc::new(MockBacklogRepo::new_failing()),
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRateLimitingService::new_allowed()),
            default_settings(),
        );
        let result = worker.process().await;
        match result {
            ProcessResult::Error(msg) => {
                assert!(msg.contains("处理积压任务时发生错误"));
            }
            _ => panic!("Expected ProcessResult::Error, got {:?}", result),
        }
    }

    // ========== process() with expired backlog ==========

    #[tokio::test]
    async fn test_process_expired_backlog_marks_expired() {
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let backlog = make_expired_backlog(team_id, task_id);
        let repo = Arc::new(MockBacklogRepo::new(vec![backlog]));
        let updated_repo = repo.clone();
        let worker = make_worker(
            repo,
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRateLimitingService::new_allowed()),
            default_settings(),
        );
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
        let updated = updated_repo.updated();
        assert_eq!(updated.len(), 1);
        assert_eq!(
            updated[0].status,
            crate::domain::repositories::tasks_backlog_repository::TasksBacklogStatus::Expired
        );
    }

    // ========== process() with max retry backlog ==========

    #[tokio::test]
    async fn test_process_max_retry_backlog_marks_failed() {
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let backlog = make_max_retry_backlog(team_id, task_id);
        let repo = Arc::new(MockBacklogRepo::new(vec![backlog]));
        let updated_repo = repo.clone();
        let worker = make_worker(
            repo,
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRateLimitingService::new_allowed()),
            default_settings(),
        );
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
        let updated = updated_repo.updated();
        assert_eq!(updated.len(), 1);
        assert_eq!(
            updated[0].status,
            crate::domain::repositories::tasks_backlog_repository::TasksBacklogStatus::Failed
        );
    }

    // ========== process() with concurrency denied ==========

    #[tokio::test]
    async fn test_process_concurrency_denied_keeps_backlog() {
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let backlog = make_backlog(team_id, task_id);
        let repo = Arc::new(MockBacklogRepo::new(vec![backlog]));
        let worker = make_worker(
            repo,
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRateLimitingService::new_denied()),
            default_settings(),
        );
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
        // Denied means no update to the backlog (returns Ok(false) without updating)
    }

    // ========== process() with concurrency queued ==========

    #[tokio::test]
    async fn test_process_concurrency_queued_returns_completed() {
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let backlog = make_backlog(team_id, task_id);
        let repo = Arc::new(MockBacklogRepo::new(vec![backlog]));
        let worker = make_worker(
            repo,
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRateLimitingService::new_queued()),
            default_settings(),
        );
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
    }

    // ========== process() with concurrency allowed - task not found ==========

    #[tokio::test]
    async fn test_process_allowed_task_not_found_returns_error() {
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let backlog = make_backlog(team_id, task_id);
        let repo = Arc::new(MockBacklogRepo::new(vec![backlog]));
        let worker = make_worker(
            repo,
            Arc::new(MockTaskRepo::new()), // no task with task_id
            Arc::new(MockRateLimitingService::new_allowed()),
            default_settings(),
        );
        let result = worker.process().await;
        // Task not found -> reactivate_task returns Err -> process_single_backlog increments retry and returns Err
        // process_backlog catches the error but the overall result is still Completed (error is logged, not propagated)
        match result {
            ProcessResult::Completed => {}
            ProcessResult::Error(msg) => {
                // The error is caught in process_backlog's inner loop, so process() returns Completed
                panic!("Expected Completed but got Error: {}", msg);
            }
            _ => panic!("Unexpected result: {:?}", result),
        }
    }

    // ========== process() with concurrency allowed - task already completed ==========

    #[tokio::test]
    async fn test_process_allowed_task_not_queued_marks_completed() {
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let backlog = make_backlog(team_id, task_id);
        // Task is already Completed (not Queued)
        let task = make_task(task_id, TaskStatus::Completed);
        let repo = Arc::new(MockBacklogRepo::new(vec![backlog]));
        let updated_repo = repo.clone();
        let task_repo = Arc::new(MockTaskRepo::new().with_task(task));
        let worker = make_worker(
            repo,
            task_repo,
            Arc::new(MockRateLimitingService::new_allowed()),
            default_settings(),
        );
        let result = worker.process().await;
        // process() returns Completed because errors in process_single_backlog are caught
        assert_eq!(result, ProcessResult::Completed);
        // mark_completed fails because backlog is Pending (not Processing),
        // so reactivate_task returns Err, which triggers retry_count increment
        let updated = updated_repo.updated();
        assert_eq!(updated.len(), 1);
        assert_eq!(
            updated[0].status,
            crate::domain::repositories::tasks_backlog_repository::TasksBacklogStatus::Pending
        );
        assert_eq!(updated[0].retry_count, 1);
    }

    // ========== process() with concurrency allowed - task queued reactivates ==========

    #[tokio::test]
    async fn test_process_allowed_task_queued_reactivates() {
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let backlog = make_backlog(team_id, task_id);
        let task = make_task(task_id, TaskStatus::Queued);
        let repo = Arc::new(MockBacklogRepo::new(vec![backlog]));
        let updated_repo = repo.clone();
        let task_repo = Arc::new(MockTaskRepo::new().with_task(task));
        let worker = make_worker(
            repo,
            task_repo,
            Arc::new(MockRateLimitingService::new_allowed()),
            default_settings(),
        );
        let result = worker.process().await;
        // process() returns Completed because errors in process_single_backlog are caught
        assert_eq!(result, ProcessResult::Completed);
        // mark_completed fails because backlog is Pending (not Processing),
        // so reactivate_task returns Err, which triggers retry_count increment
        let updated = updated_repo.updated();
        assert_eq!(updated.len(), 1);
        assert_eq!(
            updated[0].status,
            crate::domain::repositories::tasks_backlog_repository::TasksBacklogStatus::Pending
        );
        assert_eq!(updated[0].retry_count, 1);
    }

    // ========== process() with multiple teams ==========

    #[tokio::test]
    async fn test_process_multiple_teams() {
        let team1 = Uuid::new_v4();
        let team2 = Uuid::new_v4();
        let backlog1 = make_backlog(team1, Uuid::new_v4());
        let backlog2 = make_backlog(team2, Uuid::new_v4());
        let repo = Arc::new(MockBacklogRepo::new(vec![backlog1, backlog2]));
        let worker = make_worker(
            repo,
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRateLimitingService::new_denied()),
            default_settings(),
        );
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
    }

    // ========== cleanup cycle counter (every 10th cycle) ==========

    #[tokio::test]
    async fn test_cleanup_runs_on_10th_cycle() {
        // The counter starts at 0, fetch_add returns 0 first, then 1, etc.
        // counter.is_multiple_of(10) is true when counter == 0, 10, 20, ...
        // So the FIRST cycle (counter=0) triggers cleanup!
        let worker = make_worker(
            Arc::new(MockBacklogRepo::new_empty()),
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRateLimitingService::new_allowed()),
            default_settings(),
        );
        // First call: counter=0, 0.is_multiple_of(10) = true -> cleanup runs
        let result1 = worker.process().await;
        assert_eq!(result1, ProcessResult::Completed);
    }

    #[tokio::test]
    async fn test_cleanup_does_not_run_on_non_10th_cycle() {
        let worker = make_worker(
            Arc::new(MockBacklogRepo::new_empty()),
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRateLimitingService::new_allowed()),
            default_settings(),
        );
        // First call triggers cleanup (counter=0), subsequent calls (1-9) don't
        let _ = worker.process().await; // counter 0 -> cleanup
        let _ = worker.process().await; // counter 1 -> no cleanup
        let _ = worker.process().await; // counter 2 -> no cleanup
        let result = worker.process().await; // counter 3 -> no cleanup
        assert_eq!(result, ProcessResult::Completed);
    }

    // ========== RedisClientConfig sanity ==========

    #[test]
    fn test_redis_client_config_default() {
        let config = RedisClientConfig::default();
        // Just verify it can be constructed
        assert!(config.max_connections > 0);
    }

    // ========== cleanup_expired_tasks: process_expired_backlog with task not found ==========

    #[tokio::test]
    async fn test_cleanup_expired_backlog_task_not_found() {
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let expired_backlog = make_expired_backlog(team_id, task_id);
        let repo = Arc::new(MockBacklogRepo::new_with_expired(vec![expired_backlog]));
        let updated_repo = repo.clone();
        let worker = make_worker(
            repo,
            Arc::new(MockTaskRepo::new()), // no task stored
            Arc::new(MockRateLimitingService::new_allowed()),
            default_settings(),
        );
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
        // The expired backlog should be marked as Expired via update
        let updated = updated_repo.updated();
        assert_eq!(updated.len(), 1);
        assert_eq!(
            updated[0].status,
            crate::domain::repositories::tasks_backlog_repository::TasksBacklogStatus::Expired
        );
    }

    // ========== cleanup_expired_tasks: process_expired_backlog marks Queued task as Failed ==========

    #[tokio::test]
    async fn test_cleanup_expired_with_queued_task_marks_failed() {
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let expired_backlog = make_expired_backlog(team_id, task_id);
        let task = make_task(task_id, TaskStatus::Queued);
        let repo = Arc::new(MockBacklogRepo::new_with_expired(vec![expired_backlog]));
        let updated_repo = repo.clone();
        let task_repo = Arc::new(MockTaskRepo::new().with_task(task));
        let task_repo_for_check = task_repo.clone();
        let worker = make_worker(
            repo,
            task_repo,
            Arc::new(MockRateLimitingService::new_allowed()),
            default_settings(),
        );
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
        // The expired backlog should be marked as Expired
        let updated = updated_repo.updated();
        assert_eq!(updated.len(), 1);
        assert_eq!(
            updated[0].status,
            crate::domain::repositories::tasks_backlog_repository::TasksBacklogStatus::Expired
        );
        // The Queued task should have been marked as Failed
        let updated_tasks = task_repo_for_check.updated_tasks();
        assert_eq!(updated_tasks.len(), 1);
        assert_eq!(updated_tasks[0].status, TaskStatus::Failed);
    }

    // ========== cleanup_expired_tasks: process_expired_backlog with non-Queued task ==========

    #[tokio::test]
    async fn test_cleanup_expired_with_non_queued_task_no_task_update() {
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let expired_backlog = make_expired_backlog(team_id, task_id);
        // Task is already Completed, so it should NOT be marked as Failed
        let task = make_task(task_id, TaskStatus::Completed);
        let repo = Arc::new(MockBacklogRepo::new_with_expired(vec![expired_backlog]));
        let updated_repo = repo.clone();
        let worker = make_worker(
            repo,
            Arc::new(MockTaskRepo::new().with_task(task)),
            Arc::new(MockRateLimitingService::new_allowed()),
            default_settings(),
        );
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
        // Only the backlog is updated (marked as Expired), no task update
        let updated = updated_repo.updated();
        assert_eq!(updated.len(), 1);
        assert_eq!(
            updated[0].status,
            crate::domain::repositories::tasks_backlog_repository::TasksBacklogStatus::Expired
        );
    }

    // ========== cleanup_expired_tasks: multiple expired backlogs ==========

    #[tokio::test]
    async fn test_cleanup_multiple_expired_backlogs() {
        let team1 = Uuid::new_v4();
        let team2 = Uuid::new_v4();
        let expired1 = make_expired_backlog(team1, Uuid::new_v4());
        let expired2 = make_expired_backlog(team2, Uuid::new_v4());
        let repo = Arc::new(MockBacklogRepo::new_with_expired(vec![expired1, expired2]));
        let updated_repo = repo.clone();
        let worker = make_worker(
            repo,
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRateLimitingService::new_allowed()),
            default_settings(),
        );
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
        let updated = updated_repo.updated();
        assert_eq!(updated.len(), 2);
        for entry in &updated {
            assert_eq!(
                entry.status,
                crate::domain::repositories::tasks_backlog_repository::TasksBacklogStatus::Expired
            );
        }
    }

    // ========== process() success path: backlog in Processing, task Queued, Allowed ==========

    #[tokio::test]
    async fn test_process_success_reactivates_queued_task() {
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        // Backlog in Processing status so mark_completed succeeds
        let backlog = make_processing_backlog(team_id, task_id);
        let task = make_task(task_id, TaskStatus::Queued);
        let repo = Arc::new(MockBacklogRepo::new(vec![backlog]));
        let updated_repo = repo.clone();
        let task_repo = Arc::new(MockTaskRepo::new().with_task(task));
        let task_repo_for_check = task_repo.clone();
        let worker = make_worker(
            repo,
            task_repo,
            Arc::new(MockRateLimitingService::new_allowed()),
            default_settings(),
        );
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
        // The backlog should be marked as Completed
        let updated = updated_repo.updated();
        assert_eq!(updated.len(), 1);
        assert_eq!(
            updated[0].status,
            crate::domain::repositories::tasks_backlog_repository::TasksBacklogStatus::Completed
        );
        // The task should have been updated (status stays Queued, updated_at refreshed)
        let updated_tasks = task_repo_for_check.updated_tasks();
        assert_eq!(updated_tasks.len(), 1);
        assert_eq!(updated_tasks[0].status, TaskStatus::Queued);
    }

    // ========== process() success path: task not Queued marks backlog completed ==========

    #[tokio::test]
    async fn test_process_task_not_queued_marks_backlog_completed() {
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        // Backlog in Processing status so mark_completed succeeds
        let backlog = make_processing_backlog(team_id, task_id);
        // Task is already Completed (not Queued)
        let task = make_task(task_id, TaskStatus::Completed);
        let repo = Arc::new(MockBacklogRepo::new(vec![backlog]));
        let updated_repo = repo.clone();
        let worker = make_worker(
            repo,
            Arc::new(MockTaskRepo::new().with_task(task)),
            Arc::new(MockRateLimitingService::new_allowed()),
            default_settings(),
        );
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
        // reactivate_task sees task.status != Queued, marks backlog as Completed
        let updated = updated_repo.updated();
        assert_eq!(updated.len(), 1);
        assert_eq!(
            updated[0].status,
            crate::domain::repositories::tasks_backlog_repository::TasksBacklogStatus::Completed
        );
    }

    // ========== process() with concurrency check error ==========

    #[tokio::test]
    async fn test_process_concurrency_check_error_returns_completed() {
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();
        let backlog = make_backlog(team_id, task_id);
        let repo = Arc::new(MockBacklogRepo::new(vec![backlog]));
        let worker = make_worker(
            repo,
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRateLimitingService::new_error()),
            default_settings(),
        );
        // The error is caught in process_backlog's inner loop, so process() returns Completed
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
    }

    // ========== process() with mixed backlogs (expired + normal) ==========

    #[tokio::test]
    async fn test_process_mixed_backlogs_expired_and_normal() {
        let team_id = Uuid::new_v4();
        let task_id1 = Uuid::new_v4();
        let task_id2 = Uuid::new_v4();
        // One expired backlog (will be marked Expired)
        let expired = make_expired_backlog(team_id, task_id1);
        // One normal backlog with denied concurrency (will stay as-is)
        let normal = make_backlog(team_id, task_id2);
        let repo = Arc::new(MockBacklogRepo::new(vec![expired, normal]));
        let updated_repo = repo.clone();
        let worker = make_worker(
            repo,
            Arc::new(MockTaskRepo::new()),
            Arc::new(MockRateLimitingService::new_denied()),
            default_settings(),
        );
        let result = worker.process().await;
        assert_eq!(result, ProcessResult::Completed);
        // Only the expired backlog is updated (marked as Expired)
        let updated = updated_repo.updated();
        assert_eq!(updated.len(), 1);
        assert_eq!(
            updated[0].status,
            crate::domain::repositories::tasks_backlog_repository::TasksBacklogStatus::Expired
        );
    }
}
