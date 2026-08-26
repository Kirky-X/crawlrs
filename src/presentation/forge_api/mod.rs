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

pub mod scrape;
pub mod search;

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
