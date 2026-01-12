// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// 引擎模块
///
/// 提供各种网页爬取和抓取引擎的实现
/// 包括不同的浏览器引擎、HTTP客户端和相关的支持组件
pub mod circuit_breaker;
pub mod client;
pub mod health_monitor;
pub mod router;
#[allow(deprecated)]
pub mod traits;
pub mod validators;

// New unified EngineClient API
pub mod engine_client;

pub use engine_client::{
    EngineClient, EngineError, EngineHealthStatus, PageAction, ScrapeOptions, ScrapeRequest,
    ScrapeResponse, ScreenshotConfig, ScrollDirection,
};
