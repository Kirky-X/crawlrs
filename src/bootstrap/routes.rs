// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Route configuration and application builder.

use crate::config::settings::Settings;
use crate::di::infrastructure_module::GeoRestrictionRepositoryComponent;
use crate::di::{AppState, AppStateExt};
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::infrastructure::database::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
use crate::infrastructure::database::repositories::webhook_repo_impl::WebhookRepoImpl;
use crate::presentation::handlers::{
    audit_handler, crawl_handler, extract_handler, metrics_handler, scrape_handler, search_handler,
    team_handler, webhook_handler,
};
use crate::presentation::middleware::auth_middleware::AuthState;
use crate::presentation::middleware::rate_limit_middleware::RateLimitMiddleware;
use crate::presentation::middleware::team_semaphore_middleware::team_semaphore_middleware;
use crate::presentation::routes;
use crate::presentation::routes::task::task_routes;
use crate::presentation::state::CrawlHandlerState;
use axum::{
    routing::{delete, get, post, put},
    Extension, Router,
};
use std::sync::Arc;
use tower_http::cors::{Any, CorsLayer};

// 导入常量
use crate::common::constants::server_config::CORS_MAX_AGE_SECS;

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
pub fn create_public_routes(state: &AppState) -> Router {
    Router::new()
        .route("/health", get(routes::health_check))
        .route("/metrics", get(metrics_handler::metrics))
        .route("/v1/version", get(routes::version))
        .with_state(Arc::new(state.clone()))
}

/// Create the protected API routes using AppState.
///
/// # Arguments
///
/// * `state` - Application state with resolved dependencies
/// * `settings` - Application settings
pub fn create_protected_routes_with_state(state: &AppState, settings: Arc<Settings>) -> Router {
    let team_semaphore = state.team_semaphore.clone();
    let queue = state.task_queue.clone();
    let task_repo = state.task_repo.clone();
    let result_repo = state.result_repo.clone();
    let rate_limiting_service = state.rate_limiting_service.clone();
    let rate_limit_middleware = RateLimitMiddleware::new(rate_limiting_service.clone());
    let crawl_repo = state.crawl_repo.clone();
    let webhook_repo = state.webhook_repo.clone();
    let tasks_backlog_repo = state.webhook_event_repo(); // Use webhook_event_repo for now
    let search_engine_service = state.search_client();
    let team_service = state.team_service.clone();
    let geo_location_service = state.geo_location_service();
    let credits_repo = state.credits_repo();

    // Create geo restriction repository for extension (使用 DI 组件)
    let geo_restriction_repo: Arc<dyn GeoRestrictionRepository> = Arc::new(
        GeoRestrictionRepositoryComponent::new(state.db_pool.clone()),
    );

    // Create concrete DatabaseGeoRestrictionRepository for handlers that need the concrete type
    let geo_restriction_repo_impl: Arc<DatabaseGeoRestrictionRepository> =
        Arc::new(DatabaseGeoRestrictionRepository::new(state.db_pool.clone()));

    // Create concrete WebhookRepoImpl for handlers that need the concrete type
    let webhook_repo_impl: Arc<WebhookRepoImpl> =
        Arc::new(WebhookRepoImpl::new(state.db_pool.clone()));

    // Create Arc<AppState> for handlers that need unified state, and derive
    // CrawlHandlerState from it for crawl handlers (decoupled for testability).
    let app_state_arc = Arc::new(state.clone());
    let crawl_handler_state = Arc::new(CrawlHandlerState::from_app_state(&app_state_arc));

    // Auth state for middleware - wrap in Arc and set global state
    let auth_scope_service = state.auth_scope_service.as_ref().map(|arc| (**arc).clone());
    let auth_state = Arc::new(AuthState::new_for_middleware(
        state.db_pool.clone(),
        auth_scope_service,
    ));
    // Set global auth state for middleware
    crate::presentation::middleware::auth_middleware::set_global_auth_state(auth_state.clone());

    let app: Router = Router::new()
        .route("/v1/scrape", post(scrape_handler::create_scrape))
        .route("/v1/scrape/{id}", get(scrape_handler::get_scrape_status))
        .route(
            "/v1/extract",
            post(extract_handler::extract::<DatabaseGeoRestrictionRepository>),
        )
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
        .route("/v1/search", post(search_handler::search))
        .route("/v1/teams/me", get(team_handler::get_team_info))
        .route("/v1/teams/me/usage", get(team_handler::get_team_usage))
        .route(
            "/v1/teams/geo-restrictions",
            get(team_handler::get_team_geo_restrictions::<DatabaseGeoRestrictionRepository>),
        )
        .route(
            "/v1/teams/geo-restrictions",
            put(team_handler::update_team_geo_restrictions::<DatabaseGeoRestrictionRepository>),
        )
        .route("/v1/audit/logs", get(audit_handler::get_audit_logs))
        .route("/v1/audit/denied", get(audit_handler::get_denied_requests))
        .layer(axum::middleware::from_fn(
            crate::presentation::middleware::auth_middleware::auth_middleware(),
        ))
        .layer(Extension(geo_restriction_repo))
        .layer(Extension(team_semaphore))
        .layer(Extension(queue))
        .layer(Extension(task_repo))
        .layer(Extension(result_repo))
        .layer(Extension(geo_location_service.clone()))
        .layer(Extension(rate_limit_middleware))
        .layer(Extension(settings))
        .layer(Extension(rate_limiting_service))
        .layer(Extension(crawl_repo))
        .layer(Extension(webhook_repo))
        .layer(Extension(tasks_backlog_repo))
        .layer(Extension(search_engine_service))
        .layer(Extension(state.search_service.clone()))
        .layer(Extension(team_service))
        .layer(Extension(geo_location_service))
        .layer(Extension(crawl_handler_state)) // CrawlHandlerState for crawl handlers
        .layer(Extension(credits_repo))
        .layer(Extension(webhook_repo_impl))
        .layer(Extension(geo_restriction_repo_impl));

    app
}

