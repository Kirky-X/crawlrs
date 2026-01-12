// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![deprecated(
    since = "1.0.0",
    note = "Use engine_client::EngineClient, ScrapeRequest, and ScrapeResponse instead. \
            See https://github.com/Kirky-X/crawlrs/blob/main/docs/migration.md for migration guide."
)]

use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;

/// 引擎错误类型
#[derive(Error, Debug, Clone)]
pub enum EngineError {
    /// 请求失败
    #[error("Request failed: {0}")]
    RequestFailed(String),

    /// 请求超时
    #[error("Request timeout after {0:?}")]
    Timeout(Duration),

    /// 所有引擎都失败
    #[error("All engines failed: {0}")]
    AllEnginesFailed(String),

    /// SSRF 保护错误
    #[error("SSRF protection: {0}")]
    SsrfProtection(String),

    /// 浏览器错误
    #[error("Browser error: {0}")]
    BrowserError(String),

    /// 任务过期
    #[error("Task expired")]
    Expired,

    /// 其他错误
    #[error("Other error: {0}")]
    Other(String),
}

impl EngineError {
    /// 检查错误是否可以重试
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::RequestFailed(_) => true, // 网络错误通常可重试
            Self::Timeout(_) => true,
            Self::AllEnginesFailed(_) => false,
            Self::SsrfProtection(_) => false, // SSRF 错误不应重试
            Self::BrowserError(_) => true,    // 浏览器错误可能可重试
            Self::Expired => false,
            Self::Other(msg) => {
                // 某些特定错误可以重试
                !msg.contains("connection refused")
            }
        }
    }

    /// 从 reqwest 错误创建 EngineError
    #[inline]
    pub fn from_reqwest(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            Self::Timeout(Duration::from_secs(30))
        } else if e.is_connect() {
            Self::RequestFailed(format!("Connection failed: {}", e))
        } else if e.is_request() {
            Self::RequestFailed(format!("Request error: {}", e))
        } else {
            Self::RequestFailed(e.to_string())
        }
    }
}

/// 抓取请求
pub struct ScrapeRequest {
    /// 目标URL
    pub url: String,
    /// 请求头
    pub headers: HashMap<String, String>,
    /// 超时时间
    pub timeout: Duration,
    /// 是否需要JavaScript支持
    pub needs_js: bool,
    /// 是否需要截图
    pub needs_screenshot: bool,
    /// 截图配置
    pub screenshot_config: Option<ScreenshotConfig>,
    /// 是否移动端
    pub mobile: bool,
    /// 代理配置 (URL)
    pub proxy: Option<String>,
    /// 是否跳过TLS验证
    pub skip_tls_verification: bool,
    /// 是否需要TLS指纹对抗
    pub needs_tls_fingerprint: bool,
    /// 是否使用Fire Engine (CDP)
    pub use_fire_engine: bool,
    /// 页面交互动作
    pub actions: Vec<ScrapeAction>,
    /// 同步等待时长（毫秒）
    pub sync_wait_ms: u32,
}

impl ScrapeRequest {
    /// 创建一个新的抓取请求
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
            actions: Vec::new(),
            sync_wait_ms: 0,
        }
    }
}

/// 页面交互动作
#[derive(Debug, Clone)]
pub enum ScrapeAction {
    /// 等待
    Wait { milliseconds: u64 },
    /// 点击
    Click { selector: String },
    /// 滚动
    Scroll { direction: String },
    /// 截图
    Screenshot { full_page: Option<bool> },
    /// 输入
    Input { selector: String, text: String },
}

/// 截图配置
#[derive(Debug, Clone)]
pub struct ScreenshotConfig {
    /// 是否全屏
    pub full_page: bool,
    /// 元素选择器
    pub selector: Option<String>,
    /// 质量 (1-100)
    pub quality: Option<u8>,
    /// 格式 (png, jpeg)
    pub format: Option<String>,
}

impl Default for ScreenshotConfig {
    fn default() -> Self {
        Self {
            full_page: true,
            selector: None,
            quality: None,
            format: Some("png".to_string()),
        }
    }
}

