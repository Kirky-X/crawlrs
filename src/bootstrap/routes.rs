// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Route configuration and application builder.

use crate::bootstrap::infrastructure::InfrastructureComponents;
use crate::bootstrap::services::ServicesComponents;
use crate::config::settings::Settings;
use crate::domain::auth::ApiKeyScope;
use crate::infrastructure::repositories::{
    crawl_repo_impl::CrawlRepositoryImpl, credits_repo_impl::CreditsRepositoryImpl,
    database_geo_restriction_repo::DatabaseGeoRestrictionRepository,
    scrape_result_repo_impl::ScrapeResultRepositoryImpl, task_repo_impl::TaskRepositoryImpl,
    webhook_repo_impl::WebhookRepoImpl,
};
use crate::presentation::handlers::{
    audit_handler, crawl_handler, extract_handler, metrics_handler, scrape_handler, search_handler,
    team_handler, webhook_handler,
};
use crate::presentation::middleware::auth_middleware::AuthState;
use crate::presentation::middleware::team_semaphore_middleware::team_semaphore_middleware;
use crate::presentation::routes::{self, task::task_routes};
use axum::{
    routing::{delete, get, post, put},
    Extension, Router,
};
use std::sync::Arc;

/// Create the public API routes (no authentication required).
pub fn create_public_routes() -> Router {
    Router::new()
        .route("/health", get(routes::health_check))
        .route("/metrics", get(metrics_handler::metrics))
        .route("/v1/version", get(routes::version))
}

/// Create the protected API routes (authentication required).
///
/// # Arguments
///
/// * `repositories` - Application repositories
/// * `services` - Application services
/// * `settings` - Application settings (Arc wrapped)
pub fn create_protected_routes(
    repositories: &crate::bootstrap::infrastructure::Repositories,
    services: &ServicesComponents,
    settings: Arc<Settings>,
) -> Router {
    let geo_restriction_repo = repositories.geo_restriction_repo.clone();
    let team_semaphore = services.team_semaphore.clone();
    let queue = services.queue.clone();
    let task_repo = repositories.task_repo.clone();
    let result_repo = repositories.result_repo.clone();
    let redis_client = services.rate_limiter.redis_client_clone();
    let rate_limiter = services.rate_limiter.clone();
    let rate_limiting_service = services.rate_limiting_service.clone();
    let crawl_repo = repositories.crawl_repo.clone();
    let webhook_repo = repositories.webhook_repo.clone();
    let tasks_backlog_repo = repositories.tasks_backlog_repo.clone();
    let search_engine_service = services.search_engine_service.clone();
    let team_service = services.team_service.clone();

    // Auth state for middleware
    let auth_state = AuthState {
        db: repositories.task_repo.db_clone(),
        auth_scope_service: None,
        team_id: uuid::Uuid::nil(),
        api_key_id: uuid::Uuid::nil(),
        scope: ApiKeyScope::default(),
        api_key_cache: None,
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
            post(
                crawl_handler::create_crawl::<
                    CrawlRepositoryImpl,
                    TaskRepositoryImpl,
                    WebhookRepoImpl,
                    ScrapeResultRepositoryImpl,
                    DatabaseGeoRestrictionRepository,
                >,
            ),
        )
        .route(
            "/v1/crawl/{id}",
            get(crawl_handler::get_crawl::<
                CrawlRepositoryImpl,
                TaskRepositoryImpl,
                WebhookRepoImpl,
                ScrapeResultRepositoryImpl,
                DatabaseGeoRestrictionRepository,
            >),
        )
        .route(
            "/v1/crawl/{id}/results",
            get(crawl_handler::get_crawl_results::<
                CrawlRepositoryImpl,
                TaskRepositoryImpl,
                WebhookRepoImpl,
                ScrapeResultRepositoryImpl,
                DatabaseGeoRestrictionRepository,
            >),
        )
        .route(
            "/v1/crawl/{id}",
            delete(
                crawl_handler::cancel_crawl::<
                    CrawlRepositoryImpl,
                    TaskRepositoryImpl,
                    WebhookRepoImpl,
                    ScrapeResultRepositoryImpl,
                    DatabaseGeoRestrictionRepository,
                >,
            ),
        )
        .route(
            "/v1/search",
            post(
                search_handler::search::<
                    CrawlRepositoryImpl,
                    TaskRepositoryImpl,
                    CreditsRepositoryImpl,
                >,
            ),
        )
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
        .layer(Extension(team_service))
}

