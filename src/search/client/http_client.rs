// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 共享的 HTTP 客户端模块
//!
//! 提供所有搜索引擎共用的 HTTP 客户端实例，避免重复创建。
//! 此模块已重构，现在委托给统一的 `crate::utils::http_client` 模块。

// 重新导出统一的 HTTP 客户端单例，保持向后兼容
pub use crate::utils::http_client::HTTP_CLIENT as SHARED_HTTP_CLIENT;

// 重新导出工厂函数，保持向后兼容
pub use crate::utils::http_client::create_http_client_with_timeout;
