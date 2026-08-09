// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use anyhow::{Context, Result};
use chrono::Utc;
use dashmap::DashMap;
use log::{debug, error, info, warn};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::OwnedSemaphorePermit;
use tokio::time::sleep;
use uuid::Uuid;

use crate::application::dto::crawl_request::CrawlConfigDto;
use crate::application::dto::scrape_request::ScrapeRequestDto;
use crate::application::use_cases::create_scrape::CreateScrapeUseCaseTrait;
use crate::config::settings::Settings;
use crate::domain::models::{Task, TaskStatus};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::services::extraction_service::ExtractionServiceTrait;
use crate::domain::services::retry_handler::RetryHandler;
use crate::domain::services::webhook_service::WebhookService;
use crate::utils::regex_cache::RegexCache;
// T053/R-frontier-001：URL 分层去重器（Bloom 预筛 + DB 保权威）
use crate::utils::dedup::Deduplicator;

use crate::common::CacheContext;
use crate::common::CacheMode;
use crate::common::HttpMethod;
use crate::domain::services::team_semaphore::TeamSemaphore;
use crate::engines::engine_client::{
    EngineClient, PageAction, ScrapeOptions, ScrapeRequest, ScrapeResponse, ScreenshotConfig,
    ScrollDirection,
};
use crate::infrastructure::oxcache::CacheService;
use crate::presentation::helpers::ssrf::is_internal_url;
use crate::queue::task_queue::TaskQueue;
// H-4 职责拆分：请求合并协调器（替代原 request_coalescer 字段 + try_coalesce 方法）
use crate::workers::coalesce_coordinator::CoalesceCoordinator;
// R-security-004/005：优雅退出协调器（design.md D3，T007/T008）
use crate::workers::shutdown::ShutdownCoordinator;
// HIGH-2 SRP 拆分：cache key 生成、URL 脱敏
use crate::workers::cache_utils::{self, redact_url_for_log};
// H-4 职责拆分：Markdown 后处理器（gated `markdown` 特性，替代原 maybe_generate_markdown 方法）
#[cfg(feature = "content")]
use crate::workers::markdown_post_processor::MarkdownPostProcessor;
// T074/R-content-001：正文提取门面（与 markdown 特性配合，only_main_content 前置提取）
#[cfg(feature = "content")]
use crate::domain::services::content_extractor::ContentExtractionFacade;
use crate::utils::retry_policy::RetryPolicy;
// T028/R-identity-002：重试指令与分类器（消费 RetryTracker + RetryDirective）
use crate::utils::retry::{RetryDirective, RetryTracker};
// T067/R-frontier-004：自适应爬取停止条件
use crate::utils::robots::RobotsCheckerTrait;
// T019（R-runtime-001）：内存感知调度器接入 scrape_worker
// MemoryScheduler 依赖 SystemMonitorTrait（metrics 特性门控），故整块接入由 metrics 门控
#[cfg(feature = "metrics")]
use crate::workers::scheduler::memory_scheduler::{Admission, MemoryScheduler};

// T026 拆分：提取到独立模块的函数导入
pub(super) use super::crawl_link_extractor::{
    check_robots_txt as check_robots_txt_fn, extract_and_queue_links as extract_and_queue_links_fn,
    update_crawl_completion_status as update_crawl_completion_status_fn,
};
pub(super) use super::scrape_executor::{
    process_text_encoding, save_result, try_read_scrape_cache, try_write_scrape_cache,
};
pub(super) use super::scrape_response_builder::{
    build_crawl_request as build_crawl_request_fn,
    build_extract_request as build_extract_request_fn, parse_crawl_payload, parse_extract_payload,
};

// Test-only imports for cfg(test) wrappers and functions
#[cfg(test)]
use crate::application::dto::extract_request::ExtractRequestDto;
#[cfg(test)]
use crate::workers::errors::ScrapeWorkerError;

/// 从缓存获取正则表达式
///
/// T066 后 `should_crawl` 委托 `UrlPatternFilter`（内部自管 regex 缓存），
/// 此函数仅保留供测试验证 `RegexCache` 行为，标记 `#[cfg(test)]` 避免生产死代码。
#[cfg(test)]
fn get_cached_regex(pattern: &str, cache: &RegexCache) -> Result<regex::Regex, ScrapeWorkerError> {
    cache
        .get_or_insert(pattern)
        .map_err(ScrapeWorkerError::RegexError)
}

/// 抓取工作者
pub struct ScrapeWorker {
    repository: Arc<dyn TaskRepository>,
    result_repository: Arc<dyn ScrapeResultRepository>,
    crawl_repository: Arc<dyn CrawlRepository>,
    webhook_service: Arc<dyn WebhookService>,
    credits_repository: Arc<dyn CreditsRepository>,
    engine_client: Arc<EngineClient>,
    _create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait>,
    team_semaphore: Arc<TeamSemaphore>,
    /// 请求合并协调器（H-4 职责拆分，T035/R-runtime-002）
    ///
    /// 同 URL 并发请求只允许首个执行实际抓取，其余 worker 等待广播后从
    /// `result_repo` 读取结果，避免重复网络往返。所有 worker 共享同一实例
    /// （由 `WorkerManager` 从 `ServicesComponents.request_coalescer` +
    /// `repository` + `result_repository` 构造注入）。
    coalesce_coordinator: Arc<CoalesceCoordinator>,
    /// Markdown 后处理器（H-4 职责拆分，T042/R-content-001）
    ///
    /// 无状态服务，根据任务 `formats` 字段判断是否生成 Markdown。
    /// gated `markdown` 特性：关闭时本字段不存在，相关分支也不编译。
    #[cfg(feature = "content")]
    markdown_post_processor: MarkdownPostProcessor,
    token_usage: Arc<DashMap<Uuid, AtomicI64>>,
    robots_checker: Arc<dyn RobotsCheckerTrait>,
    settings: Arc<Settings>,
    worker_id: Uuid,
    default_concurrency_limit: usize,
    retry_handler: RetryHandler,
    extraction_service: Arc<dyn ExtractionServiceTrait>,
    /// T066 后 `should_crawl` 委托 `UrlPatternFilter`，此字段不再被生产代码读取。
    /// 保留以维持 builder API 兼容（`with_regex_cache` + 构造器签名），
    /// 全面移除需更新 30+ 调用点，作为独立重构任务处理。
    #[allow(dead_code)]
    regex_cache: RegexCache,
    /// 内存感知调度器（T019/R-runtime-001）
    ///
    /// `metrics` 启用时由 `WorkerManager` 注入；`process_task` 在获取并发许可前
    /// 调用 `admit()`，Pressure 时延后、Critical 时重排到 backlog。
    #[cfg(feature = "metrics")]
    memory_scheduler: Arc<MemoryScheduler>,
    /// URL 分层去重器（T053/R-frontier-001）
    ///
    /// UrlNormalizer + Bloom + HashSet 三层组合，用于 `extract_and_queue_links`
    /// 预筛 URL 是否已爬：
    /// - Bloom 阴性 → 绝对新，直接入队并 insert
    /// - Bloom 阳性 → 可能已爬，回落 `find_existing_urls` DB 校验（保权威）
    ///
    /// `RwLock` 因为 `extract_and_queue_links` 是 `&self`，但 bloom insert 需 `&mut`。
    /// `Arc` 是为后续支持跨 worker 共享（当前每 worker 独立实例）。
    deduplicator: Arc<parking_lot::RwLock<Deduplicator>>,
    /// 高级缓存服务（T059/R-cache-002）
    ///
    /// 由 `WorkerManager` 从 `InfrastructureComponents.cache_service` 注入，
    /// 所有 worker 共享同一实例。`process_scrape_task` 在读写缓存前经
    /// `CacheContext` 门控：`is_cacheable() && should_read()` 查缓存命中直返，
    /// `is_cacheable() && should_write()` 抓取成功后写回。
    cache_service: Arc<dyn CacheService>,
    /// 优雅退出协调器（R-security-004/005，design.md D3）
    ///
    /// 由 `WorkerManager` 注入，所有 worker 共享同一实例。`run()` 循环开头
    /// 检查 `is_shutting_down()`，置位后不再 acquire 新任务，完成当前任务即退出。
    shutdown_coordinator: Arc<ShutdownCoordinator>,
}

