// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! ScrapeWorker 依赖聚合参数对象
//!
//! 将 `ScrapeWorker::new()` 的 15+ 个参数聚合为单一结构体，
//! 消除 `too_many_arguments` clippy 告警，提高可维护性。

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
use crate::utils::regex_cache::RegexCache;
use crate::utils::robots::RobotsCheckerTrait;
use crate::workers::coalesce_coordinator::CoalesceCoordinator;
#[cfg(feature = "metrics")]
use crate::workers::scheduler::memory_scheduler::MemoryScheduler;
use std::sync::Arc;

/// ScrapeWorker 构造依赖集合
///
/// 聚合 `ScrapeWorker::new()` 所需的全部外部依赖，
/// 由 `ScrapeWorkerBuilder::build()` 或 `WorkerManager` 构造后传入。
pub struct ScrapeWorkerDeps {
    pub repository: Arc<dyn TaskRepository>,
    pub result_repository: Arc<dyn ScrapeResultRepository>,
    pub crawl_repository: Arc<dyn CrawlRepository>,
    pub webhook_service: Arc<dyn WebhookService>,
    pub credits_repository: Arc<dyn CreditsRepository>,
    pub engine_client: Arc<EngineClient>,
    pub create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait>,
    pub team_semaphore: Arc<TeamSemaphore>,
    pub coalesce_coordinator: Arc<CoalesceCoordinator>,
    pub robots_checker: Arc<dyn RobotsCheckerTrait>,
    pub settings: Arc<Settings>,
    pub default_concurrency_limit: usize,
    pub extraction_service: Arc<dyn ExtractionServiceTrait>,
    pub regex_cache: RegexCache,
    pub cache_service: Arc<dyn CacheService>,
    /// 内存感知调度器（仅 metrics 特性启用时存在）
    #[cfg(feature = "metrics")]
    pub memory_scheduler: Arc<MemoryScheduler>,
}
