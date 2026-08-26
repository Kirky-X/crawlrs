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
