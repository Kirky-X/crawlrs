// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! sdforge SDK 基础示例
//!
//! 演示如何使用 sdforge 0.4 的 `#[forge]` 宏定义 HTTP 端点。
//!
//! sdforge 基于 axum + tower，通过过程宏自动生成路由注册代码：
//! - `#[forge(...)]` 标记异步函数为端点处理函数
//! - 通过 inventory 自动收集路由，`sdforge::http::build()` 返回 axum Router
//! - `#[state]` 参数从 axum Extension 中注入依赖
//!
//! 本示例展示 API 用法，不启动服务器；如需运行服务器请参考主项目 main.rs。
//!
//! # 使用方法
//!
//! ```bash
//! cargo run --example basic_sdk
//! ```

use log::info;
use sdforge::prelude::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ============================================================================
// DTO — 请求与响应类型
// ============================================================================

/// 创建用户请求
#[derive(Debug, Deserialize)]
struct CreateUserRequest {
    /// 用户名
    name: String,
    /// 邮箱
    email: String,
}

/// 用户响应
#[derive(Debug, Serialize)]
struct UserResponse {
    /// 用户 ID
    id: Uuid,
    /// 用户名
    name: String,
    /// 邮箱
    email: String,
    /// 创建时间（ISO 8601）
    created_at: String,
}

/// 列表查询参数（通过 query string 传递）
#[derive(Debug, Deserialize)]
struct ListUsersQuery {
    /// 每页数量（默认 10）
    limit: Option<u32>,
    /// 偏移量（默认 0）
    offset: Option<u32>,
}

/// 用户列表响应
#[derive(Debug, Serialize)]
struct ListUsersResponse {
    /// 用户列表
    users: Vec<UserResponse>,
    /// 总数
    total: u32,
}

// ============================================================================
// 端点定义 — 通过 #[forge] 宏注册 HTTP 路由
// ============================================================================

/// 创建用户端点
///
/// `#[forge(...)]` 宏的参数：
/// - `name`：端点唯一标识（用于内部注册）
/// - `version`：API 版本（生成 /api/v1/... 路径前缀）
/// - `path`：路由路径
/// - `method`：HTTP 方法
/// - `description`：OpenAPI 文档描述
#[forge(
    name = "create_user",
    version = "v1",
    path = "/sdk/users",
    method = "POST",
    description = "Create a new user"
)]
async fn create_user(req: CreateUserRequest) -> Result<UserResponse, ApiError> {
    if req.name.trim().is_empty() {
        return Err(ApiError::InvalidInput {
            message: "name cannot be empty".to_string(),
            field: Some("name".to_string()),
            value: None,
        });
    }

    if !req.email.contains('@') {
        return Err(ApiError::InvalidInput {
            message: "invalid email format".to_string(),
            field: Some("email".to_string()),
            value: Some(serde_json::json!(req.email)),
        });
    }

    Ok(UserResponse {
        id: Uuid::new_v4(),
        name: req.name,
        email: req.email,
        created_at: chrono::Utc::now().to_rfc3339(),
    })
}

/// 查询用户列表端点
#[forge(
    name = "list_users",
    version = "v1",
    path = "/sdk/users",
    method = "GET",
    description = "List users with pagination"
)]
async fn list_users(query: ListUsersQuery) -> Result<ListUsersResponse, ApiError> {
    let limit = query.limit.unwrap_or(10).min(100);
    let offset = query.offset.unwrap_or(0);

    // 模拟数据库查询（实际项目通过 #[state] 注入仓储）
    Ok(ListUsersResponse {
        users: Vec::new(),
        total: offset + limit,
    })
}

/// 获取单个用户端点
///
/// 参数 `id` 与路径中的 `:id` 占位符同名，sdforge 会自动推断为 Path 参数。
#[forge(
    name = "get_user",
    version = "v1",
    path = "/sdk/users/:id",
    method = "GET",
    description = "Get user by ID"
)]
async fn get_user(id: String) -> Result<UserResponse, ApiError> {
    let user_id = Uuid::parse_str(&id).map_err(|_| ApiError::InvalidInput {
        message: format!("invalid UUID: {}", id),
        field: Some("id".to_string()),
        value: Some(serde_json::json!(id)),
    })?;

    // 模拟未找到的情况
    Err(ApiError::NotFound {
        resource: "user".to_string(),
        resource_id: Some(user_id.to_string()),
    })
}

// ============================================================================
// 主函数 — 展示如何构建 SDK Router
// ============================================================================

