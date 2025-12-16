// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

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
    /// 其他错误
    #[error("Other error: {0}")]
    Other(String),
}

impl EngineError {
    /// 判断错误是否可重试
    ///
    /// # 返回值
    ///
    /// 如果错误是可重试的则返回true，否则返回false
    pub fn is_retryable(&self) -> bool {
        match self {
            EngineError::RequestFailed(e) => {
                e.is_timeout() || e.is_connect() || e.status().is_some_and(|s| s.is_server_error())
            }
            EngineError::Timeout => true,
            EngineError::Other(_) => false, // Assume other errors (like validation) are not retryable
            _ => false,
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
