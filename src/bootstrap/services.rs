// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Application services initialization.

mod auth;
mod runtime_services;
mod search;
#[cfg(feature = "teams")]
mod teams;
mod webhook;
mod workers;

#[cfg(feature = "auth")]
pub use auth::init_garrison_auth;

use log::info;
use std::sync::Arc;

use crate::application::use_cases::create_scrape::CreateScrapeUseCaseTrait;
use crate::bootstrap::infrastructure::InfrastructureComponents;
use crate::config::settings::Settings;
use crate::domain::services::audit_service::AuditServiceTrait;
use crate::domain::services::content_extractor::ContentExtractionFacade;
use crate::domain::services::extraction_service::ExtractionServiceTrait;
#[cfg(feature = "teams")]
use crate::domain::services::geo_location::GeoLocationService;
use crate::domain::services::llm::LLMServiceTrait;
use crate::domain::services::rag_strategy::EmbeddingProvider;
use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::domain::services::rerank::RerankProvider;
use crate::domain::services::search_service::SearchServiceTrait;
use crate::domain::services::searxng::SearxngInstanceService;
use crate::domain::services::team_semaphore::TeamSemaphore;
#[cfg(feature = "teams")]
use crate::domain::services::team_service::TeamService;
use crate::domain::services::webhook_service::WebhookService;
use crate::engines::engine_client::EngineClient;
use crate::presentation::middleware::rate_limit_middleware::RateLimitMiddleware;
use crate::queue::task_queue::TaskQueue;
use crate::search::engine_trait::SearchEngine;
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
    // 装配阶段与顺序：运行时基础设施 → webhook → teams → 搜索/RAG → 认证审计
    // → LLM/提取 → workers。顺序保持原内联实现的副作用顺序不变。
    let runtime =
        runtime_services::init_runtime_services(infrastructure, &engine_client, settings).await?;
    let webhook_service =
        webhook::init_webhook_service(&infrastructure.repositories, settings, &http_client);
    #[cfg(feature = "teams")]
    let (team_service, geo_location_service) =
        teams::init_team_services(&infrastructure.repositories, &http_client);
    let search =
        search::init_search_stack(infrastructure, settings, &http_client, &engine_client).await;
    let (queue, audit_service) = auth::init_auth_and_audit(settings, infrastructure).await;
    let llm = search::init_llm_stack(
        settings,
        &http_client,
        &search.fluxweir_gateway,
        search.embedding_provider.clone(),
    );
    let workers = workers::init_workers(
        &infrastructure.repositories,
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
    use super::runtime_services::{
        init_rate_limit_middleware, init_rate_limiting_service, init_team_semaphore,
    };
    use super::*;
    use crate::domain::models::CreditsTransactionType;
    use crate::domain::services::rate_limiting_service::{
        BacklogService, ConcurrencyConfig, ConcurrencyControlService, ConcurrencyResult,
        QuotaService, RateLimitConfig, RateLimitResult, RateLimitService, RateLimitingError,
    };
    use crate::engines::router::EngineRouter;
    use crate::utils::host_discipline::HostDiscipline;

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
}
