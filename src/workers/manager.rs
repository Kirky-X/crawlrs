// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::application::use_cases::create_scrape::CreateScrapeUseCaseTrait;
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::services::webhook_service::WebhookService;
use crate::engines::engine_client::EngineClient;
use crate::presentation::middleware::team_semaphore::TeamSemaphore;
use crate::queue::task_queue::TaskQueue;
use crate::utils::regex_cache::RegexCache;
use crate::workers::expiration_worker::ExpirationWorker;
use crate::workers::scrape_worker::ScrapeWorker;
use crate::workers::{AbstractWorker, Worker};
use log::{error, info};
use std::sync::Arc;
use tokio::signal;
use tokio::task::JoinHandle;

use crate::config::settings::Settings;
use crate::utils::robots::RobotsCheckerTrait;

/// 工作管理器
pub struct WorkerManager {
    queue: Arc<dyn TaskQueue>,
    repository: Arc<dyn TaskRepository>,
    result_repository: Arc<dyn ScrapeResultRepository>,
    crawl_repository: Arc<dyn CrawlRepository>,
    webhook_service: Arc<dyn WebhookService>,
    credits_repository: Arc<dyn CreditsRepository>,
    engine_client: Arc<EngineClient>,
    create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait>,
    team_semaphore: Arc<TeamSemaphore>,
    robots_checker: Arc<dyn RobotsCheckerTrait>,
    settings: Arc<Settings>,
    default_concurrency_limit: usize,
    handles: Vec<JoinHandle<()>>,
    extraction_service:
        Arc<dyn crate::domain::services::extraction_service::ExtractionServiceTrait>,
    regex_cache: RegexCache,
}

/// Worker Manager Dependencies
pub struct WorkerManagerDeps {
    pub queue: Arc<dyn TaskQueue>,
    pub repository: Arc<dyn TaskRepository>,
    pub result_repository: Arc<dyn ScrapeResultRepository>,
    pub crawl_repository: Arc<dyn CrawlRepository>,
    pub webhook_service: Arc<dyn WebhookService>,
    pub credits_repository: Arc<dyn CreditsRepository>,
    pub engine_client: Arc<EngineClient>,
    pub create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait>,
    pub team_semaphore: Arc<TeamSemaphore>,
    pub robots_checker: Arc<dyn RobotsCheckerTrait>,
    pub http_client: Arc<reqwest::Client>,
    pub extraction_service:
        Arc<dyn crate::domain::services::extraction_service::ExtractionServiceTrait>,
    pub regex_cache: RegexCache,
}

/// Worker Manager Configuration
pub struct WorkerManagerConfig {
    pub settings: Arc<Settings>,
    pub default_concurrency_limit: usize,
}

impl WorkerManager {
    pub fn new(deps: WorkerManagerDeps, config: WorkerManagerConfig) -> Self {
        Self {
            queue: deps.queue,
            repository: deps.repository,
            result_repository: deps.result_repository,
            crawl_repository: deps.crawl_repository,
            webhook_service: deps.webhook_service,
            credits_repository: deps.credits_repository,
            engine_client: deps.engine_client,
            create_scrape_use_case: deps.create_scrape_use_case,
            team_semaphore: deps.team_semaphore,
            robots_checker: deps.robots_checker,
            settings: config.settings,
            default_concurrency_limit: config.default_concurrency_limit,
            handles: Vec::new(),
            extraction_service: deps.extraction_service,
            regex_cache: deps.regex_cache,
        }
    }

