// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Scrape request DTO with input validation

use crate::common::CacheMode;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use validator::Validate;

/// Maximum allowed URL length (2048 characters)
pub const MAX_URL_LENGTH: usize = 2048;
/// Maximum number of allowed tags in include/exclude lists
pub const MAX_TAG_COUNT: usize = 50;
/// Maximum number of allowed actions
pub const MAX_ACTION_COUNT: usize = 20;
/// Maximum metadata object depth
pub const MAX_METADATA_DEPTH: usize = 5;

/// 爬取请求数据传输对象
///
/// 用于封装客户端发起的网页爬取请求的相关参数
/// 拒绝未知字段以增强安全性
#[derive(Debug, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct ScrapeRequestDto {
    /// 要爬取的网页URL (仅支持 http/https)
    #[validate(length(min = 1, max = 2048))]
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
    #[validate(range(
        min = 0,
        max = 30000,
        message = "sync_wait_ms must be between 0 and 30000"
    ))]
    pub sync_wait_ms: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize, Default)]
#[serde(deny_unknown_fields)]
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
    /// 缓存模式（T058/R-cache-002，design.md §13）
    ///
    /// 控制本次抓取的缓存读写行为。`None`（默认）等价于 `Some(CacheMode::Enabled)`。
    /// 详见 [`crate::common::CacheMode`]。
    ///
    /// 与 `bypass_cache` 的优先级：`bypass_cache=Some(true)` 覆盖 `cache_mode` 为
    /// `Some(CacheMode::Bypass)`（应急绕过读，正常写回）。
    pub cache_mode: Option<CacheMode>,
    /// 应急绕过缓存读（T058/R-cache-002，design.md §13）
    ///
    /// `Some(true)` → 等价于 `cache_mode=Some(CacheMode::Bypass)`（跳过读，正常写回），
    /// 用于运行时不信任缓存脏数据的应急场景。`Some(false)` 或 `None` → 忽略，按 `cache_mode` 走。
    ///
    /// 详见 [`crate::common::CacheMode::Bypass`]。
    pub bypass_cache: Option<bool>,
}

impl ScrapeOptionsDto {
    /// 计算 `cache_mode` 与 `bypass_cache` 桥接后的有效缓存模式（架构审查 HIGH-3 修复）
    ///
    /// 优先级：`bypass_cache=Some(true)` 覆盖 `cache_mode` 为 `Bypass`；
    /// 其余情况按 `cache_mode` 走；两者皆 `None` 时返回 `None`（等价于 `Enabled`）。
    ///
    /// # 返回
    ///
    /// - `Some(CacheMode::Bypass)`：`bypass_cache=Some(true)`，或 `cache_mode=Some(Bypass)`
    /// - `Some(mode)`：`bypass_cache!=Some(true)` 且 `cache_mode=Some(mode)`
    /// - `None`：两者皆 `None`，由调用方 `unwrap_or_default()` 解析为 `Enabled`
    ///
    /// 此方法消除原 `create_scrape.rs` + `scrape_worker.rs` 中的重复桥接逻辑（DRY 修复）。
    #[must_use]
    pub fn effective_cache_mode(&self) -> Option<CacheMode> {
        if self.bypass_cache.unwrap_or(false) {
            Some(CacheMode::Bypass)
        } else {
            self.cache_mode
        }
    }
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
