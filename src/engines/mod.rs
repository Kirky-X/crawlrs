// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// 引擎模块
///
/// 提供各种网页爬取和抓取引擎的实现
/// 包括不同的浏览器引擎、HTTP客户端和相关的支持组件
pub mod browser_downloader; // 新增：浏览器自动下载管理器
pub mod circuit_breaker;
pub mod client;
pub mod health_monitor;
pub mod router;
pub mod validators;

// Shared validation utilities for SSRF protection
pub mod shared;

// New unified EngineClient API
pub mod engine_client;
pub mod traits;

pub use engine_client::{
    EngineClient, EngineError, EngineHealthStatus, PageAction, ScrapeOptions, ScrapeRequest,
    ScrapeResponse, ScreenshotConfig, ScrollDirection,
};

pub use engine_client::ScraperEngine;

// 导出浏览器下载管理器
pub use browser_downloader::{
    BrowserDownloadConfig, BrowserDownloadError, BrowserDownloadManager, DownloadStatus,
};