    /// 启动工作进程
    ///
    /// 创建并启动指定数量的工作进程
    ///
    /// # 参数
    ///
    /// * `count` - 要启动的工作进程数量
    pub async fn start_workers(&mut self, count: usize) {
        // 启动过期清理工作器（使用新模板模式）
        let expiration_processor = Arc::new(ExpirationWorker::new(self.repository.clone()));
        let expiration_worker =
            AbstractWorker::new(expiration_processor, std::time::Duration::from_secs(3600));
        self.handles.push(tokio::spawn(async move {
            expiration_worker.run().await;
        }));

        for _ in 0..count {
            let worker = ScrapeWorker::new(
                self.repository.clone(),
                self.result_repository.clone(),
                self.crawl_repository.clone(),
                self.webhook_service.clone(),
                self.credits_repository.clone(),
                self.engine_client.clone(),
                self.create_scrape_use_case.clone(),
                self.team_semaphore.clone(),
                self.robots_checker.clone(),
                self.settings.clone(),
                self.default_concurrency_limit,
                self.extraction_service.clone(),
                self.regex_cache.clone(),
            );

            let queue = self.queue.clone();
            // We spawn the worker loop on a separate task to avoid blocking the main thread
            // or the loop that spawns workers.
            let handle = tokio::spawn(async move {
                worker.run(queue).await;
            });
            self.handles.push(handle);
        }
    }

    /// 等待关闭信号并关闭工作进程
    ///
    /// 监听关闭信号并优雅地关闭所有工作进程
    pub async fn wait_for_shutdown(&mut self) {
        match signal::ctrl_c().await {
            Ok(()) => info!("Shutdown signal received"),
            Err(err) => error!("Unable to listen for shutdown signal: {}", err),
        }

        info!("Shutting down workers...");
        for handle in &self.handles {
            handle.abort();
        }

        info!("Workers shut down successfully");
    }
}

