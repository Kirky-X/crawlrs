// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// HTTP请求处理器模块
///
/// 包含各个API端点的具体处理逻辑
/// 每个处理器负责处理特定类型的HTTP请求并返回响应
pub mod crawl_handler;
pub mod extract_handler;
pub mod scrape_handler;
pub mod search_handler;
pub mod webhook_handler;
