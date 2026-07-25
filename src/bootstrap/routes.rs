// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Route configuration and application builder.

use crate::config::settings::Settings;
use crate::di::{CrawlRsState, CrawlRsStateExt};
// R-teams-004 / T014：teams-off 时不导入 teams 相关类型
#[cfg(feature = "teams")]
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
#[cfg(feature = "teams")]
use crate::infrastructure::database::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
use crate::infrastructure::database::repositories::webhook_repo_impl::WebhookRepoImpl;
use crate::presentation::handlers::{
    audit_handler, crawl_handler, extract_handler, metrics_handler, scrape_handler, search_handler,
    webhook_handler,
};
// R-teams-002 / T012：teams-off 时 team_handler 模块不编译
#[cfg(feature = "teams")]
use crate::presentation::handlers::team_handler;
use crate::presentation::middleware::auth_middleware::AuthState;
use crate::presentation::middleware::rate_limit_middleware::RateLimitMiddleware;
use crate::presentation::middleware::team_semaphore_middleware::team_semaphore_middleware;
use crate::presentation::routes;
use crate::presentation::routes::task::task_routes;
use crate::presentation::state::CrawlHandlerState;
use axum::{
    routing::{delete, get, post},
    Extension, Router,
};
// R-teams-002 / T012：put 仅在 teams-on 时被使用（/v1/teams/geo-restrictions PUT）
#[cfg(feature = "teams")]
use axum::routing::put;
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

// 导入常量
use crate::common::constants::server_config::CORS_MAX_AGE_SECS;

// auth feature 关闭时需要的默认身份常量与 scope 构造器（T009）
#[cfg(not(feature = "auth"))]
use crate::common::constants::default_identity::{DEFAULT_API_KEY_ID, DEFAULT_TEAM_ID};
#[cfg(not(feature = "auth"))]
use crate::domain::auth::ApiKeyScope;

/// 创建 CORS 中间件层
///
/// 基于配置创建适合开发/生产环境的 CORS 配置
fn create_cors_layer(settings: &Settings) -> CorsLayer {
    let allowed_origins: Vec<String> = settings
        .cors
        .allowed_origins
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();

    let cors_layer = if allowed_origins.is_empty() || allowed_origins.iter().any(|o| o == "*") {
        // 生产环境不应使用通配符，这里仅作为开发回退
        log::warn!("CORS 使用通配符 '*'，建议在生产环境中配置具体的来源");
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        let origins: Vec<axum::http::HeaderValue> = allowed_origins
            .iter()
            .filter_map(|origin| origin.parse().ok())
            .collect();

        if origins.is_empty() {
            log::warn!("CORS 配置无效，允许所有来源作为回退");
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any)
        } else {
            CorsLayer::new()
                .allow_origin(origins)
                .allow_methods([
                    axum::http::Method::GET,
                    axum::http::Method::POST,
                    axum::http::Method::PUT,
                    axum::http::Method::DELETE,
                    axum::http::Method::PATCH,
                    axum::http::Method::HEAD,
                    axum::http::Method::OPTIONS,
                ])
                .allow_headers([
                    axum::http::HeaderName::from_static("authorization"),
                    axum::http::HeaderName::from_static("content-type"),
                    axum::http::HeaderName::from_static("x-api-key"),
                    axum::http::HeaderName::from_static("x-request-id"),
                ])
                .expose_headers([axum::http::HeaderName::from_static("x-request-id")])
                .max_age(std::time::Duration::from_secs(CORS_MAX_AGE_SECS))
        }
    };

    cors_layer
}

/// Create public API routes (no authentication required).
pub fn create_public_routes(state: &CrawlRsState) -> Router {
    Router::new()
        .route("/health", get(routes::health_check))
        .route("/metrics", get(metrics_handler::metrics))
        .route("/v1/version", get(routes::version))
        .with_state(Arc::new(state.clone()))
}

/// 构造单租户降级模式下的默认身份 `AuthState` 模板（`auth` feature 关闭时使用）。
///
/// 模板携带：
/// - `pool`: 共享 `CrawlRsState` 的 `DbPool`，供下游业务 handler 使用
/// - `team_id`: `DEFAULT_TEAM_ID`（固定值，单租户标识）
/// - `api_key_id`: `DEFAULT_API_KEY_ID`（固定值，与 team_id 区分）
/// - `scope`: `ApiKeyScope::full_access()`（read/write/admin=true、limit=u32::MAX）
///
/// 其余字段（`auth_scope_service` / `api_key_cache` / `auth_rate_limiter` / `trusted_proxies`）
/// 均为 `None`：单租户降级模式下不查 DB 加载 scope、不做缓存、不做暴力破解防护、
/// 不解析 trusted proxies。
///
/// 此函数仅在三处路由装配点（`create_protected_routes_with_state` /
/// `create_v2_routes_with_state` / `build_api_app_with_state` 的 SDK 路由）
/// 调用，构造的模板通过 `from_fn_with_state(template, default_identity_middleware)`
/// 注入到 `FromFnLayer`，由 layer 在每请求 `Service::call` 内 `clone()` 一次传给
/// `default_identity_middleware`（见 diting 架构审查 MEDIUM-1 / 性能审查 LOW-2）。
///
/// # Security
///
/// 首次调用时通过 `OnceLock` 保证只打印一次 WARNING 日志，告知运维本实例运行在
/// 无鉴权模式（见 tiangang 安全审查 MEDIUM-2）。三处路由装配点都会调用此函数，
/// 但日志只在第一次调用时输出，避免启动日志噪声。
#[cfg(not(feature = "auth"))]
fn build_default_identity_template(state: &CrawlRsState) -> AuthState {
    static WARN_ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    WARN_ONCE.get_or_init(|| {
        log::warn!(
            "auth feature disabled — running in single-tenant degraded mode; \
             NO authentication, NO brute-force protection. \
             DO NOT expose to public networks."
        );
    });

    AuthState::new(
        state.db_pool.clone(),
        DEFAULT_TEAM_ID,
        DEFAULT_API_KEY_ID,
        ApiKeyScope::full_access(),
    )
}

