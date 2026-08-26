// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Route configuration and application builder.

use crate::config::settings::Settings;
use crate::di::{CrawlRsState, CrawlRsStateExt};
// R-teams-004 / T014：teams-off 时不导入 teams 相关类型
#[cfg(feature = "teams")]
use crate::infrastructure::database::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
// R-wh-001 / T028：webhook feature 关闭时不导入 WebhookRepoImpl
use crate::common::constants::server_config::CORS_MAX_AGE_SECS;
use crate::infrastructure::database::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl;
use crate::infrastructure::database::repositories::task_repo_impl::TaskRepositoryImpl;
#[cfg(feature = "webhook")]
use crate::infrastructure::database::repositories::webhook_repo_impl::WebhookRepoImpl;
use crate::presentation::handlers::metrics_handler;
#[cfg(not(feature = "auth"))]
use crate::presentation::middleware::auth_types::AuthState;
use crate::presentation::middleware::rate_limit_middleware::RateLimitMiddleware;
use crate::presentation::middleware::team_semaphore_middleware::team_semaphore_middleware;
use crate::presentation::routes;
use crate::presentation::state::CrawlHandlerState;
use axum::{routing::get, Extension, Router};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

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
        // R-security-002：非 test/development 环境下对 CORS 通配符输出显式生产告警
        let env = std::env::var(crate::common::constants::env_vars::ENV)
            .or_else(|_| std::env::var(crate::common::constants::env_vars::APP_ENVIRONMENT))
            .unwrap_or_else(|_| "development".to_string());
        let is_test_env = env.to_lowercase() == "test"
            || std::env::var("CRAWLRS__TEST_MODE").unwrap_or_default() == "true";
        if !is_test_env {
            log::warn!("CORS allows all origins in production");
        }
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
    let ready_routes = Router::new()
        .route("/ready", get(routes::readiness_check))
        .layer(Extension(state.db_pool.clone()))
        .layer(Extension(state.cache_service.clone()));

    Router::new()
        .route("/health", get(routes::health_check))
        .merge(ready_routes)
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
/// 其余字段（`api_key_cache` / `auth_rate_limiter` / `trusted_proxies`）已在 Stage 3 DTO 化中删除，
/// 单租户降级模式下不查 DB 加载 scope、不做缓存、不做暴力破解防护、不解析 trusted proxies。
///
/// 此函数仅在 `build_api_app_with_state` 的 forge 路由装配点调用，
/// 构造的模板通过 `from_fn_with_state(template, default_identity_middleware)`
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
             protected routes default-denied (401) unless \
             allow_unauthenticated_protected() is called. \
             DO NOT expose to public networks."
        );
    });

    // 匿名受限身份：denied scope（read/write/admin 全 false、限额 0）+ 无 token_hash
    // （default_identity_middleware 注入时显式传 None）。T038 起不再注入 full_access。
    AuthState::new(
        state.db_pool.clone(),
        DEFAULT_TEAM_ID,
        DEFAULT_API_KEY_ID,
        ApiKeyScope::denied(),
    )
}

