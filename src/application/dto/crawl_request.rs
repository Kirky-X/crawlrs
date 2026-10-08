// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Crawl request DTO with URL validation

use crate::utils::SafeUrl;
use once_cell::sync::Lazy;
use regex::Regex;
use sdforge::validator::Validate;
use serde::{Deserialize, Serialize};

/// Maximum crawl depth limit
pub const MAX_CRAWL_DEPTH: u32 = 100;
/// Maximum concurrent pages
pub const MAX_CONCURRENCY: u32 = 50;

/// URL scheme validation: only http and https are allowed (SSRF mitigation).
static HTTP_URL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^https?://").unwrap());

fn is_http_url(value: &str) -> Result<(), sdforge::validator::ValidationError> {
    if HTTP_URL_RE.is_match(value) {
        Ok(())
    } else {
        Err(
            sdforge::validator::ValidationError::new("invalid_url_scheme")
                .with_message("URL must start with http:// or https://".into()),
        )
    }
}

/// sitemap_url 专用 scheme 校验：消息经启动期 i18n 全局束解析
/// （validation.ftl `validation-sitemap-url-scheme`；未初始化回退 key 本身）。
///
/// 注意：`#[validate(custom(..., message = ...))]` 的 `message` 属性在宏展开时
/// 以编译期字面量覆盖函数返回的 `err.message`——接入运行时 i18n 必须省略该属性，
/// 让本函数构造的错误消息生效。
fn is_http_sitemap_url(value: &str) -> Result<(), sdforge::validator::ValidationError> {
    if HTTP_URL_RE.is_match(value) {
        Ok(())
    } else {
        Err(
            sdforge::validator::ValidationError::new("invalid_url_scheme")
                .with_message(crate::i18n::tr_log("validation-sitemap-url-scheme").into()),
        )
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[validate(crate = "sdforge::validator")]
#[serde(deny_unknown_fields)]
pub struct CrawlRequestDto {
    /// URL to crawl
    #[validate(length(
        min = 1,
        max = 2048,
        message = "URL length must be between 1 and 2048 characters"
    ))]
    #[validate(custom(
        function = "is_http_url",
        message = "URL must start with http:// or https://"
    ))]
    pub url: String,
    /// Validated SafeUrl (populated after validation)
    #[serde(skip)]
    pub validated_url: Option<SafeUrl>,
    #[validate(length(
        min = 1,
        max = 255,
        message = "Task name length must be between 1 and 255 characters"
    ))]
    pub name: Option<String>,
    #[validate(nested)]
    pub config: CrawlConfigDto,
    /// 同步等待时长（毫秒，默认 5000，最大 30000）
    #[validate(range(
        min = 0,
        max = 30000,
        message = "sync_wait_ms must be between 0 and 30000"
    ))]
    pub sync_wait_ms: Option<u32>,
    /// 任务过期时间
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
    /// sitemap 流式请求源（F11）：true 时该 crawl 以 sitemap 作为 URL 发现源
    /// （受 `CRAWLRS__SITEMAP__ENABLED` 主开关门控，关闭时请求被显式拒绝）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sitemap_seed: Option<bool>,
    /// 显式 sitemap 地址；缺省从种子 URL 派生 `scheme://host[:port]/sitemap.xml`
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[validate(custom(function = "is_http_sitemap_url"))]
    pub sitemap_url: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Validate)]
#[validate(crate = "sdforge::validator")]
#[serde(deny_unknown_fields)]
pub struct CrawlConfigDto {
    #[validate(range(max = 100, message = "max_depth must be at most 100"))]
    pub max_depth: u32,
    pub include_patterns: Option<Vec<String>>,
    pub exclude_patterns: Option<Vec<String>>,
    pub strategy: Option<String>,
    #[validate(range(max = 86400000, message = "crawl_delay_ms must be at most 86400000"))]
    pub crawl_delay_ms: Option<u64>,
    #[validate(range(max = 50, message = "max_concurrency must be at most 50"))]
    pub max_concurrency: Option<u32>,
    /// 最大抓取页数上限（超出提前终止；缺省 1000）
    #[validate(range(max = 100000, message = "max_pages must be at most 100000"))]
    pub max_pages: Option<u32>,
    pub proxy: Option<String>,
    pub headers: Option<serde_json::Value>,
    pub extraction_rules: Option<
        std::collections::HashMap<
            String,
            crate::domain::services::extraction_service::ExtractionRule,
        >,
    >,
    /// LLM Prompt 提取（自由文本描述需要提取的内容）
    pub extraction_prompt: Option<String>,
    /// JSON Schema 驱动的结构化提取
    pub extraction_schema: Option<serde_json::Value>,
}