/// Create the protected API routes using CrawlRsState.
///
/// # Arguments
///
/// * `state` - Application state with resolved dependencies
/// * `settings` - Application settings
pub fn create_protected_routes_with_state(state: &CrawlRsState, settings: Arc<Settings>) -> Router {
    let team_semaphore = state.team_semaphore.clone();
    let queue = state.task_queue.clone();
    let task_repo = state.task_repo.clone();
    let result_repo = state.result_repo.clone();
    let rate_limiting_service = state.rate_limiting_service.clone();
    let rate_limit_middleware = RateLimitMiddleware::new(rate_limiting_service.clone());
    let crawl_repo = state.crawl_repo.clone();
    let webhook_repo = state.webhook_repo.clone();
    let webhook_event_repo = state.webhook_event_repo();
    let search_engine_service = state.search_client();
    // R-teams-004 / T014：teams 相关字段在 teams-off 时不编译
    #[cfg(feature = "teams")]
    let team_service = state.team_service.clone();
    #[cfg(feature = "teams")]
    let geo_location_service = state.geo_location_service();
    let credits_repo = state.credits_repo();

    // 构造一次具体实现，同时用于 trait object Extension 和泛型 handler Extension
    // （架构 HIGH-2：消除重复构造——之前 trait object 和 concrete 各 new 一次）
    //
    // R-teams-004 / T014：teams-off 时不构造 geo_restriction_repo_impl / geo_restriction_repo
    //   （DatabaseGeoRestrictionRepository 仅供 teams-on 的 extract_handler 使用）
    #[cfg(feature = "teams")]
    let geo_restriction_repo_impl: Arc<DatabaseGeoRestrictionRepository> =
        Arc::new(DatabaseGeoRestrictionRepository::new(state.db_pool.clone()));
    #[cfg(feature = "teams")]
    let geo_restriction_repo: Arc<dyn GeoRestrictionRepository> =
        geo_restriction_repo_impl.clone();

    // WebhookRepoImpl 同理：构造一次，复用给 Extension layer
    let webhook_repo_impl: Arc<WebhookRepoImpl> =
        Arc::new(WebhookRepoImpl::new(state.db_pool.clone()));

    // Create Arc<CrawlRsState> for handlers that need unified state, and derive
    // CrawlHandlerState from it for crawl handlers (decoupled for testability).
    let app_state_arc = Arc::new(state.clone());
    let crawl_handler_state = Arc::new(CrawlHandlerState::from_app_state(&app_state_arc));

    // Auth state for middleware
    //
    // auth-on：通过 `GLOBAL_AUTH_STATE` 全局共享，`auth_middleware()` 从中读取
    //   （含真实 `auth_scope_service` 用于按 API Key 加载 scope）。
    // auth-off：通过 `from_fn_with_state(template, default_identity_middleware)` 直接注入模板，
    //   `default_identity_middleware` 通过 `State<AuthState>` 提取器读取；
    //   模板在下方 `.layer()` 调用处构造（携带 `DEFAULT_TEAM_ID`/`DEFAULT_API_KEY_ID`/`full_access` scope）。
    #[cfg(feature = "auth")]
    {
        let auth_scope_service = state.auth_scope_service.as_ref().map(|arc| (**arc).clone());
        let auth_state = Arc::new(AuthState::new_for_middleware(
            state.db_pool.clone(),
            auth_scope_service,
        ));
        crate::presentation::middleware::auth_middleware::set_global_auth_state(auth_state);
    }

    let app: Router = Router::new()
        .route("/v1/scrape", post(scrape_handler::create_scrape))
        .route("/v1/scrape/{id}", get(scrape_handler::get_scrape_status))
        .route(
            "/v1/webhooks",
            post(webhook_handler::create_webhook::<WebhookRepoImpl>),
        )
        .route(
            "/v1/webhooks",
            get(webhook_handler::list_webhooks::<WebhookRepoImpl>),
        )
        .route("/v1/crawl", post(crawl_handler::create_crawl))
        .route("/v1/crawl/{id}", get(crawl_handler::get_crawl))
        .route(
            "/v1/crawl/{id}/results",
            get(crawl_handler::get_crawl_results),
        )
        .route("/v1/crawl/{id}", delete(crawl_handler::cancel_crawl))
        .route("/v1/search", post(search_handler::search));

    // R-teams-003 / T013：extract 路由按 teams feature 分裂
    //
    // teams-on：保留 GR 泛型（`extract::<DatabaseGeoRestrictionRepository>`），
    //   handler 需要 `Extension<Arc<GR>>` 与 `Extension<Arc<TeamService>>` 参数
    // teams-off：移除 GR 泛型，handler 不接收 GR/TeamService 参数
    #[cfg(feature = "teams")]
    let app = app.route(
        "/v1/extract",
        post(extract_handler::extract::<DatabaseGeoRestrictionRepository>),
    );
    #[cfg(not(feature = "teams"))]
    let app = app.route("/v1/extract", post(extract_handler::extract));

    // R-teams-002 / T012：teams 路由组单独 cfg 门控
    //
    // teams-on：注册 /v1/teams/me、/v1/teams/me/usage、/v1/teams/geo-restrictions (GET/PUT) 4 条路由
    // teams-off：跳过注册（端点不存在，返回 404 而非编译失败）
    //
    // 注意：teams-off 时 team_handler 模块本身不编译（见 handlers/mod.rs 的 cfg 门控），
    // 所以这里引用 team_handler::* 必须 cfg 门控避免 unresolved import。
    #[cfg(feature = "teams")]
    let app = app
        .route("/v1/teams/me", get(team_handler::get_team_info))
        .route("/v1/teams/me/usage", get(team_handler::get_team_usage))
        .route(
            "/v1/teams/geo-restrictions",
            get(team_handler::get_team_geo_restrictions::<DatabaseGeoRestrictionRepository>),
        )
        .route(
            "/v1/teams/geo-restrictions",
            put(team_handler::update_team_geo_restrictions::<DatabaseGeoRestrictionRepository>),
        );

    let app = app
        .route("/v1/audit/logs", get(audit_handler::get_audit_logs))
        .route("/v1/audit/denied", get(audit_handler::get_denied_requests));

    // 认证中间件层（条件编译，T009）
    //
    // auth-on：`auth_middleware()` 从 `GLOBAL_AUTH_STATE` 读取真实 AuthState
    //   （含 auth_scope_service / api_key_cache / auth_rate_limiter / trusted_proxies）。
    // auth-off：`from_fn_with_state(template, default_identity_middleware)` 注入默认身份模板，
    //   模板携带 `DEFAULT_TEAM_ID`/`DEFAULT_API_KEY_ID`/`full_access` scope 与 db_pool，
    //   `default_identity_middleware` 克隆模板并注入 extensions（不查 DB、不校验 token）。
    //
    // 注意：两分支返回的 `FromFnLayer` 类型参数不同（`S=()` vs `S=AuthState`），
    // 无法用 `let layer = if ... { from_fn(...) } else { from_fn_with_state(...) }` 统一类型，
    // 必须用 shadowing + cfg 分别调用 `.layer()`。
    #[cfg(feature = "auth")]
    let app = app.layer(axum::middleware::from_fn(
        crate::presentation::middleware::auth_middleware::auth_middleware(),
    ));
    #[cfg(not(feature = "auth"))]
    let app = {
        let template = build_default_identity_template(state);
        app.layer(axum::middleware::from_fn_with_state(
            template,
            crate::presentation::middleware::auth_middleware::default_identity_middleware,
        ))
    };

    // R-teams-004 / T014：teams 相关 Extension 层在 teams-off 时不装配
    //
    // teams-on：附加 geo_restriction_repo / team_service / geo_location_service / geo_restriction_repo_impl
    //   四个 Extension 层（供 teams-on 版本的 extract_handler / team_handler / crawl_handler 等使用）
    // teams-off：跳过这四个 Extension 层（对应 handler 不接收这些参数，trait object 缺失不会触发 panic）
    #[cfg(feature = "teams")]
    let app = app
        .layer(Extension(geo_restriction_repo))
        .layer(Extension(geo_location_service.clone()))
        .layer(Extension(team_service))
        .layer(Extension(geo_restriction_repo_impl));

    app
        .layer(Extension(team_semaphore))
        .layer(Extension(queue))
        .layer(Extension(task_repo))
        .layer(Extension(result_repo))
        .layer(Extension(rate_limit_middleware))
        .layer(Extension(settings))
        .layer(Extension(rate_limiting_service))
        .layer(Extension(crawl_repo))
        .layer(Extension(webhook_repo))
        .layer(Extension(webhook_event_repo))
        .layer(Extension(search_engine_service))
        .layer(Extension(state.search_service.clone()))
        .layer(Extension(crawl_handler_state)) // CrawlHandlerState for crawl handlers
        .layer(Extension(credits_repo))
        .layer(Extension(webhook_repo_impl))
}

