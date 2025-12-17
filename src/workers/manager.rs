// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::application::usecases::create_scrape::CreateScrapeUseCase;
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::storage_repository::StorageRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crate::engines::router::EngineRouter;
use crate::infrastructure::cache::redis_client::RedisClient;
use crate::queue::task_queue::TaskQueue;
use crate::workers::scrape_worker::ScrapeWorker;
use std::sync::Arc;
use tokio::signal;
use tokio::task::JoinHandle;
use tracing::{error, info};

use crate::utils::robots::RobotsChecker;

/// 工作管理器
pub struct WorkerManager<Q, R, S, C, ST>
where
    Q: TaskQueue + 'static,
    R: TaskRepository + 'static,
    S: ScrapeResultRepository + 'static,
    C: CrawlRepository + 'static,
    ST: StorageRepository + 'static,
{
    queue: Arc<Q>,
    repository: Arc<R>,
    result_repository: Arc<S>,
    crawl_repository: Arc<C>,
    storage_repository: Option<Arc<ST>>,
    webhook_event_repository: Arc<dyn WebhookEventRepository + Send + Sync>,
    router: Arc<EngineRouter>,
    create_scrape_use_case: Arc<CreateScrapeUseCase>,
    redis: RedisClient,
    robots_checker: Arc<RobotsChecker>,
    default_concurrency_limit: usize,
    handles: Vec<JoinHandle<()>>,
}

impl<Q, R, S, C, ST> WorkerManager<Q, R, S, C, ST>
where
    Q: TaskQueue + Send + Sync,
    R: TaskRepository + Send + Sync,
    S: ScrapeResultRepository + Send + Sync,
    C: CrawlRepository + Send + Sync,
    ST: StorageRepository + Send + Sync,
{
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        queue: Arc<Q>,
        repository: Arc<R>,
        result_repository: Arc<S>,
        crawl_repository: Arc<C>,
        storage_repository: Option<Arc<ST>>,
        webhook_repository: Arc<dyn WebhookEventRepository + Send + Sync>,
        router: Arc<EngineRouter>,
        create_scrape_use_case: Arc<CreateScrapeUseCase>,
        redis: RedisClient,
        robots_checker: Arc<RobotsChecker>,
        default_concurrency_limit: usize,
    ) -> Self {
        Self {
            queue,
            repository,
            result_repository,
            crawl_repository,
            storage_repository,
            webhook_event_repository: webhook_repository,
            router,
            create_scrape_use_case,
            redis,
            robots_checker,
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
        for _ in 0..count {
            let worker = ScrapeWorker::new(
                self.repository.clone(),
                self.result_repository.clone(),
                self.crawl_repository.clone(),
                self.storage_repository.clone(),
                self.webhook_event_repository.clone(),
                self.router.clone(),
                self.create_scrape_use_case.clone(),
                self.redis.clone(),
                self.robots_checker.clone(),
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
