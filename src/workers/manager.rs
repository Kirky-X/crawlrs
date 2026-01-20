// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::application::use_cases::create_scrape::CreateScrapeUseCase;
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::storage_repository::StorageRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crate::domain::services::llm_service::LLMService;
use crate::engines::engine_client::EngineClient;
#[cfg(feature = "redis-cache")]
use crate::infrastructure::cache::redis_client::RedisClient;
use crate::queue::task_queue::TaskQueue;
use crate::utils::regex_cache::RegexCache;
use crate::workers::expiration_worker::ExpirationWorker;
use crate::workers::scrape_worker::ScrapeWorker;
use crate::workers::{AbstractWorker, Worker};
use std::sync::Arc;
use tokio::signal;
use tokio::task::JoinHandle;
use tracing::{error, info};

use crate::config::settings::Settings;
use crate::utils::robots::RobotsCheckerTrait;

/// 工作管理器
pub struct WorkerManager {
    queue: Arc<dyn TaskQueue>,
    repository: Arc<dyn TaskRepository>,
    result_repository: Arc<dyn ScrapeResultRepository>,
    crawl_repository: Arc<dyn CrawlRepository>,
    storage_repository: Option<Arc<dyn StorageRepository + Send + Sync>>,
    webhook_event_repository: Arc<dyn WebhookEventRepository + Send + Sync>,
    credits_repository: Arc<dyn CreditsRepository>,
    engine_client: Arc<EngineClient>,
    create_scrape_use_case: Arc<CreateScrapeUseCase>,
    redis: RedisClient,
    robots_checker: Arc<dyn RobotsCheckerTrait>,
    settings: Arc<Settings>,
    default_concurrency_limit: usize,
    handles: Vec<JoinHandle<()>>,
    llm_service: LLMService,
    regex_cache: RegexCache,
}

/// Worker Manager Dependencies
pub struct WorkerManagerDeps {
    pub queue: Arc<dyn TaskQueue>,
    pub repository: Arc<dyn TaskRepository>,
    pub result_repository: Arc<dyn ScrapeResultRepository>,
    pub crawl_repository: Arc<dyn CrawlRepository>,
    pub storage_repository: Option<Arc<dyn StorageRepository + Send + Sync>>,
    pub webhook_event_repository: Arc<dyn WebhookEventRepository + Send + Sync>,
    pub credits_repository: Arc<dyn CreditsRepository>,
    pub engine_client: Arc<EngineClient>,
    pub create_scrape_use_case: Arc<CreateScrapeUseCase>,
    pub redis: RedisClient,
    pub robots_checker: Arc<dyn RobotsCheckerTrait>,
    pub http_client: Arc<reqwest::Client>,
    pub llm_service: LLMService,
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
            webhook_event_repository: deps.webhook_event_repository,
            credits_repository: deps.credits_repository,
            engine_client: deps.engine_client,
            create_scrape_use_case: deps.create_scrape_use_case,
            redis: deps.redis,
            robots_checker: deps.robots_checker,
            settings: config.settings,
            default_concurrency_limit: config.default_concurrency_limit,
            handles: Vec::new(),
            llm_service: deps.llm_service,
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
                self.webhook_event_repository.clone(),
                self.credits_repository.clone(),
                self.engine_client.clone(),
                self.create_scrape_use_case.clone(),
                self.redis.clone(),
                self.robots_checker.clone(),
                self.settings.clone(),
                self.default_concurrency_limit,
                self.llm_service.clone(),
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