/// Create v2 task routes using CrawlRsState.
///
/// # Arguments
///
/// * `state` - Application state with resolved dependencies
pub fn create_v2_routes_with_state(state: &CrawlRsState) -> Router {
    let task_repo = state.task_repo.clone();
    let result_repo = state.result_repo.clone();
    let crawl_repo = state.crawl_repo.clone();
    let webhook_repo = state.webhook_repo.clone();
    let webhook_event_repo = state.webhook_event_repo();
    let team_semaphore = state.team_semaphore.clone();

    // Auth state for middleware
    //
    // auth-on：`ensure_global_auth_state_set` 仅在未设置时填充（避免覆盖 protected routes
    //   已设置的完整 state——protected routes 带 auth_scope_service，v2 routes 不带）。
    // auth-off：不调用全局状态，模板通过下方 `from_fn_with_state` 直接注入。
    #[cfg(feature = "auth")]
    {
        // Use new_for_middleware to ensure global cache is initialized
        let auth_state = Arc::new(AuthState::new_for_middleware(state.db_pool.clone(), None));
        crate::presentation::middleware::auth_middleware::ensure_global_auth_state_set(auth_state);
    }

    let app = task_routes()
        .layer(Extension(task_repo.clone()))
        .layer(Extension(result_repo.clone()));

    // 认证中间件层（条件编译，T009）
    //
    // auth-on：`auth_middleware()` 从 `GLOBAL_AUTH_STATE` 读取真实 AuthState。
    // auth-off：`from_fn_with_state(template, default_identity_middleware)` 注入默认身份模板，
    //   模板携带 `DEFAULT_TEAM_ID`/`DEFAULT_API_KEY_ID`/`full_access` scope 与 db_pool。
    #[cfg(feature = "auth")]
    let app = app.layer(axum::middleware::from_fn(
        crate::presentation::middleware::auth_middleware::auth_middleware(),
    ));
    #[cfg(not(feature = "auth"))]
    let app = {
        let template = build_default_identity_template(state);
        app.layer(axum::middleware::from_fn_with_state(
            template,
            crate::presentation::middleware::auth_middleware::default_identity_middleware,
        ))
    };

    app.layer(axum::middleware::from_fn(team_semaphore_middleware))
        .layer(Extension(team_semaphore))
        .layer(Extension(task_repo.clone()))
        .layer(Extension(result_repo.clone()))
        .layer(Extension(crawl_repo.clone()))
        .layer(Extension(webhook_repo.clone()))
        .layer(Extension(webhook_event_repo.clone()))
}

