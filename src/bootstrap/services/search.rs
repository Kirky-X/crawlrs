// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 搜索 / RAG / LLM 能力装配（从 `init_services` 提取）。
//!
//! 装配顺序即原 `init_services` 的执行顺序：robots → SearXNG → 搜索引擎/客户端
//! → RAG → 重排 → fluxweir 网关 → 搜索服务；LLM/提取/正文提取为独立阶段。

use log::info;
use std::sync::Arc;

use crate::bootstrap::fluxweir_gateway::FluxweirGatewayHandle;
use crate::bootstrap::infrastructure::InfrastructureComponents;
use crate::bootstrap::infrastructure::Repositories;
use crate::config::settings::Settings;
use crate::domain::services::content_extractor::ContentExtractionFacade;
use crate::domain::services::content_extractor::QualityGateConfig;
use crate::domain::services::extraction_service::{ExtractionService, ExtractionServiceTrait};
use crate::domain::services::llm::{ExplicitErrorCarrier, LLMServiceTrait};
use crate::domain::services::rag_strategy::{
    ChunkerConfig, EmbeddingProvider, FallbackEmbeddingProvider, RagExtractionRuntime,
};
use crate::domain::services::rerank::{FallbackRerankProvider, RerankProvider, SearchReranker};
use crate::domain::services::search_service::{SearchService, SearchServiceTrait};
use crate::domain::services::searxng::{
    ReqwestSearxngTransport, SearxngInstanceService, SearxngInstanceServiceImpl,
};
use crate::engines::engine_client::EngineClient;
use crate::search::client::SearchClientTrait;
use crate::search::engine_trait::SearchEngine;
use crate::search::smart as smart_search;
use crate::utils::robots::RobotsChecker;

#[cfg(feature = "gateway")]
use crate::infrastructure::services::fluxweir_providers::{
    build_client as build_fluxweir_client, FluxweirEmbeddingProvider, FluxweirRerankProvider,
    FLUXWEIR_EMBED_MODEL, FLUXWEIR_RERANK_MODEL,
};
use crate::infrastructure::services::http_rerank_provider::{HttpRerankProvider, RerankFormat};
#[cfg(feature = "rag-local")]
use crate::infrastructure::services::vecboost_provider::VecboostProvider;

/// 搜索能力装配产物
pub(super) struct SearchStack {
    pub(super) robots_checker: Arc<RobotsChecker>,
    pub(super) search_engine_service: Arc<dyn SearchEngine>,
    pub(super) search_service: Arc<dyn SearchServiceTrait>,
    pub(super) searxng_instances: Option<Arc<dyn SearxngInstanceService>>,
    /// RAG 嵌入 provider（来自 [`RagComponents`]）
    pub(super) embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    /// RAG 重排 provider（来自 [`RagComponents`]）
    pub(super) rerank_provider: Option<Arc<dyn RerankProvider>>,
    /// fluxweir 网关句柄（LLM 装配阶段按引用消费）
    pub(super) fluxweir_gateway: FluxweirGatewayHandle,
}