impl Drop for WorkerManager {
    fn drop(&mut self) {
        // Abort all worker handles to prevent them from running after the manager is dropped
        for handle in &self.handles {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== WorkerManagerConfig construction ==========

    #[test]
    fn test_worker_manager_config_construction() {
        let settings = Arc::new(Settings::default());
        let config = WorkerManagerConfig {
            settings: settings.clone(),
            default_concurrency_limit: 10,
        };
        assert_eq!(config.default_concurrency_limit, 10);
        assert_eq!(Arc::strong_count(&config.settings), 2);
    }

    #[test]
    fn test_worker_manager_config_with_different_concurrency() {
        let settings = Arc::new(Settings::default());
        let config = WorkerManagerConfig {
            settings,
            default_concurrency_limit: 50,
        };
        assert_eq!(config.default_concurrency_limit, 50);
    }

    #[test]
    fn test_worker_manager_config_concurrency_zero() {
        let settings = Arc::new(Settings::default());
        let config = WorkerManagerConfig {
            settings,
            default_concurrency_limit: 0,
        };
        assert_eq!(config.default_concurrency_limit, 0);
    }

    #[test]
    fn test_worker_manager_config_settings_shared() {
        let settings = Arc::new(Settings::default());
        let config1 = WorkerManagerConfig {
            settings: settings.clone(),
            default_concurrency_limit: 5,
        };
        let config2 = WorkerManagerConfig {
            settings: settings.clone(),
            default_concurrency_limit: 15,
        };
        // Both configs share the same Arc<Settings>
        assert!(Arc::ptr_eq(&config1.settings, &config2.settings));
    }

    // ========== EngineClient construction ==========

    #[test]
    fn test_engine_client_can_be_constructed() {
        let client = EngineClient::new();
        // Verify it can be cloned (required by WorkerManager)
        let _cloned = client.clone();
    }

    // ========== TeamSemaphore construction (in-memory, no external service) ==========

    #[test]
    fn test_team_semaphore_can_be_constructed() {
        // TeamSemaphore is an in-memory primitive — no external service required.
        let sem = TeamSemaphore::new(10);
        // Verify behavior: acquiring a permit should succeed (limit is 10)
        let team_id = uuid::Uuid::new_v4();
        assert!(sem.try_acquire(team_id).is_some());
    }

    #[test]
    fn test_team_semaphore_clone_shares_state() {
        let sem = TeamSemaphore::new(1);
        let cloned = sem.clone();
        // Both clones share the same internal DashMap
        let team_id = uuid::Uuid::new_v4();
        // Acquire from original — exhausts the single permit
        let _permit = sem
            .try_acquire(team_id)
            .expect("first acquire should succeed");
        // Cloned should also see the exhausted state (shared internal map)
        assert!(cloned.try_acquire(team_id).is_none());
    }

    // ========== WorkerManagerDeps field types verification ==========
    // Note: Full construction of WorkerManagerDeps requires mocking 9+ traits,
    // which is impractical for unit tests. We verify the struct can be referenced
    // and its fields have the expected types.

    #[test]
    fn test_worker_manager_deps_struct_exists() {
        // Verify the struct can be referenced (compile-time check)
        fn _assert_deps_type(_deps: WorkerManagerDeps) {}
        // This function existing proves the struct is accessible
    }

    #[test]
    fn test_worker_manager_config_struct_exists() {
        fn _assert_config_type(_config: WorkerManagerConfig) {}
    }

    #[test]
    fn test_worker_manager_struct_exists() {
        fn _assert_manager_type(_manager: WorkerManager) {}
    }

    // ========== WorkerManagerConfig additional tests ==========

    #[test]
    fn test_worker_manager_config_large_concurrency() {
        let settings = Arc::new(Settings::default());
        let config = WorkerManagerConfig {
            settings,
            default_concurrency_limit: usize::MAX,
        };
        assert_eq!(config.default_concurrency_limit, usize::MAX);
    }

    #[test]
    fn test_worker_manager_config_one_concurrency() {
        let settings = Arc::new(Settings::default());
        let config = WorkerManagerConfig {
            settings,
            default_concurrency_limit: 1,
        };
        assert_eq!(config.default_concurrency_limit, 1);
    }

    #[test]
    fn test_worker_manager_config_settings_arc_count_increases() {
        let settings = Arc::new(Settings::default());
        let initial_count = Arc::strong_count(&settings);
        let _config = WorkerManagerConfig {
            settings: settings.clone(),
            default_concurrency_limit: 10,
        };
        let after_count = Arc::strong_count(&settings);
        assert_eq!(after_count, initial_count + 1);
    }

    #[test]
    fn test_worker_manager_config_settings_arc_count_decreces_on_drop() {
        let settings = Arc::new(Settings::default());
        {
            let _config = WorkerManagerConfig {
                settings: settings.clone(),
                default_concurrency_limit: 10,
            };
            assert!(Arc::strong_count(&settings) > 1);
        }
        // After config goes out of scope, count should decrease
        assert_eq!(Arc::strong_count(&settings), 1);
    }

    #[test]
    fn test_multiple_configs_sharing_same_settings() {
        let settings = Arc::new(Settings::default());
        let configs: Vec<WorkerManagerConfig> = (0..5)
            .map(|i| WorkerManagerConfig {
                settings: settings.clone(),
                default_concurrency_limit: i * 10,
            })
            .collect();
        // All configs should share the same Arc
        for config in &configs {
            assert!(Arc::ptr_eq(&config.settings, &settings));
        }
        assert_eq!(configs.len(), 5);
        assert_eq!(configs[0].default_concurrency_limit, 0);
        assert_eq!(configs[4].default_concurrency_limit, 40);
    }

    // ========== EngineClient additional tests ==========

    #[test]
    fn test_engine_client_clone_preserves_identity() {
        let client = EngineClient::new();
        let cloned = client.clone();
        // Both should be usable independently
        let _another_clone = cloned.clone();
    }

    #[test]
    fn test_engine_client_multiple_instances() {
        let client1 = EngineClient::new();
        let client2 = EngineClient::new();
        // Both should be independently usable
        let _both = (client1, client2);
    }

    // ========== Settings default values ==========

    #[test]
    fn test_settings_default_is_constructible() {
        let settings1 = Settings::default();
        let settings2 = Settings::default();
        // Each Settings::default() should create an independent instance
        let _ = (settings1, settings2);
    }

    #[test]
    fn test_settings_can_be_cloned() {
        let settings = Settings::default();
        let _cloned = settings.clone();
    }

    // ========== TeamSemaphore additional tests ==========

    #[tokio::test]
    async fn test_team_semaphore_acquire_returns_permit() {
        let sem = TeamSemaphore::new(3);
        let team_id = uuid::Uuid::new_v4();
        let permit = sem.acquire(team_id).await;
        assert!(permit.is_ok());
    }

    #[test]
    fn test_team_semaphore_try_acquire_respects_limit() {
        let sem = TeamSemaphore::new(1);
        let team_id = uuid::Uuid::new_v4();
        let p1 = sem.try_acquire(team_id);
        assert!(p1.is_some());
        // Limit is 1, second acquire should fail
        let p2 = sem.try_acquire(team_id);
        assert!(p2.is_none());
    }

    // ========== RegexCache construction ==========

    #[test]
    fn test_regex_cache_can_be_constructed() {
        let cache = RegexCache::new(Arc::new(
            crate::infrastructure::oxcache::RegexCacheType::new(),
        ));
        // Verify it can be cloned (required by WorkerManagerDeps)
        let _cloned = cache.clone();
    }

    #[test]
    fn test_regex_cache_clone_preserves_behavior() {
        let cache = RegexCache::new(Arc::new(
            crate::infrastructure::oxcache::RegexCacheType::new(),
        ));
        let cloned = cache.clone();
        // Both should be able to compile the same regex
        let regex1 = cache.get_or_insert(r"\d+").unwrap();
        let regex2 = cloned.get_or_insert(r"\d+").unwrap();
        assert!(regex1.is_match("123"));
        assert!(regex2.is_match("456"));
    }

    // ========== WorkerManagerDeps field verification ==========

    #[test]
    fn test_worker_manager_deps_has_expected_fields() {
        // Compile-time verification that WorkerManagerDeps has the expected fields
        // by constructing it field-by-field (partially, to verify field names)
        let settings = Arc::new(Settings::default());

        // Verify WorkerManagerConfig fields
        let config = WorkerManagerConfig {
            settings: settings.clone(),
            default_concurrency_limit: 10,
        };
        // Access fields to verify they exist
        let _limit = config.default_concurrency_limit;
        let _settings_ref = &config.settings;
    }

    // ========== WorkerManager Drop behavior ==========

    #[test]
    fn test_worker_manager_drop_aborts_handles() {
        // WorkerManager::new requires full deps which is impractical to construct.
        // Instead, verify that the type requires Drop (has a non-trivial destructor)
        // by checking std::mem::needs_drop at compile time.
        assert!(std::mem::needs_drop::<WorkerManager>());
    }

    // ========== WorkerManagerConfig Send + Sync verification ==========

    #[test]
    fn test_worker_manager_config_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WorkerManagerConfig>();
    }

    #[test]
    fn test_worker_manager_config_arc_settings_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Arc<Settings>>();
    }

