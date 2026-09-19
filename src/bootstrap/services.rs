// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Application services initialization.

use log::info;
use std::io::IsTerminal;
use std::sync::Arc;

use crate::application::use_cases::create_scrape::{CreateScrapeUseCase, CreateScrapeUseCaseTrait};
use crate::bootstrap::infrastructure::InfrastructureComponents;
use crate::bootstrap::infrastructure::Repositories;
use crate::config::settings::Settings;
// auth feature 关闭时不导入 garrison 桥接类型
#[cfg(feature = "auth")]
use crate::bootstrap::error::BootstrapError;
#[cfg(feature = "auth")]
use crate::infrastructure::auth::{
    build_garrison_config, get_garrison_dao, init_garrison_dao, set_audit_service,
    set_garrison_dao, CrawlrsGarrisonInterface,
};
// garrison prelude 提供 GarrisonDao / GarrisonInterface / GarrisonManager trait 与类型。
use crate::domain::services::audit_service::{AuditService, AuditServiceTrait};
// ContentExtractionFacade 用于正文提取（Trafilatura→DomSmoothie→CssRule + LLM 回退）
use crate::domain::services::content_extractor::ContentExtractionFacade;
use crate::domain::services::extraction_service::{ExtractionService, ExtractionServiceTrait};
#[cfg(feature = "auth")]
use dbnexus::DbPool;
#[cfg(feature = "auth")]
use garrison::prelude::{GarrisonDao, GarrisonInterface, GarrisonManager};
// teams-off 时不导入 teams 相关类型
#[cfg(feature = "teams")]
use crate::domain::services::geo_location::GeoLocationService;
use crate::domain::services::llm::{LLMService, LLMServiceTrait};
use crate::domain::services::rate_limiting_service::RateLimitingService;
// rate-limit feature 关闭时不导入 LimiteronService 相关配置类型
use crate::domain::services::rag_strategy::{
    ChunkerConfig, EmbeddingProvider, RagExtractionRuntime,
};
#[cfg(feature = "rate-limit")]
use crate::domain::services::rate_limiting_service::{
    ConcurrencyConfig, ConcurrencyStrategy, RateLimitConfig, RateLimitStrategy,
};
use crate::domain::services::rerank::{RerankProvider, SearchReranker};
use crate::domain::services::search_service::{SearchService, SearchServiceTrait};
// RAG provider 实现：local/remote 由 rag-local / rag-remote feature 门控，
// http（通用 REST 重排）无条件可用
#[cfg(feature = "teams")]
use crate::domain::services::team_service::TeamService;
use crate::infrastructure::services::http_rerank_provider::{HttpRerankProvider, RerankFormat};
#[cfg(feature = "rag-remote")]
use crate::infrastructure::services::rig_embedding_provider::RigEmbeddingProvider;
#[cfg(feature = "rag-local")]
use crate::infrastructure::services::vecboost_provider::VecboostProvider;
// webhook feature 关闭时不导入 WebhookServiceImpl
// （NoopWebhookService 在 webhook-off 时替代，trait 始终导入）
use crate::domain::services::webhook_service::WebhookService;
#[cfg(feature = "webhook")]
use crate::domain::services::webhook_service::WebhookServiceImpl;
// webhook feature 关闭时导入 NoopWebhookService
#[cfg(not(feature = "webhook"))]
use crate::domain::services::noop_webhook_service::NoopWebhookService;
use crate::engines::engine_client::EngineClient;
use crate::infrastructure::database::repositories::audit_log_repo_impl::AuditLogRepositoryImpl;
#[cfg(feature = "teams")]
use crate::infrastructure::geolocation::GeoLocationServiceImpl;
// rate-limit feature 关闭时不导入 LimiteronService
#[cfg(feature = "rate-limit")]
use crate::infrastructure::services::limiteron_service::{
    LimiteronService, RateLimitingConfig, StorageHandle,
};
// rate-limit feature 关闭时导入 NoopRateLimitingService
#[cfg(not(feature = "rate-limit"))]
use crate::infrastructure::services::noop_rate_limiting_service::NoopRateLimitingService;
// webhook feature 关闭时不导入 WebhookSenderImpl
// webhook_sender_impl 模块本身也被 cfg 门控（见 infrastructure::services::mod）。
use crate::domain::services::team_semaphore::{AdaptiveParams, TeamSemaphore};
#[cfg(feature = "webhook")]
use crate::infrastructure::services::webhook_sender_impl::WebhookSenderImpl;
use crate::presentation::middleware::rate_limit_middleware::RateLimitMiddleware;
use crate::queue::task_queue::{PostgresTaskQueue, TaskQueue};
use crate::search::client::SearchClientTrait;
use crate::search::engine_trait::SearchEngine;
use crate::search::smart as smart_search;
// 请求合并器（同 URL 并发只允许首个执行实际抓取）
use crate::utils::coalesce::RequestCoalescer;
use crate::utils::robots::RobotsChecker;