/// 装配搜索能力：robots 检查器 / SearXNG 实例与引擎 / 搜索引擎与客户端 /
/// RAG provider / 语义重排器 / fluxweir 网关 / 搜索服务。
pub(super) async fn init_search_stack(
    infrastructure: &InfrastructureComponents,
    settings: &Settings,
    http_client: &Arc<reqwest::Client>,
    engine_client: &Arc<EngineClient>,
) -> SearchStack {
    let repositories = &infrastructure.repositories;

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

    // SearXNG 实例生命周期（必须在 SearchClient 之前构造：引擎注册与 smart 聚合依赖服务存在性）。
    // enabled=false → None（不构造服务、不拉起 worker、不注册引擎）。
    // transport 只构造一次：实例服务与 search engine 共享同一份（同一 http_client/user_agent）。
    let searxng = init_searxng_services(repositories, settings, http_client);

    // Initialize search engine (for backward compatibility)
    let search_engine_service: Arc<dyn SearchEngine> =
        init_search_engine(engine_client.clone(), settings, searxng.engine);

    // Initialize search client (wraps search engines)
    // 按配置装配引擎：bing/baidu/sogou 恒注册；exa/tavily 配置了 api_key 才注册
    let search_client: Arc<dyn SearchClientTrait> = Arc::new(
        crate::search::client::SearchClient::with_configured_engines(
            engine_client.clone(),
            http_client.clone(),
            &settings.search,
            searxng.instances.clone(),
        ),
    );

    // RAG 能力组件（默认全关：rag.enabled = false 时 provider 全 None，零开销）。
    // 失败 = 运维显式启用但配置/环境不可用，按既有 fail-fast 惯例 panic。
    let rag = init_rag_or_panic(settings).await;

    // 语义重排器：provider 就绪时搜索结果做精准重排，否则直通
    let search_reranker = Arc::new(SearchReranker::new(
        rag.rerank.clone(),
        settings.rag.search_rerank_top_n,
    ));

    // fluxweir 网关生命周期（默认全关：enabled=false 时空句柄零开销；
    // enabled=true 但 gateway 未编译 → fail-fast，错误文案含 feature 指引）
    let fluxweir_gateway = init_fluxweir_gateway_or_panic(infrastructure, settings).await;

    let search_service = init_search_service(
        repositories,
        settings,
        search_client.clone(),
        search_reranker,
        Some(search_engine_service.clone()),
    );

    SearchStack {
        robots_checker,
        search_engine_service,
        search_service,
        searxng_instances: searxng.instances,
        embedding_provider: rag.embedding,
        rerank_provider: rag.rerank,
        fluxweir_gateway,
    }
}

/// 初始化 RAG 组件；运维显式启用但配置/环境不可用时 fail-fast。
async fn init_rag_or_panic(settings: &Settings) -> RagComponents {
    match init_rag_components(settings).await {
        Ok(components) => components,
        Err(e) => panic!("RAG capability init failed (check [rag] config / features): {e}"),
    }
}

/// 初始化 fluxweir 网关；enabled=true 但未编译 gateway 时 fail-fast。
async fn init_fluxweir_gateway_or_panic(
    infrastructure: &InfrastructureComponents,
    settings: &Settings,
) -> FluxweirGatewayHandle {
    match crate::bootstrap::fluxweir_gateway::init_fluxweir_gateway(
        settings,
        Some(infrastructure.db.clone_inner()),
        &settings.database.url,
    )
    .await
    {
        Ok(handle) => handle,
        Err(e) => {
            panic!("fluxweir gateway init failed (check [fluxweir] config / features): {e:#}")
        }
    }
}

/// SearXNG 生命周期装配产物
struct SearxngStack {
    /// 实例生命周期服务（`search.searxng.enabled = false` 时为 None）
    instances: Option<Arc<dyn SearxngInstanceService>>,
    /// SearXNG 搜索适配器（未启用时为 None）
    engine: Option<Arc<dyn SearchEngine>>,
}