impl std::fmt::Debug for ScrapeWorker {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ScrapeWorker")
            .field("worker_id", &self.worker_id)
            .field("default_concurrency_limit", &self.default_concurrency_limit)
            .finish_non_exhaustive()
    }
}

impl ScrapeWorker {
    /// 创建新的抓取工作器实例
    ///
    /// 接受 `ScrapeWorkerDeps` 参数对象，聚合所有外部依赖。
    /// 由 `ScrapeWorkerBuilder::build()` 或 `WorkerManager` 构造传入。
    pub fn new(deps: ScrapeWorkerDeps) -> Self {
        // 根据任务类型选择合适的重试策略
        let retry_policy = RetryPolicy::slow(); // 网络请求适合慢速重试策略
        let retry_handler = RetryHandler::new(deps.repository.clone(), retry_policy.clone());

        Self {
            repository: deps.repository,
            result_repository: deps.result_repository,
            crawl_repository: deps.crawl_repository,
            webhook_service: deps.webhook_service,
            credits_repository: deps.credits_repository,
            engine_client: deps.engine_client,
            _create_scrape_use_case: deps.create_scrape_use_case,
            team_semaphore: deps.team_semaphore,
            coalesce_coordinator: deps.coalesce_coordinator,
            #[cfg(feature = "content")]
            markdown_post_processor: MarkdownPostProcessor::new(
                Arc::new(crate::domain::services::markdown_service::HtmdMarkdownService::new()),
                Some(Arc::new(ContentExtractionFacade::new(None))),
            ),
            token_usage: Arc::new(DashMap::new()),
            robots_checker: deps.robots_checker,
            settings: deps.settings,
            worker_id: Uuid::new_v4(),
            default_concurrency_limit: deps.default_concurrency_limit,
            retry_handler,
            extraction_service: deps.extraction_service,
            regex_cache: deps.regex_cache,
            #[cfg(feature = "metrics")]
            memory_scheduler: deps.memory_scheduler,
            // T053/R-frontier-001：默认每 worker 独立 Deduplicator
            // 后续可由 WorkerManager 通过 Builder 注入共享实例优化 DB 查询量
            deduplicator: Arc::new(parking_lot::RwLock::new(Deduplicator::new())),
            cache_service: deps.cache_service,
            // R-security-004/005：默认独立协调器，由 WorkerManager 通过
            // `with_shutdown_coordinator` 注入共享实例。
            shutdown_coordinator: Arc::new(ShutdownCoordinator::default()),
        }
    }

    /// 注入优雅退出协调器（R-security-004/005）
    ///
    /// 由 `WorkerManager` 在构造后调用，使所有 worker 共享同一实例；
    /// 关闭信号到达后 `run()` 循环检查 `is_shutting_down()` 协同退出。
    pub fn with_shutdown_coordinator(mut self, coordinator: Arc<ShutdownCoordinator>) -> Self {
        self.shutdown_coordinator = coordinator;
        self
    }

    /// 运行抓取工作器
    pub async fn run(&self, queue: Arc<dyn TaskQueue>) {
        info!("Scrape worker {} started", self.worker_id);

        loop {
            // R-security-004：优雅退出检查（design.md D3，T008）
            //
            // flag 置位后不再 acquire 新任务；若当前有任务正在执行，
            // 该任务完成返回后本循环即退出（不抢占正在执行的任务）。
            if self.shutdown_coordinator.is_shutting_down() {
                info!(
                    "Scrape worker {} received shutdown, exiting after current task",
                    self.worker_id
                );
                break;
            }

            match self.process_next_task(&queue).await {
                Ok(processed) => {
                    if !processed {
                        sleep(Duration::from_secs(1)).await;
                    }
                }
                Err(e) => {
                    error!("Error processing task: {}", e);
                    sleep(Duration::from_secs(1)).await;
                }
            }
        }
    }

    /// Builder 内部使用：替换 deduplicator 字段
    ///
    /// 用于 `ScrapeWorkerBuilder::build` 在调用 `ScrapeWorker::new`（内部默认
    /// 初始化 deduplicator）后，注入外部共享实例。`None` 保留默认实例。
    pub(crate) fn with_deduplicator_opt(
        mut self,
        dedup: Option<Arc<parking_lot::RwLock<Deduplicator>>>,
    ) -> Self {
        if let Some(d) = dedup {
            self.deduplicator = d;
        }
        self
    }

