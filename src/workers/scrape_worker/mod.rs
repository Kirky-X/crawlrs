// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

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

use crate::application::dto::crawl_request::CrawlConfigDto;
use crate::application::dto::scrape_request::{ScrapeOptionsDto, ScrapeRequestDto};
use crate::application::use_cases::create_scrape::CreateScrapeUseCaseTrait;
use crate::config::settings::Settings;
use crate::domain::id::Id;
use crate::domain::models::{Task, TaskStatus};
use crate::domain::repositories::crawl_repository::CrawlRepository;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::services::extraction_service::ExtractionServiceTrait;
use crate::domain::services::retry_handler::RetryHandler;
use crate::domain::services::webhook_service::WebhookService;
// URL 分层去重器（Bloom 预筛 + DB 保权威）
use crate::utils::dedup::Deduplicator;

use crate::common::CacheContext;
use crate::common::CacheMode;
use crate::common::HttpMethod;
use crate::domain::services::team_semaphore::TeamSemaphore;
use crate::engines::engine_client::{
    EngineClient, EngineError, PageAction, ScrapeOptions, ScrapeRequest, ScrapeResponse,
    ScreenshotConfig, ScrollDirection, WaitFor,
};
use crate::infrastructure::oxcache::CacheService;
use crate::infrastructure::security::ssrf::is_internal_url;
use crate::queue::task_queue::TaskQueue;
// 请求合并协调器（替代原 request_coalescer 字段 + try_coalesce 方法）
use crate::workers::coalesce_coordinator::CoalesceCoordinator;
// 优雅退出协调器
use crate::workers::shutdown::ShutdownCoordinator;
// cache key 生成
use crate::workers::cache_utils;
// 请求合并键（租户 + 规范化 URL + 完整请求身份指纹）
use crate::workers::coalesce_coordinator::CoalesceKey;
// URL 日志脱敏
use crate::utils::url_redaction::redact_url_for_log;
// Markdown 后处理器（gated `markdown` 特性，替代原 maybe_generate_markdown 方法）
use crate::utils::retry_policy::RetryPolicy;
#[cfg(feature = "content")]
use crate::workers::markdown_post_processor::MarkdownPostProcessor;
// 重试指令与分类器（消费 RetryTracker + RetryDirective）
use crate::utils::retry::{RetryDirective, RetryTracker};
// 自适应爬取停止条件
use crate::utils::robots::RobotsCheckerTrait;
// 内存感知调度器接入 scrape_worker
// MemoryScheduler 依赖 SystemMonitorTrait（metrics 特性门控），故整块接入由 metrics 门控
#[cfg(feature = "metrics")]
use crate::workers::scheduler::memory_scheduler::MemoryScheduler;

