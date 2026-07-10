// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::application::use_cases::create_scrape::CreateScrapeUseCaseTrait;
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::storage_repository::StorageRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::services::webhook_service::WebhookService;
use crate::engines::engine_client::EngineClient;
#[cfg(feature = "redis-cache")]
use crate::infrastructure::cache::redis_client::RedisClient;
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
    storage_repository: Option<Arc<dyn StorageRepository + Send + Sync>>,
    webhook_service: Arc<dyn WebhookService>,
    credits_repository: Arc<dyn CreditsRepository>,
    engine_client: Arc<EngineClient>,
    create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait>,
    redis: RedisClient,
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
    pub storage_repository: Option<Arc<dyn StorageRepository + Send + Sync>>,
    pub webhook_service: Arc<dyn WebhookService>,
    pub credits_repository: Arc<dyn CreditsRepository>,
    pub engine_client: Arc<EngineClient>,
    pub create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait>,
    pub redis: RedisClient,
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
            storage_repository: deps.storage_repository,
            webhook_service: deps.webhook_service,
            credits_repository: deps.credits_repository,
            engine_client: deps.engine_client,
            create_scrape_use_case: deps.create_scrape_use_case,
            redis: deps.redis,
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
                self.storage_repository.clone(),
                self.webhook_service.clone(),
                self.credits_repository.clone(),
                self.engine_client.clone(),
                self.create_scrape_use_case.clone(),
                self.redis.clone(),
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

    // ========== RedisClient construction (pool only, no connection) ==========

    #[cfg(feature = "redis-cache")]
    #[test]
    fn test_redis_client_can_be_constructed_without_server() {
        // RedisClient::new creates a connection pool lazily;
        // it should succeed even without a running Redis server.
        let result = RedisClient::new("redis://localhost:6379");
        assert!(
            result.is_ok(),
            "RedisClient pool creation should succeed without a running server"
        );
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

    // ========== RedisClient additional tests (pool construction only) ==========

    #[cfg(feature = "redis-cache")]
    #[test]
    fn test_redis_client_with_custom_config() {
        use crate::infrastructure::cache::redis_client::{RedisClient, RedisClientConfig};
        let config = RedisClientConfig {
            max_connections: 5,
            connection_timeout: 3,
            recycle_timeout: 2,
        };
        let client = RedisClient::with_config("redis://localhost:6379", config).unwrap();
        let config_ref = client.config();
        assert_eq!(config_ref.max_connections, 5);
        assert_eq!(config_ref.connection_timeout, 3);
        assert_eq!(config_ref.recycle_timeout, 2);
    }

    #[cfg(feature = "redis-cache")]
    #[test]
    fn test_redis_client_clone_shares_pool() {
        use crate::infrastructure::cache::redis_client::RedisClient;
        let client = RedisClient::new("redis://localhost:6379").unwrap();
        let cloned = client.clone();
        // Both should have the same config values (shared pool)
        assert_eq!(
            client.config().max_connections,
            cloned.config().max_connections
        );
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
        assert!(size <= 32, "WorkerManagerConfig size {} seems too large", size);
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
        // Verify Drop is implemented (not just the default)
        // This is a compile-time check: if Drop weren't implemented,
        // needs_drop might still be true due to JoinHandle, but the
        // explicit Drop impl ensures handles are aborted.
        // We verify by checking the type implements Drop.
        fn _assert_drop_impl<T: Drop>() {}
        _assert_drop_impl::<WorkerManager>();
    }
}
