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
use crate::domain::services::llm::LLMServiceTrait;
use crate::domain::services::rag_strategy::{
    ChunkerConfig, EmbeddingProvider, RagExtractionRuntime,
};
use crate::domain::services::rerank::{RerankProvider, SearchReranker};
use crate::domain::services::search_service::SearchServiceTrait;
use crate::domain::services::searxng::{
    ReqwestSearxngTransport, SearxngInstanceService, SearxngInstanceServiceImpl,
};
use crate::engines::engine_client::EngineClient;
use crate::search::client::SearchClientTrait;
use crate::search::engine_trait::SearchEngine;
use crate::utils::robots::RobotsChecker;

use super::{init_llm_service, init_rag_components, init_search_engine, init_search_service};

/// 搜索能力装配产物
pub(super) struct SearchStack {
    pub(super) robots_checker: Arc<RobotsChecker>,
    pub(super) search_engine_service: Arc<dyn SearchEngine>,
    pub(super) search_service: Arc<dyn SearchServiceTrait>,
    pub(super) searxng_instances: Option<Arc<dyn SearxngInstanceService>>,
    /// RAG 嵌入 provider（来自 [`super::RagComponents`]）
    pub(super) embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    /// RAG 重排 provider（来自 [`super::RagComponents`]）
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
    let rag = match init_rag_components(settings).await {
        Ok(components) => components,
        Err(e) => panic!("RAG capability init failed (check [rag] config / features): {e}"),
    };

    // 语义重排器：provider 就绪时搜索结果做精准重排，否则直通
    let search_reranker = Arc::new(SearchReranker::new(
        rag.rerank.clone(),
        settings.rag.search_rerank_top_n,
    ));

    // fluxweir 网关生命周期（默认全关：enabled=false 时空句柄零开销；
    // enabled=true 但 gateway 未编译 → fail-fast，错误文案含 feature 指引）
    let fluxweir_gateway = match crate::bootstrap::fluxweir_gateway::init_fluxweir_gateway(
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
    };

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