    /// 测试 helper：获取 deduplicator 引用（仅 `#[cfg(test)]` 可用）
    ///
    /// 用于单元测试预填充 Bloom，模拟"URL 已爬"场景：
    /// ```ignore
    /// let worker = build_mock_worker().await;
    /// {
    ///     let mut dedup = worker.deduplicator_for_test().write();
    ///     dedup.insert("https://example.com/page1");
    /// }
    /// // 现在 page1 在 Bloom 中阳性，调用 extract_and_queue_links 时
    /// // 会走 find_existing_urls DB 校验路径
    /// ```
    #[cfg(test)]
    pub(crate) fn deduplicator_for_test(&self) -> Arc<parking_lot::RwLock<Deduplicator>> {
        self.deduplicator.clone()
    }

    async fn process_next_task(&self, queue: &dyn TaskQueue) -> Result<bool> {
        let task_opt = queue.dequeue(self.worker_id).await?;

        if let Some(task) = task_opt {
            self.process_task(task).await?;
            return Ok(true);
        }

        Ok(false)
    }

    fn acquire_concurrency_permit(&self, task: &Task) -> Option<OwnedSemaphorePermit> {
        self.team_semaphore.try_acquire(task.team_id)
    }

    async fn process_task(&self, mut task: Task) -> Result<()> {
        debug!(
            "process_task: task_id={}, url={}, task_type={}",
            task.id, task.url, task.task_type
        );
        info!("Processing task");

        // Check Task Expiration
        if let Some(expires_at) = task.expires_at {
            if Utc::now() > expires_at {
                warn!("Task {} expired at {}", task.id, expires_at);
                self.repository.mark_failed(task.id).await?;
                // Trigger failure webhook if needed
                self.trigger_webhook(&task, Some("Task expired".to_string()))
                    .await;
                return Ok(());
            }
        }

        // T019（R-runtime-001）：内存感知准入检查
        //
        // 在获取并发许可（Team Semaphore）之前先采样内存状态并决定是否放行：
        // - Normal  → Proceed：进入并发获取流程
        // - Pressure → Defer：复用现有 backlog 重排逻辑延后（design.md §5：
        //   不新建持久队列，复用 scheduled_at + Queued 状态机）
        // - Critical → Reschedule：同样经 backlog 重排，但语义为拒绝当前批次
        //
        // 采样策略：每次 process_task 调用 `update_state()` 读取最新内存使用率。
        // 这与 `MemoryScheduler::spawn_monitor` 的后台 1s 采样互补——后台采样驱动
        // 优雅关闭信号，此处 per-task 采样保证 admit() 返回最新决策。
        #[cfg(feature = "metrics")]
        {
            self.memory_scheduler.update_state();
            match self.memory_scheduler.admit().await {
                Admission::Proceed => {}
                Admission::Defer => {
                    warn!(
                        "Memory pressure detected, deferring task {} (team_id={}) via backlog reschedule",
                        task.id, task.team_id
                    );
                    // 复用现有 backlog 重排逻辑：延后 30 秒重新入队
                    task.scheduled_at = Some(Utc::now() + chrono::Duration::seconds(30));
                    task.status = TaskStatus::Queued;
                    self.repository.update(&task).await?;
                    return Ok(());
                }
                Admission::Reschedule => {
                    warn!(
                        "Memory critical detected, rescheduling task {} (team_id={}) to backlog",
                        task.id, task.team_id
                    );
                    // Critical 同样复用 backlog 重排逻辑（design.md：不新建持久队列）
                    task.scheduled_at = Some(Utc::now() + chrono::Duration::seconds(30));
                    task.status = TaskStatus::Queued;
                    self.repository.update(&task).await?;
                    return Ok(());
                }
            }
        }

        // Concurrency Check (Layer 2: Team Semaphore)
        // The permit is held for the duration of task processing and auto-releases on drop.
        let _permit = match self.acquire_concurrency_permit(&task) {
            Some(p) => p,
            None => {
                warn!(
                    "Team {} concurrency limit exceeded, rescheduling task {}",
                    task.team_id, task.id
                );
                // Reschedule logic (Backlog)
                // Delay by 30 seconds
                task.scheduled_at = Some(Utc::now() + chrono::Duration::seconds(30));
                task.status = TaskStatus::Queued;
                self.repository.update(&task).await?;
                return Ok(());
            }
        };

        let task_type = task.task_type;

        // Take task by value only for the specific branch that needs it
        // This avoids 3 unnecessary clones in the match
        let result = match task_type.as_str() {
            "scrape" => self.process_scrape_task(task).await,
            "crawl" => self.process_crawl_task(task).await,
            "extract" => self.process_extract_task(task).await,
            _ => return Err(anyhow::anyhow!("Unknown task type: {}", task_type)),
        };

        // _permit auto-releases here when it goes out of scope

        if let Err(ref e) = result {
            debug!("error: {}", e);
        } else {
            debug!("Task processing completed successfully");
        }

        result
    }

