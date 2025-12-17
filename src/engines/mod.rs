// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// 引擎模块
///
/// 提供各种网页爬取和抓取引擎的实现
/// 包括不同的浏览器引擎、HTTP客户端和相关的支持组件
pub mod circuit_breaker;
pub mod fire_engine_cdp;
#[cfg(test)]
mod fire_engine_cdp_test;
pub mod fire_engine_tls;
#[cfg(test)]
mod fire_engine_tls_test;
pub mod health_monitor;
pub mod playwright_engine;
pub mod reqwest_engine;
pub mod router;
pub mod traits;
pub mod validators;
