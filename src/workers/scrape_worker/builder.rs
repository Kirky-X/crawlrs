// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! ScrapeWorker Builder
//!
//! Provides a fluent API for constructing ScrapeWorker instances.

use super::ScrapeWorker;
use super::ScrapeWorkerDeps;
use crate::application::use_cases::create_scrape::CreateScrapeUseCaseTrait;
use crate::config::settings::Settings;
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::services::extraction_service::ExtractionServiceTrait;
use crate::domain::services::team_semaphore::TeamSemaphore;
use crate::domain::services::webhook_service::WebhookService;
use crate::engines::engine_client::EngineClient;
use crate::infrastructure::oxcache::CacheService;
use crate::utils::coalesce::RequestCoalescer;
use crate::utils::dedup::Deduplicator;
use crate::utils::regex_cache::RegexCache;
use crate::utils::robots::RobotsCheckerTrait;
use crate::workers::coalesce_coordinator::CoalesceCoordinator;
#[cfg(feature = "metrics")]
use crate::workers::scheduler::memory_scheduler::MemoryScheduler;
use crate::workers::shutdown::ShutdownCoordinator;
use std::sync::Arc;

/// ScrapeWorker 构建器
///
/// 使用 builder 模式逐步配置 ScrapeWorker 的各项依赖，
/// 最终调用 `build()` 创建实例。
pub struct ScrapeWorkerBuilder {
    repository: Option<Arc<dyn TaskRepository>>,
    result_repository: Option<Arc<dyn ScrapeResultRepository>>,
    crawl_repository: Option<Arc<dyn CrawlRepository>>,
    webhook_service: Option<Arc<dyn WebhookService>>,
    credits_repository: Option<Arc<dyn CreditsRepository>>,
    engine_client: Option<Arc<EngineClient>>,
    create_scrape_use_case: Option<Arc<dyn CreateScrapeUseCaseTrait>>,
    team_semaphore: Option<Arc<TeamSemaphore>>,
    robots_checker: Option<Arc<dyn RobotsCheckerTrait>>,
    settings: Option<Arc<Settings>>,
    default_concurrency_limit: usize,
    extraction_service: Option<Arc<dyn ExtractionServiceTrait>>,
    regex_cache: Option<RegexCache>,
    /// 内存感知调度器（T019/R-runtime-001），仅 metrics 特性启用时存在
    #[cfg(feature = "metrics")]
    memory_scheduler: Option<Arc<MemoryScheduler>>,
    /// URL 分层去重器（T053/R-frontier-001），可选注入
    ///
    /// 不设置时使用 `Deduplicator::new()`（默认配置：保留 query，1M 容量）
    deduplicator: Option<Arc<parking_lot::RwLock<Deduplicator>>>,
    /// 高级缓存服务（T059/R-cache-002，必需）
    ///
    /// 由 `WorkerManager` 从 `InfrastructureComponents.cache_service` 注入。
    cache_service: Option<Arc<dyn CacheService>>,
    /// 优雅退出协调器（R-security-004/005，可选）
    ///
    /// 不设置时使用独立默认实例（`ShutdownCoordinator::default()`）。
    shutdown_coordinator: Option<Arc<ShutdownCoordinator>>,
}

impl Default for ScrapeWorkerBuilder {
    fn default() -> Self {
        Self {
            repository: None,
            result_repository: None,
            crawl_repository: None,
            webhook_service: None,
            credits_repository: None,
            engine_client: None,
            create_scrape_use_case: None,
            team_semaphore: None,
            robots_checker: None,
            settings: None,
            default_concurrency_limit: 10,
            extraction_service: None,
            regex_cache: None,
            #[cfg(feature = "metrics")]
            memory_scheduler: None,
            deduplicator: None,
            cache_service: None,
            shutdown_coordinator: None,
        }
    }
}

impl ScrapeWorkerBuilder {
    /// 创建新的构建器
    pub fn new() -> Self {
        Self::default()
    }

    /// 获取默认并发限制
    pub fn default_concurrency_limit(&self) -> usize {
        self.default_concurrency_limit
    }

    /// 设置任务仓储 (必需)
    pub fn with_repository(mut self, repository: Arc<dyn TaskRepository>) -> Self {
        self.repository = Some(repository);
        self
    }

    /// 设置结果仓储 (必需)
    pub fn with_result_repository(
        mut self,
        result_repository: Arc<dyn ScrapeResultRepository>,
    ) -> Self {
        self.result_repository = Some(result_repository);
        self
    }

    /// 设置爬取仓储 (必需)
    pub fn with_crawl_repository(mut self, crawl_repository: Arc<dyn CrawlRepository>) -> Self {
        self.crawl_repository = Some(crawl_repository);
        self
    }

    /// 设置 Webhook 服务 (必需)
    pub fn with_webhook_service(mut self, webhook_service: Arc<dyn WebhookService>) -> Self {
        self.webhook_service = Some(webhook_service);
        self
    }

    /// 设置积分仓储 (必需)
    pub fn with_credits_repository(
        mut self,
        credits_repository: Arc<dyn CreditsRepository>,
    ) -> Self {
        self.credits_repository = Some(credits_repository);
        self
    }

    /// 设置引擎客户端 (必需)
    pub fn with_engine_client(mut self, engine_client: Arc<EngineClient>) -> Self {
        self.engine_client = Some(engine_client);
        self
    }

    /// 设置创建抓取用例 (必需)
    pub fn with_create_scrape_use_case(
        mut self,
        create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait>,
    ) -> Self {
        self.create_scrape_use_case = Some(create_scrape_use_case);
        self
    }

