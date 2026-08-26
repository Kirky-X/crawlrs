// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Management 域路由的 sdforge 直注注册（audit / admin / teams / webhook / extract）。
//!
//! 全部走 `RouteRegistration` 直注（design D1′）：带 Query DTO 或 JSON body 的
//! 端点保持原提取器原样组装；同路径多方法资源以单条 MethodRouter 链式组合，
//! 规避上游 build() 按路径去重的覆盖丢失问题。feature 门控与迁移前的
//! register_*_routes 完全对应。

use inventory::submit;
use sdforge::prelude::{ApiMetadata, HttpRoute, RouteRegistration};

use crate::presentation::forge_api::route_metadata;
use crate::presentation::handlers::audit_handler;

fn audit_logs_route() -> HttpRoute {
    HttpRoute::new(
        "/v1/audit/logs".to_string(),
        axum::routing::get(audit_handler::get_audit_logs),
        route_metadata("audit_logs", "v1", "Query audit logs"),
        None,
    )
}

fn audit_logs_metadata() -> ApiMetadata {
    route_metadata("audit_logs", "v1", "Query audit logs")
}

fn audit_denied_route() -> HttpRoute {
    HttpRoute::new(
        "/v1/audit/denied".to_string(),
        axum::routing::get(audit_handler::get_denied_requests),
        route_metadata("audit_denied", "v1", "Query denied requests"),
        None,
    )
}

fn audit_denied_metadata() -> ApiMetadata {
    route_metadata("audit_denied", "v1", "Query denied requests")
}

submit!(RouteRegistration::new(
    "audit_logs",
    "v1",
    audit_logs_route,
    audit_logs_metadata,
));

submit!(RouteRegistration::new(
    "audit_denied",
    "v1",
    audit_denied_route,
    audit_denied_metadata,
));

/// R-key-lifecycle-001：admin api-keys（auth feature 门控，整段随 feature 编译）。
#[cfg(feature = "auth")]
mod admin {
    use super::{route_metadata, HttpRoute, RouteRegistration};
    use axum::routing::post;
    use inventory::submit;

    use crate::presentation::handlers::api_key_handler;

    fn admin_api_keys_route() -> HttpRoute {
        HttpRoute::new(
            "/v1/admin/api-keys".to_string(),
            post(api_key_handler::create_api_key),
            route_metadata("admin_api_keys", "v1", "Create an admin API key"),
            None,
        )
    }

    fn admin_api_keys_metadata() -> super::ApiMetadata {
        route_metadata("admin_api_keys", "v1", "Create an admin API key")
    }

    submit!(RouteRegistration::new(
        "admin_api_keys",
        "v1",
        admin_api_keys_route,
        admin_api_keys_metadata,
    ));
}

/// R-teams-002：teams 路由（teams feature 门控）。
#[cfg(feature = "teams")]
mod teams {
    use axum::{Extension, response::Response};
    use sdforge::prelude::*;
    use std::sync::Arc;

    use crate::domain::repositories::credits_repository::CreditsRepository;
    use crate::domain::repositories::scrape_result_repository::ScrapeResultRepository;
    use crate::domain::repositories::task_repository::TaskRepository;
    use crate::infrastructure::database::repositories::database_geo_restriction_repo::DatabaseGeoRestrictionRepository;
    use crate::presentation::handlers::team_handler;
    use crate::presentation::middleware::auth_middleware::AuthState;
    use super::{route_metadata, ApiMetadata, HttpRoute, RouteRegistration};
    use inventory::submit;

    #[forge(
        name = "team_info",
        version = "v1",
        path = "/v1/teams/me",
        method = "GET",
        no_prefix = true,
        stream = true,
        description = "Get current team info"
    )]
    async fn team_info_route(
        #[state] credits_repo: Arc<dyn CreditsRepository>,
        #[state] task_repo: Arc<dyn TaskRepository>,
        #[state] auth_state: AuthState,
    ) -> Result<Response, Response> {
        Ok(
            team_handler::get_team_info(
                Extension(credits_repo),
                Extension(task_repo),
                Extension(auth_state),
            )
            .await
            .into_response(),
        )
    }

    #[forge(
        name = "team_usage",
        version = "v1",
        path = "/v1/teams/me/usage",
        method = "GET",
        no_prefix = true,
        stream = true,
        description = "Get current team usage"
    )]
    async fn team_usage_route(
        #[state] credits_repo: Arc<dyn CreditsRepository>,
        #[state] scrape_result_repo: Arc<dyn ScrapeResultRepository>,
        #[state] auth_state: AuthState,
    ) -> Result<Response, Response> {
        Ok(
            team_handler::get_team_usage(
                Extension(credits_repo),
                Extension(scrape_result_repo),
                Extension(auth_state),
            )
            .await
            .into_response(),
        )
    }

    // GET+PUT 同路径多方法：单条 RouteRegistration 直注，规避 build() 按路径去重的覆盖丢失
    fn team_geo_restrictions_route() -> HttpRoute {
        let method_router =
            axum::routing::get(team_handler::get_team_geo_restrictions::<DatabaseGeoRestrictionRepository>)
                .put(team_handler::update_team_geo_restrictions::<DatabaseGeoRestrictionRepository>);
        HttpRoute::new(
            "/v1/teams/geo-restrictions".to_string(),
            method_router,
            route_metadata("team_geo_restrictions", "v1", "Team geo restrictions"),
            None,
        )
    }

    fn team_geo_restrictions_metadata() -> ApiMetadata {
        route_metadata("team_geo_restrictions", "v1", "Team geo restrictions")
    }

    submit!(RouteRegistration::new(
        "team_geo_restrictions",
        "v1",
        team_geo_restrictions_route,
        team_geo_restrictions_metadata,
    ));
}

/// R-wh-001：webhook 路由（webhook feature 门控）。
#[cfg(feature = "webhook")]
mod webhook {
    use super::{route_metadata, ApiMetadata, HttpRoute, RouteRegistration};
    use crate::infrastructure::database::repositories::webhook_repo_impl::WebhookRepoImpl;
    use crate::presentation::handlers::webhook_handler;
    use inventory::submit;

    // POST+GET 同路径多方法：单条直注，规避 build() 按路径去重的覆盖丢失
    fn webhooks_route() -> HttpRoute {
        let method_router =
            axum::routing::post(webhook_handler::create_webhook::<WebhookRepoImpl>)
                .get(webhook_handler::list_webhooks::<WebhookRepoImpl>);
        HttpRoute::new(
            "/v1/webhooks".to_string(),
            method_router,
            route_metadata("webhooks", "v1", "Create and list webhooks"),
            None,
        )
    }

    fn webhooks_metadata() -> ApiMetadata {
        route_metadata("webhooks", "v1", "Create and list webhooks")
    }

    submit!(RouteRegistration::new(
        "webhooks",
        "v1",
        webhooks_route,
        webhooks_metadata,
    ));
}
