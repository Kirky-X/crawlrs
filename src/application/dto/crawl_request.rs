// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl request DTO with URL validation

use crate::utils::SafeUrl;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use validator::Validate;

/// Maximum crawl depth limit
pub const MAX_CRAWL_DEPTH: u32 = 100;
/// Maximum concurrent pages
pub const MAX_CONCURRENCY: u32 = 50;

/// URL scheme validation: only http and https are allowed (SSRF mitigation).
static HTTP_URL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^https?://").unwrap());

fn is_http_url(value: &str) -> Result<(), validator::ValidationError> {
    if HTTP_URL_RE.is_match(value) {
        Ok(())
    } else {
        Err(validator::ValidationError::new("invalid_url_scheme")
            .with_message("URL must start with http:// or https://".into()))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
#[serde(deny_unknown_fields)]
pub struct CrawlRequestDto {
    /// URL to crawl
    #[validate(length(min = 1, max = 2048, message = "URL长度必须在1-2048个字符之间"))]
    #[validate(custom(
        function = "is_http_url",
        message = "URL must start with http:// or https://"
    ))]
    pub url: String,
    /// Validated SafeUrl (populated after validation)
    #[serde(skip)]
    pub validated_url: Option<SafeUrl>,
    #[validate(length(min = 1, max = 255, message = "任务名称长度必须在1-255个字符之间"))]
    pub name: Option<String>,
    #[validate(nested)]
    pub config: CrawlConfigDto,
    /// 同步等待时长（毫秒，默认 5000，最大 30000）
    #[validate(range(min = 0, max = 30000, message = "sync_wait_ms必须在0-30000之间"))]
    pub sync_wait_ms: Option<u32>,
    /// 任务过期时间
    pub expires_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Deserialize, Serialize, Clone, Validate)]
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
    pub proxy: Option<String>,
    pub headers: Option<serde_json::Value>,
    pub extraction_rules: Option<
        std::collections::HashMap<
            String,
            crate::domain::services::extraction_service::ExtractionRule,
        >,
    >,
}
