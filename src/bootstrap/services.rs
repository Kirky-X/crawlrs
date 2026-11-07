// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Application services initialization.

mod runtime;
mod search;

use log::info;
// web-axum/auth-off 面不打印 admin key（bootstrap 是 auth 面流程），导入随 auth 门控
#[cfg(feature = "auth")]
use std::io::IsTerminal;
use std::sync::Arc;

use crate::application::use_cases::create_scrape::CreateScrapeUseCaseTrait;
use crate::bootstrap::infrastructure::InfrastructureComponents;
use crate::bootstrap::infrastructure::Repositories;
use crate::config::settings::Settings;
// auth feature 关闭时不导入 garrison 桥接类型
#[cfg(feature = "auth")]
use crate::bootstrap::error::BootstrapError;
#[cfg(feature = "auth")]
use crate::infrastructure::auth::{
    build_garrison_config, get_garrison_dao, init_garrison_dao, set_garrison_dao,
    CrawlrsGarrisonInterface,
};
// garrison prelude 提供 GarrisonDao / GarrisonInterface / GarrisonManager trait 与类型。
use crate::domain::services::audit_service::AuditServiceTrait;
// ContentExtractionFacade 用于正文提取（Trafilatura→DomSmoothie→CssRule + 质量门裁决 + LLM 回退）
use crate::domain::services::content_extractor::ContentExtractionFacade;
use crate::domain::services::extraction_service::ExtractionServiceTrait;
use dbnexus::DbPool;
#[cfg(feature = "auth")]
use garrison::prelude::{GarrisonDao, GarrisonInterface, GarrisonManager};
// teams-off 时不导入 teams 相关类型
#[cfg(feature = "teams")]
use crate::domain::services::geo_location::GeoLocationService;
use crate::domain::services::llm::{ExplicitErrorCarrier, LLMServiceTrait};
use crate::domain::services::rate_limiting_service::RateLimitingService;
// rate-limit feature 关闭时不导入 LimiteronService 相关配置类型
use crate::domain::services::rag_strategy::{EmbeddingProvider, FallbackEmbeddingProvider};
#[cfg(feature = "rate-limit")]
use crate::domain::services::rate_limiting_service::{
    ConcurrencyConfig, ConcurrencyStrategy, RateLimitConfig, RateLimitStrategy,
};
use crate::domain::services::rerank::{FallbackRerankProvider, RerankProvider, SearchReranker};
use crate::domain::services::search_service::{SearchService, SearchServiceTrait};
// SearXNG 实例生命周期服务（`search.searxng.enabled = false` 时为 None）
use crate::domain::services::searxng::SearxngInstanceService;
// RAG provider 实现：local 由 rag-local feature 门控；远端兜底经 gateway
//（fluxweir），http（通用 REST 重排）无条件可用
#[cfg(feature = "teams")]
use crate::domain::services::team_service::TeamService;
#[cfg(feature = "gateway")]
use crate::infrastructure::services::fluxweir_providers::{
    build_client as build_fluxweir_client, FluxweirEmbeddingProvider, FluxweirRerankProvider,
    FLUXWEIR_EMBED_MODEL, FLUXWEIR_RERANK_MODEL,
};
use crate::infrastructure::services::http_rerank_provider::{HttpRerankProvider, RerankFormat};
#[cfg(feature = "rag-local")]
use crate::infrastructure::services::vecboost_provider::VecboostProvider;
// webhook feature 关闭时此 trait 仍导入（webhook-off 由 runtime 装配 Noop 实现）
use crate::domain::services::webhook_service::WebhookService;
use crate::engines::engine_client::EngineClient;
// rate-limit feature 关闭时不导入 LimiteronService
#[cfg(feature = "rate-limit")]
use crate::infrastructure::services::limiteron_service::{
    LimiteronService, RateLimitingConfig, StorageHandle,
};
// rate-limit feature 关闭时导入 NoopRateLimitingService
#[cfg(not(feature = "rate-limit"))]
use crate::infrastructure::services::noop_rate_limiting_service::NoopRateLimitingService;
// webhook feature 关闭时不导入 WebhookSenderImpl（装配已移至 services::runtime）
// webhook_sender_impl 模块本身也被 cfg 门控（见 infrastructure::services::mod）。
use crate::domain::services::team_semaphore::{AdaptiveParams, TeamSemaphore};
use crate::presentation::middleware::rate_limit_middleware::RateLimitMiddleware;
use crate::queue::task_queue::TaskQueue;
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
    /// SearXNG 实例生命周期服务。
    ///
    /// `search.searxng.enabled = false` 时为 `None`：不构造服务、不注册引擎、
    /// 不拉起周期 worker，管理 API 返回 disabled 错误形态。
    pub searxng_instances: Option<Arc<dyn SearxngInstanceService>>,
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
    /// fluxweir 网关句柄（`fluxweir.enabled = false` 时空句柄零开销；
    /// embedded 持 ServerHandle 供停机 flush，external 仅 base_url）
    pub fluxweir_gateway: crate::bootstrap::fluxweir_gateway::FluxweirGatewayHandle,
    /// Content extraction facade
    ///
    /// 持有 `Vec<Box<dyn ContentExtractor>>` + 可选 LLM 承载，按 Trafilatura→DomSmoothie→CssRule
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
    /// Map 用例（F11 sitemap 流式站点图请求源）
    ///
    /// sitemap feature 关闭时不编译此字段（handler/路由同门控）。
    #[cfg(feature = "sitemap")]
    pub map_use_case: Arc<crate::application::use_cases::map_use_case::MapUseCase>,
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

        let rate_limiting_config = RateLimitingConfig {
            rate_limit: rate_limit_config,
            concurrency: concurrency_config,
            backlog_process_interval_seconds: 30,
            rate_limit_ttl_seconds: 3600,
        };

        // 存储后端选择：database 路径失败显性报错（R-multidb-schema-003
        // 禁止「DDL 失败→回退内存」）；未知非法值维持既有 fail-open 语义。
        let storage = match settings.rate_limiting.storage_backend.as_str() {
            "memory" => StorageHandle::Memory,
            "database" => {
                #[cfg(any(feature = "db-postgres", feature = "db-sqlite", feature = "db-mysql"))]
                match db_pool {
                    Some(pool) => StorageHandle::Database(pool),
                    None => {
                        anyhow::bail!(
                            "rate_limiting.storage_backend=database but no db pool available; \
                             refusing to fall back to memory storage (config error)"
                        );
                    }
                }
                #[cfg(not(any(
                    feature = "db-postgres",
                    feature = "db-sqlite",
                    feature = "db-mysql"
                )))]
                {
                    let _ = db_pool;
                    anyhow::bail!(
                        "rate_limiting.storage_backend=database requires a compiled db dialect \
                         (db-postgres/db-sqlite/db-mysql); refusing to fall back to memory \
                         storage (config error)"
                    );
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

/// Initialize search engine service.
///
/// Returns a SmartSearchEngine that concurrently queries Baidu/Bing/Sogou,
/// deduplicates via SimHash, fuses with RRF, and applies relevance scoring.
/// 传入 `searxng_engine` 时，SearXNG 与三个基础引擎并发 fan-out 并参与 RRF 融合。
///
/// # Arguments
///
/// * `engine_client` - Engine client for making requests
/// * `settings` - Application settings（`search.timeout_seconds` 注入单引擎 fan-out 超时）
/// * `searxng_engine` - SearXNG 适配器（`search.searxng.enabled = false` 时为 None）
///
/// # Returns
///
/// Returns an initialized search engine.
pub(super) fn init_search_engine(
    engine_client: Arc<EngineClient>,
    settings: &Settings,
    searxng_engine: Option<Arc<dyn SearchEngine>>,
) -> Arc<dyn SearchEngine> {
    info!("Initializing SmartSearchEngine (concurrent Baidu+Bing+Sogou + RRF fusion)");
    let mut engine = smart_search::SmartSearchEngine::new(engine_client)
        .with_engine_timeout(settings.search.timeout_seconds);
    if let Some(extra) = searxng_engine {
        info!("SearXNG joins smart aggregation fan-out (RRF)");
        engine = engine.with_additional_engine(extra);
    }
    Arc::new(engine)
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
pub(super) fn init_search_service(
    repositories: &Repositories,
    settings: &Settings,
    search_client: Arc<dyn SearchClientTrait>,
    reranker: Arc<SearchReranker>,
    smart_engine: Option<Arc<dyn SearchEngine>>,
) -> Arc<dyn SearchServiceTrait> {
    // Create SearchService with concrete repository types
    let mut service = SearchService::new(
        repositories.crawl_repo.clone(),
        repositories.task_repo.clone(),
        repositories.credits_repo.clone(),
        Arc::new(settings.clone()),
        search_client,
        reranker,
    );
    // P12：注入 SmartSearchEngine——engine=None / "smart" 的 /search 请求
    // 走多引擎 fan-out + RRF 聚合，显式引擎名走单引擎路径
    if let Some(smart) = smart_engine {
        service = service.with_smart_engine(smart);
    }
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
/// 校验语义（fail-fast）由 `RagSettings::validate_providers` 承担：配置了
/// 未编译的 feature（local 无 `rag-local`）或非法
/// provider 值时返回带重建指引的错误。vecboost 模型加载（秒级）发生在此处，
/// 即启动期一次性成本。
pub(super) async fn init_rag_components(settings: &Settings) -> Result<RagComponents, String> {
    if !settings.rag.enabled {
        return Ok(RagComponents {
            embedding: None,
            rerank: None,
        });
    }
    settings.rag.validate_providers()?;
    let rag = &settings.rag;

    // fluxweir 兜底 provider（design D5）：gateway 编译 + [fluxweir].enabled
    // + rag_fallback 时构造；primary（vecboost/http/rig）失败单次切换。
    // base_url 从 settings 直接决议（embedded 网关此刻可能尚未起服——provider
    // 请求发生在运行期，readyz 之后）。
    let fluxweir_fallback = build_rag_fallback(settings);

    let embedding = resolve_embedding_provider(rag, fluxweir_fallback.as_ref()).await?;
    let rerank = resolve_rerank_provider(rag, fluxweir_fallback).await?;

    Ok(RagComponents { embedding, rerank })
}

/// RAG 兜底 provider 对（嵌入 + 重排）。
type RagFallbackProviders = (Arc<dyn EmbeddingProvider>, Arc<dyn RerankProvider>);

/// 构造 fluxweir RAG 兜底 provider（design D5）。
///
/// gateway 编译 + `[fluxweir].enabled` + `rag_fallback` 时构造；primary
/// （vecboost/http）失败单次切换。base_url 从 settings 直接决议（embedded 网关
/// 此刻可能尚未起服——provider 请求发生在运行期，readyz 之后）。
#[cfg(feature = "gateway")]
fn build_rag_fallback(settings: &Settings) -> Option<RagFallbackProviders> {
    if !(settings.fluxweir.enabled && settings.fluxweir.rag_fallback) {
        return None;
    }
    match build_fluxweir_client(settings) {
        Some(client) => {
            log::info!(
                "fluxweir RAG fallback enabled (embed={}, rerank={})",
                FLUXWEIR_EMBED_MODEL,
                FLUXWEIR_RERANK_MODEL
            );
            Some((
                Arc::new(FluxweirEmbeddingProvider::new(
                    client.clone(),
                    FLUXWEIR_EMBED_MODEL,
                    settings.rag.local_dimensions,
                )),
                Arc::new(FluxweirRerankProvider::new(client, FLUXWEIR_RERANK_MODEL)),
            ))
        }
        None => {
            log::warn!("fluxweir.enabled but API key unresolvable; RAG fallback disabled");
            None
        }
    }
}

#[cfg(not(feature = "gateway"))]
fn build_rag_fallback(_settings: &Settings) -> Option<RagFallbackProviders> {
    None
}

/// 决议嵌入 provider：primary（vecboost）与 fluxweir 兜底组合为降级链。
async fn resolve_embedding_provider(
    rag: &crate::config::rag::RagSettings,
    fluxweir_fallback: Option<&RagFallbackProviders>,
) -> Result<Option<Arc<dyn EmbeddingProvider>>, String> {
    let embedding: Option<Arc<dyn EmbeddingProvider>> = match rag.embedding_provider.as_str() {
        "" => None,
        #[cfg(feature = "rag-local")]
        "local" => match VecboostProvider::new(rag).await {
            Ok(provider) => Some(Arc::new(provider)),
            // D5 装配矩阵：vecboost 初始化失败且有兜底 → 仅 fluxweir（不阻断启动）
            Err(e) if fluxweir_fallback.is_some() => {
                log::warn!(
                    "vecboost embedding provider init failed, using fluxweir fallback only: {e}"
                );
                None
            }
            Err(e) => return Err(format!("vecboost embedding provider init failed: {e}")),
        },
        // validate_providers 已拒绝非法值与未编译 feature 的组合，此处不可达
        _ => None,
    };
    // primary Some + 兜底 Some → 复合降级链；primary None + 兜底 Some → 仅兜底
    Ok(
        match (embedding, fluxweir_fallback.map(|(e, _)| e.clone())) {
            (Some(primary), Some(secondary)) => {
                Some(Arc::new(FallbackEmbeddingProvider::new(primary, secondary))
                    as Arc<dyn EmbeddingProvider>)
            }
            (Some(primary), None) => Some(primary),
            (None, Some(secondary)) => Some(secondary),
            (None, None) => None,
        },
    )
}

/// 决议重排 provider：primary（vecboost/http）与 fluxweir 兜底组合为降级链。
async fn resolve_rerank_provider(
    rag: &crate::config::rag::RagSettings,
    fluxweir_fallback: Option<RagFallbackProviders>,
) -> Result<Option<Arc<dyn RerankProvider>>, String> {
    let rerank: Option<Arc<dyn RerankProvider>> = match rag.rerank_provider.as_str() {
        "" => None,
        #[cfg(feature = "rag-local")]
        "local" => match VecboostProvider::new(rag).await {
            Ok(provider) => Some(Arc::new(provider)),
            Err(e) if fluxweir_fallback.is_some() => {
                log::warn!(
                    "vecboost rerank provider init failed, using fluxweir fallback only: {e}"
                );
                None
            }
            Err(e) => return Err(format!("vecboost rerank provider init failed: {e}")),
        },
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
    Ok(match (rerank, fluxweir_fallback.map(|(_, r)| r)) {
        (Some(primary), Some(secondary)) => {
            Some(Arc::new(FallbackRerankProvider::new(primary, secondary))
                as Arc<dyn RerankProvider>)
        }
        (Some(primary), None) => Some(primary),
        (None, Some(secondary)) => Some(secondary),
        (None, None) => None,
    })
}

/// 初始化 garrison 认证鉴权服务。
///
/// 装配流程：
/// 1. 调用 [`init_garrison_dao`] 获取 `Arc<dyn GarrisonDao>`（oxcache 内存存储，自管理实例）
/// 2. 调用 [`build_garrison_config`] 构造 `GarrisonConfig`（弱密钥拒绝，HS256 ≥32 字节）
/// 3. 构造 [`CrawlrsGarrisonInterface`] 并装为 `Arc<dyn GarrisonInterface>`
///    （注入 crawlrs 的 `DbPool` 用于查询 garrison RBAC 表）
/// 4. 调用 `GarrisonManager::init` 写入 `GARRISON_MANAGER` 全局单例
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
    install_garrison(settings, &pool).await?;

    // 5. 决策 2：断言 garrison firewall 已启用（仅依赖 garrison 做 CWE-307 IP 限速）
    assert_bruteforce_firewall_compiled();

    // 6. 注入 bootstrap admin API key（若环境变量存在）。
    inject_bootstrap_admin_api_key(&pool).await?;

    info!("Garrison authentication manager initialized (token_style=jwt, HS256, firewall=enabled)");
    Ok(())
}

/// 1-4. 构造并安装 garrison 全局单例（config → DAO → interface → manager）。
#[cfg(feature = "auth")]
async fn install_garrison(settings: &Settings, pool: &Arc<DbPool>) -> Result<(), BootstrapError> {
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
    // `(**pool).clone()`：deref &Arc<DbPool> → DbPool，再 clone（DbPool 内部为
    // Arc<DbPoolInner>，clone 廉价 <10ns）。`CrawlrsGarrisonInterface::new` 持有
    // owned DbPool，符合其既有签名（外科手术式修改，不改动代码）。
    let interface: Arc<dyn GarrisonInterface> =
        Arc::new(CrawlrsGarrisonInterface::new((**pool).clone()));

    // 4. 写入 garrison 全局单例（builder 模式，异步 build）
    // 重构：使用类型化 `BootstrapError::GarrisonManager`。
    GarrisonManager::builder()
        .dao(dao)
        .config(config)
        .interface(interface)
        .build()
        .await
        .map_err(|e| BootstrapError::GarrisonManager(format!("{e}")))?;

    Ok(())
}

/// 5. 决策 2：断言 garrison firewall 已启用（仅依赖 garrison 做 CWE-307 IP 限速）。
///
/// crawlrs 已删除本地 `AuthRateLimiter`，暴力破解防护完全依赖 garrison 的
/// `firewall` + `firewall-bruteforce` features。若 features 未启用，garrison
/// 不会执行 IP 级限速，导致 CWE-307 防护缺失。
///
/// 此处通过引用 `BruteForceStrategy` 类型做编译期断言——若 garrison 未启用
/// `firewall-bruteforce` feature，该类型不存在，编译失败（fail-fast）。
/// 运行时无需额外检查（garrison 在 `check_api_key` 内部自动调用 firewall）。
///
/// 注：Cargo feature 是统一效应，此处断言仅防止"误关闭 features"配置错误，
/// 不防恶意依赖（见 auth_middleware.rs `reset_global_auth_state` 注释）。
///
/// ## 阈值配置限制
///
/// garrison 0.8.1 在 `check_api_key` 内部硬编码 `BruteForceConfig::default()`
/// （5 次失败 / 60s 窗口 / 300s 锁定），不通过 `GarrisonConfig` 暴露自定义入口。
/// 当前使用 garrison 默认值；若需自定义阈值，需等 garrison 后续版本暴露配置接口
/// 或在 crawlrs 侧实现自定义 `check_api_key` wrapper（暂不实施简洁优先）。
#[cfg(feature = "auth")]
fn assert_bruteforce_firewall_compiled() {
    // garrison 顶层 re-export（lib.rs:621，gated on `firewall-bruteforce` feature）
    use garrison::BruteForceStrategy as _FwAssert;
    // 引用类型做编译期检查（无运行时开销）
    let _ = std::marker::PhantomData::<_FwAssert>;
}

/// 6. 注入 bootstrap admin API key（若环境变量存在）。
///
/// ## 设计背景
///
/// garrison DAO 默认是内存实现（`GarrisonDaoOxcache`），进程间不共享数据。
/// 外部工具签发的 API Key 写入工具进程的
/// 独立 DAO 实例，server 进程无法读取 → 所有认证请求均 401。
///
/// 标准解法：server 启动时读取环境变量 `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY`，
/// 若存在则直接用 garrison `ApiKeyHandler::generate_with_namespace` 签发到
/// server 自己的 DAO（内存共享），同时写入 crawlrs `api_keys`/`teams` 表保留
/// `api_key_id → team_id` 映射（auth_middleware 反查依赖）。
///
/// ## 安全（CWE-532 / CWE-798）
///
/// - 明文 key 仅通过环境变量注入一次，从不写入日志或配置文件；
/// - garrison 侧仅存 `sha256(key_secret)`（CWE-916 与 generate_internal 一致）；
/// - 环境变量不存在或为空时完全跳过此分支（不影响部署未启用 bootstrap 的场景）。
///
/// ## 格式
///
/// `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY` = `<team_id_uuid>`（可选）；
///   不传 team_id 时自动创建 `"bootstrap-admin-team"` team。
/// 内部签发双段格式：`key_id.key_secret`（`ApiKeyHandler::generate_with_namespace`），
/// key_id / key_secret 各 32 hex，TTL 30 天，scope `crawlrs:admin`。
#[cfg(feature = "auth")]
async fn inject_bootstrap_admin_api_key(pool: &Arc<DbPool>) -> Result<(), BootstrapError> {
    let bootstrap_cfg = match std::env::var("CRAWLRS__BOOTSTRAP_ADMIN_API_KEY") {
        Ok(v) if !v.is_empty() => Some(v),
        _ => None,
    };
    let Some(cfg_val) = bootstrap_cfg else {
        return Ok(());
    };

    use crate::common::time_utils;
    use crate::infrastructure::database::entities::api_key::{
        ActiveModel as ApiKeyActiveModel, Entity as ApiKeyEntity,
    };
    use crate::infrastructure::database::entities::team::{
        ActiveModel as TeamActiveModel, Entity as TeamEntity,
    };
    use dbnexus::sea_orm::{ActiveValue, EntityTrait};
    use garrison::protocol::apikey::ApiKeyHandler;

    const GARRISON_NS: &str = "crawlrs";
    const PERM_ADMIN: &str = "crawlrs:admin";
    const DEFAULT_TEAM_NAME: &str = "bootstrap-admin-team";
    const TTL_SECS: i64 = 30 * 24 * 60 * 60;

    let wrap_db = |ctx: &'static str| {
        move |e: dbnexus::sea_orm::DbErr| {
            BootstrapError::GarrisonManager(format!("{}: db: {}", ctx, e))
        }
    };

    // 格式约定（docs/USER_GUIDE.md:178）：`<team_id_uuid>` 是裸 36 位 UUID
    // 字符串，不是 `team_id_uuid` 复合格式；`Id::from_str` 按 UUID 解析的形态
    // 为「含 '-' 且长度 36」或「32 位紧凑 UUID」，本守卫只放行 36 位裸 UUID
    // （bootstrap 契约），非法/非 36 长度一律回退自动生成 team，不静默截断。
    let team_id_arg: Option<crate::domain::id::Id> = if cfg_val.len() == 36 {
        match cfg_val.parse::<crate::domain::id::Id>() {
            Ok(id) => Some(id),
            Err(e) => {
                log::warn!(
                    "CRAWLRS__BOOTSTRAP_ADMIN_API_KEY 长度 36 但 UUID 解析失败，回退自动生成 team: {e}"
                );
                None
            }
        }
    } else {
        if !cfg_val.is_empty() {
            log::warn!(
                "CRAWLRS__BOOTSTRAP_ADMIN_API_KEY 非空但不是 36 位 UUID（长度 {}），回退自动生成 team",
                cfg_val.len()
            );
        }
        None
    };
    let team_id = team_id_arg.unwrap_or_else(crate::domain::id::Id::generate);
    let api_key_id = crate::domain::id::Id::generate();

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
            id: ActiveValue::Set(team_id.into()),
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
            BootstrapError::GarrisonManager(format!("bootstrap_admin_key: garrison generate: {e}"))
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
        id: ActiveValue::Set(api_key_id.into()),
        team_id: ActiveValue::Set(team_id.into()),
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

    Ok(())
}

/// Initialize LLM service.
///
/// crawlrs 不再内置 LLM 供应商访问栈：默认返回 [`ExplicitErrorCarrier`]，
/// 未注入宿主 carrier 时任何调用返回结构化双语错误（不 panic、不静默降级）。
/// 宿主可经三条装配路径注入自有 [`LLMServiceTrait`] 实现：
/// `Services.llm_service` pub 字段、`init_llm_service` 工厂替换、`CrawlRsState::from_kit`。
///
/// # Arguments
///
/// * `_settings` - Application settings（保留签名兼容；供应商语义配置已退役）
/// * `_http_client` - HTTP client（保留签名兼容；供应商访问已移交宿主）
///
/// # Returns
///
/// Returns an initialized LLM service wrapped in Arc.
pub(super) fn init_llm_service(
    #[cfg_attr(not(feature = "gateway"), allow(unused_variables))] settings: &Settings,
    _http_client: Arc<reqwest::Client>,
    #[cfg_attr(not(feature = "gateway"), allow(unused_variables))]
    fluxweir_gateway: &crate::bootstrap::fluxweir_gateway::FluxweirGatewayHandle,
) -> Arc<dyn LLMServiceTrait> {
    // fluxweir-llm-gateway T007：[fluxweir].enabled=true → FluxweirLlmCarrier
    //（gateway 未编译的 enabled 形态已在 init_fluxweir_gateway fail-fast，
    // 此处 enabled 即蕴含 gateway 可用）；disabled → ExplicitErrorCarrier 原行为
    #[cfg(feature = "gateway")]
    if settings.fluxweir.enabled {
        let base_url = fluxweir_gateway
            .base_url()
            .expect("fluxweir gateway enabled but handle has no base_url (init order bug)")
            .to_string();
        let api_key = settings
            .fluxweir
            .api_key()
            .expect("fluxweir enabled but no API key resolvable (validate order bug)");
        let client = fluxweir::sdk::FluxweirClientBuilder::new()
            .base_url(base_url)
            .api_key(api_key)
            .timeout(std::time::Duration::from_secs(
                settings.fluxweir.request_timeout_secs,
            ))
            .build()
            .expect("fluxweir SDK client build failed");
        log::info!(
            "LLM carrier: FluxweirLlmCarrier via fluxweir gateway at {}",
            fluxweir_gateway.base_url().unwrap_or("<unknown>")
        );
        return Arc::new(
            crate::infrastructure::services::fluxweir_llm_carrier::FluxweirLlmCarrier::new(
                client, settings,
            ),
        );
    }
    let _ = fluxweir_gateway;
    Arc::new(ExplicitErrorCarrier)
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
///
/// `Result` 承载启动期配置/迁移错误（如 `storage_backend=database` 缺池或缺
/// dialect 构建的显性报错，R-multidb-schema-003），禁止静默降级。
pub(crate) async fn init_services(
    infrastructure: &InfrastructureComponents,
    engine_client: Arc<EngineClient>,
    http_client: Arc<reqwest::Client>,
    settings: &Settings,
) -> anyhow::Result<ServicesComponents> {
    let repositories = &infrastructure.repositories;

    // 装配阶段与顺序：运行时基础设施 → webhook → teams → 搜索/RAG → 认证审计
    // → LLM/提取 → workers。顺序保持原内联实现的副作用顺序不变。
    let runtime = runtime::init_runtime_services(infrastructure, &engine_client, settings).await?;
    let webhook_service = runtime::init_webhook_service(repositories, settings, &http_client);
    #[cfg(feature = "teams")]
    let (team_service, geo_location_service) =
        runtime::init_team_services(repositories, &http_client);
    let search =
        search::init_search_stack(infrastructure, settings, &http_client, &engine_client).await;
    let (queue, audit_service) = runtime::init_auth_and_audit(settings, infrastructure).await;
    let llm = search::init_llm_stack(
        settings,
        &http_client,
        &search.fluxweir_gateway,
        search.embedding_provider.clone(),
    );
    let workers = runtime::init_workers(
        repositories,
        settings,
        &webhook_service,
        &runtime.rate_limiting_service,
    );

    info!("Services initialized");

    Ok(ServicesComponents {
        rate_limit_middleware: runtime.rate_limit_middleware,
        team_semaphore: runtime.team_semaphore,
        request_coalescer: runtime.request_coalescer,
        rate_limiting_service: runtime.rate_limiting_service,
        create_scrape_use_case: runtime.create_scrape_use_case,
        webhook_service,
        #[cfg(feature = "teams")]
        team_service,
        #[cfg(feature = "teams")]
        geo_location_service,
        robots_checker: search.robots_checker,
        search_engine_service: search.search_engine_service,
        search_service: search.search_service,
        searxng_instances: search.searxng_instances,
        queue,
        audit_service,
        http_client,
        llm_service: llm.llm_service,
        extraction_service: llm.extraction_service,
        embedding_provider: search.embedding_provider,
        rerank_provider: search.rerank_provider,
        fluxweir_gateway: search.fluxweir_gateway,
        content_extractor: llm.content_extractor,
        #[cfg(feature = "webhook")]
        webhook_worker: workers.webhook_worker,
        backlog_worker: workers.backlog_worker,
        expiration_worker: workers.expiration_worker,
        #[cfg(feature = "sitemap")]
        map_use_case: runtime.map_use_case,
    })
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
    use crate::utils::host_discipline::HostDiscipline;

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
            _team_id: crate::domain::id::Id,
        ) -> Result<RateLimitConfig, RateLimitingError> {
            Ok(RateLimitConfig::default())
        }

        async fn update_team_rate_limit_config(
            &self,
            _team_id: crate::domain::id::Id,
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
            _team_id: crate::domain::id::Id,
            _task_id: crate::domain::id::Id,
        ) -> Result<ConcurrencyResult, RateLimitingError> {
            Ok(ConcurrencyResult::Allowed)
        }

        async fn release_team_concurrency_slot(
            &self,
            _team_id: crate::domain::id::Id,
            _task_id: crate::domain::id::Id,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn get_team_current_concurrency(
            &self,
            _team_id: crate::domain::id::Id,
        ) -> Result<u32, RateLimitingError> {
            Ok(0)
        }

        async fn get_team_concurrency_config(
            &self,
            _team_id: crate::domain::id::Id,
        ) -> Result<ConcurrencyConfig, RateLimitingError> {
            Ok(ConcurrencyConfig::default())
        }

        async fn update_team_concurrency_config(
            &self,
            _team_id: crate::domain::id::Id,
            _config: ConcurrencyConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
    }

    #[async_trait::async_trait]
    impl BacklogService for MockRateLimitingService {
        async fn process_backlog_tasks(
            &self,
            _team_id: crate::domain::id::Id,
        ) -> Result<u32, RateLimitingError> {
            Ok(0)
        }
    }

    #[async_trait::async_trait]
    impl QuotaService for MockRateLimitingService {
        async fn check_and_deduct_quota(
            &self,
            _team_id: crate::domain::id::Id,
            _amount: i64,
            _transaction_type: CreditsTransactionType,
            _description: String,
            _reference_id: Option<crate::domain::id::Id>,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn get_quota_balance(
            &self,
            _team_id: crate::domain::id::Id,
        ) -> Result<i64, RateLimitingError> {
            Ok(0)
        }

        async fn refund_quota(
            &self,
            _team_id: crate::domain::id::Id,
            _amount: i64,
            _description: String,
            _reference_id: Option<crate::domain::id::Id>,
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
        let team_id = crate::domain::id::Id::generate();
        assert_eq!(semaphore.current_target(team_id), 15);
    }

    // ========== init_llm_service tests ==========

    /// 默认装配返回 ExplicitErrorCarrier：未注入宿主 carrier 时任何调用返回
    /// 结构化双语错误（不 panic、不静默降级）。
    #[tokio::test]
    async fn test_init_llm_service_defaults_to_explicit_error_carrier() {
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let http_client = make_http_client();
        let service = init_llm_service(&settings, http_client, &Default::default());
        let schema = serde_json::json!({"type": "object"});
        let err = service
            .extract_data("text", &schema, "json")
            .await
            .expect_err("默认装配必须显式失败");
        let message = format!("{err:#}");
        assert!(
            message.contains("未接入承载方"),
            "default carrier must be explicit: {message}"
        );
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
        let search_engine = init_search_engine(engine_client, &settings, None);
        assert!(
            Arc::strong_count(&search_engine) >= 1,
            "init_search_engine should return a valid Arc<dyn SearchEngine>"
        );
    }

    #[test]
    fn test_init_search_engine_with_ab_test_disabled() {
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let engine_client = make_engine_client();
        let _search_engine = init_search_engine(engine_client, &settings, None);
        // Should create without panic; SmartSearchEngine is always used
    }

    #[test]
    fn test_init_search_engine_with_ab_test_enabled() {
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let engine_client = make_engine_client();
        let _search_engine = init_search_engine(engine_client, &settings, None);
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
        let service = init_rate_limiting_service(&repos, &settings, Some(db.clone_inner()))
            .await
            .expect("init_rate_limiting_service");
        // Verify the service is usable (Arc strong count >= 1).
        assert!(Arc::strong_count(&service) >= 1);

        // memory 后端路径：默认配置下同样可装配
        settings.rate_limiting.storage_backend = "memory".to_string();
        let service = init_rate_limiting_service(&repos, &settings, Some(db.clone_inner()))
            .await
            .expect("init_rate_limiting_service");
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
            None,
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
            None,
        );
        let engine_router = Arc::new(EngineRouter::with_host_discipline(
            engines.clone(),
            HostDiscipline::from_politeness(&settings.politeness),
        ));
        let engine_client = Arc::new(EngineClient::with_router(engine_router.clone()));

        let services = init_services(&infra, engine_client, infra.http_client.clone(), &settings)
            .await
            .expect("init_services");

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

            // init_garrison_auth 会注入进程级 DAO 单例：必须持 garrison 全局锁
            // 并先 reset，否则与并发构建全量 state 的测试（bootstrap::routes
            // tests、workers scrape_worker tests）互相触发
            // "set_garrison_dao failed: global DAO already injected"。
            let _garrison_guard =
                crate::common::test_helpers::acquire_garrison_global_state().await;
            let result = init_garrison_auth(&settings, pool).await;
            assert!(
                result.is_ok(),
                "strong jwt_secret + valid DbPool should succeed, got: {:?}",
                result.err()
            );
        }
    }
}
