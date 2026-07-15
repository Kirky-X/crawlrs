// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! SDK 自定义中间件示例
//!
//! 演示如何为 sdforge 路由添加自定义中间件，包括：
//! - 认证中间件（API Key 验证）
//! - 请求日志中间件
//! - 错误处理中间件
//! - 通过 axum Extension 注入认证状态
//!
//! sdforge 生成的路由是标准的 axum::Router，可叠加任意 axum/tower 中间件。
//! crawlrs 主项目的认证中间件位于 src/presentation/middleware/auth_middleware.rs。
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example custom_middleware
//! ```

use axum::body::Body;
use axum::http::{Request, Response, StatusCode};
use axum::middleware::Next;
use log::info;
use sdforge::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

// ============================================================================
// 认证状态 — 通过 Extension 注入到 handler
// ============================================================================

/// 认证状态（简化版，实际项目包含更多字段）
///
/// 在 crawlrs 主项目中，AuthState 通过 auth_middleware 设置到 Extension 中，
/// handler 通过 `Extension<AuthState>` 提取使用。
#[derive(Debug, Clone)]
struct AuthState {
    /// 团队 ID
    team_id: Uuid,
    /// API Key ID
    api_key_id: Uuid,
    /// 角色列表
    roles: Vec<String>,
}

// ============================================================================
// DTO — 端点请求/响应类型
// ============================================================================

#[derive(Debug, Deserialize)]
struct CreateTaskRequest {
    url: String,
}

#[derive(Debug, Serialize)]
struct CreateTaskResponse {
    id: Uuid,
    team_id: Uuid,
    url: String,
}

// ============================================================================
// 端点定义 — 通过 #[state] 注入 AuthState
// ============================================================================

/// 创建任务端点
///
/// `#[state] auth_state: AuthState` 从 axum Extension 中提取认证状态。
/// 这与 crawlrs 主项目 src/presentation/sdk/mod.rs 中的模式一致。
#[forge(
    name = "sdk_create_task_auth",
    version = "v1",
    path = "/sdk/auth/tasks",
    method = "POST",
    description = "Create task with authentication"
)]
async fn create_task_with_auth(
    #[state] auth_state: AuthState,
    req: CreateTaskRequest,
) -> Result<CreateTaskResponse, ApiError> {
    if req.url.trim().is_empty() {
        return Err(ApiError::InvalidInput {
            message: "url cannot be empty".to_string(),
            field: Some("url".to_string()),
            value: None,
        });
    }

    // 检查角色权限
    if !auth_state
        .roles
        .iter()
        .any(|r| r == "writer" || r == "admin")
    {
        return Err(ApiError::AccessDenied {
            permission: "task:create".to_string(),
            user_id: Some(auth_state.api_key_id.to_string()),
        });
    }

    Ok(CreateTaskResponse {
        id: Uuid::new_v4(),
        team_id: auth_state.team_id,
        url: req.url,
    })
}

// ============================================================================
// 自定义中间件
// ============================================================================