// 提取到独立模块的函数导入
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
    /// 请求合并协调器
    ///
    /// 同 URL 并发请求只允许首个执行实际抓取，其余 worker 等待广播后从
    /// `result_repo` 读取结果，避免重复网络往返。所有 worker 共享同一实例
    /// （由 `WorkerManager` 从 `ServicesComponents.request_coalescer` +
    /// `repository` + `result_repository` 构造注入）。
    coalesce_coordinator: Arc<CoalesceCoordinator>,
    /// Markdown 后处理器
    ///
    /// 无状态服务，根据任务 `formats` 字段判断是否生成 Markdown。
    /// gated `markdown` 特性：关闭时本字段不存在，相关分支也不编译。
    #[cfg(feature = "content")]
    markdown_post_processor: MarkdownPostProcessor,
    token_usage: Arc<DashMap<crate::domain::id::Id, AtomicI64>>,
    robots_checker: Arc<dyn RobotsCheckerTrait>,
    settings: Arc<Settings>,
    worker_id: crate::domain::id::Id,
    default_concurrency_limit: usize,
    retry_handler: RetryHandler,
    extraction_service: Arc<dyn ExtractionServiceTrait>,
    /// 内存感知调度器
    ///
    /// `metrics` 启用时由 `WorkerManager` 注入；`process_task` 在获取并发许可前
    /// 调用 `admit()`，Pressure 时延后、Critical 时重排到 backlog。
    #[cfg(feature = "metrics")]
    memory_scheduler: Arc<MemoryScheduler>,
    /// URL 分层去重器
    ///
    /// UrlNormalizer + Bloom + HashSet 三层组合，用于 `extract_and_queue_links`
    /// 预筛 URL 是否已爬：
    /// - Bloom 阴性 → 绝对新，直接入队并 insert
    /// - Bloom 阳性 → 可能已爬，回落 `find_existing_urls` DB 校验（保权威）
    ///
    /// `RwLock` 因为 `extract_and_queue_links` 是 `&self`，但 bloom insert 需 `&mut`。
    /// `Arc` 是为后续支持跨 worker 共享（当前每 worker 独立实例）。
    deduplicator: Arc<parking_lot::RwLock<Deduplicator>>,
    /// 高级缓存服务
    ///
    /// 由 `WorkerManager` 从 `InfrastructureComponents.cache_service` 注入，
    /// 所有 worker 共享同一实例。`process_scrape_task` 在读写缓存前经
    /// `CacheContext` 门控：`is_cacheable && should_read` 查缓存命中直返，
    /// `is_cacheable && should_write` 抓取成功后写回。
    cache_service: Arc<dyn CacheService>,
    /// 优雅退出协调器
    ///
    /// 由 `WorkerManager` 注入，所有 worker 共享同一实例。`run` 循环开头
    /// 检查 `is_shutting_down`，置位后不再 acquire 新任务，完成当前任务即退出。
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
    /// 本 worker 的认领身份（即任务 lock_token）
    pub fn worker_id(&self) -> crate::domain::id::Id {
        self.worker_id
    }

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
                // 按 [markdown].converter 装配（moli 链默认 / htmd 直通诊断）
                crate::domain::services::markdown::build_converter(&deps.settings.markdown),
                // bootstrap 注入的带质量门 Facade（[content_extraction] 四键 +
                // llm_service 回退）；不再本地 new(None) 另立 disabled 实例
                Some(Arc::clone(&deps.content_extractor)),
            ),
            token_usage: Arc::new(DashMap::new()),
            robots_checker: deps.robots_checker,
            settings: deps.settings,
            worker_id: crate::domain::id::Id::generate(),
            default_concurrency_limit: deps.default_concurrency_limit,
            retry_handler,
            extraction_service: deps.extraction_service,
            #[cfg(feature = "metrics")]
            memory_scheduler: deps.memory_scheduler,
            // 默认每 worker 独立 Deduplicator
            // 后续可由 WorkerManager 通过 Builder 注入共享实例优化 DB 查询量
            deduplicator: Arc::new(parking_lot::RwLock::new(Deduplicator::new())),
            cache_service: deps.cache_service,
            // 默认独立协调器，由 WorkerManager 通过
            // `with_shutdown_coordinator` 注入共享实例。
            shutdown_coordinator: Arc::new(ShutdownCoordinator::default()),
        }
    }

    /// 注入优雅退出协调器
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
            // 优雅退出检查
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
    /// let worker = build_mock_worker.await;
    /// {
    ///     let mut dedup = worker.deduplicator_for_test.write;
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

    async fn process_task(&self, task: Task) -> Result<()> {
        debug!(
            "process_task: task_id={}, url={}, task_type={}",
            task.id, task.url, task.task_type
        );
        // 锁续期心跳执行期间按锁时长的 1/3 周期续期，
        // 防止长任务锁过期被其他 worker 抢占。RAII guard 在任意返回路径自动停止。
        let _lock_heartbeat = crate::workers::lock_heartbeat::LockHeartbeat::start(
            self.repository.clone(),
            task.id,
            self.worker_id,
            self.settings.concurrency.task_lock_duration_seconds,
        );
        info!("Processing task");

        if self.handle_expired_task(&task).await? {
            return Ok(());
        }

        // 内存感知准入检查（策略与 AIMD 联动详见 [`ScrapeWorker::admit_task_by_memory`]）
        #[cfg(feature = "metrics")]
        if self.admit_task_by_memory(&task).await? {
            return Ok(());
        }

        // Concurrency Check (Layer 2: Team Semaphore)
        // The permit is held for the duration of task processing and auto-releases on drop.
        let _permit = match self.acquire_permit_or_reschedule(&task).await? {
            Some(p) => p,
            None => return Ok(()),
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

    /// 从 Task payload 解析出 [`ScrapeRequestDto`]（PERF- 拆分两步式）。
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

    /// 从已解析的 [`ScrapeRequestDto`] 构造 [`ScrapeRequest`]（PERF- 拆分两步式）。
    ///
    /// 不再读取 `task.payload`，避免重复解析与 clone。
    pub(crate) fn build_scrape_request_from_dto(dto: &ScrapeRequestDto) -> Result<ScrapeRequest> {
        let options = dto.options.as_ref();
        let headers = build_dto_headers(options);
        let needs_js = dto_needs_js(dto, options);
        let screenshot_config = build_dto_screenshot_config(options);

        Ok(ScrapeRequest {
            url: dto.url.clone(),
            options: ScrapeOptions {
                // NOTE: ScrapeOptionsDto 当前未暴露 method/body 字段，默认 Get + None。
                // 若未来 DTO 增加这些字段，需同步更新此处。
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
                actions: map_dto_actions(dto),
                sync_wait_ms: dto.sync_wait_ms.unwrap_or(0),
                // needs_mllm 在 engine-mllm 特性关闭时显性失败：任务已在队列中，静默降级
                // 到普通引擎会让用户以为做了视觉分析——失败必须显性化。
                // （API 入口的 create_scrape use-case 在提交时已先行拒绝，此处
                // 兜底覆盖直写队列/旧版本任务等绕过 use-case 的路径。）
                needs_mllm: resolve_needs_mllm(options)?,
                block_ads: options.and_then(|o| o.block_ads).unwrap_or(false),
                block_media: options.and_then(|o| o.block_media).unwrap_or(false),
                session_id: None,
                // cache_mode 桥接（bypass_cache 优先级处理）
                //
                // bypass_cache=Some(true) 覆盖 cache_mode 为 Bypass（应急绕过读，正常写回）。
                // 其余情况按 cache_mode 走；两者皆 None 时等价于 Enabled（默认）。
                cache_mode: map_dto_cache_mode(options),
                // P22 接线：DTO 的 wait_for（毫秒）映射为 DomStable 等待策略——
                // 与 create_scrape use-case 层的映射保持一致（此前此处硬编码
                // None，调用方传入的 wait_for 被静默丢弃）
                wait_for: map_wait_for(options.and_then(|o| o.wait_for)),
            },
        })
    }

    /// 从 [`Task`] 一次性解析并构造 [`ScrapeRequest`]（便利入口）。
    ///
    /// 调用方仅需 `&Task`，无需关心 dto 解析细节。内部委托
    /// `Self::parse_scrape_request_dto` + `Self::build_scrape_request_from_dto`。
    ///
    /// **若调用方需要同时使用 [`ScrapeRequestDto`] 与 [`ScrapeRequest`]**，
    /// 应直接调 `Self::parse_scrape_request_dto` 拿到 dto 后再调
    /// `Self::build_scrape_request_from_dto`，避免双解析双 clone payload。
    pub fn build_scrape_request(task: &Task) -> Result<ScrapeRequest> {
        let dto = Self::parse_scrape_request_dto(task)?;
        Self::build_scrape_request_from_dto(&dto)
    }
}

/// DTO options → 请求头 map（仅保留字符串值）
fn build_dto_headers(options: Option<&ScrapeOptionsDto>) -> HashMap<String, String> {
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
    headers
}

/// JS 渲染需求：存在动作或显式开启
fn dto_needs_js(dto: &ScrapeRequestDto, options: Option<&ScrapeOptionsDto>) -> bool {
    dto.actions.as_ref().map(|a| !a.is_empty()).unwrap_or(false)
        || options.and_then(|o| o.js_rendering).unwrap_or(false)
}

/// DTO 截图选项 → 引擎截图配置
fn build_dto_screenshot_config(options: Option<&ScrapeOptionsDto>) -> Option<ScreenshotConfig> {
    options.and_then(|o| {
        o.screenshot_options.as_ref().map(|so| ScreenshotConfig {
            full_page: so.full_page.unwrap_or(false),
            selector: so.selector.clone(),
            quality: so.quality,
            format: so.format.clone(),
        })
    })
}

/// DTO 动作 → 引擎页面动作（Screenshot 由全局 needs_screenshot 承载）
fn map_dto_actions(dto: &ScrapeRequestDto) -> Vec<PageAction> {
    dto.actions
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter_map(|a| match a {
            crate::application::dto::scrape_request::ScrapeActionDto::Wait { milliseconds } => {
                Some(PageAction::Wait { milliseconds })
            }
            crate::application::dto::scrape_request::ScrapeActionDto::Click { selector } => {
                Some(PageAction::Click { selector })
            }
            crate::application::dto::scrape_request::ScrapeActionDto::Scroll { direction } => {
                // Map string direction to ScrollDirection enum
                let dir = map_scroll_direction(&direction);
                Some(PageAction::Scroll { direction: dir })
            }
            crate::application::dto::scrape_request::ScrapeActionDto::Screenshot { .. } => {
                // Screenshot action is handled by global needs_screenshot option
                None
            }
            crate::application::dto::scrape_request::ScrapeActionDto::Input { selector, text } => {
                Some(PageAction::Input { selector, text })
            }
        })
        .collect()
}

/// 滚动方向字符串 → 枚举（未知值回落 Down）
fn map_scroll_direction(direction: &str) -> ScrollDirection {
    match direction.to_lowercase().as_str() {
        "up" => ScrollDirection::Up,
        "top" => ScrollDirection::Top,
        "bottom" => ScrollDirection::Bottom,
        _ => ScrollDirection::Down,
    }
}

/// needs_mllm 解析：特性缺失时显性失败
fn resolve_needs_mllm(options: Option<&ScrapeOptionsDto>) -> Result<bool> {
    let requested = options.and_then(|o| o.needs_mllm).unwrap_or(false);
    if requested {
        #[cfg(not(feature = "engine-mllm"))]
        anyhow::bail!("task requested needs_mllm but this build lacks the `engine-mllm` feature");
        #[cfg(feature = "engine-mllm")]
        log::debug!("task requests MLLM analysis");
    }
    Ok(requested)
}

/// cache_mode 桥接：bypass_cache 覆盖为 Bypass，否则沿用 DTO
fn map_dto_cache_mode(options: Option<&ScrapeOptionsDto>) -> Option<CacheMode> {
    let bypass = options.and_then(|o| o.bypass_cache).unwrap_or(false);
    if bypass {
        Some(CacheMode::Bypass)
    } else {
        options.and_then(|o| o.cache_mode)
    }
}

/// P22：DTO `wait_for`（毫秒）→ 引擎等待策略映射
///
/// 与 create_scrape use-case 层保持一致：毫秒数映射为 `DomStable`（DOM
/// 连续该时长无变化视为就绪）。`None` → `None`（引擎回落 NetworkIdle 默认）。
/// 增量抓取（scoped 切片）：从历史结果 headers JSON 提取缓存验证器
///
/// 返回 `(etag, last_modified)`——均为响应头原文，大小写不敏感匹配键名
/// （上游服务器发送 `ETag`/`etag` 均有可能）。两个验证器都缺失时返回
/// `None`（无法发起条件请求）。
fn extract_cache_validators(headers: &serde_json::Value) -> Option<(String, Option<String>)> {
    let map = headers.as_object()?;
    let lookup = |name: &str| {
        map.iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .and_then(|(_, v)| v.as_str())
            .map(str::to_string)
    };
    let etag = lookup("etag");
    let last_modified = lookup("last-modified");
    if etag.is_some() || last_modified.is_some() {
        Some((etag.unwrap_or_default(), last_modified))
    } else {
        None
    }
}

/// 增量抓取（scoped 切片）：304 响应合成——用历史结果内容回填 200 响应，
/// 附加 `x-crawlrs-not-modified: true` 标记头。下游（提取/落库/webhook）
/// 走常规路径，无感知 304 语义。
fn not_modified_response(
    prior: &crate::domain::models::ScrapeResult,
    response: &ScrapeResponse,
) -> ScrapeResponse {
    let mut headers = response.headers.clone();
    headers.insert("x-crawlrs-not-modified".to_string(), "true".to_string());
    // 历史结果的响应头一并回填（客户端可读到原始 ETag 等）
    for (k, v) in prior.headers.as_object().into_iter().flatten() {
        if let Some(v_str) = v.as_str() {
            headers
                .entry(k.clone())
                .or_insert_with(|| v_str.to_string());
        }
    }
    ScrapeResponse {
        js_degraded: None,
        content: prior.content.clone(),
        status_code: 200,
        screenshot: None,
        content_type: prior.content_type.clone(),
        headers,
        response_time_ms: response.response_time_ms,
        final_url: response.final_url.clone(),
        markdown: None,
        // 304 合成响应无 XHR 捕获语义（历史结果不含捕获数据）
        captured_responses: Vec::new(),
    }
}

/// 单测锚点：纯函数模块级可测（无 worker 依赖）
#[cfg(test)]
mod incremental_scrape_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extract_validators_reads_etag_and_last_modified_case_insensitive() {
        let headers =
            json!({"ETag": "\"abc123\"", "Last-Modified": "Wed, 24 Sep 2026 08:00:00 GMT"});
        let (etag, lm) = extract_cache_validators(&headers).expect("validators present");
        assert_eq!(etag, "\"abc123\"");
        assert_eq!(lm.as_deref(), Some("Wed, 24 Sep 2026 08:00:00 GMT"));

        // 小写键名（部分代理/引擎会规范化）
        let headers = json!({"etag": "\"x\"", "last-modified": "Wed, 24 Sep 2026 09:00:00 GMT"});
        let (etag, _) = extract_cache_validators(&headers).expect("lowercase keys");
        assert_eq!(etag, "\"x\"");
    }

    #[test]
    fn extract_validators_none_when_missing() {
        assert!(extract_cache_validators(&json!({"content-type": "text/html"})).is_none());
        assert!(extract_cache_validators(&serde_json::Value::Null).is_none());
        assert!(extract_cache_validators(&json!("not an object")).is_none());
    }

    #[test]
    fn not_modified_response_reuses_prior_content_with_marker() {
        use crate::domain::models::ScrapeResult;
        let prior = ScrapeResult {
            id: crate::domain::id::Id::generate(),
            task_id: crate::domain::id::Id::generate(),
            url: "https://example.com/a".to_string(),
            status_code: 200,
            content: "<html>original body</html>".to_string(),
            content_type: "text/html".to_string(),
            headers: json!({"etag": "\"v1\"", "content-type": "text/html"}),
            meta_data: serde_json::Value::Null,
            screenshot: None,
            response_time_ms: 42,
            created_at: chrono::Utc::now(),
        };
        let not_modified = ScrapeResponse {
            js_degraded: None,
            captured_responses: Vec::new(),
            status_code: 304,
            content: String::new(),
            content_type: String::new(),
            headers: {
                let mut h = std::collections::HashMap::new();
                h.insert("etag".to_string(), "\"v1\"".to_string());
                h
            },
            screenshot: None,
            response_time_ms: 5,
            final_url: None,
            markdown: None,
        };

        let reused = not_modified_response(&prior, &not_modified);
        assert_eq!(reused.status_code, 200, "304 must be rewritten as 200");
        assert_eq!(reused.content, "<html>original body</html>");
        assert_eq!(reused.content_type, "text/html");
        assert_eq!(
            reused
                .headers
                .get("x-crawlrs-not-modified")
                .map(String::as_str),
            Some("true"),
            "not-modified marker must be present"
        );
        // 历史响应头回填（客户端可读原始 ETag）
        assert_eq!(
            reused.headers.get("etag").map(String::as_str),
            Some("\"v1\"")
        );
    }

    #[test]
    fn conditional_request_dto_deserializes_all_branches() {
        use crate::application::dto::scrape_request::ScrapeRequestDto;
        let on: ScrapeRequestDto = serde_json::from_value(
            json!({"url": "https://x.com", "options": {"conditional_request": true}}),
        )
        .expect("valid");
        assert_eq!(
            on.options.as_ref().and_then(|o| o.conditional_request),
            Some(true)
        );
        let off: ScrapeRequestDto = serde_json::from_value(
            json!({"url": "https://x.com", "options": {"conditional_request": false}}),
        )
        .expect("valid");
        assert_eq!(
            off.options.as_ref().and_then(|o| o.conditional_request),
            Some(false)
        );
        let absent: ScrapeRequestDto =
            serde_json::from_value(json!({"url": "https://x.com"})).expect("valid");
        assert_eq!(
            absent.options.as_ref().and_then(|o| o.conditional_request),
            None,
            "field absent must default to None (disabled)"
        );
    }
}