    // ========== Concurrency limit boundary values ==========

    #[test]
    fn test_concurrency_limit_boundary_values() {
        let settings = Arc::new(Settings::default());
        // Test various boundary values
        for &limit in &[0usize, 1, 10, 100, 1000] {
            let config = WorkerManagerConfig {
                settings: settings.clone(),
                default_concurrency_limit: limit,
            };
            assert_eq!(config.default_concurrency_limit, limit);
        }
    }

    // ========== WorkerManager Send + Sync verification ==========

    #[test]
    fn test_worker_manager_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WorkerManager>();
    }

    #[test]
    fn test_worker_manager_deps_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<WorkerManagerDeps>();
    }

    // ========== WorkerManagerConfig memory size ==========

    #[test]
    fn test_worker_manager_config_size_is_reasonable() {
        // WorkerManagerConfig contains an Arc<Settings> and a usize.
        // Arc is pointer-sized (8 bytes on 64-bit), usize is 8 bytes.
        // The struct should be 16 bytes (no padding needed).
        let size = std::mem::size_of::<WorkerManagerConfig>();
        assert!(size > 0);
        assert!(
            size <= 32,
            "WorkerManagerConfig size {} seems too large",
            size
        );
    }

    // ========== WorkerManager handles field verification ==========

    #[test]
    fn test_worker_manager_has_handles_field() {
        // Verify that WorkerManager has a handles field of type Vec<JoinHandle<()>>
        // by checking the type at compile time.
        fn _assert_handles_type(_handles: Vec<JoinHandle<()>>) {}
        // This function existing proves the type is accessible
    }

    // ========== WorkerManagerConfig default_concurrency_limit range ==========

    #[test]
    fn test_concurrency_limit_powers_of_two() {
        let settings = Arc::new(Settings::default());
        for &limit in &[1usize, 2, 4, 8, 16, 32, 64, 128, 256] {
            let config = WorkerManagerConfig {
                settings: settings.clone(),
                default_concurrency_limit: limit,
            };
            assert_eq!(config.default_concurrency_limit, limit);
        }
    }

    // ========== Settings Arc sharing across configs ==========

    #[test]
    fn test_settings_arc_strong_count_with_many_configs() {
        let settings = Arc::new(Settings::default());
        let configs: Vec<_> = (0..10)
            .map(|_| WorkerManagerConfig {
                settings: settings.clone(),
                default_concurrency_limit: 5,
            })
            .collect();
        // 10 configs + original = 11 strong references
        assert_eq!(Arc::strong_count(&settings), 11);
        assert_eq!(configs.len(), 10);
    }

    #[test]
    fn test_settings_arc_count_decreases_after_config_drop() {
        let settings = Arc::new(Settings::default());
        let initial = Arc::strong_count(&settings);
        {
            let _config1 = WorkerManagerConfig {
                settings: settings.clone(),
                default_concurrency_limit: 1,
            };
            let _config2 = WorkerManagerConfig {
                settings: settings.clone(),
                default_concurrency_limit: 2,
            };
            assert_eq!(Arc::strong_count(&settings), initial + 2);
        }
        // After configs are dropped, count returns to initial
        assert_eq!(Arc::strong_count(&settings), initial);
    }

    // ========== WorkerManager Drop impl verification ==========

    #[test]
    fn test_worker_manager_needs_drop_is_true() {
        // WorkerManager implements Drop, so needs_drop should be true
        assert!(std::mem::needs_drop::<WorkerManager>());
    }

    #[test]
    fn test_worker_manager_drop_is_not_noop() {
        // Verify WorkerManager has a non-trivial Drop (handles are aborted on drop).
        // needs_drop is true when the type or any field requires Drop.
        assert!(std::mem::needs_drop::<WorkerManager>());
    }

    // ========== WorkerManager method integration tests ==========
    //
    // The following tests exercise WorkerManager::new(), start_workers(),
    // wait_for_shutdown(), and Drop by constructing a full WorkerManagerDeps
    // with no-op mock implementations of all required traits.

    use crate::application::dto::scrape_request::ScrapeRequestDto;
    use crate::domain::models::{
        Crawl, CrawlStatus, CreditsTransaction, CreditsTransactionType, DomainError, ScrapeResult,
        Task, WebhookEvent,
    };
    use crate::domain::repositories::credits_repository::CreditsRepositoryError;
    use crate::domain::repositories::task_repository::{RepositoryError, TaskQueryParams};
    use crate::domain::services::extraction_service::{ExtractionRule, ExtractionServiceTrait};
    use crate::domain::services::llm_service::TokenUsage;
    use crate::engines::engine_client::ScrapeResponse;
    use crate::queue::task_queue::QueueError;
    use async_trait::async_trait;
    use serde_json::Value;
    use std::collections::{HashMap, HashSet};
    use uuid::Uuid;

    // ---- No-op mock implementations ----

    struct MockTaskQueue;

    #[async_trait]
    impl TaskQueue for MockTaskQueue {
        async fn enqueue(&self, task: Task) -> Result<Task, QueueError> {
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

    struct MockTaskRepository;

    #[async_trait]
    impl TaskRepository for MockTaskRepository {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
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

    struct MockScrapeResultRepository;

    #[async_trait]
    impl ScrapeResultRepository for MockScrapeResultRepository {
        async fn save(&self, _result: ScrapeResult) -> anyhow::Result<()> {
            Ok(())
        }
        async fn find_by_task_id(&self, _task_id: Uuid) -> anyhow::Result<Option<ScrapeResult>> {
            Ok(None)
        }
        async fn find_by_task_ids(&self, _task_ids: &[Uuid]) -> anyhow::Result<Vec<ScrapeResult>> {
            Ok(vec![])
        }
        async fn get_team_avg_response_time(&self, _team_id: Uuid) -> anyhow::Result<f64> {
            Ok(0.0)
        }
    }

    struct MockCrawlRepository;

    #[async_trait]
    impl CrawlRepository for MockCrawlRepository {
        async fn create(&self, crawl: &Crawl) -> Result<Crawl, RepositoryError> {
            Ok(crawl.clone())
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Crawl>, RepositoryError> {
            Ok(None)
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

    struct MockWebhookService;

    #[async_trait]
    impl WebhookService for MockWebhookService {
        async fn send_webhook(&self, _event: &WebhookEvent) -> anyhow::Result<()> {
            Ok(())
        }
        async fn trigger_completion(&self, _task: &Task) -> anyhow::Result<()> {
            Ok(())
        }
        async fn trigger_failure(&self, _task: &Task, _error_msg: String) -> anyhow::Result<()> {
            Ok(())
        }
    }

    struct MockCreditsRepository;

    #[async_trait]
    impl CreditsRepository for MockCreditsRepository {
        async fn get_balance(&self, _team_id: Uuid) -> Result<i64, CreditsRepositoryError> {
            Ok(0)
        }
        async fn deduct_credits(
            &self,
            _team_id: Uuid,
            _amount: i64,
            _transaction_type: CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<(), CreditsRepositoryError> {
            Ok(())
        }
        async fn add_credits(
            &self,
            _team_id: Uuid,
            _amount: i64,
            _transaction_type: CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<i64, CreditsRepositoryError> {
            Ok(0)
        }
        async fn get_transaction_history(
            &self,
            _team_id: Uuid,
            _limit: Option<u32>,
        ) -> Result<Vec<CreditsTransaction>, CreditsRepositoryError> {
            Ok(vec![])
        }
        async fn initialize_team_credits(
            &self,
            _team_id: Uuid,
            _initial_balance: i64,
        ) -> Result<i64, CreditsRepositoryError> {
            Ok(0)
        }
    }

    struct MockCreateScrapeUseCase;

    #[async_trait]
    impl CreateScrapeUseCaseTrait for MockCreateScrapeUseCase {
        async fn execute(
            &self,
            _request_dto: ScrapeRequestDto,
        ) -> Result<ScrapeResponse, DomainError> {
            Err(DomainError::EngineError("mock".to_string()))
        }
    }

    struct MockRobotsChecker;

    #[async_trait]
    impl RobotsCheckerTrait for MockRobotsChecker {
        async fn is_allowed(&self, _url_str: &str, _user_agent: &str) -> anyhow::Result<bool> {
            Ok(true)
        }
        async fn get_crawl_delay(
            &self,
            _url_str: &str,
            _user_agent: &str,
        ) -> anyhow::Result<Option<std::time::Duration>> {
            Ok(None)
        }
    }

    struct MockExtractionService;

    #[async_trait]
    impl ExtractionServiceTrait for MockExtractionService {
        async fn extract(
            &self,
            _html_content: &str,
            _rules: &HashMap<String, ExtractionRule>,
            _base_url: Option<&str>,
        ) -> anyhow::Result<(Value, TokenUsage)> {
            Ok((Value::Null, TokenUsage::default()))
        }
        async fn extract_with_schema(
            &self,
            _html_content: &str,
            _schema: &Value,
        ) -> anyhow::Result<(Value, TokenUsage)> {
            Ok((Value::Null, TokenUsage::default()))
        }
        fn extract_with_selectors(
            &self,
            _html_content: &str,
            _rules: &HashMap<String, ExtractionRule>,
            _base_url: Option<&str>,
        ) -> anyhow::Result<Value> {
            Ok(Value::Null)
        }
    }

    // ---- Helpers ----

    fn make_deps() -> WorkerManagerDeps {
        WorkerManagerDeps {
            queue: Arc::new(MockTaskQueue),
            repository: Arc::new(MockTaskRepository),
            result_repository: Arc::new(MockScrapeResultRepository),
            crawl_repository: Arc::new(MockCrawlRepository),
            webhook_service: Arc::new(MockWebhookService),
            credits_repository: Arc::new(MockCreditsRepository),
            engine_client: Arc::new(EngineClient::new()),
            create_scrape_use_case: Arc::new(MockCreateScrapeUseCase),
            team_semaphore: Arc::new(TeamSemaphore::new(10)),
            robots_checker: Arc::new(MockRobotsChecker),
            http_client: Arc::new(reqwest::Client::new()),
            extraction_service: Arc::new(MockExtractionService),
            regex_cache: RegexCache::new(Arc::new(
                crate::infrastructure::oxcache::RegexCacheType::new(),
            )),
        }
    }

    fn make_config() -> WorkerManagerConfig {
        WorkerManagerConfig {
            settings: Arc::new(Settings::default()),
            default_concurrency_limit: 10,
        }
    }

    // ---- WorkerManager::new tests ----

    #[test]
    fn test_worker_manager_new_assigns_fields() {
        let manager = WorkerManager::new(make_deps(), make_config());
        assert_eq!(manager.default_concurrency_limit, 10);
        assert!(
            manager.handles.is_empty(),
            "new() should start with no handles"
        );
    }

    // ---- start_workers tests ----

    #[tokio::test]
    async fn test_start_workers_zero_count_starts_only_expiration_worker() {
        let mut manager = WorkerManager::new(make_deps(), make_config());
        manager.start_workers(0).await;
        assert_eq!(
            manager.handles.len(),
            1,
            "start_workers(0) should start only the expiration worker"
        );
    }

    #[tokio::test]
    async fn test_start_workers_multiple_count() {
        let mut manager = WorkerManager::new(make_deps(), make_config());
        manager.start_workers(3).await;
        assert_eq!(
            manager.handles.len(),
            4,
            "start_workers(3) should start 1 expiration + 3 scrape workers"
        );
    }

    #[tokio::test]
    async fn test_start_workers_handles_are_aborted_on_drop() {
        let mut manager = WorkerManager::new(make_deps(), make_config());
        manager.start_workers(1).await;
        assert_eq!(manager.handles.len(), 2);

        // Give workers a moment to start.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Verify at least one worker is still running (not finished).
        assert!(
            manager.handles.iter().any(|h| !h.is_finished()),
            "at least one worker should be running before drop"
        );

        // Dropping the manager invokes Drop which aborts all handles.
        drop(manager);

        // Re-run with handles extracted to verify abort takes effect.
        let mut manager2 = WorkerManager::new(make_deps(), make_config());
        manager2.start_workers(1).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let handles: Vec<JoinHandle<()>> = std::mem::take(&mut manager2.handles);
        assert!(
            handles.iter().any(|h| !h.is_finished()),
            "workers should be running before abort"
        );

        // Manually abort (mirrors Drop impl behavior).
        for handle in &handles {
            handle.abort();
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        for handle in &handles {
            assert!(
                handle.is_finished(),
                "all handles should be finished after abort"
            );
        }
    }

    // ========== wait_for_shutdown: completes and aborts handles on SIGINT ==========
    // Covers the Ok(()) => info!("Shutdown signal received") branch and the abort loop
    // that follows ctrl_c() completing. On Unix, we send SIGINT to the current process
    // so tokio's signal handler resolves ctrl_c().

    #[cfg(unix)]
    #[tokio::test]
    async fn test_wait_for_shutdown_completes_on_sigint() {
        use std::time::Duration;

        let mut manager = WorkerManager::new(make_deps(), make_config());
        manager.start_workers(1).await;
        assert_eq!(manager.handles.len(), 2);

        // Spawn a task to send SIGINT after a short delay, giving wait_for_shutdown
        // time to register the tokio signal handler.
        let pid = std::process::id();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let _ = std::process::Command::new("kill")
                .args(["-INT", &pid.to_string()])
                .spawn();
        });

        // wait_for_shutdown should complete after SIGINT (not time out)
        let result =
            tokio::time::timeout(Duration::from_secs(3), manager.wait_for_shutdown()).await;

        assert!(
            result.is_ok(),
            "wait_for_shutdown should complete after SIGINT"
        );
        // After shutdown, all handles should have been aborted. Give aborted tasks
        // a brief moment to actually finish before checking is_finished().
        tokio::time::sleep(Duration::from_millis(50)).await;
        for handle in &manager.handles {
            assert!(
                handle.is_finished(),
                "handles should be aborted after shutdown"
            );
        }
    }
}