/// Create the protected API routes using CrawlRsState.
/// 路径条件信号量中间件（migrate-routes-to-sdforge T010 保序迁移）。
///
/// 仅 `/v1/tasks/` 前缀执行 team 并发信号量——迁移自原 v2 任务组独立装配的语义；
/// 其余路径直通。执行顺序与迁移前一致：auth 外层注入 AuthState → 本中间件 → handler。
///
/// 注意：信号量 Extension 必须在函数体内按需读取而非作中间件参数——`from_fn`
/// 的参数提取发生在函数体之前，参数化写法会让非任务路径也因缺扩展被 500 拒绝。
async fn task_semaphore_for_tasks_paths(
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if !request.uri().path().starts_with("/v1/tasks/") {
        return next.run(request).await;
    }
    let Some(semaphore) = request
        .extensions()
        .get::<Arc<crate::domain::services::team_semaphore::TeamSemaphore>>()
        .cloned()
    else {
        use axum::response::IntoResponse as _;
        // 迁移前 v2 组恒 layer 该 Extension；缺失属装配错误，与 axum 提取拒绝同为 500
        log::error!("TeamSemaphore extension missing — route assembly error");
        return axum::http::StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    team_semaphore_middleware(Extension(semaphore), request, next).await
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
    // R-wh-001 / T028：webhook 相关字段在 webhook-off 时不编译
    #[cfg(feature = "webhook")]
    let webhook_event_repo = state.webhook_event_repo();
    #[cfg(feature = "webhook")]
    let webhook_repo = state.webhook_repo();

    // 创建 CORS 层
    let cors_layer = create_cors_layer(&settings);

    // T002（migrate-routes-to-sdforge）：forge 路由 wrapper 所需 Extension 的统一供给。
    // 以下具体类型此前仅在 protected/v2 组内 layer；业务端点迁入 forge router 后，
    // 统一在合并 app 层供给。须在 settings 被 Extension 层 move 前构造（读取并发锁时长）。
    let crawl_handler_state = {
        let app_state_arc = Arc::new(state.clone());
        Arc::new(CrawlHandlerState::from_app_state(&app_state_arc))
    };
    let task_repo_impl = Arc::new(TaskRepositoryImpl::new(
        state.db_pool.clone(),
        chrono::Duration::seconds(
            settings
                .concurrency
                .task_lock_duration_seconds
                .try_into()
                .expect("task_lock_duration_seconds exceeds i64 range"),
        ),
    ));
    let result_repo_impl = Arc::new(ScrapeResultRepositoryImpl::new(state.db_pool.clone()));
    #[cfg(feature = "webhook")]
    let webhook_repo_impl = Arc::new(WebhookRepoImpl::new(state.db_pool.clone()));
    #[cfg(feature = "teams")]
    let geo_restriction_repo_impl: Arc<DatabaseGeoRestrictionRepository> =
        Arc::new(DatabaseGeoRestrictionRepository::new(state.db_pool.clone()));

    // 业务端点全部由 forge router（sdforge inventory）承载（migrate-routes-to-sdforge），
    // 此处仅保留 public 健康面手写装配。
    let app = Router::new().merge(public_routes);

    // Forge routes（SDK + 平台业务路由共用同一次 inventory 收集，见
    // presentation::forge_api 模块文档的单一调用点约束）。
    // CRITICAL: auth middleware is mandatory — forge handlers extract
    // team_id/api_key_id from AuthState set by the middleware, never from the
    // request body.
    //
    // auth-on：`from_fn_with_state(pool, auth_middleware_inner)` 注入 DbPool。
    // auth-off：`from_fn_with_state(template, default_identity_middleware)` 注入默认身份模板，
    //   模板携带 `DEFAULT_TEAM_ID`/`DEFAULT_API_KEY_ID`/`full_access` scope 与 db_pool。
    // 信号量内层（先 layer）：仅 /v1/tasks/* 生效，auth 之后、handler 之前执行
    #[cfg(feature = "auth")]
    let forge_router = crate::presentation::forge_api::build_forge_router()
        .layer(axum::middleware::from_fn(task_semaphore_for_tasks_paths))
        .layer(axum::middleware::from_fn_with_state(
            state.db_pool.clone(),
            crate::presentation::middleware::auth_middleware::auth_middleware_inner,
        ));
    #[cfg(not(feature = "auth"))]
    let forge_router = {
        let template = build_default_identity_template(state);
        crate::presentation::forge_api::build_forge_router()
            .layer(axum::middleware::from_fn(task_semaphore_for_tasks_paths))
            .layer(axum::middleware::from_fn_with_state(
                template,
                crate::presentation::middleware::auth_middleware::default_identity_middleware,
            ))
    };

    let app = app
        .merge(forge_router)
        .layer(Extension(state.search_service.clone()))
        .layer(Extension(state.task_queue.clone()))
        .layer(Extension(state.crawl_repo.clone()));

    let app = app.layer(cors_layer)
        // i18n 中间件：从 Accept-Language 协商 locale，注入 Extension<Locale>
        .layer(axum::middleware::from_fn(
            crate::i18n::i18n_middleware,
        ))
        .layer(Extension(state.i18n_bundle.clone()))
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
        .layer(Extension(rate_limit_middleware))
        .layer(Extension(credits_repo))
        .layer(Extension(settings))
        .layer(Extension(search_engine_service))
        .layer(Extension(rate_limiting_service.clone()))
        .layer(Extension(state.audit_service()))
        // forge 路由统一供给（T002）：具体类型实现，供 wrapper 的 #[state] 提取
        .layer(Extension(crawl_handler_state))
        .layer(Extension(task_repo_impl))
        .layer(Extension(result_repo_impl));

    // R-wh-001 / T028：webhook Extension 层在 webhook-off 时不装配
    #[cfg(feature = "webhook")]
    let app = app
        .layer(Extension(webhook_event_repo))
        .layer(Extension(webhook_repo.clone()))
        .layer(Extension(webhook_repo_impl));

    // R-teams-004 / T014：teams-on 时附加 teams 相关 Extension
    //   （供 SDK/forge 路由中的 teams 相关 handler 使用）
    // teams-off：跳过，对应变量未声明
    #[cfg(feature = "teams")]
    let app = app
        .layer(Extension(geo_restriction_repo))
        .layer(Extension(state.geo_location_service()))
        .layer(Extension(state.team_service.clone()))
        .layer(Extension(geo_restriction_repo_impl));

    app
}

// Note: create_public_routes and build_api_app_with_state are not unit-tested
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
    // `build_api_app_with_state`). They require a fully constructed
    // `CrawlRsState`, built via `AsyncKit` against the externally-managed
    // `TEST_DATABASE_URL` PostgreSQL instance (no Docker required).

    /// Build CrawlRsState using `TEST_DATABASE_URL` (no Docker required).
    ///
    /// Returns Err if `TEST_DATABASE_URL` is not set or kit construction fails.
    async fn build_test_state() -> anyhow::Result<CrawlRsState> {
        let db_url = crate::common::test_helpers::resolve_test_database_url().ok_or_else(|| {
            anyhow::anyhow!(
                "No test database available: set TEST_DATABASE_URL or ensure Docker is running"
            )
        })?;
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
        crate::common::test_helpers::skip_if_no_test_db()
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
        let _garrison_guard = crate::common::test_helpers::acquire_garrison_global_state().await;
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

    /// T013（migrate-routes-to-sdforge）：原 `create_protected_routes_with_state`
    /// 构造测试的等价改造——protected 业务端点现由完整 app 的 forge router 承载。
    /// 断言语义保持：受保护端点已注册且被 auth 中间件拦截（非 404）。
    #[tokio::test]
    async fn test_protected_endpoint_served_by_forge_router() {
        if skip_if_no_test_db() {
            return;
        }
        let _garrison_guard = crate::common::test_helpers::acquire_garrison_global_state().await;
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
                    .method(Method::POST)
                    .uri("/v1/search")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_ne!(
            response.status(),
            StatusCode::NOT_FOUND,
            "POST /v1/search must be registered via forge router (got {})",
            response.status()
        );
    }

    /// T013：原 `create_v2_routes_with_state` 构造测试的等价改造——task 路由现由
    /// forge router 直注承载。断言语义保持：/v1/tasks/_query 已注册且方法为 POST。
    #[tokio::test]
    async fn test_v2_task_paths_served_by_forge_router() {
        if skip_if_no_test_db() {
            return;
        }
        let _garrison_guard = crate::common::test_helpers::acquire_garrison_global_state().await;
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
                    .method(Method::POST)
                    .uri("/v1/tasks/_query")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        assert_ne!(
            response.status(),
            StatusCode::NOT_FOUND,
            "POST /v1/tasks/_query must be registered via forge router (got {})",
            response.status()
        );
        assert_ne!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED,
            "POST must be allowed on /v1/tasks/_query (got {})",
            response.status()
        );
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
        let _garrison_guard = crate::common::test_helpers::acquire_garrison_global_state().await;
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
        let _garrison_guard = crate::common::test_helpers::acquire_garrison_global_state().await;
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

    /// T027-4：验证 `POST /v1/admin/api-keys` 路由按 auth feature 门控注册。
    ///
    /// # auth-on
    ///
    /// 路由被注册，未经认证的请求被 `auth_middleware_inner` 拦截返回 4xx
    /// （401 缺 Bearer / 403 权限不足），**而非 404**——证明路由可达。
    /// handler 内再做 `has_permission(Admin)` 二次校验（CWE-862 纵深防御）。
    ///
    /// # auth-off
    ///
    /// `api_key_handler` 模块不编译，路由不注册，请求返回 404。
    ///
    /// # Spec
    ///
    /// - R-key-lifecycle-001
    #[cfg(feature = "auth")]
    #[tokio::test]
    async fn test_admin_api_keys_endpoint_registered_when_auth_on() {
        if skip_if_no_test_db() {
            return;
        }
        // T034 修复：build_test_state() → init_services() → init_garrison_auth() →
        // set_garrison_dao，缺少此守卫会与其它调用 init_services 的并行测试竞态，
        // 导致 "global DAO already injected" panic。
        let _garrison_guard = crate::common::test_helpers::acquire_garrison_global_state().await;
        let state = match build_test_state().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[skip] failed to build CrawlRsState: {e}");
                return;
            }
        };
        let settings = Arc::new(load_test_settings());
        // T013：protected 组装配已并入完整 app（业务端点由 forge router 承载）
        let router = build_api_app_with_state(&state, settings);

        // T016：inventory 注册表直接断言（与下方 HTTP 断言互补）
        {
            use sdforge::prelude::RouteRegistration;
            let has_admin_registration = inventory::iter::<RouteRegistration>()
                .any(|r| r.name == "admin_api_keys");
            assert!(
                has_admin_registration,
                "auth-on build must contain an admin_api_keys route registration"
            );
        }

        // 发送未认证 POST 请求（无 Authorization header）
        let response = router
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/v1/admin/api-keys")
                    .header("content-type", "application/json")
                    .body(Body::from("{}"))
                    .expect("Failed to build request"),
            )
            .await
            .expect("Failed to get response");

        // 路由存在则被 auth 中间件拦截（4xx），不存在则 404
        assert_ne!(
            response.status(),
            StatusCode::NOT_FOUND,
            "POST /v1/admin/api-keys must be registered when auth feature is on (got {})",
            response.status()
        );
    }

    /// T027-4（auth-off 分支）：验证 admin api-keys 路由在 auth-off 时不注册。
    ///
    /// T016 修法：auth-off 下 `default_identity_middleware` 位于 layer 包裹的
    /// fallback 之上，未匹配路径同样被 401 拒绝——HTTP 状态码无法区分「路由未
    /// 注册」与「默认拒绝」（axum 将 `Router::layer` 应用于 fallback 的既有语义，
    /// 与迁移前 protected 路由器同构，属预存在行为，非本迁移引入）。故改为直接
    /// 断言 inventory 注册表（compile-time 收集的确定性事实，不依赖全局标志）。
    #[cfg(not(feature = "auth"))]
    #[tokio::test]
    async fn test_admin_api_keys_endpoint_not_registered_when_auth_off() {
        use sdforge::prelude::RouteRegistration;

        let has_admin_registration = inventory::iter::<RouteRegistration>()
            .any(|r| r.name == "admin_api_keys");
        assert!(
            !has_admin_registration,
            "auth-off build must NOT contain an admin_api_keys route registration \
             (inventory is compile-time collected, so this is deterministic)"
        );
    }

    // ========== 业务端点冒烟测试（migrate-routes-to-sdforge T003 回归防护网）==========
    //
    // 对全部 19 个业务端点逐一发起未认证请求：
    // - auth-on：必须被最外层 auth 中间件以 401/403 拦截——证明「路由可达 + 方法匹配」，
    //   而非 404（路由未注册）；
    // - auth-off：请求穿透默认身份中间件进入业务层——断言非 404/405/500/501，
    //   其中 500 会暴露合并 app 层 Extension 供给缺口。
    //
    // 迁移过渡期同时压住手写与 sdforge 两条注册路径；迁移完成后作为单一注册面的
    // 可达性回归网。feature 门控行与生产路由注册的 cfg 完全对应。

    struct SmokeEndpoint {
        method: &'static str,
        path: &'static str,
    }

    fn smoke_endpoints() -> Vec<SmokeEndpoint> {
        let mut v = vec![
            SmokeEndpoint {
                method: "POST",
                path: "/v1/scrape",
            },
            SmokeEndpoint {
                method: "GET",
                path: "/v1/scrape/3f0e3e57-2f6d-4cbb-9e8e-6a1a5b1f1234",
            },
            SmokeEndpoint {
                method: "POST",
                path: "/v1/search",
            },
            SmokeEndpoint {
                method: "POST",
                path: "/v1/crawl",
            },
            SmokeEndpoint {
                method: "GET",
                path: "/v1/crawl/3f0e3e57-2f6d-4cbb-9e8e-6a1a5b1f1234",
            },
            SmokeEndpoint {
                method: "GET",
                path: "/v1/crawl/3f0e3e57-2f6d-4cbb-9e8e-6a1a5b1f1234/results",
            },
            SmokeEndpoint {
                method: "DELETE",
                path: "/v1/crawl/3f0e3e57-2f6d-4cbb-9e8e-6a1a5b1f1234",
            },
            SmokeEndpoint {
                method: "GET",
                path: "/v1/audit/logs",
            },
            SmokeEndpoint {
                method: "GET",
                path: "/v1/audit/denied",
            },
            SmokeEndpoint {
                method: "POST",
                path: "/v1/tasks/_query",
            },
            SmokeEndpoint {
                method: "POST",
                path: "/v1/tasks/_cancel",
            },
        ];
        // R-wh-001：webhook 门控组
        #[cfg(feature = "webhook")]
        v.extend([
            SmokeEndpoint {
                method: "POST",
                path: "/v1/webhooks",
            },
            SmokeEndpoint {
                method: "GET",
                path: "/v1/webhooks",
            },
        ]);
        // R-teams-002：extract / teams 门控组
        #[cfg(feature = "teams")]
        v.extend([
            SmokeEndpoint {
                method: "POST",
                path: "/v1/extract",
            },
            SmokeEndpoint {
                method: "GET",
                path: "/v1/teams/me",
            },
            SmokeEndpoint {
                method: "GET",
                path: "/v1/teams/me/usage",
            },
            SmokeEndpoint {
                method: "GET",
                path: "/v1/teams/geo-restrictions",
            },
            SmokeEndpoint {
                method: "PUT",
                path: "/v1/teams/geo-restrictions",
            },
        ]);
        // R-key-lifecycle-001：admin 门控组
        #[cfg(feature = "auth")]
        v.push(SmokeEndpoint {
            method: "POST",
            path: "/v1/admin/api-keys",
        });
        v
    }

    // ========== 条件信号量中间件直通性（T010）==========

    /// 非 /v1/tasks/ 路径必须直通：即使无任何 Extension（无信号量、无 AuthState）
    /// 也到达 handler，证明信号量逻辑未被触发。
    #[tokio::test]
    async fn test_task_semaphore_middleware_passes_through_non_task_paths() {
        use tower::ServiceExt;

        async fn ok_handler() -> StatusCode {
            StatusCode::OK
        }

        let app: Router = Router::new()
            .route("/other", get(ok_handler))
            .layer(axum::middleware::from_fn(task_semaphore_for_tasks_paths));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/other")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(
            response.status(),
            StatusCode::OK,
            "non-/v1/tasks/ paths must bypass the semaphore entirely \
             (no Extension<TeamSemaphore> present, so any semaphore invocation would fail)"
        );
    }

    /// 全业务端点可达性 + 认证语义冒烟（19 端点）。
    #[tokio::test]
    async fn smoke_all_business_endpoints_reachable() {
        if skip_if_no_test_db() {
            return;
        }
        let _garrison_guard = crate::common::test_helpers::acquire_garrison_global_state().await;
        let state = match build_test_state().await {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[skip] failed to build CrawlRsState: {e}");
                return;
            }
        };
        let app = build_api_app_with_state(&state, Arc::new(load_test_settings()));

        for ep in smoke_endpoints() {
            let req = Request::builder()
                .method(Method::from_bytes(ep.method.as_bytes()).unwrap())
                .uri(ep.path)
                .header("content-type", "application/json")
                .body(Body::from("{}"))
                .expect("request build");
            let resp = app.clone().oneshot(req).await.expect("oneshot");
            let status = resp.status().as_u16();

            #[cfg(feature = "auth")]
            {
                assert!(
                    status == 401 || status == 403,
                    "{} {} expected 401/403 from auth middleware, got {} \
                     (404 = route missing; 500 = missing Extension)",
                    ep.method,
                    ep.path,
                    status
                );
            }
            #[cfg(not(feature = "auth"))]
            {
                assert!(
                    !matches!(status, 404 | 405 | 500 | 501),
                    "{} {} unexpectedly returned {} \
                     (404 = route missing; 405 = wrong method; 500 = missing Extension)",
                    ep.method,
                    ep.path,
                    status
                );
            }
        }
    }
}