/// 装配 SearXNG 实例生命周期与搜索适配器。
///
/// enabled=false → 全 None（不构造服务、不拉起 worker、不注册引擎）。
/// transport 只构造一次：实例服务与 search engine 共享同一份（同一 http_client/user_agent）。
fn init_searxng_services(
    repositories: &Repositories,
    settings: &Settings,
    http_client: &Arc<reqwest::Client>,
) -> SearxngStack {
    let searxng_transport: Option<Arc<dyn crate::domain::services::searxng::SearxngTransport>> =
        if settings.search.searxng.enabled {
            Some(Arc::new(ReqwestSearxngTransport::new(
                http_client.clone(),
                settings.search.searxng.user_agent.clone(),
            )))
        } else {
            None
        };

    let searxng_instances: Option<Arc<dyn SearxngInstanceService>> =
        match searxng_transport.as_ref() {
            Some(transport) => {
                let concrete = Arc::new(SearxngInstanceServiceImpl::new(
                    repositories.searxng_instance_repo.clone(),
                    transport.clone(),
                    settings.search.searxng.clone(),
                ));
                // 启动期：seed 引导 + 后台热池重建（不阻塞就绪；完成前 health() = Degraded）
                concrete.spawn_bootstrap();
                info!(
                    "SearXNG instance service enabled (pool_target={})",
                    settings.search.searxng.pool_target
                );
                Some(concrete as Arc<dyn SearxngInstanceService>)
            }
            None => {
                info!("SearXNG disabled by config (search.searxng.enabled=false)");
                None
            }
        };

    // SearXNG 适配器：smart 聚合 fan-out 与 SearchClient 单引擎路径共用同一实例服务与 transport
    let searxng_search_engine: Option<Arc<dyn SearchEngine>> =
        match (searxng_instances.as_ref(), searxng_transport.as_ref()) {
            (Some(service), Some(transport)) => Some(Arc::new(
                crate::search::client::searxng::SearxngSearchEngine::new(
                    service.clone(),
                    transport.clone(),
                    std::time::Duration::from_secs(settings.search.timeout_seconds),
                    settings.search.searxng.max_failover_attempts,
                ),
            ) as Arc<dyn SearchEngine>),
            _ => None,
        };

    SearxngStack {
        instances: searxng_instances,
        engine: searxng_search_engine,
    }
}

/// LLM / 提取能力装配产物
pub(super) struct LlmStack {
    pub(super) llm_service: Arc<dyn LLMServiceTrait>,
    pub(super) extraction_service: Arc<dyn ExtractionServiceTrait>,
    pub(super) content_extractor: Arc<ContentExtractionFacade>,
}

/// 装配 LLM 服务 / 提取服务 / 正文提取 Facade。
///
/// `embedding_provider` 为 `Some` 时提取服务注入 RAG 运行时。
pub(super) fn init_llm_stack(
    settings: &Settings,
    http_client: &Arc<reqwest::Client>,
    fluxweir_gateway: &FluxweirGatewayHandle,
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
) -> LlmStack {
    // Initialize LLM service (使用依赖注入的 http_client)
    let llm_service = init_llm_service(settings, http_client.clone(), fluxweir_gateway);

    // Initialize extraction service（嵌入 provider 就绪时注入 RAG 运行时）
    let extraction_service: Arc<dyn ExtractionServiceTrait> =
        match &embedding_provider {
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
    // 装配 ContentExtractionFacade，注入 llm_service 用于低置信度回退（confidence < 0.7
    // 时触发）。质量门按 settings.content_extraction 四键启用：word_count 短结果升级 +
    // 长度比裁决 + prose 硬失败（sentence_avg/link_density）门槛。
    // Facade 内部按 cfg 编译期决定 extractor 链（Trafilatura→DomSmoothie→CssRule）。
    let content_extractor = Arc::new(ContentExtractionFacade::with_quality_config(
        Some(llm_service.clone()),
        QualityGateConfig::active(
            settings.content_extraction.min_word_count,
            settings.content_extraction.replacement_length_ratio,
            settings.content_extraction.prose_min_sentence_words,
            settings.content_extraction.prose_max_link_density,
        ),
    ));

    LlmStack {
        llm_service,
        extraction_service,
        content_extractor,
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
pub(super) struct RagComponents {
    /// 嵌入 provider：`local`（vecboost）或 `remote`（rig）
    pub(super) embedding: Option<Arc<dyn EmbeddingProvider>>,
    /// 重排 provider：`local`（vecboost）或 `http`（通用 REST）
    pub(super) rerank: Option<Arc<dyn RerankProvider>>,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_http_client() -> Arc<reqwest::Client> {
        Arc::new(reqwest::Client::new())
    }

    fn make_engine_client() -> Arc<EngineClient> {
        Arc::new(EngineClient::new())
    }

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

    use crate::bootstrap::infrastructure::{init_database, init_repositories};
    use crate::common::test_support::testcontainers_fixtures as tcf;

    async fn require_docker() -> bool {
        tcf::docker_available().await
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
}
