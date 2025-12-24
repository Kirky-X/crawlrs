// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Duration;
use thiserror::Error;

/// 引擎错误类型
#[derive(Error, Debug)]
pub enum EngineError {
    /// 请求失败
    #[error("Request failed: {0}")]
    RequestFailed(#[from] reqwest::Error),
    /// 所有引擎都失败
    #[error("All engines failed")]
    AllEnginesFailed,
    /// 超时
    #[error("Timeout")]
    Timeout,
    /// 状态过期
    #[error("Status expired")]
    Expired,
    /// 其他错误
    #[error("Other error: {0}")]
    Other(String),
}

impl EngineError {
    /// 检查错误是否可以重试
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::RequestFailed(e) => {
                // 网络错误、连接超时等可以重试
                e.is_timeout() || e.is_connect() || e.is_request()
            }
            Self::Timeout => true,
            Self::Expired => false, // 任务过期不应重试
            Self::AllEnginesFailed => false,
            Self::Other(_) => false,
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
}
