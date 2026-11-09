// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 运行时基础设施装配：软 404 / 团队并发 / 请求合并 / 限流 / 抓取与 map 用例。
//!
//! 从 `init_services` 提取的装配阶段：每个函数只负责一个装配阶段，
//! 返回该阶段产出的组件；`init_services` 保留原有调用顺序与副作用顺序。

use log::info;
use std::sync::Arc;

use crate::application::use_cases::create_scrape::{CreateScrapeUseCase, CreateScrapeUseCaseTrait};
use crate::bootstrap::infrastructure::InfrastructureComponents;
use crate::bootstrap::infrastructure::Repositories;
use crate::config::settings::Settings;
use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::domain::services::team_semaphore::{AdaptiveParams, TeamSemaphore};
use crate::engines::engine_client::EngineClient;
use crate::presentation::middleware::rate_limit_middleware::RateLimitMiddleware;
use crate::workers::coalesce_coordinator::RequestCoalescer;

#[cfg(feature = "rate-limit")]
use crate::domain::services::rate_limiting_service::{
    ConcurrencyConfig, ConcurrencyStrategy, RateLimitConfig, RateLimitStrategy,
};
#[cfg(feature = "rate-limit")]
use crate::infrastructure::services::limiteron_service::{
    LimiteronService, RateLimitingConfig, StorageHandle,
};
#[cfg(not(feature = "rate-limit"))]
use crate::infrastructure::services::noop_rate_limiting_service::NoopRateLimitingService;
use dbnexus::DbPool;

/// 运行时基础设施服务（`init_services` 第一阶段产出）
pub(super) struct RuntimeServices {
    pub(super) team_semaphore: Arc<TeamSemaphore>,
    pub(super) request_coalescer: Arc<RequestCoalescer>,
    pub(super) rate_limiting_service: Arc<dyn RateLimitingService>,
    pub(super) rate_limit_middleware: RateLimitMiddleware,
    pub(super) create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait>,
    #[cfg(feature = "sitemap")]
    pub(super) map_use_case: Arc<crate::application::use_cases::map_use_case::MapUseCase>,
}

/// 装配运行时基础设施：软 404 检测器 / 团队并发 / 请求合并 / 限流 / 抓取与 map 用例。
pub(super) async fn init_runtime_services(
    infrastructure: &InfrastructureComponents,
    engine_client: &Arc<EngineClient>,
    settings: &Settings,
) -> anyhow::Result<RuntimeServices> {
    let repositories = &infrastructure.repositories;

    // F11：按配置初始化软 404 指纹检测器（进程级共享；crawl 路径消费）
    crate::workers::crawl::soft404::init_shared(settings.crawl.soft404_max_fingerprints);

    // 根据 concurrency.adaptive_enabled 选择 Fixed/Adaptive 模式
    let team_semaphore = init_team_semaphore(settings);

    // 初始化请求合并器（共享单例，所有 worker 复用）
    //
    // 共享 `DashMap<String, InFlightEntry>` 追踪 in-flight URL，避免多 worker 同 URL
    // 并发抓取。`STALE_TIMEOUT=120s` 兜底僵死条目，由 `purge_stale` 定期清理
    // （scrape_worker 在 process_task 入口按需调用）。
    let request_coalescer = Arc::new(RequestCoalescer::new());

    // Initialize rate limiting service
    let rate_limiting_service = init_rate_limiting_service(
        repositories,
        settings,
        Some(infrastructure.db.clone_inner()),
    )
    .await?;

    // Initialize rate limit middleware
    let rate_limit_middleware = init_rate_limit_middleware(rate_limiting_service.clone());

    // Initialize create scrape use case
    let create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait> =
        Arc::new(CreateScrapeUseCase::new(engine_client.clone()));

    // F11：/v1/map 用例（sitemap 流式站点图；sitemap feature 关闭时不编译）。
    // 拉取超时来自 settings.sitemap.fetch_timeout_secs（对齐引擎 MRT 语义）。
    #[cfg(feature = "sitemap")]
    let map_use_case = Arc::new(
        crate::application::use_cases::map_use_case::MapUseCase::new(Arc::new(
            crate::application::use_cases::map_use_case::EngineSitemapFetcher::new(
                engine_client.clone(),
                std::time::Duration::from_secs(settings.sitemap.fetch_timeout_secs),
            ),
        )),
    );

    Ok(RuntimeServices {
        team_semaphore,
        request_coalescer,
        rate_limiting_service,
        rate_limit_middleware,
        create_scrape_use_case,
        #[cfg(feature = "sitemap")]
        map_use_case,
    })
}

