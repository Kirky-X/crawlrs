// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 运行时服务装配（限流 / 并发 / 请求合并 / 认证审计 / workers）。
//!
//! 从 `init_services` 提取的装配阶段：每个函数只负责一个装配阶段，
//! 返回该阶段产出的组件；`init_services` 保留原有调用顺序与副作用顺序。

use std::sync::Arc;

use crate::application::use_cases::create_scrape::{CreateScrapeUseCase, CreateScrapeUseCaseTrait};
use crate::bootstrap::infrastructure::InfrastructureComponents;
use crate::bootstrap::infrastructure::Repositories;
use crate::config::settings::Settings;
use crate::domain::services::audit_service::{AuditService, AuditServiceTrait};
#[cfg(not(feature = "webhook"))]
use crate::domain::services::noop_webhook_service::NoopWebhookService;
use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::domain::services::team_semaphore::TeamSemaphore;
use crate::domain::services::webhook_service::WebhookService;
#[cfg(feature = "webhook")]
use crate::domain::services::webhook_service::WebhookServiceImpl;
use crate::engines::engine_client::EngineClient;
use crate::infrastructure::database::repositories::audit_log_repo_impl::AuditLogRepositoryImpl;
#[cfg(feature = "teams")]
use crate::infrastructure::geolocation::GeoLocationServiceImpl;
#[cfg(feature = "webhook")]
use crate::infrastructure::services::webhook_sender_impl::WebhookSenderImpl;
use crate::presentation::middleware::rate_limit_middleware::RateLimitMiddleware;
use crate::queue::task_queue::{PostgresTaskQueue, TaskQueue};
use crate::utils::coalesce::RequestCoalescer;

#[cfg(feature = "teams")]
use crate::domain::services::geo_location::GeoLocationService;
#[cfg(feature = "teams")]
use crate::domain::services::team_service::TeamService;
#[cfg(feature = "auth")]
use crate::infrastructure::auth::set_audit_service;

#[cfg(feature = "auth")]
use super::init_garrison_auth;
use super::{init_rate_limit_middleware, init_rate_limiting_service, init_team_semaphore};

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

/// 装配 webhook 服务：webhook-on → `WebhookServiceImpl`；webhook-off → `NoopWebhookService`
///（所有方法返回 Ok(())，业务逻辑放行）。
pub(super) fn init_webhook_service(
    #[cfg_attr(not(feature = "webhook"), allow(unused_variables))] repositories: &Repositories,
    #[cfg_attr(not(feature = "webhook"), allow(unused_variables))] settings: &Settings,
    http_client: &Arc<reqwest::Client>,
) -> Arc<dyn WebhookService> {
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
    webhook_service
}

/// 装配 teams 相关服务：地理定位服务 + 团队服务（teams-off 时不编译）。
#[cfg(feature = "teams")]
pub(super) fn init_team_services(
    repositories: &Repositories,
    http_client: &Arc<reqwest::Client>,
) -> (Arc<TeamService>, Arc<dyn GeoLocationService>) {
    // teams 相关服务在 teams-off 时不初始化
    let geo_location_service = Arc::new(GeoLocationServiceImpl::new(http_client.clone()));
    let team_service = Arc::new(TeamService::new(
        geo_location_service.clone(),
        repositories.geo_restriction_repo.clone(),
    ));
    (team_service, geo_location_service)
}

/// 装配 garrison 认证鉴权与审计：任务队列 + 审计服务（含 garrison listener 全局态注入）。
pub(super) async fn init_auth_and_audit(
    #[cfg_attr(not(feature = "auth"), allow(unused_variables))] settings: &Settings,
    infrastructure: &InfrastructureComponents,
) -> (Arc<dyn TaskQueue>, Arc<dyn AuditServiceTrait>) {
    let repositories = &infrastructure.repositories;

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

    let queue: Arc<dyn TaskQueue> =
        Arc::new(PostgresTaskQueue::new(repositories.task_repo.clone()));

    let audit_repo = Arc::new(AuditLogRepositoryImpl::new(
        infrastructure.db.inner().clone(),
    ));
    let audit_service: Arc<dyn AuditServiceTrait> = Arc::new(AuditService::new(audit_repo));

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
    if set_audit_service(Arc::clone(&audit_service)).is_err() {
        panic!(
            "set_audit_service failed: audit_service already injected \
             (check for duplicate init_services calls)"
        );
    }

    (queue, audit_service)
}

/// workers 装配产物
pub(super) struct WorkerServices {
    #[cfg(feature = "webhook")]
    pub(super) webhook_worker: Arc<crate::workers::webhook_worker::WebhookWorker>,
    pub(super) backlog_worker: Arc<crate::workers::backlog_worker::BacklogWorker>,
    pub(super) expiration_worker: Arc<crate::workers::expiration_worker::ExpirationWorker>,
}

/// 装配后台 workers：webhook（仅 webhook-on）/ backlog / expiration。
pub(super) fn init_workers(
    repositories: &Repositories,
    settings: &Settings,
    #[cfg_attr(not(feature = "webhook"), allow(unused_variables))] webhook_service: &Arc<
        dyn WebhookService,
    >,
    rate_limiting_service: &Arc<dyn RateLimitingService>,
) -> WorkerServices {
    // Initialize WebhookWorker（仅 webhook-on 时构造）
    // webhook-off：不构造 WebhookWorker，ServicesComponents.webhook_worker 字段不编译
    #[cfg(feature = "webhook")]
    let webhook_worker = Arc::new(crate::workers::webhook_worker::WebhookWorker::new(
        repositories.webhook_event_repo.clone(),
        webhook_service.clone(),
        crate::utils::retry_policy::RetryPolicy::default(),
    ));

    let backlog_worker = Arc::new(crate::workers::backlog_worker::BacklogWorker::new(
        repositories.tasks_backlog_repo.clone(),
        repositories.task_repo.clone(),
        rate_limiting_service.clone(),
        Arc::new(settings.clone()),
    ));

    let expiration_worker = Arc::new(crate::workers::expiration_worker::ExpirationWorker::new(
        repositories.task_repo.clone(),
        repositories.result_repo.clone(),
        settings.retention.scrape_results_days,
    ));

    WorkerServices {
        #[cfg(feature = "webhook")]
        webhook_worker,
        backlog_worker,
        expiration_worker,
    }
}
