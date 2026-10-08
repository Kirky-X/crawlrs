// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 平台业务路由的 sdforge 注册层。
//!
//! 全部业务端点经 sdforge inventory 在编译期注册，运行时由 [`build_forge_router`]
//! 一次性收集构建 Axum Router。两种注册机制（design D1′ / D2）：
//!
//! 1. **`RouteRegistration` 直注**（本变更主力）：提交 const 工厂函数指针，
//!    运行时构造「既有 handler 原样组装」的 HttpRoute——URL/方法/提取器与迁移前
//!    的手写 `.route(..)` 逐字节等价。用于带 JSON body 的端点与同路径多方法资源。
//!    （不用裸 `HttpRoute` 直注：其字段需 const 表达式，而 `post(handler)` /
//!    `String` 构造非常量。）
//! 2. **`#[forge]` 宏**：仅用于无 body 的简单端点。上游 0.5.0-rc.1 的 stream
//!    分支对 Body 参数「提取不包裹、调用却解包」自相矛盾，带 JSON body 的端点
//!    无法经宏声明，故 body 类端点一律走直注。
//!
//! # 单一调用点约束
//!
//! inventory 是进程级全局收集：`sdforge::http::build()` 在同一二进制中只能有
//! 一个生产调用点（本模块）。SDK 路由（`presentation::sdk`）与本层路由共用同
//! 一次 build —— 两者路径空间不相交（/api/v1/sdk/* vs /v1/*）。

pub mod crawl;
pub mod management;
pub mod map;
pub mod scrape;
pub mod search;
pub mod task;

use axum::Router;

/// Collect all inventory-registered routes (SDK + platform) into one Router.
pub fn build_forge_router() -> Router {
    sdforge::http::build()
}

/// 构造直注条目的路由元数据（无缓存、非流式）。
pub(crate) fn route_metadata(
    name: &str,
    version: &str,
    description: &str,
) -> sdforge::prelude::ApiMetadata {
    sdforge::prelude::ApiMetadata::new(
        name.to_string(),
        version.to_string(),
        description.to_string(),
        None,
        false,
    )
}

#[cfg(test)]
mod tests {
    use super::build_forge_router;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    /// forge inventory 当前声明的平台路径（不含 SDK——SDK 经 `#[service_api]`
    /// 独立提交，同一次 build 收集）。增删 forge 条目时必须同步更新此清单。
    const EXPECTED_FORGE_PATHS: &[(&str, &str)] = &[
        ("POST", "/v1/scrape"),
        ("GET", "/v1/scrape/{id}"),
        ("POST", "/v1/crawl"),
        ("GET", "/v1/crawl/{id}"),
        ("GET", "/v1/crawl/{id}/results"),
        ("POST", "/v1/search"),
        ("GET", "/v1/map"),
        ("POST", "/v1/extract"),
        ("GET", "/v1/teams/me"),
        ("GET", "/v1/teams/me/usage"),
        ("PUT", "/v1/teams/geo-restrictions"),
        ("POST", "/v1/webhooks"),
        ("GET", "/v1/webhooks"),
        ("POST", "/v1/tasks/_cancel"),
        ("POST", "/v1/tasks/_query"),
        ("GET", "/v1/audit/logs"),
        ("GET", "/v1/audit/denied"),
        ("POST", "/v1/admin/api-keys"),
    ];

    /// 生产装配（bootstrap/route_groups + management）的对应路径——forge 层
    /// 声明的每条路径都必须真实存在于生产装配，防止迁移期间的注册面漂移。
    /// 大括号占位差异（sdforge `/v1/scrape/{id}` vs axum 同形）按字面对齐。
    const PRODUCTION_PATHS: &[&str] = &[
        "/v1/scrape",
        "/v1/scrape/{id}",
        "/v1/scrape/{id}/_cancel",
        "/v1/crawl",
        "/v1/crawl/{id}",
        "/v1/crawl/{id}/_cancel",
        "/v1/crawl/{id}/results",
        "/v1/search",
        "/v1/map",
        "/v1/extract",
        "/v1/teams/me",
        "/v1/teams/me/usage",
        "/v1/teams/geo-restrictions",
        "/v1/webhooks",
        "/v1/tasks/_cancel",
        "/v1/tasks/_query",
        "/v1/audit/logs",
        "/v1/audit/denied",
        "/v1/admin/api-keys",
    ];

    /// 迁移契约（sdforge 单一注册面）：forge inventory 的每条 (method, path)
    /// 都必须能在路由集合中探活（非 404）。探针法不依赖 State/Extension——
    /// 缺依赖的处理器返回 500，仍能证明「已注册」。
    #[tokio::test]
    async fn forge_inventory_routes_stay_aligned_with_production() {
        let router = build_forge_router();
        for (method, path) in EXPECTED_FORGE_PATHS {
            let response = router
                .clone()
                .oneshot(
                    Request::builder()
                        .method(*method)
                        .uri(*path)
                        .body(axum::body::Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_ne!(
                response.status(),
                StatusCode::NOT_FOUND,
                "forge inventory 声明了 {method} {path} 但 build_forge_router 探活 404——条目失效或方法/路径漂移"
            );
        }
    }

    /// forge 声明的路径必须是生产装配路径的子集（路径空间不漂移）。
    #[test]
    fn forge_paths_are_subset_of_production_paths() {
        for (method, path) in EXPECTED_FORGE_PATHS {
            assert!(
                PRODUCTION_PATHS.contains(path),
                "forge inventory 声明的 {method} {path} 不在生产装配路径集合中——注册面漂移"
            );
        }
    }

    /// 已知未迁入 forge 的路径（本次锚定时的缺口，等价性补全的待办清单）：
    /// 公共端点（health/metrics/version）与两个 `_cancel` 端点。
    /// 有人往 forge 补齐这些条目时，须把它们移入 EXPECTED_FORGE_PATHS。
    #[test]
    fn known_migration_gaps_are_pinned() {
        for path in [
            "/health",
            "/metrics",
            "/v1/version",
            "/v1/scrape/{id}/_cancel",
            "/v1/crawl/{id}/_cancel",
        ] {
            assert!(
                !EXPECTED_FORGE_PATHS
                    .iter()
                    .any(|(_, p)| *p == path),
                "{path} 已迁入 forge——请把它从 known_migration_gaps 移入 EXPECTED_FORGE_PATHS 并更新迁移状态"
            );
        }
    }
}
