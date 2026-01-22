// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Route configuration and application builder.

use crate::config::settings::Settings;
use crate::di::infrastructure_module::{
    GeoRestrictionRepositoryComponent,
};
use crate::di::AppState;
use crate::di::AppStateExt;
use crate::domain::auth::ApiKeyScope;
use crate::domain::repositories::geo_restriction_repository::GeoRestrictionRepository;
use crate::infrastructure::database::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
use crate::infrastructure::database::repositories::webhook_repo_impl::WebhookRepoImpl;
use crate::presentation::handlers::{
    audit_handler, crawl_handler, extract_handler, metrics_handler, scrape_handler, search_handler,
    team_handler, webhook_handler,
};
use crate::presentation::middleware::auth_middleware::AuthState;
use crate::presentation::middleware::team_semaphore_middleware::team_semaphore_middleware;
use crate::presentation::routes;
use crate::presentation::routes::task::task_routes;
use axum::{
    routing::{delete, get, post, put},
    Extension, Router,
};
use std::sync::Arc;

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
    let redis_client = state.redis_client.clone();
    let rate_limiter = crate::presentation::middleware::rate_limit_middleware::RateLimiter::new(
        (*redis_client).clone(),
        settings.rate_limiting.default_rpm,
    );
    let rate_limiting_service = state.rate_limiting_service.clone();
    let crawl_repo = state.crawl_repo.clone();
    let webhook_repo = state.webhook_repo.clone();
    let tasks_backlog_repo = state.webhook_event_repo(); // Use webhook_event_repo for now
    let search_engine_service = state.search_client();
    let team_service = state.team_service.clone();
    let geo_location_service = state.redis_client(); // Placeholder

    // Create geo restriction repository for extension (使用 DI 组件)
    let geo_restriction_repo: Arc<dyn GeoRestrictionRepository> =
        Arc::new(GeoRestrictionRepositoryComponent::new(state.db.clone()));

    // Auth state for middleware
    let auth_state = AuthState {
        db: state.db.clone(),
        auth_scope_service: None,
        team_id: uuid::Uuid::nil(),
        api_key_id: uuid::Uuid::nil(),
        scope: ApiKeyScope::default(),
        api_key_cache: None,
        auth_rate_limiter: None,
    };

    Router::new()
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
            "/v1/crawl",
            post(crawl_handler::create_crawl),
        )
        .route(
            "/v1/crawl/{id}",
            get(crawl_handler::get_crawl),
        )
        .route(
            "/v1/crawl/{id}/results",
            get(crawl_handler::get_crawl_results),
        )
        .route(
            "/v1/crawl/{id}",
            delete(crawl_handler::cancel_crawl),
        )
        .route("/v1/search", post(search_handler::search))
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
        .layer(axum::middleware::from_fn_with_state(
            auth_state.clone(),
            crate::presentation::middleware::auth_middleware::auth_middleware,
        ))
        .layer(Extension(geo_restriction_repo))
        .layer(Extension(team_semaphore))
        .layer(Extension(queue))
        .layer(Extension(task_repo))
        .layer(Extension(result_repo))
        .layer(Extension(redis_client))
        .layer(Extension(rate_limiter))
        .layer(Extension(settings))
        .layer(Extension(rate_limiting_service))
        .layer(Extension(crawl_repo))
        .layer(Extension(webhook_repo))
        .layer(Extension(tasks_backlog_repo))
        .layer(Extension(search_engine_service))
        .layer(Extension(state.search_service.clone()))
        .layer(Extension(team_service))
        .layer(Extension(geo_location_service))
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

    let auth_state = AuthState {
        db: state.db.clone(),
        auth_scope_service: None,
        team_id: uuid::Uuid::nil(),
        api_key_id: uuid::Uuid::nil(),
        scope: ApiKeyScope::default(),
        api_key_cache: None,
        auth_rate_limiter: None,
    };

    task_routes()
        .layer(Extension(task_repo.clone()))
        .layer(Extension(result_repo.clone()))
        .layer(axum::middleware::from_fn_with_state(
            auth_state.clone(),
            crate::presentation::middleware::auth_middleware::auth_middleware,
        ))
        .layer(axum::middleware::from_fn_with_state(
            team_semaphore.clone(),
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

    let redis_client = state.redis_client.clone();
    let rate_limiter = crate::presentation::middleware::rate_limit_middleware::RateLimiter::new(
        (*redis_client).clone(),
        settings.rate_limiting.default_rpm,
    );
    let rate_limiting_service = state.rate_limiting_service.clone();
    let search_engine_service = state.search_client();
    let tasks_backlog_repo = state.webhook_event_repo();
    let queue = state.task_queue.clone();
    let geo_restriction_repo: Arc<dyn GeoRestrictionRepository> =
        Arc::new(GeoRestrictionRepositoryComponent::new(state.db.clone()));
    let credits_repo = state.credits_repo();
    let crawl_repo = state.crawl_repo.clone();
    let webhook_event_repo = state.webhook_event_repo();
    let webhook_repo = state.webhook_repo();

    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .merge(v2_routes)
        .layer(axum::extract::DefaultBodyLimit::max(10 * 1024 * 1024)) // 10MB limit
        .layer(Extension(state.team_semaphore.clone()))
        .layer(Extension(queue))
        .layer(Extension(state.task_repo.clone()))
        .layer(Extension(state.result_repo.clone()))
        .layer(Extension(crawl_repo))
        .layer(Extension(webhook_event_repo))
        .layer(Extension(webhook_repo.clone()))
        .layer(Extension(redis_client))
        .layer(Extension(rate_limiter))
        .layer(Extension(state.crawl_repo.clone()))
        .layer(Extension(credits_repo))
        .layer(Extension(geo_restriction_repo))
        .layer(Extension(settings))
        .layer(Extension(search_engine_service))
        .layer(Extension(tasks_backlog_repo.clone()))
        .layer(Extension(rate_limiting_service.clone()))
        .layer(Extension(state.audit_service()))
}
