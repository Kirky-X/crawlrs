// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// 表现层模块
///
/// 负责处理HTTP请求和响应，提供RESTful API接口
/// 包含错误处理、请求提取、处理器、中间件和路由配置
pub mod errors;
pub mod extractors;
pub mod handlers;
pub mod middleware;
pub mod routes;