#[tokio::main]
async fn main() {
    log::set_max_level(log::LevelFilter::Info);

    info!("🚀 开始 sdforge SDK 基础示例");
    info!("=====================================\n");

    // 1. #[forge] 宏说明
    info!("1️⃣  #[forge] 宏介绍");
    info!("-----------------------------");
    info!("📖 sdforge 通过 #[forge(...)] 宏将异步函数转换为 HTTP 端点：");
    info!("   - 自动生成 axum 路由注册代码");
    info!("   - 通过 inventory 收集所有 #[forge] 标记的端点");
    info!("   - 调用 sdforge::http::build() 即可获取 axum::Router");
    info!("");
    info!("📌 宏参数：");
    info!("   name        : 端点唯一标识");
    info!("   version     : API 版本（v1 → /api/v1/...）");
    info!("   path        : 路由路径（支持 :param 占位符）");
    info!("   method      : HTTP 方法（GET/POST/PUT/DELETE）");
    info!("   description : OpenAPI 文档描述");
    info!("");

    // 2. 参数注入说明
    info!("2️⃣  参数注入");
    info!("-----------------------------");
    info!("📖 sdforge 支持多种参数注入方式：");
    info!("   - 普通参数：从 JSON body 反序列化（需 impl Deserialize）");
    info!("   - #[path]  : 从 URL 路径参数提取");
    info!("   - #[query] : 从 query string 提取");
    info!("   - #[state] : 从 axum Extension 注入依赖（如 Arc<dyn Trait>）");
    info!("   - #[header]: 从 HTTP header 提取");
    info!("");

    // 3. DTO 设计
    info!("3️⃣  DTO 设计");
    info!("-----------------------------");
    info!("📖 请求和响应使用独立的 DTO，与领域模型解耦：");
    info!("   - 请求类型派生 Deserialize");
    info!("   - 响应类型派生 Serialize");
    info!("   - 敏感字段（如密码）不应出现在响应中");
    info!("");
    info!("📌 本示例定义了：");
    info!("   - CreateUserRequest（POST /sdk/users 的请求体）");
    info!("   - UserResponse（用户响应，包含 id/name/email/created_at）");
    info!("   - ListUsersQuery（GET /sdk/users 的分页参数）");
    info!("");

    // 4. 错误处理
    info!("4️⃣  错误处理");
    info!("-----------------------------");
    info!("📖 sdforge 通过 ApiError 枚举统一错误响应：");
    info!("   - InvalidInput      : 400 Bad Request（输入校验失败）");
    info!("   - NotFound          : 404 Not Found（资源不存在）");
    info!("   - AuthenticationFailed : 401 Unauthorized（认证失败）");
    info!("   - AccessDenied      : 403 Forbidden（权限不足）");
    info!("   - RateLimitExceeded : 429 Too Many Requests");
    info!("   - Internal          : 500 Internal Server Error");
    info!("");
    info!("📌 ApiError 自动实现 IntoResponse，转换为标准 HTTP 响应");
    info!("");

    // 5. 构建 Router
    info!("5️⃣  构建 SDK Router");
    info!("-----------------------------");
    info!("📖 调用 sdforge::http::build() 收集所有 #[forge] 注册的端点");
    info!("   返回 axum::Router，可直接用于 axum::serve()");
    info!("");
    info!("📌 实际启动服务器示例：");
    info!("   let router = sdforge::http::build();");
    info!("   let listener = tokio::net::TcpListener::bind(\"0.0.0.0:8080\").await?;");
    info!("   axum::serve(listener, router).await?;");
    info!("");

    // 6. crawlrs 项目中的实际用法
    info!("6️⃣  crawlrs 项目中的 SDK");
    info!("-----------------------------");
    info!("📖 crawlrs 在 src/presentation/sdk/mod.rs 中使用 sdforge：");
    info!("   - sdk_search   : POST /api/v1/sdk/search");
    info!("   - sdk_create_task : POST /api/v1/sdk/tasks");
    info!("   - sdk_scrape   : POST /api/v1/sdk/scrape");
    info!("   - sdk_create_crawl : POST /api/v1/sdk/crawl");
    info!("   - 通过 build_sdk_router() 返回 axum::Router");
    info!("");
    info!("💡 提示:");
    info!("   - 启用 crawlrs 的 api-sdk 特性以使用 SDK 端点");
    info!("   - 生产环境需配合 auth_middleware 进行认证");
    info!("   - 通过 #[state] 注入业务服务（TaskQueue、CrawlRepository 等）");
}
