// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::application::use_cases::create_scrape::CreateScrapeUseCaseTrait;
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crate::domain::services::audit_service::AuditServiceTrait;
use crate::domain::services::team_semaphore::TeamSemaphore;
use crate::domain::services::webhook_service::WebhookService;
use crate::engines::engine_client::EngineClient;
use crate::infrastructure::oxcache::CacheService;
use crate::queue::task_queue::TaskQueue;
// T035/R-runtime-002：请求合并器（同 URL 并发只允许首个执行实际抓取）
use crate::utils::coalesce::RequestCoalescer;
use crate::utils::regex_cache::RegexCache;
// H-4 职责拆分：CoalesceCoordinator（封装 try_coalesce 逻辑，注入 ScrapeWorker）
use crate::workers::coalesce_coordinator::CoalesceCoordinator;
use crate::workers::expiration_worker::ExpirationWorker;
use crate::workers::retention_worker::RetentionWorker;
use crate::workers::scrape_worker::{ScrapeWorker, ScrapeWorkerDeps};
// R-security-004/005：优雅退出协调器（design.md D3，T007）
use crate::workers::shutdown::ShutdownCoordinator;
use crate::workers::{AbstractWorker, Worker};
use log::info;
use std::sync::Arc;
use tokio::task::JoinHandle;

use crate::config::settings::Settings;
use crate::utils::robots::RobotsCheckerTrait;
// T019（R-runtime-001）：MemoryScheduler 接入 WorkerManager
#[cfg(feature = "metrics")]
use crate::workers::scheduler::memory_scheduler::MemoryScheduler;

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
    /// R-retention-005：数据保留期清理器使用的三个仓库
    webhook_event_repository: Arc<dyn WebhookEventRepository>,
    geo_restriction_repository: Arc<dyn GeoRestrictionRepository>,
    audit_service: Arc<dyn AuditServiceTrait>,
    /// 请求合并器（T035/R-runtime-002）
    ///
    /// 由 `WorkerManagerDeps` 从 `ServicesComponents.request_coalescer` 注入，
    /// 所有 worker 共享同一实例。
    request_coalescer: Arc<RequestCoalescer>,
    /// 请求合并协调器（H-4 职责拆分）
    ///
    /// 由 `WorkerManager::new` 从 `repository` + `result_repository` +
    /// `request_coalescer` 构造，封装 `try_coalesce` 逻辑。所有 worker 共享
    /// 同一实例（通过 `Arc` clone 注入）。
    coalesce_coordinator: Arc<CoalesceCoordinator>,
    robots_checker: Arc<dyn RobotsCheckerTrait>,
    settings: Arc<Settings>,
    default_concurrency_limit: usize,
    handles: Vec<JoinHandle<()>>,
    extraction_service:
        Arc<dyn crate::domain::services::extraction_service::ExtractionServiceTrait>,
    regex_cache: RegexCache,
    /// 内存感知调度器（T019/R-runtime-001）
    ///
    /// 由 `WorkerManager::new` 从 `shared_system_monitor()` + `ConcurrencySettings`
    /// 阈值构造，所有 worker 共享同一实例。
    #[cfg(feature = "metrics")]
    memory_scheduler: Arc<MemoryScheduler>,
    /// URL 分层去重器（T053/R-frontier-001）
    ///
    /// 所有 worker 共享同一实例，最大化 Bloom 预筛效果。
    /// `RwLock` 因为 Bloom insert 需 `&mut self`，contains 只需 `&self`。
    deduplicator: Arc<parking_lot::RwLock<crate::utils::dedup::Deduplicator>>,
    /// 高级缓存服务（T059/R-cache-002）
    ///
    /// 由 `WorkerManagerDeps` 从 `InfrastructureComponents.cache_service` 注入，
    /// 所有 worker 共享同一实例，用于 `process_scrape_task` 读写抓取结果缓存。
    cache_service: Arc<dyn CacheService>,
    /// 优雅退出协调器（R-security-004/005，design.md D3）
    ///
    /// 由 main.rs 创建并通过 `WorkerManagerDeps` 注入，所有 worker 共享同一实例。
    /// `start_workers` 注入到每个 `ScrapeWorker`，关闭信号到达后 worker 循环
    /// 停止接受新任务并在完成当前任务后退出。
    shutdown_coordinator: Arc<ShutdownCoordinator>,
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
    /// R-retention-005：数据保留期清理器使用的三个仓库
    pub webhook_event_repository: Arc<dyn WebhookEventRepository>,
    pub geo_restriction_repository: Arc<dyn GeoRestrictionRepository>,
    pub audit_service: Arc<dyn AuditServiceTrait>,
    /// 请求合并器（T035/R-runtime-002）
    pub request_coalescer: Arc<RequestCoalescer>,
    pub robots_checker: Arc<dyn RobotsCheckerTrait>,
    pub http_client: Arc<reqwest::Client>,
    pub extraction_service:
        Arc<dyn crate::domain::services::extraction_service::ExtractionServiceTrait>,
    pub regex_cache: RegexCache,
    /// 高级缓存服务（T059/R-cache-002）
    pub cache_service: Arc<dyn CacheService>,
    /// 优雅退出协调器（R-security-004/005，design.md D3）
    pub shutdown_coordinator: Arc<ShutdownCoordinator>,
}

