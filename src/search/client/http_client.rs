// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 共享的 HTTP 客户端模块
//!
//! 提供所有搜索引擎共用的 HTTP 客户端工厂函数。

// 重新导出工厂函数，保持向后兼容
pub use crate::utils::http_client::create_http_client_with_timeout;