    async fn process_scrape_task(&self, mut task: Task) -> Result<()> {
        debug!("task_id: {}", task.id);

        // PERF-H1 修复：解析一次 payload，dto 与 ScrapeRequest 复用同一份解析结果。
        //
        // 旧实现：
        //   1) build_scrape_request(&task) 内部 from_value(payload.clone()) 得 dto
        //   2) handle_scrape_success(&task, &response) 内部再 from_value(payload.clone()) 得 dto
        // → 每次抓取 clone + 解析 payload 2 次。
        //
        // 新实现：
        //   1) parse_scrape_request_dto(&task) 得 dto（仅 1 次 clone + 解析）
        //   2) build_scrape_request_from_dto(&dto) 复用 dto 构造 ScrapeRequest（零额外解析）
        //   3) handle_scrape_success(&task, dto.as_ref(), &response) 复用 dto 引用（零额外解析）
        // → 每次抓取 clone + 解析 payload 1 次。
        let (scrape_request_dto, scrape_request) = match Self::parse_scrape_request_dto(&task) {
            Ok(dto) => {
                let req = match Self::build_scrape_request_from_dto(&dto) {
                    Ok(r) => r,
                    Err(e) => {
                        error!(
                            "Failed to build scrape request from dto, using default: {}",
                            e
                        );
                        ScrapeRequest::new(task.url.clone()).timeout(Duration::from_secs(
                            self.settings.timeouts.engines.default_timeout_seconds,
                        ))
                    }
                };
                (Some(dto), req)
            }
            Err(e) => {
                error!("Failed to parse task payload, using default: {}", e);
                let req = ScrapeRequest::new(task.url.clone()).timeout(Duration::from_secs(
                    self.settings.timeouts.engines.default_timeout_seconds,
                ));
                (None, req)
            }
        };

        // SSRF 防护 (CWE-918)：静态校验 options.proxy 不指向内部网络（防御纵深）。
        // handler 层已通过 validate_url 完成完整 DNS 解析校验，
        // 此处仅用静态检查拦截直接入队的恶意任务（如 private IP / localhost），不依赖网络。
        if let Some(ref proxy_url) = scrape_request.options.proxy {
            if is_internal_url(proxy_url) {
                warn!(
                    "SSRF via proxy blocked in worker proxy={} task_id={} team_id={}",
                    proxy_url, task.id, task.team_id
                );
                self.repository.mark_failed(task.id).await?;
                return Ok(());
            }
        }

        // T035/R-runtime-002 + H-4 职责拆分：请求合并——同 URL 并发只允许首个执行实际抓取
        //
        // 调用 CoalesceCoordinator（独立组件），返回 `Some(guard)` 表示获得执行权，
        // guard 在抓取完成（含错误路径）后随作用域结束 Drop，自动从 in_flight 移除条目并广播给等待方。
        // 返回 `None` 表示已被其他 worker 处理（等待方从 result_repo 读到结果，
        // 或任务被延后重排），调用方应直接返回 Ok。
        let _coalesce_guard = match self
            .coalesce_coordinator
            .try_coalesce(&task.url, &task)
            .await?
        {
            Some(g) => g,
            None => return Ok(()),
        };

        // T038/R-runtime-003：抓取成功/失败回填 AIMDController
        //
        // 由 `TeamSemaphore` 封装——Fixed 模式 noop；Adaptive 模式调用
        // `AIMDController::record_*` 并经 `AdaptiveSemaphore::set_target` 推入新 target。
        // guard 在 match 块作用域结束时 Drop，确保先广播给等待方再释放。

        // T059/R-cache-002：高级缓存模式门控
        //
        // 构造 `CacheContext`，按 `cache_mode` 决定读写行为：
        // - 读缓存：`is_cacheable() && should_read()` → 查缓存，命中直返跳过 `engine_client.scrape()`
        // - 写缓存：`is_cacheable() && should_write()` 且抓取成功 → 序列化写回
        //
        // `cache_mode=None`（默认）等价于 `Enabled`（`unwrap_or_default()`）。
        // 不可缓存的请求（data:/blob:/POST）跳过整个缓存流程。
        let cache_ctx = CacheContext {
            url: scrape_request.url.clone(),
            method: scrape_request.options.method,
            mode: scrape_request.options.cache_mode.unwrap_or_default(),
        };

        // HIGH-1 改进：cache key 纳入 ScrapeOptions 影响字段（headers/needs_js/session_id）
        //
        // 由 `cache_utils::generate_scrape_cache_key` 统一生成，读/写共用同一 key。
        // 旧实现仅 `scrape:{method}:{url}` 会导致同 URL 不同 options 的缓存串扰
        // （如 needs_js=true 拿到渲染后 DOM 与 needs_js=false 拿到原始 HTML 共享缓存）。
        let cache_key = cache_utils::generate_scrape_cache_key(&cache_ctx, &scrape_request.options);

        // 读缓存门控
        let cached_response = if cache_ctx.is_cacheable() && cache_ctx.should_read() {
            match try_read_scrape_cache(&cache_ctx, &cache_key, self.cache_service.as_ref()).await {
                Ok(Some(cached)) => {
                    // 性能审查 LOW-1：debug 禁用时跳过 redact_url_for_log 调用（~1μs）
                    // log crate 的 debug! 宏本身已 lazy format_args，但函数参数在宏调用前已求值，
                    // 需 log_enabled! 守卫才能避免 redact_url_for_log 的 Url::parse + String 分配。
                    if log::log_enabled!(log::Level::Debug) {
                        debug!(
                            "Cache hit, returning cached response url={} mode={:?}",
                            redact_url_for_log(&cache_ctx.url),
                            cache_ctx.mode
                        );
                    }
                    Some(cached)
                }
                Ok(None) => None,
                Err(e) => {
                    // 规则12：缓存读失败不吞，记录后降级为 miss（不阻塞抓取）
                    // T062 安全审查 MEDIUM-2：日志使用脱敏 URL，防止 query 参数泄露
                    warn!(
                        "Cache read failed, falling back to scrape url={} error={}",
                        redact_url_for_log(&cache_ctx.url),
                        e
                    );
                    None
                }
            }
        } else {
            None
        };

        let response = match cached_response {
            Some(cached) => Ok(cached),
            None => {
                let resp = self.engine_client.scrape(&scrape_request).await;
                // 写缓存门控：仅抓取成功且 should_write() 时写回
                if let Ok(ref r) = resp {
                    if cache_ctx.is_cacheable() && cache_ctx.should_write() {
                        if let Err(e) = try_write_scrape_cache(
                            &cache_ctx,
                            &cache_key,
                            r,
                            self.cache_service.as_ref(),
                            self.settings.cache.types.search.ttl_seconds,
                        )
                        .await
                        {
                            // 规则12：缓存写失败不吞，记录但不影响抓取结果
                            // T062 安全审查 MEDIUM-2：日志使用脱敏 URL
                            warn!(
                                "Cache write failed url={} error={}",
                                redact_url_for_log(&cache_ctx.url),
                                e
                            );
                        }
                    }
                }
                resp
            }
        };

        match response {
            Ok(response) => {
                self.team_semaphore.record_success(task.team_id);
                debug!("status_code: {}", response.status_code);
                info!("Scrape successful, status: {}", response.status_code);

                // 性能审查 H-1 修复：handle_scrape_success 改为 owned ScrapeResponse，
                // 调用前提前提取 has_screenshot 标志（response 将被 move 消费）
                let has_screenshot = response.screenshot.is_some();
                let has_proxy = scrape_request.options.proxy.is_some();

                if let Err(e) = self
                    .handle_scrape_success(&task, scrape_request_dto.as_ref(), response)
                    .await
                {
                    error!("Scrape success handler failed: {}", e);
                    debug!("error: {}", e);
                    self.handle_failure(&mut task).await?;
                } else {
                    debug!("Scrape success handler completed successfully");
                    // 扣除基础费用及高级功能费用 (PRD-253)
                    self.deduct_feature_credits(task.team_id, task.id, has_screenshot, has_proxy)
                        .await;
                }
                Ok(())
            }
            Err(e) => {
                self.team_semaphore.record_failure(task.team_id);
                error!("Scrape failed: {}", e);
                debug!("error: {}", e);

                // T028/R-identity-002：分类错误并计算重试指令（observability + reason-specific 限制）
                let retry_reason = e.retry_reason();
                let directive =
                    RetryDirective::for_attempt(retry_reason, task.attempt_count as u32);
                debug!(
                    "T028 retry classification: reason={:?} attempt={} directive={:?}",
                    retry_reason, task.attempt_count, directive
                );

                // If it's a timeout error, mark as failed immediately instead of rescheduling
                let err_str = e.to_string().to_lowercase();
                if err_str.contains("timeout")
                    || err_str.contains("expired")
                    || err_str.contains("all engines failed")
                {
                    debug!("Timeout or AllEnginesFailed detected, marking task as failed");
                    // Fetch task to ensure we have latest state
                    if let Ok(Some(mut t)) = self.repository.find_by_id(task.id).await {
                        t.status = TaskStatus::Failed;
                        t.completed_at = Some(Utc::now());
                        // 性能审查 M-5 修复：直接操作 t.payload，避免 clone 整个 JSON
                        // （原实现 let mut payload = t.payload.clone() + 后续 t.payload = payload
                        // 在失败路径上多分配一次 JSON Value）
                        if let Some(obj) = t.payload.as_object_mut() {
                            obj.insert("error".to_string(), json!(e.to_string()));
                        }
                        self.repository.update(&t).await?;
                    }
                } else {
                    // T028/R-identity-002：reason-specific 重试限制检查
                    // RetryTracker 各 reason 独立计数——AntiBot/FeatureToggle 达上限后
                    // 即使总 max_retries 未耗尽也立即标记失败，避免无谓重试。
                    let mut tracker = RetryTracker::new_default();
                    for _ in 0..task.attempt_count {
                        tracker.record(retry_reason);
                    }
                    if !tracker.should_retry(retry_reason) {
                        info!(
                            "T028: reason {:?} retry limit reached (attempt={}), \
                             marking task {} as failed",
                            retry_reason, task.attempt_count, task.id
                        );
                        if let Ok(Some(mut t)) = self.repository.find_by_id(task.id).await {
                            t.status = TaskStatus::Failed;
                            t.completed_at = Some(Utc::now());
                            if let Some(obj) = t.payload.as_object_mut() {
                                obj.insert("error".to_string(), json!(e.to_string()));
                                obj.insert(
                                    "retry_limit_reason".to_string(),
                                    json!(format!("{:?}", retry_reason)),
                                );
                            }
                            self.repository.update(&t).await?;
                        }
                    } else {
                        self.handle_failure(&mut task).await?;
                    }
                }

                // 触发失败 Webhook
                self.trigger_webhook(&task, Some(e.to_string())).await;
                Ok(())
            }
        }
    }