    /// 设置团队信号量 (必需)
    pub fn with_team_semaphore(mut self, team_semaphore: Arc<TeamSemaphore>) -> Self {
        self.team_semaphore = Some(team_semaphore);
        self
    }

    /// 设置 Robots 检查器 (必需)
    pub fn with_robots_checker(mut self, robots_checker: Arc<dyn RobotsCheckerTrait>) -> Self {
        self.robots_checker = Some(robots_checker);
        self
    }

    /// 设置配置 (必需)
    pub fn with_settings(mut self, settings: Arc<Settings>) -> Self {
        self.settings = Some(settings);
        self
    }

    /// 设置默认并发限制
    pub fn with_default_concurrency_limit(mut self, limit: usize) -> Self {
        self.default_concurrency_limit = limit;
        self
    }

    /// 设置提取服务 (必需)
    pub fn with_extraction_service(
        mut self,
        extraction_service: Arc<dyn ExtractionServiceTrait>,
    ) -> Self {
        self.extraction_service = Some(extraction_service);
        self
    }

    /// 设置正则缓存 (必需)
    pub fn with_regex_cache(mut self, regex_cache: RegexCache) -> Self {
        self.regex_cache = Some(regex_cache);
        self
    }

    /// 设置高级缓存服务（T059/R-cache-002，必需）
    ///
    /// 由 `WorkerManager` 从 `InfrastructureComponents.cache_service` 注入，
    /// 用于 `process_scrape_task` 读写抓取结果缓存。
    pub fn with_cache_service(mut self, cache_service: Arc<dyn CacheService>) -> Self {
        self.cache_service = Some(cache_service);
        self
    }

    /// 设置内存感知调度器（T019/R-runtime-001，metrics 特性启用时必需）
    #[cfg(feature = "metrics")]
    pub fn with_memory_scheduler(mut self, memory_scheduler: Arc<MemoryScheduler>) -> Self {
        self.memory_scheduler = Some(memory_scheduler);
        self
    }

    /// 设置 URL 分层去重器（T053/R-frontier-001，可选）
    ///
    /// 不调用时使用默认 `Deduplicator::new()`。生产环境推荐由 `WorkerManager`
    /// 创建一个共享实例注入所有 worker，最大化 Bloom 预筛效果。
    pub fn with_deduplicator(
        mut self,
        deduplicator: Arc<parking_lot::RwLock<Deduplicator>>,
    ) -> Self {
        self.deduplicator = Some(deduplicator);
        self
    }

    /// 设置优雅退出协调器（R-security-004/005，可选）
    ///
    /// 由 `WorkerManager` 注入共享实例，使所有 worker 在关闭信号到达时
    /// 协同退出。不设置时使用独立默认实例。
    pub fn with_shutdown_coordinator(
        mut self,
        shutdown_coordinator: Arc<ShutdownCoordinator>,
    ) -> Self {
        self.shutdown_coordinator = Some(shutdown_coordinator);
        self
    }

    /// 构建 ScrapeWorker 实例
    pub fn build(self) -> Result<ScrapeWorker, &'static str> {
        let repository = self.repository.ok_or("repository is required")?;
        let result_repository = self
            .result_repository
            .ok_or("result_repository is required")?;
        let crawl_repository = self
            .crawl_repository
            .ok_or("crawl_repository is required")?;
        let webhook_service = self.webhook_service.ok_or("webhook_service is required")?;
        let credits_repository = self
            .credits_repository
            .ok_or("credits_repository is required")?;
        let engine_client = self.engine_client.ok_or("engine_client is required")?;
        let create_scrape_use_case = self
            .create_scrape_use_case
            .ok_or("create_scrape_use_case is required")?;
        let team_semaphore = self.team_semaphore.ok_or("team_semaphore is required")?;
        let robots_checker = self.robots_checker.ok_or("robots_checker is required")?;
        let settings = self.settings.ok_or("settings is required")?;
        let extraction_service = self
            .extraction_service
            .ok_or("extraction_service is required")?;
        let regex_cache = self.regex_cache.ok_or("regex_cache is required")?;
        let cache_service = self.cache_service.ok_or("cache_service is required")?;
        #[cfg(feature = "metrics")]
        let memory_scheduler = self
            .memory_scheduler
            .ok_or("memory_scheduler is required (metrics feature enabled)")?;
        let shutdown_coordinator = self
            .shutdown_coordinator
            .unwrap_or_else(|| Arc::new(ShutdownCoordinator::default()));

        // H-4 职责拆分：构造 CoalesceCoordinator（共享 repository + result_repository + 新建 coalescer）
        let request_coalescer = Arc::new(RequestCoalescer::new());
        let coalesce_coordinator = Arc::new(CoalesceCoordinator::new(
            repository.clone(),
            result_repository.clone(),
            request_coalescer,
        ));

        Ok(ScrapeWorker::new(ScrapeWorkerDeps {
            repository,
            result_repository,
            crawl_repository,
            webhook_service,
            credits_repository,
            engine_client,
            create_scrape_use_case,
            team_semaphore,
            coalesce_coordinator,
            robots_checker,
            settings,
            default_concurrency_limit: self.default_concurrency_limit,
            extraction_service,
            regex_cache,
            cache_service,
            #[cfg(feature = "metrics")]
            memory_scheduler,
        })
        .with_shutdown_coordinator(shutdown_coordinator)
        .with_deduplicator_opt(self.deduplicator))
    }
}