/// Create v2 task routes using AppState.
///
/// # Arguments
///
/// * `state` - Application state with resolved dependencies
pub fn create_v2_routes_with_state(state: &AppState) -> Router {
    let task_repo = state.task_repo.clone();
    let result_repo = state.result_repo.clone();
    let crawl_repo = state.crawl_repo.clone();
    let webhook_repo = state.webhook_repo.clone();
    let webhook_event_repo = state.webhook_event_repo();
    let team_semaphore = state.team_semaphore.clone();

    // Use new_for_middleware to ensure global cache is initialized
    let auth_state = Arc::new(AuthState::new_for_middleware(state.db_pool.clone(), None));
    // Set global auth state for middleware (will be overwritten but that's ok)
    crate::presentation::middleware::auth_middleware::set_global_auth_state(auth_state);

    task_routes()
        .layer(Extension(task_repo.clone()))
        .layer(Extension(result_repo.clone()))
        .layer(Extension(team_semaphore))
        .layer(axum::middleware::from_fn(
            crate::presentation::middleware::auth_middleware::auth_middleware(),
        ))
        .layer(axum::middleware::from_fn(team_semaphore_middleware))
        .layer(Extension(task_repo.clone()))
        .layer(Extension(result_repo.clone()))
        .layer(Extension(crawl_repo.clone()))
        .layer(Extension(webhook_repo.clone()))
        .layer(Extension(webhook_event_repo.clone()))
}

/// Build the complete API application router using AppState.
///
/// # Arguments
///
/// * `state` - Application state with resolved dependencies from DI container
/// * `settings` - Application settings
///
/// # Returns
///
/// Returns the configured API router.
pub fn build_api_app_with_state(state: &AppState, settings: Arc<Settings>) -> Router {
    let public_routes = create_public_routes(state);
    let protected_routes = create_protected_routes_with_state(state, settings.clone());
    let v2_routes = create_v2_routes_with_state(state);

    let rate_limiting_service = state.rate_limiting_service.clone();
    let rate_limit_middleware = RateLimitMiddleware::new(rate_limiting_service.clone());
    let search_engine_service = state.search_client();
    let tasks_backlog_repo = state.webhook_event_repo();
    let queue = state.task_queue.clone();
    let geo_restriction_repo: Arc<dyn GeoRestrictionRepository> = Arc::new(
        GeoRestrictionRepositoryComponent::new(state.db_pool.clone()),
    );
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

    // SDK routes (only when api-sdk feature is enabled)
    // CRITICAL: auth_middleware is mandatory — SDK handlers extract team_id/api_key_id
    // from AuthState set by the middleware, never from the request body.
    #[cfg(feature = "api-sdk")]
    let app = app.merge(
        crate::presentation::sdk::build_sdk_router()
            .layer(axum::middleware::from_fn(
                crate::presentation::middleware::auth_middleware::auth_middleware(),
            ))
            .layer(Extension(state.search_service.clone()))
            .layer(Extension(state.task_queue.clone()))
            .layer(Extension(state.crawl_repo.clone())),
    );

    app.layer(cors_layer)
        // Security headers middleware - should be applied early in the middleware chain
        .layer(axum::middleware::from_fn(
            crate::presentation::middleware::security_headers_middleware::security_headers_middleware,
        ))
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024)) // 10MB limit
        .layer(Extension(state.team_semaphore.clone()))
        .layer(Extension(queue))
        .layer(Extension(state.task_repo.clone()))
        .layer(Extension(state.result_repo.clone()))
        .layer(Extension(crawl_repo))
        .layer(Extension(webhook_event_repo))
        .layer(Extension(webhook_repo.clone()))
        .layer(Extension(rate_limit_middleware))
        .layer(Extension(state.crawl_repo.clone()))
        .layer(Extension(credits_repo))
        .layer(Extension(geo_restriction_repo))
        .layer(Extension(settings))
        .layer(Extension(search_engine_service))
        .layer(Extension(tasks_backlog_repo.clone()))
        .layer(Extension(rate_limiting_service.clone()))
        .layer(Extension(state.audit_service()))
}

// Note: create_public_routes, create_protected_routes_with_state,
// create_v2_routes_with_state, and build_api_app_with_state are not unit-tested
// here because they require a fully constructed AppState (trait-kit AsyncKit with
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
}