    // H-4 职责拆分：`try_coalesce` 方法已迁移至 `CoalesceCoordinator`（独立组件）。
    // 原 `request_coalescer` 字段已替换为 `coalesce_coordinator: Arc<CoalesceCoordinator>`。
    // 调用方在 `process_scrape_task` 中通过 `self.coalesce_coordinator.try_coalesce(...)` 触发。

    // T033 拆分：process_crawl_task / handle_crawl_success / handle_crawl_failure
    // 已迁移至 `crawl_task.rs`（partial impl block）

    /// 使用配置的规则提取数据（支持 rules > prompt > schema 优先级）
    async fn extract_data_with_rules(
        &self,
        task: &Task,
        response: &ScrapeResponse,
        config: &CrawlConfigDto,
    ) -> Option<Value> {
        if let Some(rules) = &config.extraction_rules {
            match self
                .extraction_service
                .extract(&response.content, rules, Some(&task.url))
                .await
            {
                Ok((data, usage)) => {
                    self.deduct_token_credits(
                        task.team_id,
                        task.id,
                        &usage,
                        "Tokens used for extraction",
                    )
                    .await;
                    Some(data)
                }
                Err(e) => {
                    error!("Extraction failed for url {}: {}", task.url, e);
                    None
                }
            }
        } else if let Some(prompt) = &config.extraction_prompt {
            if !prompt.is_empty() {
                match self
                    .extract_with_prompt(&response.content, prompt, &task.url)
                    .await
                {
                    Ok((data, usage)) => {
                        self.deduct_token_credits(
                            task.team_id,
                            task.id,
                            &usage,
                            "Tokens used for prompt extraction",
                        )
                        .await;
                        Some(data)
                    }
                    Err(e) => {
                        error!("Prompt extraction failed for url {}: {}", task.url, e);
                        None
                    }
                }
            } else {
                None
            }
        } else if let Some(schema) = &config.extraction_schema {
            match self.extract_with_schema(&response.content, schema).await {
                Ok((data, usage)) => {
                    self.deduct_token_credits(
                        task.team_id,
                        task.id,
                        &usage,
                        "Tokens used for schema extraction",
                    )
                    .await;
                    Some(data)
                }
                Err(e) => {
                    error!("Schema extraction failed for url {}: {}", task.url, e);
                    None
                }
            }
        } else {
            None
        }
    }

    /// 使用 Prompt 提取数据（共享辅助方法）
    ///
    /// 将 prompt 包装为 `ExtractionRule { use_llm: true }` 后调用 `extraction_service.extract()`。
    /// 供 Scrape 路径（handle_scrape_success）和 Crawl 路径（extract_data_with_rules）共同调用。
    async fn extract_with_prompt(
        &self,
        html: &str,
        prompt: &str,
        base_url: &str,
    ) -> Result<(Value, crate::domain::services::llm::TokenUsage)> {
        let mut rules = HashMap::with_capacity(1);
        rules.insert(
            "extracted_data".to_string(),
            crate::domain::services::extraction_service::ExtractionRule {
                selector: None,
                attr: None,
                is_array: false,
                use_llm: Some(true),
                llm_prompt: Some(prompt.to_string()),
                output_format: None,
            },
        );
        self.extraction_service
            .extract(html, &rules, Some(base_url))
            .await
    }