/// Build the complete API application router using CrawlRsState.
///
/// # Arguments
///
/// * `state` - Application state with resolved dependencies from DI container
/// * `settings` - Application settings
///
/// # Returns
///
/// Returns the configured API router.
pub fn build_api_app_with_state(state: &CrawlRsState, settings: Arc<Settings>) -> Router {
    let public_routes = create_public_routes(state);
    let protected_routes = create_protected_routes_with_state(state, settings.clone());
    let v2_routes = create_v2_routes_with_state(state);

    let rate_limiting_service = state.rate_limiting_service.clone();
    let rate_limit_middleware = RateLimitMiddleware::new(rate_limiting_service.clone());
    let search_engine_service = state.search_client();
    let queue = state.task_queue.clone();
    // R-teams-004 / T014：teams-off 时不构造 geo_restriction_repo
    //   （DatabaseGeoRestrictionRepository 仅供 teams-on SDK 路由使用）
    #[cfg(feature = "teams")]
    let geo_restriction_repo = state.geo_restriction_repo();
    let credits_repo = state.credits_repo();
    let crawl_repo = state.crawl_repo.clone();
    let webhook_event_repo = state.webhook_event_repo();
    let webhook_repo = state.webhook_repo();

    // 创建 CORS 层
    let cors_layer = create_cors_layer(&settings);

    let app = Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .merge(v2_routes);

    // SDK routes (always enabled; sdforge is non-optional since Task9)
    // CRITICAL: auth middleware is mandatory — SDK handlers extract team_id/api_key_id
    // from AuthState set by the middleware, never from the request body.
    //
    // auth-on：`auth_middleware()` 从 `GLOBAL_AUTH_STATE` 读取真实 AuthState。
    // auth-off：`from_fn_with_state(template, default_identity_middleware)` 注入默认身份模板，
    //   模板携带 `DEFAULT_TEAM_ID`/`DEFAULT_API_KEY_ID`/`full_access` scope 与 db_pool。
    #[cfg(feature = "auth")]
    let sdk_router = crate::presentation::sdk::build_sdk_router().layer(
        axum::middleware::from_fn(crate::presentation::middleware::auth_middleware::auth_middleware()),
    );
    #[cfg(not(feature = "auth"))]
    let sdk_router = {
        let template = build_default_identity_template(state);
        crate::presentation::sdk::build_sdk_router().layer(axum::middleware::from_fn_with_state(
            template,
            crate::presentation::middleware::auth_middleware::default_identity_middleware,
        ))
    };

    let app = app
        .merge(sdk_router)
        .layer(Extension(state.search_service.clone()))
        .layer(Extension(state.task_queue.clone()))
        .layer(Extension(state.crawl_repo.clone()));

    let app = app.layer(cors_layer)
        // Security headers middleware - should be applied early in the middleware chain
        .layer(axum::middleware::from_fn(
            crate::presentation::middleware::security_headers_middleware::security_headers_middleware,
        ))
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024)) // 10MB limit
        // 架构 HIGH-3：以下 Extension layers 供 SDK 路由使用（SDK router 仅 layer 了
        // search_service / task_queue / crawl_repo 三个）。protected/v2 路由已在各自
        // 子函数中 layer 过，此处重复 layer 对它们无功能影响（Axum 外层 Extension 覆盖
        // 内层），但为 SDK 路由提供必要的依赖注入。
        .layer(Extension(state.team_semaphore.clone()))
        .layer(Extension(queue))
        .layer(Extension(state.task_repo.clone()))
        .layer(Extension(state.result_repo.clone()))
        .layer(Extension(crawl_repo))
        .layer(Extension(webhook_event_repo))
        .layer(Extension(webhook_repo.clone()))
        .layer(Extension(rate_limit_middleware))
        .layer(Extension(credits_repo))
        .layer(Extension(settings))
        .layer(Extension(search_engine_service))
        .layer(Extension(rate_limiting_service.clone()))
        .layer(Extension(state.audit_service()));

    // R-teams-004 / T014：teams-on 时附加 geo_restriction_repo Extension
    //   （供 SDK 路由中的 teams 相关 handler 使用）
    // teams-off：跳过，geo_restriction_repo 变量未声明
    #[cfg(feature = "teams")]
    let app = app.layer(Extension(geo_restriction_repo));

    app
}