/// API Key 认证中间件
///
/// 从 `X-API-Key` header 提取 API Key 并验证。
/// 验证通过后将 AuthState 注入到 Extension 中，供后续 handler 使用。
///
/// 这是 crawlrs 主项目 auth_middleware 的简化版本：
/// - 实际版本查询数据库验证 API Key
/// - 实际版本包含速率限制、IP 锁定等功能
/// - 实际版本支持 Bearer Token 和 API Key 两种认证方式
async fn api_key_auth_middleware(mut req: Request<Body>, next: Next) -> Response<Body> {
    let api_key = req
        .headers()
        .get("X-API-Key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string());

    let api_key = match api_key {
        Some(k) => k,
        None => {
            return Response::builder()
                .status(StatusCode::UNAUTHORIZED)
                .body(Body::from(r#"{"error":"missing X-API-Key header"}"#))
                .unwrap();
        }
    };

    // 简化验证：实际项目查询数据库
    // 此处使用硬编码值仅作演示
    if api_key != "valid-api-key-12345" {
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .body(Body::from(r#"{"error":"invalid API key"}"#))
            .unwrap();
    }

    // 构造认证状态并注入 Extension
    let auth_state = AuthState {
        team_id: Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
        api_key_id: Uuid::parse_str("00000000-0000-0000-0000-000000000002").unwrap(),
        roles: vec!["writer".to_string()],
    };

    req.extensions_mut().insert(auth_state);
    next.run(req).await
}

/// 请求日志中间件
///
/// 记录每个请求的方法、路径和处理耗时。
async fn request_log_middleware(req: Request<Body>, next: Next) -> Response<Body> {
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let start = std::time::Instant::now();

    let response = next.run(req).await;

    let elapsed = start.elapsed();
    info!(
        "{} {} → {} ({}ms)",
        method,
        path,
        response.status(),
        elapsed.as_millis()
    );

    response
}

/// 错误处理中间件
///
/// 捕获 handler 返回的错误响应，添加统一的错误格式。
async fn error_handler_middleware(req: Request<Body>, next: Next) -> Response<Body> {
    // 在把 req 移交给 next 之前，先克隆出后续日志需要的方法与路径
    let method = req.method().clone();
    let path = req.uri().path().to_string();
    let response = next.run(req).await;

    if response.status().is_server_error() {
        log::error!("Server error: {} {} → {}", method, path, response.status());
    }

    response
}

// ============================================================================
// 主函数 — 展示中间件注册方式
// ============================================================================

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始 sdforge 自定义中间件示例");
    info!("=====================================\n");

    // 1. 中间件机制说明
    info!("1️⃣  sdforge 中间件机制");
    info!("-----------------------------");
    info!("📖 sdforge 基于 axum，生成的 Router 可叠加任意 axum/tower 中间件：");
    info!("   - 中间件按添加顺序执行（先添加的先执行）");
    info!("   - 通过 Extension 在中间件与 handler 间传递状态");
    info!("   - 支持同步和异步中间件");
    info!("");

    // 2. 认证中间件
    info!("2️⃣  认证中间件（API Key）");
    info!("-----------------------------");
    info!("📖 认证中间件负责从请求中提取凭证并验证：");
    info!("   1. 从 X-API-Key header 提取 API Key");
    info!("   2. 查询数据库验证 API Key 有效性");
    info!("   3. 构造 AuthState 并注入到 Extension");
    info!("   4. handler 通过 #[state] 提取 AuthState");
    info!("");
    info!("📌 crawlrs 主项目的 AuthState 包含：");
    info!("   - team_id: 团队 ID（多租户隔离）");
    info!("   - api_key_id: API Key ID（审计追溯）");
    info!("   - pool: 数据库连接池");
    info!("   - auth_rate_limiter: 认证速率限制器");
    info!("");

    // 3. 状态注入
    info!("3️⃣  状态注入模式");
    info!("-----------------------------");
    info!("📖 通过 Extension<T> 在中间件与 handler 间传递数据：");
    info!("   1. 中间件：req.extensions_mut().insert(state)");
    info!("   2. handler：通过 #[state] 参数提取");
    info!("");
    info!("📌 示例（auth_middleware 设置，sdk handler 提取）：");
    info!("   // 中间件中注入");
    info!("   req.extensions_mut().insert(AuthState {{ ... }});");
    info!("");
    info!("   // 端点函数中提取");
    info!("   #[forge(name = \"create_task\", ...)]");
    info!("   async fn create_task(");
    info!("       #[state] auth: AuthState,");
    info!("       req: CreateTaskRequest,");
    info!("   ) -> Result<...> {{ ... }}");
    info!("");

    // 4. 中间件注册
    info!("4️⃣  中间件注册");
    info!("-----------------------------");
    info!("📖 sdforge::http::build() 返回 axum::Router，可链式添加中间件：");
    info!("");
    info!("📌 代码示例：");
    info!("   let router = sdforge::http::build()");
    info!("       .layer(axum::middleware::from_fn(api_key_auth_middleware))");
    info!("       .layer(axum::middleware::from_fn(request_log_middleware))");
    info!("       .layer(axum::middleware::from_fn(error_handler_middleware));");
    info!("");
    info!("📌 中间件执行顺序（从外到内）：");
    info!("   请求 → error_handler → request_log → api_key_auth → handler");
    info!("   响应 ← error_handler ← request_log ← api_key_auth ← handler");
    info!("");

    // 5. 安全注意事项
    info!("5️⃣  安全注意事项");
    info!("-----------------------------");
    info!("💡 认证中间件的最佳实践：");
    info!("   - 认证失败时统一返回 401，不暴露具体失败原因");
    info!("   - 速率限制：防止暴力破解（crawlrs 使用 auth_rate_limiter）");
    info!("   - IP 锁定：连续失败后临时封禁客户端 IP");
    info!("   - 日志审计：记录所有认证尝试（成功与失败）");
    info!("   - 不要在 URL 或日志中泄露 API Key");
    info!("");

    // 6. crawlrs 实际实现
    info!("6️⃣  crawlrs 的中间件实现");
    info!("-----------------------------");
    info!("📖 crawlrs 在 src/presentation/middleware/ 中实现：");
    info!("   - auth_middleware.rs : API Key + Bearer Token 认证");
    info!("   - 包含速率限制、IP 锁定、数据库验证");
    info!("   - 在 src/bootstrap/routes.rs 中注册到 Router");
    info!("");
    info!("💡 提示:");
    info!("   - 启用 crawlrs 的 api-sdk 特性以使用完整 SDK");
    info!("   - 生产环境使用 crawlrs::presentation::middleware::auth_middleware");
    info!("   - 自定义中间件需实现 FnMut(Request, Next) -> Future<Response>");
}

/// 占位：保证中间件函数被引用，避免编译器警告
#[allow(dead_code)]
fn _ensure_middlewares_referenced() {
    let _ = api_key_auth_middleware as fn(Request<Body>, Next) -> _;
    let _ = request_log_middleware as fn(Request<Body>, Next) -> _;
    let _ = error_handler_middleware as fn(Request<Body>, Next) -> _;
}

/// 占位：保证 AuthState 与 Arc 类型被引用
#[allow(dead_code)]
fn _ensure_types_referenced(_state: &AuthState, _arc: &Arc<AuthState>) {}
