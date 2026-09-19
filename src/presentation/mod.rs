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