    /// 使用 Schema 提取数据（共享辅助方法）
    ///
    /// 直接调用 `extraction_service.extract_with_schema()`。
    /// 供 Scrape 路径（handle_scrape_success）和 Crawl 路径（extract_data_with_rules）共同调用。
    async fn extract_with_schema(
        &self,
        html: &str,
        schema: &Value,
    ) -> Result<(Value, crate::domain::services::llm::TokenUsage)> {
        self.extraction_service
            .extract_with_schema(html, schema)
            .await
    }

    // T033 拆分：handle_crawl_failure 已迁移至 `crawl_task.rs`

    // T033 拆分：process_extract_task / handle_rules_extraction / handle_prompt_extraction
    // / handle_schema_extraction / save_extract_result 已迁移至 `extract_task.rs`

    async fn handle_scrape_success(
        &self,
        task: &Task,
        scrape_request_dto: Option<&ScrapeRequestDto>,
        response: ScrapeResponse,
    ) -> Result<()> {
        debug!("task_id: {}", task.id);

        // 文本编码处理 - 集成文本处理功能
        // 性能审查 H-2 修复：process_text_encoding 返回 Cow<'_, str>，禁用路径零 clone
        let processed_content = match process_text_encoding(task, &response).await {
            Ok(content) => content.into_owned(),
            Err(e) => {
                warn!("文本编码处理失败，使用原始内容: {}", e);
                response.content.clone()
            }
        };

        // PERF-H1 修复：复用 process_scrape_task 已解析的 ScrapeRequestDto 引用，
        // 不再在 handle_scrape_success 内部二次 from_value(task.payload.clone())。
        let parsed_req = scrape_request_dto;

        // 创建处理后的响应用于后续处理
        // T042/R-content-001 + H-4 职责拆分：调用 MarkdownPostProcessor（独立组件）
        // 若 formats 含 "markdown" 则生成 Markdown，否则返回 Ok(None)
        //
        // 架构审查 M-1（错误显性化）：generate() 现返回 Result<Option<String>, _>，
        // 区分"未请求 markdown"（Ok(None)）与"转换失败/空结果"（Err）。
        // 调用方策略（design.md §10）：markdown 为增强字段，失败不阻断基础抓取结果，
        // 错误时记录告警并继续（generated_markdown = None）。
        // T074/R-content-001：generate() 改为 async，支持 only_main_content 前置正文提取
        #[cfg(feature = "content")]
        let generated_markdown: Option<String> = if let Some(req) = parsed_req.as_ref() {
            self.markdown_post_processor
                .generate(task.id, req, &processed_content)
                .await
                .unwrap_or_else(|e| {
                    warn!(
                        "task_id: {}, markdown post-processing failed: {}",
                        task.id, e
                    );
                    None
                })
        } else {
            None
        };
        #[cfg(not(feature = "content"))]
        let generated_markdown: Option<String> = None;

        // 性能审查 H-1 修复：handle_scrape_success 改为 owned ScrapeResponse，
        // 构造 processed_response 时直接 move 字段，避免 clone screenshot(100KB+)/headers/...
        let processed_response = ScrapeResponse {
            content: processed_content,
            status_code: response.status_code,
            screenshot: response.screenshot,
            content_type: response.content_type,
            headers: response.headers,
            response_time_ms: response.response_time_ms,
            final_url: response.final_url,
            markdown: generated_markdown.or(response.markdown),
        };

        // 解析 ScrapeRequest 以检查是否有提取规则
        let mut extracted_data = None;
        if let Some(req) = parsed_req.as_ref() {
            if let Some(rules) = &req.extraction_rules {
                match self
                    .extraction_service
                    .extract(&processed_response.content, rules, Some(&task.url))
                    .await
                {
                    Ok((data, usage)) => {
                        extracted_data = Some(data);
                        // Record usage (PRD-334: Tokens Billing)
                        if usage.total_tokens > 0 {
                            // 1. Record in-memory for real-time tracking
                            self.token_usage
                                .entry(task.team_id)
                                .or_insert_with(|| AtomicI64::new(0))
                                .fetch_add(usage.total_tokens as i64, Ordering::Relaxed);

                            // 2. Convert to credits and deduct from database
                            // Rate: 10 credits per 1000 tokens, minimum 1 credit for any usage
                            let credits_to_deduct =
                                std::cmp::max(1, (usage.total_tokens as i64 * 10 + 999) / 1000);
                            if credits_to_deduct > 0 {
                                if let Err(e) = self
                                    .credits_repository
                                    .deduct_credits(
                                        task.team_id,
                                        credits_to_deduct,
                                        crate::domain::models::CreditsTransactionType::Extract,
                                        format!(
                                            "Tokens used for extraction ({} tokens)",
                                            usage.total_tokens
                                        ),
                                        Some(task.id),
                                    )
                                    .await
                                {
                                    error!("Failed to deduct credits for token usage: {}", e);
                                } else {
                                    info!(
                                        "Deducted {} credits for {} tokens for team {}",
                                        credits_to_deduct, usage.total_tokens, task.team_id
                                    );
                                }
                            }
                        }
                    }
                    Err(e) => {
                        error!("Extraction failed for url {}: {}", task.url, e);
                    }
                }
            } else if let Some(prompt) = &req.extraction_prompt {
                // extraction_prompt 为空字符串时视为未设置
                if !prompt.is_empty() {
                    match self
                        .extract_with_prompt(&processed_response.content, prompt, &task.url)
                        .await
                    {
                        Ok((data, usage)) => {
                            extracted_data = Some(data);
                            self.deduct_token_credits(
                                task.team_id,
                                task.id,
                                &usage,
                                "Tokens used for prompt extraction",
                            )
                            .await;
                        }
                        Err(e) => {
                            error!("Prompt extraction failed for url {}: {}", task.url, e);
                        }
                    }
                }
            } else if let Some(schema) = &req.extraction_schema {
                match self
                    .extract_with_schema(&processed_response.content, schema)
                    .await
                {
                    Ok((data, usage)) => {
                        extracted_data = Some(data);
                        self.deduct_token_credits(
                            task.team_id,
                            task.id,
                            &usage,
                            "Tokens used for schema extraction",
                        )
                        .await;
                    }
                    Err(e) => {
                        error!("Schema extraction failed for url {}: {}", task.url, e);
                    }
                }
            }
        }

        save_result(
            task,
            &processed_response,
            extracted_data,
            self.result_repository.as_ref(),
        )
        .await?;
        debug!("task_id: {}, About to mark task as completed", task.id);
        self.repository.mark_completed(task.id).await?;
        debug!(
            "task_id: {}, Successfully marked task as completed",
            task.id
        );

        self.trigger_webhook(task, None).await;
        Ok(())
    }

