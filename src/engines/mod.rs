// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

/// 引擎模块
///
/// 提供各种网页爬取和抓取引擎的实现
/// 包括不同的浏览器引擎、HTTP客户端和相关的支持组件
pub mod browser_downloader; // 浏览器自动下载管理器
pub mod circuit_breaker;
pub mod client;
pub mod health_monitor;
pub mod provider; // ProxyProvider trait 抽象（DIP）
pub mod proxy_pool; // 代理轮换池
pub mod router;
pub mod router_metrics; // ARC-003: 路由指标收集器
pub mod upgrade_probe; // 流式 HTTP→Chrome 升级探测
pub mod validators;

// 反爬虫检测模块— 由 `antibot` feature 门控
#[cfg(feature = "content")]
pub mod antibot;

// JS 注入模块— 由 `engine-playwright` feature 门控
// 依赖 chromiumoxide::Page，仅在浏览器引擎启用时可用
#[cfg(feature = "engine-playwright")]
pub mod js_inject;

// 请求拦截模块由 `engine-playwright` feature 门控
// 依赖 chromiumoxide CDP Fetch domain + network::ResourceType
#[cfg(feature = "engine-playwright")]
pub mod intercept;

// 页面加载后等待策略由 `engine-playwright` feature 门控
// 依赖 chromiumoxide::Page；WaitFor 枚举本身在 engine_client.rs（非 feature-gated），
// 此模块仅包含 `impl WaitFor::wait` 方法实现。
#[cfg(feature = "engine-playwright")]
pub mod wait;

// MLLM 自主导航爬取引擎由 `engine-mllm` feature 门控
// 依赖 engine-playwright（浏览器）+ llm（视觉模型）
#[cfg(feature = "engine-mllm")]
pub mod mllm;

// Shared validation utilities for SSRF protection
pub mod shared;

// New unified EngineClient API
pub mod engine_client;
pub mod types; // ARC-002: 引擎层 DTO 类型

pub use engine_client::{
    EngineClient, EngineError, EngineHealthStatus, PageAction, ScrapeOptions, ScrapeRequest,
    ScrapeResponse, ScreenshotConfig, ScrollDirection, WaitFor,
};

pub use engine_client::ScraperEngine;

// ProxyProvider trait 抽象
pub use provider::ProxyProvider;
// ClientHandle 封装
pub use client::ClientHandle;

// 导出浏览器下载管理器
pub use browser_downloader::{
    BrowserDownloadConfig, BrowserDownloadError, BrowserDownloadManager, DownloadStatus,
};
