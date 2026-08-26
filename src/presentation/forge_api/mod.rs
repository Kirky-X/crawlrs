// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 平台业务路由的 sdforge 注册层。
//!
//! 全部业务端点经 `#[forge]` 宏或 `inventory::submit!(HttpRoute::new(..))`
//! 在编译期注册到 sdforge inventory；运行时由 [`build_forge_router`]
//! 一次性收集构建 Axum Router。
//!
//! # 单一调用点约束
//!
//! inventory 是进程级全局收集：`sdforge::http::build()` 在同一二进制中
//! 只能有一个生产调用点（本模块）。SDK 路由（`presentation::sdk`）与本层
//! 路由共用同一次 build —— 两者路径空间不相交（/api/v1/sdk/* vs /v1/*），
//! 由 build() 内部按完整路径去重后统一注册。

use axum::Router;

/// Collect all inventory-registered routes (SDK + platform) into one Router.
pub fn build_forge_router() -> Router {
    sdforge::http::build()
}