    // H-4 职责拆分：`maybe_generate_markdown` 方法已迁移至 `MarkdownPostProcessor`（独立组件）。
    // 原 `HtmdMarkdownService` 调用已替换为 `self.markdown_post_processor.generate(...)`，
    // 由 `handle_scrape_success` 中调用。

    async fn trigger_webhook(&self, task: &Task, error_msg: Option<String>) {
        let result = match error_msg {
            Some(msg) => self.webhook_service.trigger_failure(task, msg).await,
            None => self.webhook_service.trigger_completion(task).await,
        };

        if let Err(e) = result {
            error!("Failed to trigger webhook for task {}: {}", task.id, e);
        }
    }

    async fn handle_failure(&self, task: &mut Task) -> Result<()> {
        match self.retry_handler.handle_failure(task).await {
            crate::domain::services::retry_handler::HandleFailureResult::Retried { .. } => Ok(()),
            crate::domain::services::retry_handler::HandleFailureResult::Failed => Ok(()),
            crate::domain::services::retry_handler::HandleFailureResult::Error(e) => Err(e),
        }
    }

    async fn deduct_feature_credits(
        &self,
        team_id: Uuid,
        task_id: Uuid,
        screenshot: bool,
        proxy: bool,
    ) {
        let mut extra_credits = 0;

        // 2. Screenshot: 2 Credits
        if screenshot {
            extra_credits += 2;
        }

        // 3. Proxy: 1 Credit
        if proxy {
            extra_credits += 1;
        }

        if extra_credits > 0 {
            if let Err(e) = self
                .credits_repository
                .deduct_credits(
                    team_id,
                    extra_credits,
                    crate::domain::models::CreditsTransactionType::Scrape,
                    format!(
                        "Extra credits for scrape (screenshot/proxy) for task {}",
                        task_id
                    ),
                    Some(task_id),
                )
                .await
            {
                error!("Failed to deduct extra credits for task {}: {}", task_id, e);
            }
        }
    }

    async fn deduct_token_credits(
        &self,
        team_id: Uuid,
        task_id: Uuid,
        usage: &crate::domain::services::llm::TokenUsage,
        description: &str,
    ) {
        if usage.total_tokens > 0 {
            // 1. Record in-memory for real-time tracking
            self.token_usage
                .entry(team_id)
                .or_insert_with(|| AtomicI64::new(0))
                .fetch_add(usage.total_tokens as i64, Ordering::Relaxed);

            // 2. Convert to credits and deduct from database
            // Rate: 10 credits per 1000 tokens, minimum 1 credit for any usage
            let credits_to_deduct = std::cmp::max(1, (usage.total_tokens as i64 * 10 + 999) / 1000);
            if credits_to_deduct > 0 {
                if let Err(e) = self
                    .credits_repository
                    .deduct_credits(
                        team_id,
                        credits_to_deduct,
                        crate::domain::models::CreditsTransactionType::Extract,
                        format!("{} ({} tokens)", description, usage.total_tokens),
                        Some(task_id),
                    )
                    .await
                {
                    error!("Failed to deduct credits for token usage: {}", e);
                } else {
                    info!(
                        "Deducted {} credits for {} tokens for team {}",
                        credits_to_deduct, usage.total_tokens, team_id
                    );
                }
            }
        }
    }

    /// 从 Task payload 解析出 [`ScrapeRequestDto`]（PERF-H1 重构：拆分两步式）。
    ///
    /// 该方法仅负责反序列化，调用方可继续调 [`Self::build_scrape_request_from_dto`]
    /// 构造 [`ScrapeRequest`]，或直接复用 dto 引用避免二次解析。
    ///
    /// # 性能要点
    ///
    /// `serde_json::from_value` 需要 owned `Value`，因此 `task.payload` 必须 clone 一次。
    /// 调用方拿到 dto 后应在所有后续路径（构造 [`ScrapeRequest`]、生成 markdown、
    /// 检查 extraction_rules）复用同一引用，禁止再次 `from_value(task.payload.clone())`。
    pub(crate) fn parse_scrape_request_dto(task: &Task) -> Result<ScrapeRequestDto> {
        serde_json::from_value(task.payload.clone()).context("Failed to parse task payload")
    }