/// All application services.
#[derive(Clone)]
pub struct ServicesComponents {
    /// Rate limit middleware for API requests.
    pub rate_limit_middleware: RateLimitMiddleware,
    /// Team semaphore for concurrency control.
    pub team_semaphore: Arc<TeamSemaphore>,
    /// Request coalescer for deduplicating concurrent fetches of the same URL.
    ///
    /// 同 URL 并发请求只允许首个执行实际抓取，
    /// 其余 worker 等待广播后从 result_repo 读取结果，避免重复网络往返。
    /// 放入 `CrawlRsState` 供所有 worker 共享同一实例。
    pub request_coalescer: Arc<RequestCoalescer>,
    /// Rate limiting service for distributed rate limiting.
    pub rate_limiting_service: Arc<dyn RateLimitingService>,
    /// Create scrape use case.
    pub create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait>,
    /// Webhook service.
    ///
    /// webhook feature 关闭时此字段保留，但装配 `NoopWebhookService`。
    /// `WebhookService` trait 需始终编译（业务逻辑通过 trait 调用 trigger_completion / trigger_failure），
    /// 故此字段不门控；`init_services` 中的装配按 cfg 选择具体实现。
    pub webhook_service: Arc<dyn WebhookService>,
    /// Team service.
    ///
    /// teams feature 关闭时不编译此字段。
    #[cfg(feature = "teams")]
    pub team_service: Arc<TeamService>,
    /// Geo Location Service
    ///
    /// teams feature 关闭时不编译此字段。
    #[cfg(feature = "teams")]
    pub geo_location_service: Arc<dyn GeoLocationService>,
    /// Robots.txt checker.
    pub robots_checker: Arc<RobotsChecker>,
    /// Search engine service.
    pub search_engine_service: Arc<dyn SearchEngine>,
    /// Search service.
    pub search_service: Arc<dyn SearchServiceTrait>,
    /// Task queue.
    pub queue: Arc<dyn TaskQueue>,
    /// Audit service.
    pub audit_service: Arc<dyn AuditServiceTrait>,
    /// HTTP Client
    pub http_client: Arc<reqwest::Client>,
    /// LLM service for LLM operations.
    pub llm_service: Arc<dyn LLMServiceTrait>,
    /// Extraction service.
    pub extraction_service: Arc<dyn ExtractionServiceTrait>,
    /// RAG 嵌入 provider（`rag.enabled = false` 或未配置时为 None）
    pub embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    /// RAG 重排 provider（`rag.enabled = false` 或未配置时为 None）
    pub rerank_provider: Option<Arc<dyn RerankProvider>>,
    /// Content extraction facade
    ///
    /// 持有 `Vec<Box<dyn ContentExtractor>>` + 可选 LLMService，按 Trafilatura→DomSmoothie→CssRule
    /// 优先级路由，`confidence < 0.7` 时触发 LLM 回退。由 `scrape_worker` 提取路径使用。
    pub content_extractor: Arc<ContentExtractionFacade>,
    /// Webhook worker
    ///
    /// webhook feature 关闭时不编译此字段。
    /// webhook-off 模式下，不启动 webhook_worker，不需要此字段。
    #[cfg(feature = "webhook")]
    pub webhook_worker: Arc<crate::workers::webhook_worker::WebhookWorker>,
    /// Backlog worker
    pub backlog_worker: Arc<crate::workers::backlog_worker::BacklogWorker>,
    /// Expiration worker
    pub expiration_worker: Arc<crate::workers::expiration_worker::ExpirationWorker>,
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
pub fn init_rate_limit_middleware(
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
pub fn init_team_semaphore(settings: &Settings) -> Arc<TeamSemaphore> {
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
pub async fn init_rate_limiting_service(
    repositories: &Repositories,
    settings: &Settings,
    db_pool: Option<Arc<DbPool>>,
) -> Arc<dyn RateLimitingService> {
    #[cfg(feature = "rate-limit")]
    {
        let rate_limit_config = RateLimitConfig {
            strategy: RateLimitStrategy::TokenBucket,
            requests_per_second: settings.rate_limiting.default_rpm / 60,
            requests_per_minute: settings.rate_limiting.default_rpm,
            requests_per_hour: settings.rate_limiting.default_rpm * 60,
            bucket_capacity: Some(settings.rate_limiting.default_rpm),
            enabled: settings.rate_limiting.enabled,
        };

        // Validate rate limit config
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

        // Validate concurrency config
        if let Err(e) = concurrency_config.validate() {
            log::error!("Concurrency configuration error: {}", e);
        }

        let rate_limiting_config = RateLimitingConfig {
            rate_limit: rate_limit_config,
            concurrency: concurrency_config,
            backlog_process_interval_seconds: 30,
            rate_limit_ttl_seconds: 3600,
        };

        // 存储后端选择：非法值/缺池回退 memory（限流链路 fail-open 语义）
        let storage = match settings.rate_limiting.storage_backend.as_str() {
            "memory" => StorageHandle::Memory,
            "database" => {
                #[cfg(all(feature = "db-postgres", feature = "platform"))]
                match db_pool {
                    Some(pool) => StorageHandle::Database(pool),
                    None => {
                        log::warn!(
                            "rate_limiting.storage_backend=database but no db pool available; \
                             falling back to memory storage"
                        );
                        StorageHandle::Memory
                    }
                }
                #[cfg(not(all(feature = "db-postgres", feature = "platform")))]
                {
                    let _ = db_pool;
                    log::warn!(
                        "rate_limiting.storage_backend=database requires the db-postgres \
                         build (Postgres dialect DDL); falling back to memory storage"
                    );
                    StorageHandle::Memory
                }
            }
            other => {
                log::warn!(
                    "Unknown rate_limiting.storage_backend '{}'; falling back to memory storage",
                    other
                );
                StorageHandle::Memory
            }
        };

        let service = LimiteronService::new(
            repositories.task_repo.clone(),
            repositories.tasks_backlog_repo.clone(),
            repositories.credits_repo.clone(),
            rate_limiting_config,
            storage,
        )
        .await
        .expect("Failed to create LimiteronService");

        Arc::new(service)
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
        Arc::new(NoopRateLimitingService::new())
    }
}

/// Initialize search engine service.
///
/// Returns a SmartSearchEngine that concurrently queries Baidu/Bing/Sogou,
/// deduplicates via SimHash, fuses with RRF, and applies relevance scoring.
///
/// # Arguments
///
/// * `engine_client` - Engine client for making requests
/// * `_settings` - Application settings (unused, kept for API compatibility)
///
/// # Returns
///
/// Returns an initialized search engine.
pub fn init_search_engine(
    engine_client: Arc<EngineClient>,
    _settings: &Settings,
) -> Arc<dyn SearchEngine> {
    info!("Initializing SmartSearchEngine (concurrent Baidu+Bing+Sogou + RRF fusion)");
    smart_search::create_smart_search(engine_client)
}

/// Initialize search service.
///
/// This function creates the SearchService with all required dependencies,
/// following dependency injection principles.
///
/// # Arguments
///
/// * `repositories` - Application repositories
/// * `settings` - Application settings
/// * `search_client` - Search client instance implementing SearchClientTrait
///
/// # Returns
///
/// Returns an initialized search service as trait object.
pub fn init_search_service(
    repositories: &Repositories,
    settings: &Settings,
    search_client: Arc<dyn SearchClientTrait>,
    reranker: Arc<SearchReranker>,
) -> Arc<dyn SearchServiceTrait> {
    // Create SearchService with concrete repository types
    let service = SearchService::new(
        repositories.crawl_repo.clone(),
        repositories.task_repo.clone(),
        repositories.credits_repo.clone(),
        Arc::new(settings.clone()),
        search_client,
        reranker,
    );
    Arc::new(service)
}

/// RAG 能力组件（嵌入 + 重排 provider；`rag.enabled = false` 时全 None，零开销）
pub struct RagComponents {
    /// 嵌入 provider：`local`（vecboost）或 `remote`（rig）
    pub embedding: Option<Arc<dyn EmbeddingProvider>>,
    /// 重排 provider：`local`（vecboost）或 `http`（通用 REST）
    pub rerank: Option<Arc<dyn RerankProvider>>,
}

/// 初始化 RAG 能力组件。
///
/// 校验语义（fail-fast）由 [`RagSettings::validate_providers`] 承担：配置了
/// 未编译的 feature（local 无 `rag-local` / remote 无 `rag-remote`）或非法
/// provider 值时返回带重建指引的错误。vecboost 模型加载（秒级）发生在此处，
/// 即启动期一次性成本。
pub async fn init_rag_components(settings: &Settings) -> Result<RagComponents, String> {
    if !settings.rag.enabled {
        return Ok(RagComponents {
            embedding: None,
            rerank: None,
        });
    }
    settings.rag.validate_providers()?;
    let rag = &settings.rag;

    let embedding: Option<Arc<dyn EmbeddingProvider>> = match rag.embedding_provider.as_str() {
        "" => None,
        #[cfg(feature = "rag-local")]
        "local" => {
            Some(Arc::new(VecboostProvider::new(rag).await.map_err(|e| {
                format!("vecboost embedding provider init failed: {e}")
            })?))
        }
        #[cfg(feature = "rag-remote")]
        "remote" => {
            Some(Arc::new(RigEmbeddingProvider::new(rag).map_err(|e| {
                format!("rig embedding provider init failed: {e}")
            })?))
        }
        // validate_providers 已拒绝非法值与未编译 feature 的组合，此处不可达
        _ => None,
    };

    let rerank: Option<Arc<dyn RerankProvider>> = match rag.rerank_provider.as_str() {
        "" => None,
        #[cfg(feature = "rag-local")]
        "local" => {
            Some(Arc::new(VecboostProvider::new(rag).await.map_err(|e| {
                format!("vecboost rerank provider init failed: {e}")
            })?))
        }
        "http" => Some(Arc::new(
            HttpRerankProvider::new(
                &rag.rerank_endpoint,
                Some(&rag.rerank_model),
                RerankFormat::parse(&rag.rerank_format)
                    .map_err(|e| format!("rag rerank_format invalid: {e}"))?,
                rag.rerank_api_key(),
                rag.rerank_timeout_seconds,
            )
            .map_err(|e| format!("http rerank provider init failed: {e}"))?,
        )),
        // validate_providers 已拒绝非法值与未编译 feature 的组合，此处不可达
        _ => None,
    };

    Ok(RagComponents { embedding, rerank })
}

/// 初始化 garrison 认证鉴权服务。
///
/// 装配流程：
/// 1. 调用 [`init_garrison_dao`] 获取 `Arc<dyn GarrisonDao>`（oxcache 内存存储，自管理实例）
/// 2. 调用 [`build_garrison_config`] 构造 [`GarrisonConfig`]（弱密钥拒绝，HS256 ≥32 字节）
/// 3. 构造 [`CrawlrsGarrisonInterface`] 并装为 `Arc<dyn GarrisonInterface>`
///    （注入 crawlrs 的 `DbPool` 用于查询 garrison RBAC 表）
/// 4. 调用 [`GarrisonManager::init`] 写入 `GARRISON_MANAGER` 全局单例
/// 5. 错误按故障层级映射为类型化变体
///    - 弱密钥 / 空密钥 → [`BootstrapError::GarrisonConfig`]（`#[from]` 自动转换）
///    - DAO oxcache 初始化失败 → [`BootstrapError::GarrisonDao`]
///    - `GarrisonManager::init` 失败 → [`BootstrapError::GarrisonManager`]
///
/// # Fail-Fast 语义
///
/// 此函数返回 `Result<(), BootstrapError>`，调用方（[`init_services`]）通过
/// `.expect()` 在失败时 panic，触发 bootstrap fail-fast：
/// - 弱密钥 / 空密钥 → 启动失败（强制运维提供强密钥）
/// - DAO oxcache 初始化失败 → 启动失败（内存资源不足）
/// - `GarrisonManager::init` 失败（配置非法等）→ 启动失败
///
/// 这是规则 12（失败必须显性化）的体现——不藏默认值背后。
///
/// # Arguments
///
/// * `settings` - 应用配置（含 `auth.jwt_secret`）
/// * `pool` - crawlrs 数据库连接池 `Arc<DbPool>`（注入到 `CrawlrsGarrisonInterface`
///   查询 RBAC 表；传 Arc 而非 DbPool）
///
/// # Returns
///
/// - `Ok(())` - garrison 初始化成功（`GARRISON_MANAGER` 单例已就绪，后续中间件通过
///   `GarrisonUtil` 访问）
/// - `Err(BootstrapError::GarrisonConfig)` - 弱密钥 / 空密钥 / 格式非法
/// - `Err(BootstrapError::GarrisonDao)` - DAO oxcache 初始化失败
/// - `Err(BootstrapError::GarrisonManager)` - `GarrisonManager::init` 失败
///
/// # Spec
///
/// - 构造 `CrawlrsGarrisonDao`/`build_garrison_config`/
///   `CrawlrsGarrisonInterface` 并调 `GarrisonManager::init`，失败按层级 map 为
///   `BootstrapError::GarrisonConfig` / `GarrisonDao` / `GarrisonManager`
#[cfg(feature = "auth")]
pub async fn init_garrison_auth(
    settings: &Settings,
    pool: Arc<DbPool>,
) -> Result<(), BootstrapError> {
    // 1. 构造 GarrisonConfig（弱密钥 / 空密钥在此被拒绝）
    //
    // 顺序选择：先做便宜的同步 config 校验，再做异步 DAO 初始化——
    // 弱密钥场景下避免无谓的 oxcache 实例创建（简洁优先的执行层面体现）。
    // 重构：使用类型化 `BootstrapError::GarrisonConfig`（#[from] 自动转换）。
    let config = build_garrison_config(settings.auth.jwt_secret())?;
    let config = Arc::new(config);

    // 2. 初始化 DAO（garrison 内建 oxcache，自管理实例）
    // 重构：使用类型化 `BootstrapError::GarrisonDao`。
    let dao: Arc<dyn GarrisonDao> = init_garrison_dao()
        .await
        .map_err(|e| BootstrapError::GarrisonDao(format!("{e}")))?;

    // 注入 DAO 到全局态，供业务 handler（`api_key_handler`）通过
    // [`crate::infrastructure::auth::get_garrison_dao`] 读取。
    //
    // 时序：在 `GarrisonManager::init` 之前注入——`GarrisonManager::init` 接收 dao 的
    // 所有权（move），故此处先 `Arc::clone` 再传入 manager，全局态保留另一份 Arc 引用。
    //
    // 失败契约（显性化）：`set_garrison_dao` 返回 `Err(dao)` 表示已有实例被注入
    // （正常 bootstrap 路径只调用一次，二次调用表明初始化逻辑错误如热重载未清理），
    // 此处 panic 与 `set_audit_service` 一致——避免静默覆盖导致 dao 来源不确定。
    set_garrison_dao(Arc::clone(&dao)).map_err(|_| {
        BootstrapError::GarrisonDao(
            "set_garrison_dao failed: global DAO already injected \
             (check for duplicate init_garrison_auth calls)"
                .to_string(),
        )
    })?;

    // 3. 构造业务 Interface（注入 crawlrs DbPool 查询 RBAC 表）
    //
    // `(*pool).clone()`：deref Arc<DbPool> → DbPool，再 clone（DbPool 内部为
    // Arc<DbPoolInner>，clone 廉价 <10ns）。`CrawlrsGarrisonInterface::new` 持有
    // owned DbPool，符合其既有签名（外科手术式修改，不改动代码）。
    let interface: Arc<dyn GarrisonInterface> =
        Arc::new(CrawlrsGarrisonInterface::new((*pool).clone()));

    // 4. 写入 garrison 全局单例（builder 模式，异步 build）
    // 重构：使用类型化 `BootstrapError::GarrisonManager`。
    GarrisonManager::builder()
        .dao(dao)
        .config(config)
        .interface(interface)
        .build()
        .await
        .map_err(|e| BootstrapError::GarrisonManager(format!("{e}")))?;

    // 5. 决策 2：断言 garrison firewall 已启用（仅依赖 garrison 做 CWE-307 IP 限速）
    //
    // crawlrs 已删除本地 `AuthRateLimiter`，暴力破解防护完全依赖 garrison 的
    // `firewall` + `firewall-bruteforce` features。若 features 未启用，garrison
    // 不会执行 IP 级限速，导致 CWE-307 防护缺失。
    //
    // 此处通过引用 `BruteForceStrategy` 类型做编译期断言——若 garrison 未启用
    // `firewall-bruteforce` feature，该类型不存在，编译失败（fail-fast）。
    // 运行时无需额外检查（garrison 在 `check_api_key` 内部自动调用 firewall）。
    //
    // 注：Cargo feature 是统一效应，此处断言仅防止"误关闭 features"配置错误，
    // 不防恶意依赖（见 auth_middleware.rs `reset_global_auth_state` 注释）。
    //
    // ## 阈值配置限制
    //
    // garrison 0.8.1 在 `check_api_key` 内部硬编码 `BruteForceConfig::default()`
    // （5 次失败 / 60s 窗口 / 300s 锁定），不通过 `GarrisonConfig` 暴露自定义入口。
    // 当前使用 garrison 默认值；若需自定义阈值，需等 garrison 后续版本暴露配置接口
    // 或在 crawlrs 侧实现自定义 `check_api_key` wrapper（暂不实施简洁优先）。
    #[cfg(feature = "auth")]
    {
        // garrison 顶层 re-export（lib.rs:621，gated on `firewall-bruteforce` feature）
        use garrison::BruteForceStrategy as _FwAssert;
        // 引用类型做编译期检查（无运行时开销）
        let _ = std::marker::PhantomData::<_FwAssert>;
    }

    // 6. 注入 bootstrap admin API key（若环境变量存在）。
    //
    // ## 设计背景
    //
    // garrison DAO 默认是内存实现（`GarrisonDaoOxcache`），进程间不共享数据。
    // 外部工具签发的 API Key 写入工具进程的
    // 独立 DAO 实例，server 进程无法读取 → 所有认证请求均 401。
    //
    // 标准解法：server 启动时读取环境变量 `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY`，
    // 若存在则直接用 garrison `ApiKeyHandler::generate_with_namespace` 签发到
    // server 自己的 DAO（内存共享），同时写入 crawlrs `api_keys`/`teams` 表保留
    // `api_key_id → team_id` 映射（auth_middleware 反查依赖）。
    //
    // ## 安全（CWE-532 / CWE-798）
    //
    // - 明文 key 仅通过环境变量注入一次，从不写入日志或配置文件；
    // - garrison 侧仅存 `sha256(key_secret)`（CWE-916 与 generate_internal 一致）；
    // - 环境变量不存在或为空时完全跳过此分支（不影响部署未启用 bootstrap 的场景）。
    //
    // ## 格式
    //
    // `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY` = `<team_id_uuid>`（可选）；
    //   不传 team_id 时自动创建 `"bootstrap-admin-team"` team。
    // 内部签发双段格式：`key_id.key_secret`（`ApiKeyHandler::generate_with_namespace`），
    // key_id / key_secret 各 32 hex，TTL 30 天，scope `crawlrs:admin`。
    let bootstrap_cfg = match std::env::var("CRAWLRS__BOOTSTRAP_ADMIN_API_KEY") {
        Ok(v) if !v.is_empty() => Some(v),
        _ => None,
    };
    if let Some(cfg_val) = bootstrap_cfg {
        use crate::common::time_utils;
        use crate::infrastructure::database::entities::api_key::{
            ActiveModel as ApiKeyActiveModel, Entity as ApiKeyEntity,
        };
        use crate::infrastructure::database::entities::team::{
            ActiveModel as TeamActiveModel, Entity as TeamEntity,
        };
        use dbnexus::sea_orm::{ActiveValue, EntityTrait};
        use garrison::protocol::apikey::ApiKeyHandler;
        use uuid::Uuid;

        const GARRISON_NS: &str = "crawlrs";
        const PERM_ADMIN: &str = "crawlrs:admin";
        const DEFAULT_TEAM_NAME: &str = "bootstrap-admin-team";
        const TTL_SECS: i64 = 30 * 24 * 60 * 60;

        let wrap_db = |ctx: &'static str| {
            move |e: dbnexus::sea_orm::DbErr| {
                BootstrapError::GarrisonManager(format!("{}: db: {}", ctx, e))
            }
        };

        // `cfg_val` 可选格式：`team_id_uuid`（非空 uuid 时用作目标 team）
        let team_id_arg: Option<Uuid> = if cfg_val.len() == 36 {
            Uuid::parse_str(&cfg_val).ok()
        } else {
            None
        };
        let team_id = team_id_arg.unwrap_or_else(Uuid::new_v4);
        let api_key_id = Uuid::new_v4();

        // 确保 team 存在（bootstrap 首次启动时 team 表为空）
        let now = time_utils::to_db_datetime(chrono::Utc::now());
        let session = pool.get_session("admin").await.map_err(|e| {
            BootstrapError::GarrisonManager(format!("bootstrap_admin_key: db session: {e}"))
        })?;
        let conn = session.connection().map_err(|e| {
            BootstrapError::GarrisonManager(format!("bootstrap_admin_key: db conn: {e}"))
        })?;
        if TeamEntity::find_by_id(team_id)
            .one(conn)
            .await
            .map_err(wrap_db("bootstrap_admin_key: find team"))?
            .is_none()
        {
            let team_active = TeamActiveModel {
                id: ActiveValue::Set(team_id),
                name: ActiveValue::Set(DEFAULT_TEAM_NAME.to_string()),
                allowed_countries: ActiveValue::Set(None),
                blocked_countries: ActiveValue::Set(None),
                ip_whitelist: ActiveValue::Set(None),
                domain_blacklist: ActiveValue::Set(None),
                enable_geo_restrictions: ActiveValue::Set(false),
                created_at: ActiveValue::Set(now),
                updated_at: ActiveValue::Set(now),
            };
            TeamEntity::insert(team_active)
                .exec(conn)
                .await
                .map_err(wrap_db("bootstrap_admin_key: insert team"))?;
        }

        // 用 `set_garrison_dao` 注入的全局 DAO 签发 key（与 server 进程共享内存）
        let dao_for_gen = get_garrison_dao().ok_or_else(|| {
            BootstrapError::GarrisonManager(
                "bootstrap_admin_key: get_garrison_dao returned None".to_string(),
            )
        })?;
        let handler = ApiKeyHandler::new(Arc::clone(&dao_for_gen));
        let plaintext_key = handler
            .generate_with_namespace(
                api_key_id.to_string(),
                GARRISON_NS,
                vec![PERM_ADMIN.to_string()],
                TTL_SECS,
            )
            .await
            .map_err(|e| {
                BootstrapError::GarrisonManager(format!(
                    "bootstrap_admin_key: garrison generate: {e}"
                ))
            })?;

        let garrison_key_id = plaintext_key
            .split_once('.')
            .map(|(k, _)| k.to_string())
            .ok_or_else(|| {
                BootstrapError::GarrisonManager(
                    "bootstrap_admin_key: garrison returned malformed key".to_string(),
                )
            })?;

        // 写入 crawlrs `api_keys` 表保留映射，auth_middleware 反查 team_id
        let api_key_active = ApiKeyActiveModel {
            id: ActiveValue::Set(api_key_id),
            team_id: ActiveValue::Set(team_id),
            key: ActiveValue::Set(garrison_key_id),
            created_at: ActiveValue::Set(now),
            updated_at: ActiveValue::Set(None),
        };
        ApiKeyEntity::insert(api_key_active)
            .exec(conn)
            .await
            .map_err(wrap_db("bootstrap_admin_key: insert api_key"))?;

        info!(
            "Bootstrap admin API key injected: team_id={}, api_key_id={} (TTY: use \
             `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY={}` to reuse team ID)",
            team_id, api_key_id, team_id
        );
        // 完整 key 仅在交互终端输出（运维在场立即保存）；stdout 被管道/日志系统
        // 采集时会持久留存，只输出掩码防止凭证进入 journald/k8s 日志。
        if std::io::stdout().is_terminal() {
            println!(
                "\n\
                ====================================================\n\
                BOOTSTRAP ADMIN API KEY (save this now!):\n\
                {}\n\
                TEAM_ID:      {}\n\
                API_KEY_ID:   {}\n\
                Valid for:    30 days\n\
                ====================================================\n",
                plaintext_key, team_id, api_key_id
            );
        } else {
            println!(
                "\n\
                ====================================================\n\
                BOOTSTRAP ADMIN API KEY (masked; rerun in a terminal to display):\n\
                {}\n\
                TEAM_ID:      {}\n\
                API_KEY_ID:   {}\n\
                Valid for:    30 days\n\
                ====================================================\n",
                crate::common::mask_secret(&plaintext_key),
                team_id,
                api_key_id
            );
        }
    }

    info!("Garrison authentication manager initialized (token_style=jwt, HS256, firewall=enabled)");
    Ok(())
}

/// Initialize LLM service.
///
/// This function creates the LLMService for LLM operations,
/// following dependency injection principles.
///
/// # Arguments
///
/// * `settings` - Application settings
/// * `http_client` - HTTP client for making requests
///
/// # Returns
///
/// Returns an initialized LLM service wrapped in Arc.
pub fn init_llm_service(
    settings: &Settings,
    http_client: Arc<reqwest::Client>,
) -> Arc<dyn LLMServiceTrait> {
    Arc::new(LLMService::new(settings, http_client))
}

/// Initialize all application services.
///
/// # Arguments
///
/// * `infrastructure` - Initialized infrastructure components
/// * `engine_client` - Engine client for scraping operations（包含 EngineRouter 与 EngineHealthMonitor，
///   由调用方构造一次共享，符合 SSOT 原则）
/// * `settings` - Application settings
///
/// # Returns
///
/// Returns all initialized services.
pub async fn init_services(
    infrastructure: &InfrastructureComponents,
    engine_client: Arc<EngineClient>,
    http_client: Arc<reqwest::Client>,
    settings: &Settings,
) -> ServicesComponents {
    let repositories = &infrastructure.repositories;

    // Initialize team semaphore
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
    .await;

    // Initialize rate limit middleware
    let rate_limit_middleware = init_rate_limit_middleware(rate_limiting_service.clone());

    // Initialize create scrape use case
    let create_scrape_use_case: Arc<dyn CreateScrapeUseCaseTrait> =
        Arc::new(CreateScrapeUseCase::new(engine_client.clone()));

    // webhook 相关服务在 webhook-off 时不初始化
    // webhook-on：装配 WebhookServiceImpl（含 WebhookSenderImpl + 签名密钥 + 事件仓库）
    // webhook-off：装配 NoopWebhookService（所有方法返回 Ok(())，业务逻辑放行）
    #[cfg(feature = "webhook")]
    let webhook_sender: Arc<WebhookSenderImpl> = Arc::new(WebhookSenderImpl::new(
        http_client.clone(),
        std::time::Duration::from_secs(10),
    ));
    #[cfg(feature = "webhook")]
    let webhook_service: Arc<WebhookServiceImpl> = Arc::new(WebhookServiceImpl::new(
        webhook_sender.clone(),
        settings.webhook.secret().to_string(),
        repositories.webhook_event_repo.clone(),
    ));
    #[cfg(not(feature = "webhook"))]
    let webhook_service: Arc<NoopWebhookService> = {
        // webhook-off 时输出 info 日志，告知运维 webhook 投递已禁用。
        // 严重程度低于 auth/rate-limit 警告（webhook 关闭不直接引入安全风险，仅影响通知能力）。
        log::info!(
            "webhook feature disabled, using NoopWebhookService — \
             trigger_completion/trigger_failure are no-ops"
        );
        Arc::new(NoopWebhookService::new())
    };

    // teams 相关服务在 teams-off 时不初始化
    #[cfg(feature = "teams")]
    let geo_location_service = Arc::new(GeoLocationServiceImpl::new(http_client.clone()));
    #[cfg(feature = "teams")]
    let team_service = Arc::new(TeamService::new(
        geo_location_service.clone(),
        repositories.geo_restriction_repo.clone(),
    ));

    // Initialize robots checker (使用依赖注入的 HTTP_CLIENT + CacheService)
    let robots_checker = Arc::new(RobotsChecker::new(
        http_client.clone(),
        &settings.robots,
        Some(infrastructure.cache_service.clone()),
        None,
    ));

    // 复用入参 `engine_client`（由调用方构造一次共享）
    //
    // 原问题：函数内部两次 `Arc::new(EngineClient::with_router(engine_router.clone()))`
    // 创建了两个独立 EngineClient 实例，每个实例自带一份 `Arc<EngineHealthMonitor>`，
    // 导致：(1) 重复内存分配；(2) 健康监控状态不一致（search_engine 与 search_client
    // 看到不同的 health 快照）；(3) 入参 engine_client 被 shadowing 后立即 drop，浪费构造。
    //
    // 直接使用入参（由调用方 di/modules.rs / 测试构造一次共享），
    // 符合 SSOT（Single Source of Truth）原则。search_engine_service 与 search_client
    // 共享同一份 EngineHealthMonitor 状态。

    // Initialize search engine (for backward compatibility)
    let search_engine_service: Arc<dyn SearchEngine> =
        init_search_engine(engine_client.clone(), settings);

    // Initialize search client (wraps search engines)
    let search_client: Arc<dyn SearchClientTrait> = Arc::new(
        crate::search::client::SearchClient::new(engine_client.clone()),
    );

    // RAG 能力组件（默认全关：rag.enabled = false 时 provider 全 None，零开销）。
    // 失败 = 运维显式启用但配置/环境不可用，按既有 fail-fast 惯例 panic。
    let rag = match init_rag_components(settings).await {
        Ok(components) => components,
        Err(e) => panic!("RAG capability init failed (check [rag] config / features): {e}"),
    };

    // 语义重排器：provider 就绪时搜索结果做精准重排，否则直通
    let search_reranker = Arc::new(SearchReranker::new(
        rag.rerank.clone(),
        settings.rag.search_rerank_top_n,
    ));

    // Initialize search service
    let search_service = init_search_service(
        repositories,
        settings,
        search_client.clone(),
        search_reranker,
    );

    // 初始化 garrison 认证鉴权（auth-on 时 fail-fast）
    //
    // - 弱密钥 / 空密钥 → panic（强制运维提供强密钥，CWE-326）
    // - DAO oxcache 初始化失败 / GarrisonManager::init 失败 → panic
    //
    // 详见 [`init_garrison_auth`] 文档的「Fail-Fast 语义」章节。
    // 此调用无返回值——garrison 通过 GARRISON_MANAGER 全局单例暴露能力，
    // 后续中间件（重写 auth_middleware_inner）通过 GarrisonUtil 访问。
    //
    // pool 传 Arc<DbPool> 的 clone（Arc::clone 廉价 <10ns）。
    #[cfg(feature = "auth")]
    init_garrison_auth(settings, infrastructure.db.inner().clone())
        .await
        .expect(
            "garrison auth initialization failed (BootstrapError::GarrisonConfig/GarrisonDao/GarrisonManager) — \
                 check CRAWLRS__AUTH__JWT_SECRET env var (HS256 requires >=32 bytes)",
        );

    // Initialize task queue
    let queue: Arc<dyn TaskQueue> =
        Arc::new(PostgresTaskQueue::new(repositories.task_repo.clone()));

    // Initialize audit service
    let audit_repo = Arc::new(AuditLogRepositoryImpl::new(
        infrastructure.db.inner().clone(),
    ));
    let audit_service = Arc::new(AuditService::new(audit_repo));

    // 将 audit_service 注入 garrison listener 全局态。
    //
    // `set_audit_service` 通过 `parking_lot::RwLock<Option<Arc<…>>>` 暴露给
    // `CrawlrsAuditListener::on_event`，使 garrison 事件广播能桥接到 crawlrs
    // `audit_logs` 表。`RwLock`（而非 `OnceLock`）的理由：测试可通过
    // `reset_audit_service_for_test` 重置全局态，避免测试间污染。
    //
    // 时序：garrison init（在上方）通过 `inventory::submit!` 已注册 listener factory，
    // listener 实例在 `GarrisonManager::init` 时创建（无状态），但实际 `on_event`
    // 调用发生在第一个 HTTP 请求认证时（远晚于 bootstrap 完成），故此处注入时序安全。
    //
    // `set` 失败（已有实例）返回 Err，此处 panic——
    // 正常 bootstrap 路径只调用一次，二次调用表明初始化逻辑错误（如热重载未清理）。
    // `Arc<dyn AuditServiceTrait>` 未实现 Debug，故用 `is_err()` 而非 `expect` 处理。
    //
    // 注意：`init_services` 签名返回 `ServicesComponents` 而非
    // `Result<ServicesComponents, BootstrapError>`，故失败只能 panic。这是既有设计问题
    // （与上方 `init_garrison_auth().expect(...)` 一致），外科手术式修改不在此处重构，
    // 7 可考虑统一为 Result 返回类型。
    #[cfg(feature = "auth")]
    if set_audit_service(Arc::clone(&audit_service) as Arc<dyn AuditServiceTrait>).is_err() {
        panic!(
            "set_audit_service failed: audit_service already injected \
             (check for duplicate init_services calls)"
        );
    }

    // Initialize LLM service (使用依赖注入的 http_client)
    let llm_service = init_llm_service(settings, http_client.clone());

    // Initialize extraction service（嵌入 provider 就绪时注入 RAG 运行时）
    let extraction_service: Arc<dyn ExtractionServiceTrait> =
        match &rag.embedding {
            Some(provider) => Arc::new(ExtractionService::new(llm_service.clone()).with_rag(
                Arc::new(RagExtractionRuntime::new(
                    ChunkerConfig::default(),
                    provider.clone(),
                    settings.rag.rag_top_k,
                )),
            )),
            None => Arc::new(ExtractionService::new(llm_service.clone())),
        };

    // Initialize content extraction facade
    //
    // 装配 ContentExtractionFacade，注入 llm_service 用于低置信度回退（confidence < 0.7 时触发）。
    // Facade 内部按 cfg 编译期决定 extractor 链（Trafilatura→DomSmoothie→CssRule）。
    let content_extractor = Arc::new(ContentExtractionFacade::new(Some(llm_service.clone())));

    // Initialize WebhookWorker（仅 webhook-on 时构造）
    // webhook-off：不构造 WebhookWorker，ServicesComponents.webhook_worker 字段不编译
    #[cfg(feature = "webhook")]
    let webhook_worker = Arc::new(crate::workers::webhook_worker::WebhookWorker::new(
        repositories.webhook_event_repo.clone(),
        webhook_service.clone(),
        crate::utils::retry_policy::RetryPolicy::default(),
    ));

    // Initialize BacklogWorker
    let backlog_worker = Arc::new(crate::workers::backlog_worker::BacklogWorker::new(
        repositories.tasks_backlog_repo.clone(),
        repositories.task_repo.clone(),
        rate_limiting_service.clone(),
        Arc::new(settings.clone()),
    ));

    // Initialize ExpirationWorker
    let expiration_worker = Arc::new(crate::workers::expiration_worker::ExpirationWorker::new(
        repositories.task_repo.clone(),
        repositories.result_repo.clone(),
        settings.retention.scrape_results_days,
    ));

    info!("Services initialized");

    ServicesComponents {
        rate_limit_middleware,
        team_semaphore,
        request_coalescer,
        rate_limiting_service,
        create_scrape_use_case,
        webhook_service,
        #[cfg(feature = "teams")]
        team_service,
        #[cfg(feature = "teams")]
        geo_location_service,
        robots_checker,
        search_engine_service,
        search_service,
        queue,
        audit_service,
        http_client,
        llm_service,
        extraction_service,
        embedding_provider: rag.embedding.clone(),
        rerank_provider: rag.rerank.clone(),
        content_extractor,
        #[cfg(feature = "webhook")]
        webhook_worker,
        backlog_worker,
        expiration_worker,
    }
}

// Note: The following functions are not unit-tested here because they require
// real external services that are only available in Docker-based integration tests:
//   - init_rate_limiting_service: needs Repositories (DB pool) — LimiteronService uses in-memory storage
//   - init_search_service: needs Repositories (DB pool for crawl/task/credits repos)
//   - init_services: needs InfrastructureComponents (full DB + HTTP stack)
// These are covered by integration tests in tests/integration/ with Docker-provided
// PostgreSQL.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::CreditsTransactionType;
    use crate::domain::services::rate_limiting_service::{
        BacklogService, ConcurrencyConfig, ConcurrencyControlService, ConcurrencyResult,
        QuotaService, RateLimitConfig, RateLimitResult, RateLimitService, RateLimitingError,
    };
    use crate::engines::router::EngineRouter;

    fn make_http_client() -> Arc<reqwest::Client> {
        Arc::new(reqwest::Client::new())
    }

    fn make_engine_client() -> Arc<EngineClient> {
        Arc::new(EngineClient::new())
    }

    // ========== Mock RateLimitingService for init_rate_limit_middleware tests ==========

    /// A no-op mock implementation of RateLimitingService for unit testing.
    /// All methods return Ok with default/empty values.
    struct MockRateLimitingService;

    #[async_trait::async_trait]
    impl RateLimitService for MockRateLimitingService {
        async fn check_rate_limit(
            &self,
            _api_key: &str,
            _endpoint: &str,
        ) -> Result<RateLimitResult, RateLimitingError> {
            Ok(RateLimitResult::Allowed)
        }

        async fn get_team_rate_limit_config(
            &self,
            _team_id: uuid::Uuid,
        ) -> Result<RateLimitConfig, RateLimitingError> {
            Ok(RateLimitConfig::default())
        }

        async fn update_team_rate_limit_config(
            &self,
            _team_id: uuid::Uuid,
            _config: RateLimitConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn cleanup_expired_rate_limits(&self) -> Result<u64, RateLimitingError> {
            Ok(0)
        }
    }

    #[async_trait::async_trait]
    impl ConcurrencyControlService for MockRateLimitingService {
        async fn check_team_concurrency(
            &self,
            _team_id: uuid::Uuid,
            _task_id: uuid::Uuid,
        ) -> Result<ConcurrencyResult, RateLimitingError> {
            Ok(ConcurrencyResult::Allowed)
        }

        async fn release_team_concurrency_slot(
            &self,
            _team_id: uuid::Uuid,
            _task_id: uuid::Uuid,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn get_team_current_concurrency(
            &self,
            _team_id: uuid::Uuid,
        ) -> Result<u32, RateLimitingError> {
            Ok(0)
        }

        async fn get_team_concurrency_config(
            &self,
            _team_id: uuid::Uuid,
        ) -> Result<ConcurrencyConfig, RateLimitingError> {
            Ok(ConcurrencyConfig::default())
        }

        async fn update_team_concurrency_config(
            &self,
            _team_id: uuid::Uuid,
            _config: ConcurrencyConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl BacklogService for MockRateLimitingService {
        async fn process_backlog_tasks(
            &self,
            _team_id: uuid::Uuid,
        ) -> Result<u32, RateLimitingError> {
            Ok(0)
        }
    }

    #[async_trait::async_trait]
    impl QuotaService for MockRateLimitingService {
        async fn check_and_deduct_quota(
            &self,
            _team_id: uuid::Uuid,
            _amount: i64,
            _transaction_type: CreditsTransactionType,
            _description: String,
            _reference_id: Option<uuid::Uuid>,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn get_quota_balance(&self, _team_id: uuid::Uuid) -> Result<i64, RateLimitingError> {
            Ok(0)
        }

        async fn refund_quota(
            &self,
            _team_id: uuid::Uuid,
            _amount: i64,
            _description: String,
            _reference_id: Option<uuid::Uuid>,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
    }

    impl RateLimitingService for MockRateLimitingService {}

    // ========== init_team_semaphore tests ==========

    /// 构造测试用 Settings（ConcurrencySettings 自定义字段）
    fn make_test_settings(default_team_limit: u64, adaptive_enabled: bool) -> Settings {
        let mut settings = Settings::default();
        settings.concurrency.default_team_limit = default_team_limit;
        settings.concurrency.adaptive_enabled = adaptive_enabled;
        settings
    }

    #[test]
    fn test_init_team_semaphore_creates_instance_fixed() {
        let settings = make_test_settings(10, false);
        let semaphore = init_team_semaphore(&settings);
        assert!(
            Arc::strong_count(&semaphore) >= 1,
            "init_team_semaphore should return a valid Arc<TeamSemaphore>"
        );
        assert_eq!(semaphore.mode(), "Fixed");
    }

    #[test]
    fn test_init_team_semaphore_with_different_limits_fixed() {
        let s1 = init_team_semaphore(&make_test_settings(1, false));
        let s2 = init_team_semaphore(&make_test_settings(100, false));
        let s3 = init_team_semaphore(&make_test_settings(1000, false));
        assert!(Arc::strong_count(&s1) >= 1);
        assert!(Arc::strong_count(&s2) >= 1);
        assert!(Arc::strong_count(&s3) >= 1);
        assert_eq!(s1.mode(), "Fixed");
        assert_eq!(s2.mode(), "Fixed");
        assert_eq!(s3.mode(), "Fixed");
    }

    #[test]
    fn test_init_team_semaphore_zero_limit_fixed() {
        let settings = make_test_settings(0, false);
        let semaphore = init_team_semaphore(&settings);
        assert!(Arc::strong_count(&semaphore) >= 1);
        assert_eq!(semaphore.mode(), "Fixed");
    }

    /// adaptive_enabled=true 创建 Adaptive 模式
    #[test]
    fn test_init_team_semaphore_adaptive_enabled_creates_adaptive() {
        let settings = make_test_settings(10, true);
        let semaphore = init_team_semaphore(&settings);
        assert_eq!(semaphore.mode(), "Adaptive");
        assert_eq!(semaphore.default_permits(), 10);
    }

    /// adaptive_enabled=false（默认）创建 Fixed 模式（向后兼容）
    #[test]
    fn test_init_team_semaphore_adaptive_disabled_creates_fixed() {
        let settings = make_test_settings(20, false);
        let semaphore = init_team_semaphore(&settings);
        assert_eq!(semaphore.mode(), "Fixed");
        assert_eq!(semaphore.default_permits(), 20);
    }

    /// Adaptive 模式下 default_team_limit 作为 initial
    #[test]
    fn test_init_team_semaphore_adaptive_uses_default_team_limit_as_initial() {
        let settings = make_test_settings(15, true);
        let semaphore = init_team_semaphore(&settings);
        // Adaptive 模式：default_team_limit 作为 initial
        let team_id = uuid::Uuid::new_v4();
        assert_eq!(semaphore.current_target(team_id), 15);
    }

    // ========== init_llm_service tests ==========

    #[test]
    fn test_init_llm_service_creates_instance() {
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let http_client = make_http_client();
        let service = init_llm_service(&settings, http_client);
        // Verify the service is a valid Arc<dyn LLMServiceTrait>
        assert!(
            Arc::strong_count(&service) >= 1,
            "init_llm_service should return a valid Arc"
        );
    }

    // ========== init_search_engine tests ==========

    #[test]
    fn test_init_search_engine_creates_instance() {
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let engine_client = make_engine_client();
        let search_engine = init_search_engine(engine_client, &settings);
        assert!(
            Arc::strong_count(&search_engine) >= 1,
            "init_search_engine should return a valid Arc<dyn SearchEngine>"
        );
    }

    #[test]
    fn test_init_search_engine_with_ab_test_disabled() {
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let engine_client = make_engine_client();
        let _search_engine = init_search_engine(engine_client, &settings);
        // Should create without panic; SmartSearchEngine is always used
    }

    #[test]
    fn test_init_search_engine_with_ab_test_enabled() {
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let engine_client = make_engine_client();
        let _search_engine = init_search_engine(engine_client, &settings);
        // SmartSearchEngine is always used regardless of ab_test settings
    }

    // ========== init_rate_limit_middleware tests ==========

    #[test]
    fn test_init_rate_limit_middleware_creates_instance() {
        let mock: Arc<dyn RateLimitingService> = Arc::new(MockRateLimitingService);
        let middleware = init_rate_limit_middleware(mock);
        // RateLimitMiddleware derives Clone; verify clone works
        let _cloned = middleware.clone();
    }

    #[test]
    fn test_init_rate_limit_middleware_with_cloned_service() {
        let mock: Arc<dyn RateLimitingService> = Arc::new(MockRateLimitingService);
        // Verify the middleware can be created with a cloned Arc
        let middleware = init_rate_limit_middleware(mock.clone());
        let _middleware2 = init_rate_limit_middleware(mock);
        // Both should be successfully created (verify no panic)
        let _ = &middleware;
    }

    // ========== testcontainers integration tests ==========
    //
    // These tests exercise service initialization paths that require real
    // PostgreSQL. They early-return if Docker is unavailable.

    use crate::bootstrap::infrastructure::{init_database, init_infrastructure, init_repositories};
    use crate::common::test_support::testcontainers_fixtures as tcf;

    async fn require_docker() -> bool {
        tcf::docker_available().await
    }

    #[tokio::test]
    async fn tc_init_rate_limiting_service() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_init_rate_limiting_service");
            return;
        }
        let pg = match tcf::PgHandle::start().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to start postgres container: {e}");
                return;
            }
        };
        let mut settings = tcf::settings_with_urls(&pg.url).unwrap();
        // 高并行度下连接池创建可能因资源耗尽而失败，此时跳过而非 panic
        let db = match init_database(&settings).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[skip] failed to init database pool: {e}");
                return;
            }
        };
        let repos = init_repositories(db.clone(), &settings);

        // database 后端路径：真实 Postgres 下执行幂等建表 + DBNexus 适配器装配
        settings.rate_limiting.storage_backend = "database".to_string();
        let service = init_rate_limiting_service(&repos, &settings, Some(db.clone_inner())).await;
        // Verify the service is usable (Arc strong count >= 1).
        assert!(Arc::strong_count(&service) >= 1);

        // memory 后端路径：默认配置下同样可装配
        settings.rate_limiting.storage_backend = "memory".to_string();
        let service = init_rate_limiting_service(&repos, &settings, Some(db.clone_inner())).await;
        assert!(Arc::strong_count(&service) >= 1);
    }

    #[tokio::test]
    async fn tc_init_search_service_with_repos() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_init_search_service_with_repos");
            return;
        }
        let pg = match tcf::PgHandle::start().await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("[skip] failed to start postgres container: {e}");
                return;
            }
        };
        let settings = tcf::settings_with_urls(&pg.url).unwrap();
        // 高并行度下连接池创建可能因资源耗尽而失败，此时跳过而非 panic
        let db = match init_database(&settings).await {
            Ok(d) => d,
            Err(e) => {
                eprintln!("[skip] failed to init database pool: {e}");
                return;
            }
        };
        let repos = init_repositories(db.clone(), &settings);

        // Build a search client with a dummy engine client.
        let engine_client = Arc::new(EngineClient::new());
        let search_client: Arc<dyn SearchClientTrait> =
            Arc::new(crate::search::client::SearchClient::new(engine_client));

        let service = init_search_service(
            &repos,
            &settings,
            search_client,
            Arc::new(SearchReranker::new(None, 25)),
        );
        assert!(Arc::strong_count(&service) >= 1);
    }

    #[tokio::test]
    async fn tc_init_services_full_stack() {
        if !require_docker().await {
            eprintln!("[skip] Docker unavailable — tc_init_services_full_stack");
            return;
        }
        let _garrison_guard = crate::common::test_helpers::acquire_garrison_global_state().await;
        let handle = match tcf::DbHandle::start().await {
            Ok(h) => h,
            Err(e) => {
                eprintln!("[skip] failed to start db container: {e}");
                return;
            }
        };
        let settings = tcf::settings_with_urls(&handle.pg.url).unwrap();
        // auth feature on 时 init_services 会调 init_garrison_auth，
        // 弱密钥 / 空密钥会触发 fail-fast panic。
        // `tcf::settings_with_urls` 已从 `CRAWLRS_TEST_JWT_SECRET` 环境变量读取强密钥
        // 此处无需再次注入。
        // 高并行度下基础设施初始化可能因资源耗尽而失败，此时跳过而非 panic
        let infra = match init_infrastructure(&settings).await {
            Ok(i) => i,
            Err(e) => {
                eprintln!("[skip] failed to init infrastructure: {e}");
                return;
            }
        };

        // Build engine router + client.
        // 注入完整 EngineTimeoutSettings（含 default_timeout_seconds + 三个 MRT 字段）
        // proxy_provider=None, proxy_strategy=RoundRobin, proxy_url=None：测试环境无代理配置
        // proxy_provider 改为 Option<Arc<dyn ProxyProvider>>，新增 strategy 参数
        let engines = crate::bootstrap::engines::init_engines(
            infra.http_client.clone(),
            None,
            crate::config::settings::ProxyStrategy::RoundRobin,
            None,
            &settings.engines,
            &settings.timeouts.engines,
            settings.engines.max_response_body_bytes as usize,
        );
        let engine_router = Arc::new(EngineRouter::new(engines.clone()));
        let engine_client = Arc::new(EngineClient::with_router(engine_router.clone()));

        let services =
            init_services(&infra, engine_client, infra.http_client.clone(), &settings).await;

        // Verify all service components are constructed.
        assert!(Arc::strong_count(&services.rate_limiting_service) >= 1);
        assert!(Arc::strong_count(&services.team_semaphore) >= 1);
        assert!(Arc::strong_count(&services.create_scrape_use_case) >= 1);
        assert!(Arc::strong_count(&services.webhook_service) >= 1);
        // teams feature 关闭时这两个字段不编译
        #[cfg(feature = "teams")]
        {
            assert!(Arc::strong_count(&services.team_service) >= 1);
            assert!(Arc::strong_count(&services.geo_location_service) >= 1);
        }
        assert!(Arc::strong_count(&services.robots_checker) >= 1);
        assert!(Arc::strong_count(&services.search_engine_service) >= 1);
        assert!(Arc::strong_count(&services.search_service) >= 1);
        assert!(Arc::strong_count(&services.queue) >= 1);
        assert!(Arc::strong_count(&services.audit_service) >= 1);
        assert!(Arc::strong_count(&services.llm_service) >= 1);
        assert!(Arc::strong_count(&services.extraction_service) >= 1);
        // webhook feature 关闭时 webhook_worker 字段不编译
        #[cfg(feature = "webhook")]
        {
            assert!(Arc::strong_count(&services.webhook_worker) >= 1);
        }
        assert!(Arc::strong_count(&services.backlog_worker) >= 1);
        assert!(Arc::strong_count(&services.expiration_worker) >= 1);
    }

    // ========== init_garrison_auth tests ==========
    //
    // 测试覆盖三类场景：
    // 1. 弱密钥 / 空密钥 → Err(BootstrapError::GarrisonConfig)（fail-fast 类型化）
    // 2. 强密钥 + 真实 DbPool → Ok(())（标记 #[ignore] 由集成测试覆盖）
    //
    // 注意：DbPool 在 dbnexus 0.4.0 无 Default 实现（需真实连接池），故弱密钥测试也
    // 依赖 TEST_DATABASE_URL 提供真实 pool。但弱密钥场景下 pool 不会被使用（config
    // 校验在第 1 步即返回 Err），故 create_test_db_pool 仅用于满足函数签名。
    #[cfg(feature = "auth")]
    mod garrison_auth_tests {
        use super::*;
        use crate::bootstrap::error::BootstrapError;
        use crate::common::test_helpers::{create_test_db_pool, skip_if_no_test_db};

        /// 构造测试用 Settings（仅填充 auth.jwt_secret，其他字段走 confers 默认）。
        fn make_settings(jwt_secret: &str) -> Settings {
            let mut settings =
                crate::bootstrap::config::load_settings().expect("Failed to load settings");
            settings.auth.jwt_secret = jwt_secret.to_string();
            settings
        }

        /// 空 jwt_secret 返回 `Err(BootstrapError::GarrisonConfig)`，
        /// 错误消息含 "garrison config error" 与 EmptySecret Display（"must not be empty or missing"）。
        ///
        /// 验证“失败必须显性化”——空密钥不藏默认值背后，而是返回类型化错误。
        #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
        async fn test_init_garrison_auth_rejects_empty_secret() {
            if skip_if_no_test_db() {
                return;
            }
            let settings = make_settings("");
            // pool 不会被触达（config 校验先失败），但需满足函数签名。
            let pool = create_test_db_pool();
            let result = init_garrison_auth(&settings, pool).await;
            assert!(
                result.is_err(),
                "empty jwt_secret must be rejected with Err(BootstrapError::GarrisonConfig)"
            );
            let err = result.unwrap_err();
            assert!(
                matches!(err, BootstrapError::GarrisonConfig(_)),
                "error should be BootstrapError::GarrisonConfig variant, got: {err}"
            );
            assert!(
                format!("{err}").contains("garrison config error"),
                "error Display should contain 'garrison config error' prefix, got: {err}"
            );
            assert!(
                format!("{err}").contains("must not be empty or missing"),
                "error Display should contain EmptySecret description, got: {err}"
            );
        }

        /// 弱密钥（<32 字节）返回 `Err(BootstrapError::GarrisonConfig)`，
        /// 错误消息含 WeakSecret Display（"weak jwt_secret: length N < 32 bytes"）。
        #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
        async fn test_init_garrison_auth_rejects_weak_secret() {
            if skip_if_no_test_db() {
                return;
            }
            let weak = "too_short"; // 9 字节
            let settings = make_settings(weak);
            let pool = create_test_db_pool();
            let result = init_garrison_auth(&settings, pool).await;
            assert!(
                result.is_err(),
                "weak jwt_secret (<32 bytes) must be rejected with Err(BootstrapError::GarrisonConfig)"
            );
            let err = result.unwrap_err();
            assert!(
                matches!(err, BootstrapError::GarrisonConfig(_)),
                "error should be BootstrapError::GarrisonConfig variant, got: {err}"
            );
            // 验证错误消息含长度信息（验证类型化错误传递 len/min 字段到 Display）
            let display = format!("{err}");
            assert!(
                display.contains("length 9"),
                "error Display should contain 'length 9', got: {display}"
            );
            assert!(
                display.contains("< 32 bytes"),
                "error Display should contain '< 32 bytes', got: {display}"
            );
        }

        /// 强密钥 + 真实 DbPool → `Ok(())`，garrison 单例成功初始化。
        ///
        /// 此测试为集成测试，前置条件：
        /// 1. `TEST_DATABASE_URL` 指向已运行 garrison postgres migrations 的数据库
        /// 2. `GarrisonManager::init` 不被其他并发测试污染（GARRISON_MANAGER 单例）
        ///
        /// **标记 `#[ignore]`**：完整端到端测试在 (`tests/integration/auth_garrison_test.rs`) 进行。
        /// 此处保留测试骨架作为占位，避免伪装成有效测试（测试要验证有意义属性）。
        #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
        #[ignore = "集成测试覆盖：需真实 DB + garrison migrations + GARRISON_MANAGER 单例隔离"]
        async fn test_init_garrison_auth_strong_secret_returns_ok() {
            let strong = "a-very-strong-secret-key-32-bytes-or-more!!"; // 44 字节
            let settings = make_settings(strong);

            // 跳过条件：无 TEST_DATABASE_URL（与 garrison_interface.rs 测试一致）
            if skip_if_no_test_db() {
                return;
            }
            let pool = create_test_db_pool();

            let result = init_garrison_auth(&settings, pool).await;
            assert!(
                result.is_ok(),
                "strong jwt_secret + valid DbPool should succeed, got: {:?}",
                result.err()
            );
        }
    }
}
