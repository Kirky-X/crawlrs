// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! OpenAPI 3.0 documentation for the crawlrs API.
//!
//! This module provides OpenAPI specification generation using utoipa.
//! Access Swagger UI at `/api-docs` when the server is running.

use axum::{routing::get, Router};
use utoipa::OpenApi;

use crate::presentation::handlers::{
    audit_handler, crawl_handler, extract_handler, health_handler, scrape_handler, search_handler,
    team_handler, webhook_handler,
};

/// OpenAPI specification for the crawlrs API.
///
/// This struct is automatically generated from the annotated handlers.
/// To regenerate, run with `OPENAPI_GEN=1` environment variable.
#[derive(OpenApi)]
#[openapi(
    // API Information
    info(
        title = "Crawlrs API",
        description = r#"
# Enterprise Web Scraping Platform

Crawlrs provides comprehensive web data collection capabilities including:
- **Search** - Unified search across multiple search engines
- **Scrape** - Extract data from single web pages
- **Crawl** - Automatically discover and scrape multiple pages
- **Extract** - Parse and structure data from HTML

## Authentication

All protected endpoints require an API key to be included in the request header:
- `X-API-Key: your-api-key`

## Rate Limiting

API requests are rate-limited based on your plan. Check the `X-RateLimit-Remaining` header for remaining requests.

## Errors

All errors follow the [Problem Details for HTTP APIs](https://datatracker.ietf.org/doc/html/rfc7807) format.
        "#,
        contact(
            name = "Kirky.X",
            email = "support@crawlrs.io",
            url = "https://github.com/kirky-x/crawlrs"
        ),
        license(
            name = "Apache License 2.0",
            url = "https://www.apache.org/licenses/LICENSE-2.0"
        ),
        version = "0.1.0"
    ),
    // Servers
    servers(
        (url = "http://localhost:8899", description = "Local development server"),
        (url = "https://api.crawlrs.io", description = "Production server")
    ),
    // Paths/Tags
    tags(
        (name = "Health", description = "Health check and monitoring endpoints"),
        (name = "Scrape", description = "Single page scraping endpoints"),
        (name = "Crawl", description = "Web crawling endpoints"),
        (name = "Extract", description = "Data extraction endpoints"),
        (name = "Search", description = "Search engine integration endpoints"),
        (name = "Webhooks", description = "Webhook management endpoints"),
        (name = "Teams", description = "Team management endpoints"),
        (name = "Audit", description = "Audit log endpoints")
    ),
    // Paths
    paths(
        // Health
        health_handler::health_check,
        // Scrape
        scrape_handler::create_scrape,
        scrape_handler::get_scrape_status,
        scrape_handler::cancel_scrape,
        // Crawl
        crawl_handler::create_crawl::<
            crate::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl,
            crate::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl,
            crate::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl,
            crate::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl,
            crate::infrastructure::database::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository,
        >,
        crawl_handler::get_crawl::<
            crate::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl,
            crate::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl,
            crate::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl,
            crate::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl,
            crate::infrastructure::database::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository,
        >,
        crawl_handler::get_crawl_results::<
            crate::infrastructure::repositories::crawl_repo_impl::CrawlRepositoryImpl,
            crate::infrastructure::repositories::task_repo_impl::TaskRepositoryImpl,
            crate::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl,
            crate::infrastructure::repositories::scrape_result_repo_impl::ScrapeResultRepositoryImpl,
            crate::infrastructure::database::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository,
        >,
        // Extract
        extract_handler::extract::<crate::infrastructure::database::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository>,
        // Search
        search_handler::search,
        // Webhooks
        webhook_handler::create_webhook::<crate::infrastructure::repositories::webhook_repo_impl::WebhookRepoImpl>,
        // Teams
        team_handler::get_team_geo_restrictions::<crate::infrastructure::database::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository>,
        team_handler::update_team_geo_restrictions::<crate::infrastructure::database::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository>,
        // Audit
        audit_handler::get_audit_logs,
        audit_handler::get_denied_requests,
    ),
    // Components
    components(
        schemas(
            // DTOs
            crate::application::dto::scrape_request::ScrapeRequestDto,
            crate::application::dto::scrape_response::ScrapeResponseDto,
            crate::application::dto::crawl_request::CrawlRequestDto,
            crate::application::dto::crawl_response::CrawlResponseDto,
            crate::application::dto::extract_request::ExtractRequestDto,
            crate::application::dto::extract_response::ExtractResponseDto,
            crate::application::dto::search_request::SearchRequestDto,
            crate::application::dto::search_response::SearchResultDto,
            crate::application::dto::webhook_request::CreateWebhookRequest,
            crate::application::dto::webhook_response::WebhookResponse,
            crate::application::dto::geo_restriction_request::GeoRestrictionRequest,
            crate::application::dto::geo_restriction_response::GeoRestrictionResponse,
            // Response types
            crate::presentation::handlers::response_builder::ApiResponse,
            crate::presentation::handlers::response_builder::ApiError,
            crate::presentation::handlers::response_builder::PaginationMeta,
            crate::presentation::handlers::response_builder::error_codes::ErrorCode,
            // Error types
            crate::common::error::AppError,
            // Domain models
            crate::domain::models::TaskStatus,
            // Common
            crate::common::constants::crawl_task::TaskType,
        )
    )
)]
pub struct ApiDoc;

/// Get the OpenAPI specification as JSON.
///
/// This endpoint can be used to retrieve the OpenAPI specification
/// in JSON format for integration with external tools.
#[utoipa::path(
    get,
    path = "/api-docs/openapi.json",
    tag = "Health",
    responses(
        (status = 200, description = "OpenAPI specification in JSON format", content = (application/json = String))
    )
)]
pub async fn openapi_json() -> String {
    serde_json::to_string_pretty(&ApiDoc::openapi()).unwrap_or_else(|_| "{}".to_string())
}

/// Create the Swagger UI router.
///
/// This function creates a router that serves the Swagger UI
/// at the specified base path.
pub fn swagger_ui_routes() -> Router {
    Router::new()
        .route("/api-docs", get(swagger_ui_redirect))
        .route("/api-docs/", get(swagger_ui_redirect))
        .route("/api-docs/swagger-ui.css", get(|| async { "/* CSS */" }))
        .route("/api-docs/swagger-ui.js", get(|| async { "/* JS */" }))
}

/// Redirect to Swagger UI index
async fn swagger_ui_redirect() -> &'static str {
    "Redirect to /api-docs/swagger-ui/index.html"
}