    /// 从已解析的 [`ScrapeRequestDto`] 构造 [`ScrapeRequest`]（PERF-H1 重构：拆分两步式）。
    ///
    /// 不再读取 `task.payload`，避免重复解析与 clone。
    pub(crate) fn build_scrape_request_from_dto(dto: &ScrapeRequestDto) -> Result<ScrapeRequest> {
        let options = dto.options.as_ref();

        let mut headers = HashMap::with_capacity(16);
        if let Some(opts) = options {
            if let Some(h) = &opts.headers {
                if let Some(obj) = h.as_object() {
                    for (k, v) in obj {
                        if let Some(s) = v.as_str() {
                            headers.insert(k.clone(), s.to_string());
                        }
                    }
                }
            }
        }

        let needs_js = dto.actions.as_ref().map(|a| !a.is_empty()).unwrap_or(false)
            || options.and_then(|o| o.js_rendering).unwrap_or(false);

        let screenshot_config = options.and_then(|o| {
            o.screenshot_options.as_ref().map(|so| ScreenshotConfig {
                full_page: so.full_page.unwrap_or(false),
                selector: so.selector.clone(),
                quality: so.quality,
                format: so.format.clone(),
            })
        });

        Ok(ScrapeRequest {
            url: dto.url.clone(),
            options: ScrapeOptions {
                method: HttpMethod::Get,
                body: None,
                headers,
                timeout: Duration::from_secs(options.and_then(|o| o.timeout).unwrap_or(30)),
                needs_js,
                needs_screenshot: options.and_then(|o| o.screenshot).unwrap_or(false),
                screenshot_config,
                mobile: options.and_then(|o| o.mobile).unwrap_or(false),
                proxy: options.and_then(|o| o.proxy.clone()),
                skip_tls_verification: options
                    .and_then(|o| o.skip_tls_verification)
                    .unwrap_or(false),
                needs_tls_fingerprint: options
                    .and_then(|o| o.needs_tls_fingerprint)
                    .unwrap_or(false),
                use_fire_engine: options.and_then(|o| o.use_fire_engine).unwrap_or(false),
                actions: dto
                    .actions
                    .clone()
                    .unwrap_or_default()
                    .into_iter()
                    .filter_map(|a| match a {
                        crate::application::dto::scrape_request::ScrapeActionDto::Wait {
                            milliseconds,
                        } => Some(PageAction::Wait { milliseconds }),
                        crate::application::dto::scrape_request::ScrapeActionDto::Click {
                            selector,
                        } => Some(PageAction::Click { selector }),
                        crate::application::dto::scrape_request::ScrapeActionDto::Scroll {
                            direction,
                        } => {
                            // Map string direction to ScrollDirection enum
                            let dir = match direction.to_lowercase().as_str() {
                                "up" => ScrollDirection::Up,
                                "top" => ScrollDirection::Top,
                                "bottom" => ScrollDirection::Bottom,
                                _ => ScrollDirection::Down,
                            };
                            Some(PageAction::Scroll { direction: dir })
                        }
                        crate::application::dto::scrape_request::ScrapeActionDto::Screenshot {
                            ..
                        } => {
                            // Screenshot action is handled by global needs_screenshot option
                            None
                        }
                        crate::application::dto::scrape_request::ScrapeActionDto::Input {
                            selector,
                            text,
                        } => Some(PageAction::Input { selector, text }),
                    })
                    .collect(),
                sync_wait_ms: dto.sync_wait_ms.unwrap_or(0),
                block_ads: false,
                block_media: false,
                session_id: None,
                // T058/R-cache-002：cache_mode 桥接（bypass_cache 优先级处理）
                //
                // bypass_cache=Some(true) 覆盖 cache_mode 为 Bypass（应急绕过读，正常写回）。
                // 其余情况按 cache_mode 走；两者皆 None 时等价于 Enabled（默认）。
                cache_mode: {
                    let bypass = options.and_then(|o| o.bypass_cache).unwrap_or(false);
                    if bypass {
                        Some(CacheMode::Bypass)
                    } else {
                        options.and_then(|o| o.cache_mode)
                    }
                },
                wait_for: None,
                needs_mllm: false,
            },
        })
    }

    /// 从 [`Task`] 一次性解析并构造 [`ScrapeRequest`]（便利入口）。
    ///
    /// 调用方仅需 `&Task`，无需关心 dto 解析细节。内部委托
    /// [`Self::parse_scrape_request_dto`] + [`Self::build_scrape_request_from_dto`]。
    ///
    /// **若调用方需要同时使用 [`ScrapeRequestDto`] 与 [`ScrapeRequest`]**，
    /// 应直接调 [`Self::parse_scrape_request_dto`] 拿到 dto 后再调
    /// [`Self::build_scrape_request_from_dto`]，避免双解析双 clone payload。
    pub fn build_scrape_request(task: &Task) -> Result<ScrapeRequest> {
        let dto = Self::parse_scrape_request_dto(task)?;
        Self::build_scrape_request_from_dto(&dto)
    }
}

// ===== Test-only wrappers for extracted free functions =====
// 测试代码通过 `worker.xxx()` 调用这些包装方法，
// 生产代码直接调用提取后的自由函数。
#[cfg(test)]
impl ScrapeWorker {
    async fn parse_crawl_payload(&self, task: &Task) -> Result<(Uuid, u32, CrawlConfigDto)> {
        parse_crawl_payload(task)
    }

    async fn check_robots_txt(&self, task: &Task) -> bool {
        check_robots_txt_fn(task, self.robots_checker.as_ref()).await
    }

    fn build_crawl_request(&self, task: &Task, config: &CrawlConfigDto) -> ScrapeRequest {
        build_crawl_request_fn(
            task,
            config,
            self.settings.timeouts.engines.default_timeout_seconds,
        )
    }

    async fn update_crawl_completion_status(&self, crawl_id: Uuid) {
        update_crawl_completion_status_fn(crawl_id, self.crawl_repository.as_ref()).await
    }

    async fn parse_extract_payload(&self, task: &Task) -> Result<(ExtractRequestDto, String)> {
        parse_extract_payload(task)
    }

    fn build_extract_request(&self, url: &str) -> ScrapeRequest {
        build_extract_request_fn(url, self.settings.timeouts.engines.default_timeout_seconds)
    }

    async fn try_read_scrape_cache(
        &self,
        ctx: &CacheContext,
        key: &str,
    ) -> Result<Option<ScrapeResponse>> {
        try_read_scrape_cache(ctx, key, self.cache_service.as_ref()).await
    }

    async fn try_write_scrape_cache(
        &self,
        ctx: &CacheContext,
        key: &str,
        response: &ScrapeResponse,
    ) -> Result<()> {
        try_write_scrape_cache(
            ctx,
            key,
            response,
            self.cache_service.as_ref(),
            self.settings.cache.types.search.ttl_seconds,
        )
        .await
    }

    async fn save_result(
        &self,
        task: &Task,
        response: &ScrapeResponse,
        extra_data: Option<Value>,
    ) -> Result<()> {
        save_result(task, response, extra_data, self.result_repository.as_ref()).await
    }

    async fn extract_and_queue_links(
        &self,
        task: &Task,
        response: &ScrapeResponse,
        crawl_id: Uuid,
        current_depth: u32,
        config: &CrawlConfigDto,
    ) -> Result<()> {
        extract_and_queue_links_fn(
            task,
            response,
            crawl_id,
            current_depth,
            config,
            self.repository.as_ref(),
            self.crawl_repository.as_ref(),
            &self.deduplicator,
        )
        .await
    }

    async fn process_text_encoding<'a>(
        &self,
        task: &Task,
        response: &'a ScrapeResponse,
    ) -> Result<std::borrow::Cow<'a, str>> {
        crate::workers::scrape_executor::process_text_encoding(task, response).await
    }
}

// T033 拆分：Crawl 任务处理方法（partial impl block）
mod crawl_task;
// T033 拆分：Extract 任务处理方法（partial impl block）
mod extract_task;

// Builder 子模块
mod builder;
pub use builder::ScrapeWorkerBuilder;
mod deps;
pub use deps::ScrapeWorkerDeps;

#[cfg(test)]
#[path = "../tests/scrape_worker_test.rs"]
mod tests;
