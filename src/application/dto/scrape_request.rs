// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use validator::Validate;

/// 爬取请求数据传输对象
///
/// 用于封装客户端发起的网页爬取请求的相关参数
#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct ScrapeRequestDto {
    /// 要爬取的网页URL
    #[validate(url)]
    pub url: String,
    /// 请求的数据格式列表
    pub formats: Option<Vec<String>>,
    /// 包含的HTML标签列表
    pub include_tags: Option<Vec<String>>,
    /// 排除的HTML标签列表
    pub exclude_tags: Option<Vec<String>>,
    /// 回调Webhook地址
    pub webhook: Option<String>,
    /// 提取规则
    pub extraction_rules: Option<
        std::collections::HashMap<
            String,
            crate::domain::services::extraction_service::ExtractionRule,
        >,
    >,
    /// 页面交互动作
    pub actions: Option<Vec<ScrapeActionDto>>,
    /// 抓取选项
    pub options: Option<ScrapeOptionsDto>,
    /// 自定义元数据
    pub metadata: Option<serde_json::Value>,
    /// 同步等待时长（毫秒，默认 5000，最大 30000）
    #[validate(range(min = 0, max = 30000))]
    pub sync_wait_ms: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ScrapeOptionsDto {
    /// 自定义HTTP请求头
    pub headers: Option<Value>,
    /// 等待时间（毫秒）
    pub wait_for: Option<u64>,
    /// 超时时间（秒）
    pub timeout: Option<u64>,
    /// 是否需要JavaScript渲染
    pub js_rendering: Option<bool>,
    /// 是否需要截图
    pub screenshot: Option<bool>,
    /// 截图配置
    pub screenshot_options: Option<ScreenshotOptionsDto>,
    /// 是否模拟移动设备
    pub mobile: Option<bool>,
    /// 代理配置 (URL)
    pub proxy: Option<String>,
    /// 是否跳过TLS验证
    pub skip_tls_verification: Option<bool>,
    /// 是否需要TLS指纹对抗
    pub needs_tls_fingerprint: Option<bool>,
    /// 是否使用Fire Engine (CDP)
    pub use_fire_engine: Option<bool>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ScrapeActionDto {
    Wait { milliseconds: u64 },
    Click { selector: String },
    Scroll { direction: String },
    Screenshot { full_page: Option<bool> },
    Input { selector: String, text: String },
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ScreenshotOptionsDto {
    pub full_page: Option<bool>,
    pub selector: Option<String>,
    pub quality: Option<u8>,
    pub format: Option<String>,
}
