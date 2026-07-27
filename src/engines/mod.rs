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
pub mod upgrade_probe; // T014：流式 HTTP→Chrome 升级探测
pub mod validators;

// 反爬虫检测模块（T009-T011）— 由 `antibot` feature 门控
#[cfg(feature = "antibot")]
pub mod antibot;

// JS 注入模块（T029-T031，R-jsrender-002）— 由 `engine-playwright` feature 门控
// 依赖 chromiumoxide::Page，仅在浏览器引擎启用时可用
#[cfg(feature = "engine-playwright")]
pub mod js_inject;

// 请求拦截模块（T033，R-jsrender-003）— 由 `engine-playwright` feature 门控
// 依赖 chromiumoxide CDP Fetch domain + network::ResourceType
#[cfg(feature = "engine-playwright")]
pub mod intercept;

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
