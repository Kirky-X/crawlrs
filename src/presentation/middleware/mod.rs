// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// Garrison Principal ↔ crawlrs AuthState 桥接
///
/// auth feature 启用时编译，
/// 提供 `map_perms_to_scope`、`bridge_principal_to_auth_state`、`extract_bearer` 等。
/// auth-off 时 garrison 不编译，走 feature-gate 的 `default_identity_middleware` 路径。
#[cfg(feature = "auth")]
pub mod auth_bridge;
/// 中间件模块
///
/// 提供HTTP请求处理的中间件功能
/// 包括认证、限流、信号量控制等功能
pub mod auth_middleware;
/// 认证共享类型（重构 / 决策 3）
///
/// 抽出 `AuthError` 与 `AuthState` 解决 `auth_bridge` ↔ `auth_middleware` 之间的循环依赖。
/// 两个模块都从此处导入共享类型，避免互相 `use` 形成循环。
pub mod auth_types;
// 历史说明：`limiteron_rate_limit_middleware` 与 `distributed_rate_limit_middleware`
// 曾为并行的限流方案，均无路由挂载点（零调用方），已移除。
// 现行唯一限流中间件为 `rate_limit_middleware`（经 RateLimitingService trait 走 limiteron）；
// 如需 tower 原生层语义（429/403 + RateLimit 响应头），可评估吸收 limiteron `tower-middleware`。
pub mod rate_limit_middleware;
pub mod security_headers_middleware;
pub mod team_semaphore_middleware;

/// Public endpoints that don't require authentication or rate limiting
pub const PUBLIC_ENDPOINTS: &[&str] = &["/health", "/ready", "/metrics", "/v1/version"];

/// Endpoints excluded from rate limiting
pub const RATE_LIMIT_EXCLUDED_ENDPOINTS: &[&str] = &[
    "/health",
    "/ready",
    "/metrics",
    "/v1/version",
    "/v1/extract",
    "/v1/crawl",
    "/v1/scrape",
];
