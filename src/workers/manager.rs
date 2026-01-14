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
use crate::engines::engine_client::EngineClient;
#[cfg(feature = "redis-cache")]
use crate::infrastructure::cache::redis_client::RedisClient;
use crate::queue::task_queue::TaskQueue;
use crate::workers::expiration_worker::ExpirationWorker;
use crate::workers::scrape_worker::ScrapeWorker;
use crate::workers::{AbstractWorker, Worker};
use std::sync::Arc;
use tokio::signal;
use tokio::task::JoinHandle;
use tracing::{error, info};

use crate::config::settings::Settings;
use crate::utils::robots::RobotsChecker;

/// 工作管理器
pub struct WorkerManager<Q, R, S, C, CRR>
where
    Q: TaskQueue + Clone + Send + Sync + 'static,
    R: TaskRepository + Send + Sync + 'static,
    S: ScrapeResultRepository + Send + Sync + 'static,
    C: CrawlRepository + Send + Sync + 'static,
    CRR: CreditsRepository + Send + Sync + 'static,
{
    queue: Q,
    repository: Arc<R>,
    result_repository: Arc<S>,
    crawl_repository: Arc<C>,
    storage_repository: Option<Arc<dyn StorageRepository + Send + Sync>>,
    webhook_event_repository: Arc<dyn WebhookEventRepository + Send + Sync>,
    credits_repository: Arc<CRR>,
    engine_client: Arc<EngineClient>,
    create_scrape_use_case: Arc<CreateScrapeUseCase>,
    redis: RedisClient,
    robots_checker: Arc<RobotsChecker>,
    settings: Arc<Settings>,
    default_concurrency_limit: usize,
    handles: Vec<JoinHandle<()>>,
}

impl<Q, R, S, C, CRR> WorkerManager<Q, R, S, C, CRR>
where
    Q: TaskQueue + Clone + Send + Sync + 'static,
    R: TaskRepository + Send + Sync + 'static,
    S: ScrapeResultRepository + Send + Sync + 'static,
    C: CrawlRepository + Send + Sync + 'static,
    CRR: CreditsRepository + Send + Sync + 'static,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        queue: Q,
        repository: Arc<R>,
        result_repository: Arc<S>,
        crawl_repository: Arc<C>,
        storage_repository: Option<Arc<dyn StorageRepository + Send + Sync>>,
        webhook_event_repository: Arc<dyn WebhookEventRepository + Send + Sync>,
        credits_repository: Arc<CRR>,
        engine_client: Arc<EngineClient>,
        create_scrape_use_case: Arc<CreateScrapeUseCase>,
        redis: RedisClient,
        robots_checker: Arc<RobotsChecker>,
        settings: Arc<Settings>,
        default_concurrency_limit: usize,
    ) -> Self {
        Self {
            queue,
            repository,
            result_repository,
            crawl_repository,
            storage_repository,
            webhook_event_repository,
            credits_repository,
            engine_client,
            create_scrape_use_case,
            redis,
            robots_checker,
            settings,
            default_concurrency_limit,
            handles: Vec::new(),
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

impl<Q, R, S, C, CRR> Drop for WorkerManager<Q, R, S, C, CRR>
where
    Q: TaskQueue + Clone + Send + Sync + 'static,
    R: TaskRepository + Send + Sync + 'static,
    S: ScrapeResultRepository + Send + Sync + 'static,
    C: CrawlRepository + Send + Sync + 'static,
    CRR: CreditsRepository + Send + Sync + 'static,
{
    fn drop(&mut self) {
        // Abort all worker handles to prevent them from running after the manager is dropped
        for handle in &self.handles {
            handle.abort();
        }
    }
}