/// Initialize rate limit middleware.
///
/// # Arguments
///
/// * `rate_limiting_service` - Rate limiting service for distributed rate limiting
///
/// # Returns
///
/// Returns an initialized rate limit middleware.
pub(super) fn init_rate_limit_middleware(
    rate_limiting_service: Arc<dyn RateLimitingService>,
) -> RateLimitMiddleware {
    RateLimitMiddleware::new(rate_limiting_service)
}

/// Initialize team semaphore for concurrency control.
///
/// 根据 `settings.concurrency.adaptive_enabled` 选择模式：
/// - `false`（默认）：固定并发 `default_team_limit`，行为等同之前
/// - `true`：AIMD 自适应并发（`AdaptiveParams` 默认值，每队独立 controller）
///
/// # Arguments
///
/// * `settings` - Application settings（读取 `concurrency.*` 字段）
///
/// # Returns
///
/// Returns an initialized team semaphore.
pub(super) fn init_team_semaphore(settings: &Settings) -> Arc<TeamSemaphore> {
    let default_team_limit = settings.concurrency.default_team_limit as usize;

    if settings.concurrency.adaptive_enabled {
        // AIMD 自适应模式：用 default_team_limit 作为 initial
        let params = AdaptiveParams {
            initial: default_team_limit,
            ..Default::default()
        };
        info!(
            "init_team_semaphore: Adaptive mode enabled (initial={}, min=1, max=100, threshold=10)",
            default_team_limit
        );
        Arc::new(TeamSemaphore::with_adaptive(params))
    } else {
        // Fixed 模式（默认）：行为等同之前
        Arc::new(TeamSemaphore::new(default_team_limit))
    }
}

/// 从设置构建限流与并发配置（含校验告警）。
#[cfg(feature = "rate-limit")]
fn build_rate_limiting_config(settings: &Settings) -> RateLimitingConfig {
    let rate_limit_config = RateLimitConfig {
        strategy: RateLimitStrategy::TokenBucket,
        requests_per_second: settings.rate_limiting.default_rpm / 60,
        requests_per_minute: settings.rate_limiting.default_rpm,
        requests_per_hour: settings.rate_limiting.default_rpm * 60,
        bucket_capacity: Some(settings.rate_limiting.default_rpm),
        enabled: settings.rate_limiting.enabled,
    };

    if let Err(e) = rate_limit_config.validate() {
        log::error!("Rate limit configuration error: {}", e);
    }

    let concurrency_config = ConcurrencyConfig {
        strategy: ConcurrencyStrategy::DistributedSemaphore,
        max_concurrent_tasks: settings.concurrency.default_team_limit as u32,
        max_concurrent_per_team: settings.concurrency.default_team_limit as u32,
        lock_timeout_seconds: settings.concurrency.task_lock_duration_seconds,
        enabled: true,
    };

    if let Err(e) = concurrency_config.validate() {
        log::error!("Concurrency configuration error: {}", e);
    }

    RateLimitingConfig {
        rate_limit: rate_limit_config,
        concurrency: concurrency_config,
        backlog_process_interval_seconds: 30,
        rate_limit_ttl_seconds: 3600,
    }
}