// Note: create_public_routes, create_protected_routes_with_state,
// create_v2_routes_with_state, and build_api_app_with_state are not unit-tested
// here because they require a fully constructed CrawlRsState (trait-kit AsyncKit with
// real DatabasePool and ~30 Arc<dyn Trait> dependencies). These
// functions are integration-tested via the test harness with Docker-provided
// PostgreSQL. See tests/integration/ for coverage of route wiring.

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{HeaderValue, Method, Request, StatusCode};
    use axum::routing::any;
    use tower::ServiceExt;

    // Imports for the route-builder tests below (uses TEST_DATABASE_URL).
    use crate::common::test_support::testcontainers_fixtures as tcf;
    use crate::di::modules::{
        CacheModule, DatabaseModule, EngineModule, HttpModule, InfrastructureModule,
        RepositoryModule, ServiceModule, SettingsModule,
    };
    use trait_kit::AsyncKit;

    /// Build a minimal Router with the given CorsLayer and a handler accepting any method.
    fn cors_test_app(layer: CorsLayer) -> axum::Router {
        axum::Router::new()
            .route("/ping", any(|| async { "pong" }))
            .layer(layer)
    }

    /// Load settings and override cors.allowed_origins.
    fn make_settings(origins: &str) -> Settings {
        let mut settings =
            crate::bootstrap::config::load_settings().expect("Failed to load settings");
        settings.cors.allowed_origins = origins.to_string();
        settings
    }

    // ========== create_cors_layer: wildcard branch ==========

    #[tokio::test]
    async fn test_cors_wildcard_adds_allow_origin_star() {
        // Default config has allowed_origins = "*"
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/ping")
                    .header("origin", "https://example.com")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        let allow_origin = response
            .headers()
            .get("access-control-allow-origin")
            .expect("CORS wildcard should set access-control-allow-origin");
        assert_eq!(allow_origin, "*", "wildcard config should allow origin *");
    }

    #[tokio::test]
    async fn test_cors_empty_origins_falls_back_to_wildcard() {
        let settings = make_settings("");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/ping")
                    .header("origin", "https://example.com")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("access-control-allow-origin"),
            Some(&HeaderValue::from_static("*")),
            "empty origins should fall back to wildcard"
        );
    }

    // ========== create_cors_layer: specific origins branch ==========

    #[tokio::test]
    async fn test_cors_specific_origin_reflected_for_matching_request() {
        let settings = make_settings("https://example.com");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/ping")
                    .header("origin", "https://example.com")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        let allow_origin = response
            .headers()
            .get("access-control-allow-origin")
            .expect("matching origin should get access-control-allow-origin header");
        assert_eq!(
            allow_origin, "https://example.com",
            "specific origin config should reflect the request origin"
        );
    }

    #[tokio::test]
    async fn test_cors_specific_origin_not_set_for_non_matching_request() {
        let settings = make_settings("https://example.com");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/ping")
                    .header("origin", "https://evil.com")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none(),
            "non-matching origin should NOT get CORS allow-origin header"
        );
    }

    #[tokio::test]
    async fn test_cors_multiple_specific_origins_match_one() {
        let settings = make_settings("https://example.com,https://api.example.com");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/ping")
                    .header("origin", "https://api.example.com")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("access-control-allow-origin"),
            Some(&HeaderValue::from_static("https://api.example.com")),
            "second origin in list should be allowed"
        );
    }

    #[tokio::test]
    async fn test_cors_whitespace_origins_are_trimmed() {
        let settings = make_settings(" https://example.com , https://api.example.com ");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/ping")
                    .header("origin", "https://example.com")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("access-control-allow-origin"),
            Some(&HeaderValue::from_static("https://example.com")),
            "whitespace-padded origins should be trimmed and still match"
        );
    }

    // ========== create_cors_layer: preflight (OPTIONS) tests ==========

    #[tokio::test]
    async fn test_cors_preflight_specific_origin_returns_allow_methods() {
        let settings = make_settings("https://example.com");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/ping")
                    .header("origin", "https://example.com")
                    .header("access-control-request-method", "GET")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        let allow_methods = response
            .headers()
            .get("access-control-allow-methods")
            .expect("preflight should return access-control-allow-methods");
        let methods_str = allow_methods.to_str().expect("methods header is ASCII");
        assert!(
            methods_str.contains("GET"),
            "allow-methods should include GET, got: {}",
            methods_str
        );
        assert!(
            methods_str.contains("POST"),
            "allow-methods should include POST, got: {}",
            methods_str
        );
    }

    #[tokio::test]
    async fn test_cors_preflight_specific_origin_returns_allow_headers() {
        let settings = make_settings("https://example.com");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/ping")
                    .header("origin", "https://example.com")
                    .header("access-control-request-method", "POST")
                    .header("access-control-request-headers", "content-type")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        let allow_headers = response
            .headers()
            .get("access-control-allow-headers")
            .expect("preflight should return access-control-allow-headers");
        let headers_str = allow_headers.to_str().expect("headers header is ASCII");
        assert!(
            headers_str.contains("authorization"),
            "allow-headers should include authorization, got: {}",
            headers_str
        );
        assert!(
            headers_str.contains("content-type"),
            "allow-headers should include content-type, got: {}",
            headers_str
        );
    }

    #[tokio::test]
    async fn test_cors_preflight_returns_max_age() {
        let settings = make_settings("https://example.com");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/ping")
                    .header("origin", "https://example.com")
                    .header("access-control-request-method", "GET")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        let max_age = response
            .headers()
            .get("access-control-max-age")
            .expect("preflight should return access-control-max-age");
        let max_age_str = max_age.to_str().expect("max-age is ASCII");
        let max_age_secs: u64 = max_age_str.parse().expect("max-age should be a number");
        assert_eq!(
            max_age_secs, CORS_MAX_AGE_SECS,
            "max-age should match CORS_MAX_AGE_SECS constant"
        );
    }

    #[tokio::test]
    async fn test_cors_preflight_exposes_request_id_header() {
        let settings = make_settings("https://example.com");
        let app = cors_test_app(create_cors_layer(&settings));

        // Regular GET request should include expose-headers in response
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/ping")
                    .header("origin", "https://example.com")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        let expose = response
            .headers()
            .get("access-control-expose-headers")
            .expect("response should include access-control-expose-headers");
        let expose_str = expose.to_str().expect("expose-headers is ASCII");
        assert!(
            expose_str.contains("x-request-id"),
            "expose-headers should include x-request-id, got: {}",
            expose_str
        );
    }

    // ========== create_cors_layer: invalid origin fallback ==========
    // Note: Most strings (including those with spaces) are valid HeaderValue bytes,
    // so the "origins is empty after filter_map" fallback is nearly unreachable via
    // normal string input. Here we verify the behavior of an unparseable config:
    // a string with control chars that HeaderValue::from_str rejects.

    #[tokio::test]
    async fn test_cors_unparseable_origin_falls_back_to_wildcard() {
        // A string containing a NUL byte cannot be parsed as HeaderValue,
        // so the origins vec is empty after filter_map → wildcard fallback.
        let invalid = format!("https://{}.example.com", '\0');
        let settings = make_settings(&invalid);
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/ping")
                    .header("origin", "https://example.com")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("access-control-allow-origin"),
            Some(&HeaderValue::from_static("*")),
            "unparseable origin config should fall back to wildcard"
        );
    }

    #[tokio::test]
    async fn test_cors_origin_with_spaces_is_valid_header_value() {
        // Spaces are allowed in HeaderValue, so "invalid origin with spaces"
        // is treated as a specific (but useless) allowed origin. A request
        // with a real origin should NOT get CORS headers.
        let settings = make_settings("invalid origin with spaces");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/ping")
                    .header("origin", "https://example.com")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none(),
            "request origin 'https://example.com' should not match the configured 'invalid origin with spaces'"
        );
    }

    // ========== create_cors_layer: no Origin header ==========

    #[tokio::test]
    async fn test_cors_no_origin_header_no_cors_headers() {
        let settings = make_settings("https://example.com");
        let app = cors_test_app(create_cors_layer(&settings));

        // Request without Origin header — CORS layer is a no-op
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/ping")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none(),
            "request without Origin should not get CORS headers"
        );
    }

    // ========== create_cors_layer: additional wildcard and origin parsing tests ==========

    #[tokio::test]
    async fn test_cors_wildcard_among_specific_origins_uses_wildcard() {
        // When "*" is among the origins, the wildcard branch is taken
        let settings = make_settings("https://example.com,*,https://api.example.com");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/ping")
                    .header("origin", "https://random-site.com")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("access-control-allow-origin"),
            Some(&HeaderValue::from_static("*")),
            "when '*' is in the list, wildcard should be used"
        );
    }

    #[tokio::test]
    async fn test_cors_only_wildcard_origin() {
        let settings = make_settings("*");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/ping")
                    .header("origin", "https://any-origin.com")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("access-control-allow-origin"),
            Some(&HeaderValue::from_static("*"))
        );
    }

    #[tokio::test]
    async fn test_cors_multiple_origins_first_matches() {
        let settings = make_settings("https://example.com,https://api.example.com");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/ping")
                    .header("origin", "https://example.com")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("access-control-allow-origin"),
            Some(&HeaderValue::from_static("https://example.com"))
        );
    }

    // ========== create_cors_layer: preflight with wildcard ==========

    #[tokio::test]
    async fn test_cors_preflight_wildcard_returns_allow_origin_star() {
        let settings = crate::bootstrap::config::load_settings().expect("Failed to load settings");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/ping")
                    .header("origin", "https://example.com")
                    .header("access-control-request-method", "POST")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("access-control-allow-origin"),
            Some(&HeaderValue::from_static("*")),
            "preflight with wildcard config should return *"
        );
    }

    // ========== create_cors_layer: preflight non-matching origin ==========

    #[tokio::test]
    async fn test_cors_preflight_non_matching_origin_no_allow_methods() {
        let settings = make_settings("https://example.com");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/ping")
                    .header("origin", "https://evil.com")
                    .header("access-control-request-method", "GET")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_none(),
            "non-matching origin should not get CORS headers in preflight"
        );
    }

    // ========== create_cors_layer: specific origin with different methods ==========

    #[tokio::test]
    async fn test_cors_specific_origin_post_request() {
        let settings = make_settings("https://example.com");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/ping")
                    .header("origin", "https://example.com")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("access-control-allow-origin"),
            Some(&HeaderValue::from_static("https://example.com")),
            "POST request with matching origin should get CORS header"
        );
    }

    #[tokio::test]
    async fn test_cors_specific_origin_delete_request() {
        let settings = make_settings("https://example.com");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri("/ping")
                    .header("origin", "https://example.com")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("access-control-allow-origin"),
            Some(&HeaderValue::from_static("https://example.com"))
        );
    }

    // ========== create_cors_layer: expose headers on actual response ==========

    #[tokio::test]
    async fn test_cors_specific_origin_expose_headers_on_get() {
        let settings = make_settings("https://example.com");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/ping")
                    .header("origin", "https://example.com")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        let expose = response
            .headers()
            .get("access-control-expose-headers")
            .expect("GET response should include expose-headers");
        assert!(expose.to_str().unwrap().contains("x-request-id"));
    }

    // ========== create_cors_layer: allowed methods in preflight ==========

    #[tokio::test]
    async fn test_cors_preflight_includes_all_methods() {
        let settings = make_settings("https://example.com");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/ping")
                    .header("origin", "https://example.com")
                    .header("access-control-request-method", "PUT")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        let allow_methods = response
            .headers()
            .get("access-control-allow-methods")
            .expect("preflight should return allow-methods");
        let methods_str = allow_methods.to_str().unwrap();
        assert!(methods_str.contains("GET"));
        assert!(methods_str.contains("POST"));
        assert!(methods_str.contains("PUT"));
        assert!(methods_str.contains("DELETE"));
        assert!(methods_str.contains("PATCH"));
        assert!(methods_str.contains("HEAD"));
        assert!(methods_str.contains("OPTIONS"));
    }

    // ========== create_cors_layer: allowed headers in preflight ==========

    #[tokio::test]
    async fn test_cors_preflight_includes_all_allowed_headers() {
        let settings = make_settings("https://example.com");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::OPTIONS)
                    .uri("/ping")
                    .header("origin", "https://example.com")
                    .header("access-control-request-method", "POST")
                    .header(
                        "access-control-request-headers",
                        "authorization, content-type",
                    )
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        let allow_headers = response
            .headers()
            .get("access-control-allow-headers")
            .expect("preflight should return allow-headers");
        let headers_str = allow_headers.to_str().unwrap();
        assert!(headers_str.contains("authorization"));
        assert!(headers_str.contains("content-type"));
        assert!(headers_str.contains("x-api-key"));
        assert!(headers_str.contains("x-request-id"));
    }

    // ========== create_cors_layer: trailing comma in origins ==========

    #[tokio::test]
    async fn test_cors_trailing_comma_in_origins() {
        // Trailing comma should produce an empty string that gets filtered out
        let settings = make_settings("https://example.com,");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/ping")
                    .header("origin", "https://example.com")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("access-control-allow-origin"),
            Some(&HeaderValue::from_static("https://example.com")),
            "trailing comma should be handled correctly"
        );
    }

    // ========== create_cors_layer: origins with only commas ==========

    #[tokio::test]
    async fn test_cors_origins_only_commas_falls_back_to_wildcard() {
        let settings = make_settings(",,,");
        let app = cors_test_app(create_cors_layer(&settings));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/ping")
                    .header("origin", "https://example.com")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get("access-control-allow-origin"),
            Some(&HeaderValue::from_static("*")),
            "origins with only commas should fall back to wildcard"
        );
    }

    // ========== Route builder tests using TEST_DATABASE_URL ==========
    //
    // These tests exercise the route builder functions (`create_public_routes`,
    // `create_protected_routes_with_state`, `create_v2_routes_with_state`,
    // `build_api_app_with_state`). They require a fully constructed
    // `CrawlRsState`, built via `AsyncKit` against the externally-managed
    // `TEST_DATABASE_URL` PostgreSQL instance (no Docker required).

    /// Build CrawlRsState using `TEST_DATABASE_URL` (no Docker required).
    ///
    /// Returns Err if `TEST_DATABASE_URL` is not set or kit construction fails.
    async fn build_test_state() -> anyhow::Result<CrawlRsState> {
        let db_url = std::env::var("TEST_DATABASE_URL")
            .map_err(|_| anyhow::anyhow!("TEST_DATABASE_URL not set"))?;
        let settings = Arc::new(tcf::settings_with_urls(&db_url)?);

        let mut kit = AsyncKit::new();
        kit.set_config(settings);
        kit.register::<SettingsModule>()
            .map_err(|e| anyhow::anyhow!("register SettingsModule: {e}"))?;
        kit.register::<DatabaseModule>()
            .map_err(|e| anyhow::anyhow!("register DatabaseModule: {e}"))?;
        kit.register::<HttpModule>()
            .map_err(|e| anyhow::anyhow!("register HttpModule: {e}"))?;
        kit.register::<CacheModule>()
            .map_err(|e| anyhow::anyhow!("register CacheModule: {e}"))?;
        kit.register::<RepositoryModule>()
            .map_err(|e| anyhow::anyhow!("register RepositoryModule: {e}"))?;
        kit.register::<EngineModule>()
            .map_err(|e| anyhow::anyhow!("register EngineModule: {e}"))?;
        kit.register::<InfrastructureModule>()
            .map_err(|e| anyhow::anyhow!("register InfrastructureModule: {e}"))?;
        kit.register::<ServiceModule>()
            .map_err(|e| anyhow::anyhow!("register ServiceModule: {e}"))?;

        let kit = kit
            .build()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to build kit: {e}"))?;
        let state = CrawlRsState::from_kit(&kit)?;
        Ok(state)
    }

    /// Skip helper for tests that require `TEST_DATABASE_URL`.
    fn skip_if_no_test_db() -> bool {
        if std::env::var("TEST_DATABASE_URL").is_err() {
            eprintln!("[skip] TEST_DATABASE_URL not set — test requires real DbPool");
            return true;
        }
        false
    }

    /// Load default Settings from `config/default.toml`.
    fn load_test_settings() -> Settings {
        crate::bootstrap::config::load_settings().expect("Failed to load settings")
    }

    /// `create_public_routes` should wire `/health`, `/metrics`, and
    /// `/v1/version` to handlers that respond successfully.
    #[tokio::test]
    async fn test_create_public_routes_handles_health_check() {
        if skip_if_no_test_db() {
            return;
        }
        let state = match build_test_state().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[skip] failed to build CrawlRsState: {e}");
                return;
            }
        };
        let router = create_public_routes(&state);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/health")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "public /health endpoint should return 200 OK"
        );
    }

    /// `create_protected_routes_with_state` should construct a Router that
    /// includes all v1 protected endpoints. Verify construction succeeds
    /// without panic and the router can be dropped cleanly.
    #[tokio::test]
    async fn test_create_protected_routes_constructs_without_panic() {
        if skip_if_no_test_db() {
            return;
        }
        let state = match build_test_state().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[skip] failed to build CrawlRsState: {e}");
                return;
            }
        };
        let settings = Arc::new(load_test_settings());
        // Constructing the router exercises all Extension layers and
        // set_global_auth_state calls in the function body.
        let router = create_protected_routes_with_state(&state, settings);
        // Dropping should not panic.
        drop(router);
    }

    /// `create_v2_routes_with_state` should construct a Router that wires
    /// the v2 task routes. Verify construction succeeds without panic.
    #[tokio::test]
    async fn test_create_v2_routes_constructs_without_panic() {
        if skip_if_no_test_db() {
            return;
        }
        let state = match build_test_state().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[skip] failed to build CrawlRsState: {e}");
                return;
            }
        };
        let router = create_v2_routes_with_state(&state);
        drop(router);
    }

    /// `build_api_app_with_state` should construct a complete application
    /// Router that combines public, protected, v2, and SDK routes plus the
    /// CORS and security-headers middlewares. Verify the merged router
    /// still serves the public `/health` endpoint successfully.
    #[tokio::test]
    async fn test_build_api_app_serves_public_health() {
        if skip_if_no_test_db() {
            return;
        }
        let state = match build_test_state().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[skip] failed to build CrawlRsState: {e}");
                return;
            }
        };
        let settings = Arc::new(load_test_settings());
        let router = build_api_app_with_state(&state, settings);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/health")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "merged app router should serve /health from public routes"
        );
    }

    /// Verify CORS layer is applied to the merged app router — a request
    /// with Origin header should include `access-control-allow-origin`
    /// (default config uses wildcard).
    #[tokio::test]
    async fn test_build_api_app_applies_cors_layer() {
        if skip_if_no_test_db() {
            return;
        }
        let state = match build_test_state().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[skip] failed to build CrawlRsState: {e}");
                return;
            }
        };
        let settings = Arc::new(load_test_settings());
        let router = build_api_app_with_state(&state, settings);

        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/health")
                    .header("origin", "https://example.com")
                    .body(Body::empty())
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_eq!(response.status(), StatusCode::OK);
        // Default config has allowed_origins = "*", so CORS should add
        // access-control-allow-origin: * to the response.
        assert!(
            response
                .headers()
                .get("access-control-allow-origin")
                .is_some(),
            "merged app router should apply CORS layer"
        );
    }
}