fn map_wait_for(ms: Option<u64>) -> Option<WaitFor> {
    // 复查 H3：钳制到 10s（引擎侧 WAIT_TIMEOUT_CAP）——DTO 允许 ≤60000ms，
    // 但 stable_duration 超过等待预算时 DomStable 数学上不可能满足，
    // 会把 API 合法输入变成确定性失败。超预算部分属"尽力等待"语义。
    ms.map(|ms| WaitFor::DomStable(std::time::Duration::from_millis(ms.min(10_000))))
}

// ===== Test-only wrappers for extracted free functions =====
// 测试代码通过 `worker.xxx()` 调用这些包装方法，
// 生产代码直接调用提取后的自由函数。
#[cfg(test)]
impl ScrapeWorker {
    async fn parse_crawl_payload(
        &self,
        task: &Task,
    ) -> Result<(crate::domain::id::Id, u32, CrawlConfigDto)> {
        parse_crawl_payload(task)
    }

    async fn check_robots_txt(&self, task: &Task) -> bool {
        check_robots_txt_fn(
            task,
            self.robots_checker.as_ref(),
            self.settings.robots.user_agent.as_str(),
        )
        .await
    }

    fn build_crawl_request(&self, task: &Task, config: &CrawlConfigDto) -> ScrapeRequest {
        build_crawl_request_fn(
            task,
            config,
            self.settings.timeouts.engines.default_timeout_seconds,
        )
    }

    async fn update_crawl_completion_status(&self, crawl_id: crate::domain::id::Id) {
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
        crawl_id: crate::domain::id::Id,
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

// 任务准入 / 结果提取辅助方法（partial impl block）
mod task_lifecycle;
// 抓取管线（partial impl block）
mod scrape_pipeline;
// 提取与计费（partial impl block）
mod extraction_billing;
// 成功收尾 / webhook / 失败处理（partial impl block）
mod success_finalization;
// Crawl 任务处理方法（partial impl block）
mod crawl_task;
// Extract 任务处理方法（partial impl block）
mod extract_task;

// Builder 子模块
mod builder;
pub use builder::ScrapeWorkerBuilder;
mod deps;
pub use deps::ScrapeWorkerDeps;

#[cfg(test)]
#[path = "../tests/scrape_worker_test.rs"]
mod tests;