/// 按 `storage_backend` 选择限流存储句柄。
///
/// 存储后端选择：database 路径失败显性报错（R-multidb-schema-003
/// 禁止「DDL 失败→回退内存」）；未知非法值维持既有 fail-open 语义。
#[cfg(feature = "rate-limit")]
fn select_rate_limit_storage(
    settings: &Settings,
    db_pool: Option<Arc<DbPool>>,
) -> anyhow::Result<StorageHandle> {
    match settings.rate_limiting.storage_backend.as_str() {
        "memory" => Ok(StorageHandle::Memory),
        "database" => {
            // crawlrs 当前只接线 Postgres 共享限流存储（ensure_storage_schema
            // 为 Postgres DDL）；sqlite/mysql 构建或 platform 关闭时选择
            // database 后端必须显式报错，不得回退内存。
            // cfg 必须与 StorageHandle::Database 的变体条件完全一致
            // （all(db-postgres, platform)），否则 web-axum 构建会编译失败。
            #[cfg(all(feature = "db-postgres", feature = "platform"))]
            match db_pool {
                Some(pool) => Ok(StorageHandle::Database(pool)),
                None => {
                    anyhow::bail!(
                        "rate_limiting.storage_backend=database but no db pool available; \
                         refusing to fall back to memory storage (config error)"
                    );
                }
            }
            #[cfg(not(all(feature = "db-postgres", feature = "platform")))]
            {
                let _ = db_pool;
                anyhow::bail!(
                    "rate_limiting.storage_backend=database requires db-postgres + platform \
                     (crawlrs wires the Postgres shared-storage adapter only); \
                     refusing to fall back to memory storage"
                );
            }
        }
        other => {
            log::warn!(
                "Unknown rate_limiting.storage_backend '{}'; falling back to memory storage",
                other
            );
            Ok(StorageHandle::Memory)
        }
    }
}

/// Initialize rate limiting service.
///
/// 根据 `rate-limit` feature 与 `rate_limiting.storage_backend` 配置选择装配：
/// - `rate-limit` off：`NoopRateLimitingService`（放行所有请求）
/// - `rate-limit` on + `storage_backend = memory`：`LimiteronService`（进程内存储）
/// - `rate-limit` on + `storage_backend = database`（db-postgres 构建）：
///   `LimiteronService` + Postgres 共享存储（api/worker 双进程一致）
///
/// # Arguments
///
/// * `repositories` - Application repositories
/// * `settings` - Application settings
/// * `db_pool` - dbnexus 连接池（`database` 后端所需；不可用时回退 memory）
///
/// # Returns
///
/// Returns an initialized rate limiting service as `Arc<dyn RateLimitingService>`.
pub(super) async fn init_rate_limiting_service(
    repositories: &Repositories,
    settings: &Settings,
    db_pool: Option<Arc<DbPool>>,
) -> anyhow::Result<Arc<dyn RateLimitingService>> {
    #[cfg(feature = "rate-limit")]
    {
        let rate_limiting_config = build_rate_limiting_config(settings);
        let storage = select_rate_limit_storage(settings, db_pool)?;

        let service = LimiteronService::new(
            repositories.task_repo.clone(),
            repositories.tasks_backlog_repo.clone(),
            repositories.credits_repo.clone(),
            rate_limiting_config,
            storage,
        )
        .await
        .expect("Failed to create LimiteronService");

        Ok(Arc::new(service))
    }
    // rate-limit-off 时装配 NoopRateLimitingService
    #[cfg(not(feature = "rate-limit"))]
    {
        // 避免 unused 参数 warning
        let _ = repositories;
        let _ = settings;
        let _ = db_pool;
        log::warn!(
            "rate-limit feature disabled, using NoopRateLimitingService — \
             all requests are allowed without rate limiting"
        );
        Ok(Arc::new(NoopRateLimitingService::new()))
    }
}
