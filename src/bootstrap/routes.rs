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
        tracing::warn!("CORS 使用通配符 '*'，建议在生产环境中配置具体的来源");
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
            tracing::warn!("CORS 配置无效，允许所有来源作为回退");
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
    let geo_restriction_repo: Arc<dyn GeoRestrictionRepository> =
        Arc::new(GeoRestrictionRepositoryComponent::new(state.db_pool.clone()));

    // Create concrete DatabaseGeoRestrictionRepository for handlers that need the concrete type
    let geo_restriction_repo_impl: Arc<DatabaseGeoRestrictionRepository> =
        Arc::new(DatabaseGeoRestrictionRepository::new(state.db_pool.clone()));

    // Create concrete WebhookRepoImpl for handlers that need the concrete type
    let webhook_repo_impl: Arc<WebhookRepoImpl> = Arc::new(WebhookRepoImpl::new(state.db_pool.clone()));

    // Create Arc<AppState> for crawl handlers that use unified state
    let app_state_arc = Arc::new(state.clone());

    // Auth state for middleware - wrap in Arc and set global state
    let auth_scope_service = state.auth_scope_service.as_ref().map(|arc| (**arc).clone());
    let auth_state = Arc::new(AuthState::new_for_middleware(state.db_pool.clone(), auth_scope_service));
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
        .layer(Extension(app_state_arc)) // Unified AppState for crawl handlers
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
        .layer(axum::middleware::from_fn(
            team_semaphore_middleware,
        ))
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
    let geo_restriction_repo: Arc<dyn GeoRestrictionRepository> =
        Arc::new(GeoRestrictionRepositoryComponent::new(state.db_pool.clone()));
    let credits_repo = state.credits_repo();
    let crawl_repo = state.crawl_repo.clone();
    let webhook_event_repo = state.webhook_event_repo();
    let webhook_repo = state.webhook_repo();

    // 创建 CORS 层
    let cors_layer = create_cors_layer(&settings);

    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .merge(v2_routes)
        .layer(cors_layer)
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
        .layer(Extension(state.redis_client()))
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