/// 抓取响应
pub struct ScrapeResponse {
    /// HTTP状态码
    pub status_code: u16,
    /// 响应内容
    pub content: String,
    /// 截图数据 (base64 encoded)
    pub screenshot: Option<String>,
    /// 内容类型
    pub content_type: String,
    /// 响应头
    pub headers: HashMap<String, String>,
    /// 响应时间（毫秒）
    pub response_time_ms: u64,
}

impl ScrapeResponse {
    /// 创建一个新的抓取响应
    pub fn new(_url: &str, content: &str) -> Self {
        let mut headers = HashMap::new();
        headers.insert("Content-Type".to_string(), "text/html".to_string());

        Self {
            status_code: 200,
            content: content.to_string(),
            screenshot: None,
            content_type: "text/html".to_string(),
            headers,
            response_time_ms: 0,
        }
    }
}

/// 抓取引擎特质
#[async_trait]
pub trait ScraperEngine: Send + Sync {
    /// 执行抓取
    async fn scrape(&self, request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError>;

    /// 计算对请求的支持分数（0-100）
    fn support_score(&self, request: &ScrapeRequest) -> u8;

    /// 引擎名称
    fn name(&self) -> &'static str;

    // 引擎能力方法 - 替代硬编码的引擎名检查

    /// 是否支持截图
    fn supports_screenshot(&self) -> bool {
        true
    }

    /// 是否支持 JavaScript
    fn supports_javascript(&self) -> bool {
        true
    }

    /// 是否支持 TLS 指纹
    fn supports_tls_fingerprint(&self) -> bool {
        false
    }
}

// Conversion methods for EngineClient
impl ScrapeRequest {
    /// Convert from public ScrapeRequest to internal ScrapeRequest
    #[inline]
    pub fn from_public(request: &super::engine_client::ScrapeRequest) -> Self {
        use super::engine_client::{PageAction, ScrollDirection};

        let options = &request.options;

        // Convert page actions
        let actions: Vec<ScrapeAction> = options
            .actions
            .iter()
            .map(|action| match action {
                PageAction::Wait { milliseconds } => ScrapeAction::Wait {
                    milliseconds: *milliseconds,
                },
                PageAction::Click { selector } => ScrapeAction::Click {
                    selector: selector.clone(),
                },
                PageAction::Scroll { direction } => {
                    let direction_str = match direction {
                        ScrollDirection::Down => "down",
                        ScrollDirection::Up => "up",
                        ScrollDirection::Bottom => "bottom",
                        ScrollDirection::Top => "top",
                    };
                    ScrapeAction::Scroll {
                        direction: direction_str.to_string(),
                    }
                }
                PageAction::Input { selector, text } => ScrapeAction::Input {
                    selector: selector.clone(),
                    text: text.clone(),
                },
            })
            .collect();

        // Convert screenshot config
        let screenshot_config = options
            .screenshot_config
            .as_ref()
            .map(|config| ScreenshotConfig {
                full_page: config.full_page,
                selector: config.selector.clone(),
                quality: config.quality,
                format: config.format.clone(),
            });

        Self {
            url: request.url.clone(),
            headers: options.headers.clone(),
            timeout: options.timeout,
            needs_js: options.needs_js,
            needs_screenshot: options.needs_screenshot,
            screenshot_config,
            mobile: options.mobile,
            proxy: options.proxy.clone(),
            skip_tls_verification: options.skip_tls_verification,
            needs_tls_fingerprint: options.needs_tls_fingerprint,
            use_fire_engine: options.use_fire_engine,
            actions,
            sync_wait_ms: options.sync_wait_ms,
        }
    }
}

impl ScrapeResponse {
    /// Convert from internal ScrapeResponse to public ScrapeResponse
    #[inline]
    pub fn from_internal(
        internal: ScrapeResponse,
        original_url: &str,
    ) -> super::engine_client::ScrapeResponse {
        super::engine_client::ScrapeResponse {
            status_code: internal.status_code,
            content: internal.content,
            screenshot: internal.screenshot,
            content_type: internal.content_type,
            headers: internal.headers,
            response_time_ms: internal.response_time_ms,
            final_url: Some(original_url.to_string()),
        }
    }
}