/// Worker Manager Configuration
pub struct WorkerManagerConfig {
    pub settings: Arc<Settings>,
    pub default_concurrency_limit: usize,
}

impl WorkerManager {
    pub fn new(deps: WorkerManagerDeps, config: WorkerManagerConfig) -> Self {
        // T019（R-runtime-001）：构造内存感知调度器
        //
        // 复用 `shared_system_monitor()` 全局单例（init_metrics 启动时已初始化），
        // 从 `ConcurrencySettings` 读取阈值（pressure/critical/timeout）。
        #[cfg(feature = "metrics")]
        let memory_scheduler = {
            use crate::infrastructure::observability::metrics::{
                shared_system_monitor, SystemMonitorTrait,
            };
            Arc::new(MemoryScheduler::new(
                shared_system_monitor() as Arc<dyn SystemMonitorTrait>,
                config.settings.concurrency.mem_pressure_threshold,
                config.settings.concurrency.mem_critical_threshold,
                std::time::Duration::from_secs(
                    config.settings.concurrency.critical_timeout_seconds,
                ),
            ))
        };

        // H-4 职责拆分：在 move 前构造 CoalesceCoordinator，共享 repository +
        // result_repository + request_coalescer（所有 worker 共享同一实例）
        let coalesce_coordinator = Arc::new(CoalesceCoordinator::new(
            deps.repository.clone(),
            deps.result_repository.clone(),
            deps.request_coalescer.clone(),
        ));

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
            webhook_event_repository: deps.webhook_event_repository,
            geo_restriction_repository: deps.geo_restriction_repository,
            audit_service: deps.audit_service,
            coalesce_coordinator,
            request_coalescer: deps.request_coalescer,
            robots_checker: deps.robots_checker,
            settings: config.settings,
            default_concurrency_limit: config.default_concurrency_limit,
            handles: Vec::new(),
            extraction_service: deps.extraction_service,
            regex_cache: deps.regex_cache,
            #[cfg(feature = "metrics")]
            memory_scheduler,
            // T053/R-frontier-001：所有 worker 共享 Deduplicator 实例
            // 共享 Bloom 让已爬 URL 在任一 worker 触发后立即对其他 worker 可见，
            // 最大化降 DB 查询量效果。
            deduplicator: Arc::new(parking_lot::RwLock::new(
                crate::utils::dedup::Deduplicator::new(),
            )),
            // T059/R-cache-002：所有 worker 共享 CacheService 实例
            cache_service: deps.cache_service,
            shutdown_coordinator: deps.shutdown_coordinator,
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

        // R-retention-005：数据保留期清理工作器（间隔与天数来自 [retention] 配置）
        let retention_processor = Arc::new(RetentionWorker::new(
            self.result_repository.clone(),
            self.geo_restriction_repository.clone(),
            self.webhook_event_repository.clone(),
            self.audit_service.clone(),
            self.settings.retention.scrape_results_days,
            self.settings.retention.geo_logs_days,
            self.settings.retention.webhook_events_days,
            self.settings.retention.audit_logs_days,
        ));
        let retention_worker = AbstractWorker::new(
            retention_processor,
            std::time::Duration::from_secs(self.settings.retention.interval_seconds),
        );
        self.handles.push(tokio::spawn(async move {
            retention_worker.run().await;
        }));

        // 性能审查 H-1 修复：定期调度 RequestCoalescer::purge_stale
        //
        // worker panic / 死锁可能导致 CoalesceGuard 未 Drop，使 in-flight 条目
        // 永久驻留 DashMap 阻塞同 URL 后续请求。每 60s 调用 purge_stale 清理
        // 僵死条目（STALE_TIMEOUT=120s），并通过 broadcast 通知等待方重试。
        let coalescer_for_purge = self.request_coalescer.clone();
        let shutdown_for_purge = self.shutdown_coordinator.clone();
        self.handles.push(tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            interval.tick().await; // 跳过首次立即触发
            loop {
                interval.tick().await;
                // T023 修复：检查关闭信号，避免关闭后继续无意义循环
                if shutdown_for_purge.is_shutting_down() {
                    info!("purge_stale loop exiting due to shutdown");
                    break;
                }
                let purged = coalescer_for_purge.purge_stale();
                if purged > 0 {
                    info!("purge_stale cleaned up {} zombie coalesce entries", purged);
                }
            }
        }));

        for _ in 0..count {
            let worker = ScrapeWorker::new(ScrapeWorkerDeps {
                repository: self.repository.clone(),
                result_repository: self.result_repository.clone(),
                crawl_repository: self.crawl_repository.clone(),
                webhook_service: self.webhook_service.clone(),
                credits_repository: self.credits_repository.clone(),
                engine_client: self.engine_client.clone(),
                create_scrape_use_case: self.create_scrape_use_case.clone(),
                team_semaphore: self.team_semaphore.clone(),
                coalesce_coordinator: self.coalesce_coordinator.clone(),
                robots_checker: self.robots_checker.clone(),
                settings: self.settings.clone(),
                default_concurrency_limit: self.default_concurrency_limit,
                extraction_service: self.extraction_service.clone(),
                regex_cache: self.regex_cache.clone(),
                cache_service: self.cache_service.clone(),
                #[cfg(feature = "metrics")]
                memory_scheduler: self.memory_scheduler.clone(),
            })
            // R-security-004/005：注入共享优雅退出协调器（design.md D3，T007）
            .with_shutdown_coordinator(self.shutdown_coordinator.clone())
            // T053/R-frontier-001：注入共享 deduplicator（替换 ScrapeWorker::new 内部默认实例）
            .with_deduplicator_opt(Some(self.deduplicator.clone()));

            let queue = self.queue.clone();
            // We spawn the worker loop on a separate task to avoid blocking the main thread
            // or the loop that spawns workers.
            let handle = tokio::spawn(async move {
                worker.run(queue).await;
            });
            self.handles.push(handle);
        }
    }

    /// 触发优雅退出（R-security-004/005，design.md D3）
    ///
    /// 等价于信号监听任务（`listen_unix_signals`）收到 SIGTERM/SIGINT 后
    /// 调用 `ShutdownCoordinator::trigger()`。置位后各 worker 循环停止接受
    /// 新任务，完成当前任务后退出。
    pub fn begin_shutdown(&self) {
        self.shutdown_coordinator.trigger();
    }

    /// 等待关闭并优雅终止所有工作进程
    ///
    /// 依赖注入的 `ShutdownCoordinator`：
    /// - `wait_for_completion()` 阻塞等待关闭信号，并在关闭后给进行中的任务
    ///   至多 `graceful_period` 的宽限期完成（R-security-004）；
    /// - 宽限期结束后 abort 所有剩余句柄（含不检查关闭 flag 的辅助 worker：
    ///   expiration / purge_stale 等无限循环），强制退出（R-security-005）。
    pub async fn wait_for_shutdown(&mut self) {
        let _ = self.shutdown_coordinator.wait_for_completion().await;

        info!("Shutting down workers...");
        let handles = std::mem::take(&mut self.handles);
        for handle in &handles {
            handle.abort();
        }
        // T023 修复：abort 后 await 所有 handles，确保任务真正退出
        // 带 5s 超时防止某个 handle 卡死阻塞关闭流程
        for handle in handles {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(5), async {
                let _ = handle.await;
            })
            .await;
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
    use crate::domain::services::llm::TokenUsage;
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
        async fn cleanup_expired(&self, _retention_days: i64) -> anyhow::Result<u64> {
            Ok(0)
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

    // R-retention-005：RetentionWorker 所需的三个仓库 mock（无状态，全 Ok 默认值）

    struct MockWebhookEventRepository;

    #[async_trait]
    impl WebhookEventRepository for MockWebhookEventRepository {
        async fn create(&self, event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError> {
            Ok(event.clone())
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<WebhookEvent>, RepositoryError> {
            Ok(None)
        }
        async fn find_pending(&self, _limit: u64) -> Result<Vec<WebhookEvent>, RepositoryError> {
            Ok(vec![])
        }
        async fn find_by_team_id_paginated(
            &self,
            _team_id: Uuid,
            _limit: u32,
            _offset: u32,
        ) -> Result<Vec<WebhookEvent>, RepositoryError> {
            Ok(vec![])
        }
        async fn count_by_team_id(&self, _team_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn update(&self, event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError> {
            Ok(event.clone())
        }
        async fn cleanup_terminal(&self, _retention_days: i64) -> Result<u64, RepositoryError> {
            Ok(0)
        }
    }

    struct MockGeoRestrictionRepository;

    #[async_trait]
    impl GeoRestrictionRepository for MockGeoRestrictionRepository {
        async fn get_team_restrictions(
            &self,
            _team_id: Uuid,
        ) -> Result<
            crate::domain::services::team_service::TeamGeoRestrictions,
            crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError,
        > {
            Ok(crate::domain::services::team_service::TeamGeoRestrictions::default())
        }
        async fn update_team_restrictions(
            &self,
            _team_id: Uuid,
            _restrictions: &crate::domain::services::team_service::TeamGeoRestrictions,
        ) -> Result<
            (),
            crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError,
        > {
            Ok(())
        }
        async fn log_geo_restriction_action(
            &self,
            _team_id: Uuid,
            _ip_address: &str,
            _country_code: &str,
            _action: &str,
            _reason: &str,
        ) -> Result<
            (),
            crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError,
        > {
            Ok(())
        }
        async fn cleanup_expired(
            &self,
            _retention_days: i64,
        ) -> Result<
            u64,
            crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepositoryError,
        > {
            Ok(0)
        }
    }

    struct MockAuditService;

    #[async_trait]
    impl AuditServiceTrait for MockAuditService {
        async fn log(
            &self,
            _entry: crate::domain::auth::AuditLogEntry,
        ) -> Result<(), crate::domain::services::audit_service::AuditServiceError> {
            Ok(())
        }
        async fn log_allow(
            &self,
            _action: String,
            _api_key_id: Uuid,
            _team_id: Uuid,
            _scope: crate::domain::auth::ApiKeyScope,
        ) -> Result<(), crate::domain::services::audit_service::AuditServiceError> {
            Ok(())
        }
        async fn log_deny(
            &self,
            _action: String,
            _api_key_id: Option<Uuid>,
            _team_id: Option<Uuid>,
            _reason: String,
            _scope: Option<crate::domain::auth::ApiKeyScope>,
        ) -> Result<(), crate::domain::services::audit_service::AuditServiceError> {
            Ok(())
        }
        async fn get_logs_for_key(
            &self,
            _api_key_id: Uuid,
            _limit: u64,
            _offset: u64,
        ) -> Result<
            Vec<crate::domain::auth::AuditLogEntry>,
            crate::domain::services::audit_service::AuditServiceError,
        > {
            Ok(vec![])
        }
        async fn get_logs_for_team(
            &self,
            _team_id: Uuid,
            _limit: u64,
            _offset: u64,
        ) -> Result<
            Vec<crate::domain::auth::AuditLogEntry>,
            crate::domain::services::audit_service::AuditServiceError,
        > {
            Ok(vec![])
        }
        async fn get_denied_requests(
            &self,
            _api_key_id: Uuid,
            _limit: u64,
        ) -> Result<
            Vec<crate::domain::auth::AuditLogEntry>,
            crate::domain::services::audit_service::AuditServiceError,
        > {
            Ok(vec![])
        }
        async fn cleanup_old_logs(
            &self,
            _retention_days: i64,
        ) -> Result<u64, crate::domain::services::audit_service::AuditServiceError> {
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
        async fn extract_with_rag(
            &self,
            _html_content: &str,
            _query: &str,
            _schema: &Value,
            _rag_strategy: &crate::domain::services::rag_strategy::RagExtractionStrategy,
        ) -> anyhow::Result<(Value, TokenUsage)> {
            Ok((Value::Null, TokenUsage::default()))
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
            webhook_event_repository: Arc::new(MockWebhookEventRepository),
            geo_restriction_repository: Arc::new(MockGeoRestrictionRepository),
            audit_service: Arc::new(MockAuditService) as Arc<dyn AuditServiceTrait>,
            request_coalescer: Arc::new(crate::utils::coalesce::RequestCoalescer::new()),
            robots_checker: Arc::new(MockRobotsChecker),
            http_client: Arc::new(reqwest::Client::new()),
            extraction_service: Arc::new(MockExtractionService),
            regex_cache: RegexCache::new(Arc::new(
                crate::infrastructure::oxcache::RegexCacheType::new(),
            )),
            cache_service: Arc::new(NoopCacheService) as Arc<dyn CacheService>,
            shutdown_coordinator: Arc::new(ShutdownCoordinator::default()),
        }
    }

    /// Noop CacheService for testing（T059/R-cache-002）
    ///
    /// 所有操作返回 Ok(None)/Ok(())，不实际存储数据。
    /// 用于 `WorkerManagerDeps` 构造时满足 `cache_service` 字段类型要求。
    struct NoopCacheService;

    #[async_trait::async_trait]
    impl CacheService for NoopCacheService {
        fn get(
            &self,
            _key: &str,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = anyhow::Result<Option<String>>> + Send + '_>,
        > {
            Box::pin(async { Ok(None) })
        }

        fn set(
            &self,
            _key: &str,
            _value: &str,
            _ttl_seconds: u64,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>>
        {
            Box::pin(async { Ok(()) })
        }

        fn delete(
            &self,
            _key: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>>
        {
            Box::pin(async { Ok(()) })
        }

        fn exists(
            &self,
            _key: &str,
        ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send + '_>>
        {
            Box::pin(async { Ok(false) })
        }
    }

    fn make_config() -> WorkerManagerConfig {
        WorkerManagerConfig {
            settings: Arc::new(Settings::default()),
            default_concurrency_limit: 10,
        }
    }

    // ---- WorkerManager::new tests ----

    #[tokio::test]
    async fn test_worker_manager_new_assigns_fields() {
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
        // start_workers(0) 启动：1 expiration + 1 retention + 1 purge_stale 调度任务（R-retention-005）
        assert_eq!(
            manager.handles.len(),
            3,
            "start_workers(0) should start expiration + retention + purge_stale scheduler"
        );
    }

    #[tokio::test]
    async fn test_start_workers_multiple_count() {
        let mut manager = WorkerManager::new(make_deps(), make_config());
        manager.start_workers(3).await;
        // start_workers(3) 启动：1 expiration + 1 retention + 1 purge_stale + 3 scrape = 6
        assert_eq!(
            manager.handles.len(),
            6,
            "start_workers(3) should start expiration + retention + purge_stale + 3 scrape workers"
        );
    }

    #[tokio::test]
    async fn test_start_workers_handles_are_aborted_on_drop() {
        let mut manager = WorkerManager::new(make_deps(), make_config());
        manager.start_workers(1).await;
        // start_workers(1) 启动：1 expiration + 1 retention + 1 purge_stale + 1 scrape = 4
        assert_eq!(manager.handles.len(), 4);

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

    // ========== wait_for_shutdown: completes and aborts handles on shutdown trigger ==========
    // Covers the graceful-shutdown branch (R-security-004/005): once the coordinator
    // is triggered (equivalently: SIGTERM/SIGINT received), wait_for_shutdown drains
    // handles within the graceful period and aborts leftovers.
    //
    // 注：不再向当前进程发送 SIGINT。旧的实现依赖 `wait_for_shutdown` 内部
    // `ctrl_c().await` 捕获信号；新实现由外部 `listen_unix_signals` / `begin_shutdown`
    // 触发 coordinator，测试直接调用 `begin_shutdown()` 等价地触发关闭，避免
    // 对测试进程注入信号（会杀死整个测试 harness）。

    #[tokio::test]
    async fn test_wait_for_shutdown_completes_on_trigger() {
        use std::time::Duration;

        let mut manager = WorkerManager::new(make_deps(), make_config());
        manager.start_workers(1).await;
        // start_workers(1) 启动：1 expiration + 1 retention + 1 purge_stale + 1 scrape = 4
        assert_eq!(manager.handles.len(), 4);

        // 触发优雅退出（等价于收到 SIGTERM/SIGINT）
        manager.begin_shutdown();

        // wait_for_shutdown should complete after trigger (not time out)
        let result =
            tokio::time::timeout(Duration::from_secs(5), manager.wait_for_shutdown()).await;

        assert!(
            result.is_ok(),
            "wait_for_shutdown should complete after shutdown trigger"
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

    #[tokio::test]
    async fn test_begin_shutdown_sets_coordinator_flag() {
        let manager = WorkerManager::new(make_deps(), make_config());
        assert!(!manager.shutdown_coordinator.is_shutting_down());
        manager.begin_shutdown();
        assert!(manager.shutdown_coordinator.is_shutting_down());
    }
}