/// Create v2 task routes.
///
/// # Arguments
///
/// * `repositories` - Application repositories
/// * `services` - Application services
pub fn create_v2_routes(
    repositories: &crate::bootstrap::infrastructure::Repositories,
    services: &ServicesComponents,
) -> Router {
    let task_repo = repositories.task_repo.clone();
    let result_repo = repositories.result_repo.clone();
    let crawl_repo = repositories.crawl_repo.clone();
    let webhook_repo = repositories.webhook_repo.clone();
    let webhook_event_repo = repositories.webhook_event_repo.clone();
    let team_semaphore = services.team_semaphore.clone();

    let auth_state = AuthState {
        db: task_repo.db_clone(),
        auth_scope_service: None,
        team_id: uuid::Uuid::nil(),
        api_key_id: uuid::Uuid::nil(),
        scope: ApiKeyScope::default(),
        api_key_cache: None,
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

/// Build the complete API application router.
///
/// # Arguments
///
/// * `infrastructure` - Initialized infrastructure components
/// * `services` - Initialized services
/// * `settings` - Application settings (Arc wrapped)
///
/// # Returns
///
/// Returns the configured API router.
pub fn build_api_app(
    infrastructure: &InfrastructureComponents,
    services: &ServicesComponents,
    settings: Arc<Settings>,
) -> Router {
    let public_routes = create_public_routes();
    let protected_routes =
        create_protected_routes(&infrastructure.repositories, services, settings.clone());
    let v2_routes = create_v2_routes(&infrastructure.repositories, services);

    let redis_client = infrastructure.redis_client.clone();
    let rate_limiter = services.rate_limiter.clone();
    let rate_limiting_service = services.rate_limiting_service.clone();
    let search_engine_service = services.search_engine_service.clone();
    let tasks_backlog_repo = infrastructure.repositories.tasks_backlog_repo.clone();
    let queue = services.queue.clone();
    let geo_restriction_repo = infrastructure.repositories.geo_restriction_repo.clone();
    let credits_repo = infrastructure.repositories.credits_repo.clone();
    let crawl_repo = infrastructure.repositories.crawl_repo.clone();
    let webhook_event_repo = infrastructure.repositories.webhook_event_repo.clone();
    let webhook_repo = infrastructure.repositories.webhook_repo.clone();

    Router::new()
        .merge(public_routes)
        .merge(protected_routes)
        .merge(v2_routes)
        .layer(Extension(services.team_semaphore.clone()))
        .layer(Extension(queue))
        .layer(Extension(infrastructure.repositories.task_repo.clone()))
        .layer(Extension(infrastructure.repositories.result_repo.clone()))
        .layer(Extension(crawl_repo))
        .layer(Extension(webhook_event_repo))
        .layer(Extension(webhook_repo.clone()))
        .layer(Extension(redis_client))
        .layer(Extension(rate_limiter))
        .layer(Extension(infrastructure.repositories.crawl_repo.clone()))
        .layer(Extension(credits_repo))
        .layer(Extension(geo_restriction_repo))
        .layer(Extension(settings))
        .layer(Extension(search_engine_service))
        .layer(Extension(tasks_backlog_repo.clone()))
        .layer(Extension(rate_limiting_service.clone()))
        .layer(Extension(services.audit_service.clone()))
}
