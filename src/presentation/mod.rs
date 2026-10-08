// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

/// 表现层模块
///
/// 负责处理HTTP请求和响应，提供RESTful API接口
/// 包含错误处理、请求提取、处理器、中间件和路由配置
pub mod errors;
pub mod extractors;
pub mod handlers;
pub mod helpers;
pub mod middleware;
pub mod routes;
pub mod sdk;
pub mod state;

/// 平台业务路由的 sdforge inventory 注册层（sdforge 单一注册面迁移，在途）。
///
/// **为什么挂在 `forge-full` 特性下**：inventory 是链接期全局收集——本模块一旦
/// 编译进二进制，`sdforge::http::build()`（当前唯一调用点 `build_sdk_router`）
/// 就会同时收集 SDK 与平台两侧路由，与 `bootstrap/routes.rs` 手写的
/// protected/v2 路由组重复注册，启动即 panic（`Overlapping method route`）。
/// 因此 inventory 激活与手写装配不可并存：迁移切换必须是「一次调用、三族
/// 中间件（protected/v2/sdk）统一排序」的整体切换，涉及
/// team_semaphore_middleware 等中间件作用域变化——待设计裁决后实施。
/// 在此之前，本模块经 `--features forge-full` 编译并接受路由等价性测试
/// （`forge_api::tests`），杜绝「handler 演进、注册面静默腐烂」。
#[cfg(feature = "forge-full")]
pub mod forge_api;
