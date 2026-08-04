// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::services::extraction_service::ExtractionRule;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use validator::Validate;

/// URL scheme validation: only http and https are allowed (SSRF mitigation).
static HTTP_URL_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^https?://").unwrap());

/// Validate that every URL in a list starts with http:// or https://.
fn validate_url_list(urls: &[String]) -> Result<(), validator::ValidationError> {
    for url in urls {
        if !HTTP_URL_RE.is_match(url) {
            return Err(validator::ValidationError::new("invalid_url_scheme")
                .with_message(format!("URL must start with http:// or https://: {}", url).into()));
        }
    }
    Ok(())
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct ExtractRequestDto {
    #[validate(length(min = 1, max = 100, message = "urls must have 1-100 entries"))]
    #[validate(custom(function = "validate_url_list"))]
    pub urls: Vec<String>,
    pub prompt: Option<String>,
    pub schema: Option<Value>,
    pub model: Option<String>,
    /// 提取规则（用于复杂提取场景）
    pub rules: Option<HashMap<String, ExtractionRule>>,
    /// 同步等待时长（毫秒，默认 5000，最大 30000）
    #[validate(range(max = 30000, message = "sync_wait_ms must be at most 30000"))]
    pub sync_wait_ms: Option<u32>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ExtractResponseDto {
    pub results: Vec<ExtractResultDto>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ExtractResultDto {
    pub url: String,
    pub data: Value,
    pub error: Option<String>,
}
