// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! `/v1/map` 请求 DTO（bdd-acceptance-hardening R-map-001）。

use serde::{Deserialize, Serialize};
use validator::Validate;

/// serde 默认 limit（与 Firecrawl /map 常用默认对齐）
fn default_limit() -> Option<u32> {
    Some(1000)
}

/// `POST /v1/map` 请求体：从站点 sitemap 发现 URL。
#[derive(Debug, Deserialize, Serialize, Clone, Validate)]
pub struct MapRequestDto {
    /// 站点 origin（如 `https://a.com`），将抓取 `{origin}/sitemap.xml`
    #[validate(custom(
        function = "crate::application::dto::scrape_request::is_http_url",
        message = "URL must start with http:// or https://"
    ))]
    pub url: String,
    /// glob 白名单（`*`/`?` 通配），先 include 后 exclude
    #[serde(default)]
    pub include_patterns: Option<Vec<String>>,
    /// glob 黑名单（`*`/`?` 通配）
    #[serde(default)]
    pub exclude_patterns: Option<Vec<String>>,
    /// 返回 URL 数上限（1..=10_000，缺省 1000）
    #[serde(default = "default_limit")]
    #[validate(range(min = 1, max = 10_000))]
    pub limit: Option<u32>,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R-map-001：合法最小请求解析成功，limit 取 serde 默认 1000。
    #[test]
    fn minimal_request_parses_with_default_limit() {
        let dto: MapRequestDto =
            serde_json::from_value(serde_json::json!({"url": "https://a.com"})).expect("parse");
        assert_eq!(dto.url, "https://a.com");
        assert_eq!(dto.limit, Some(1000));
        assert!(dto.include_patterns.is_none());
        assert!(dto.exclude_patterns.is_none());
        assert!(dto.validate().is_ok());
    }

    /// R-map-001：`not-a-url` 校验失败。
    #[test]
    fn invalid_url_fails_validation() {
        let dto: MapRequestDto =
            serde_json::from_value(serde_json::json!({"url": "not-a-url"})).expect("parse");
        assert!(dto.validate().is_err(), "non-http url must fail validation");
    }

    /// R-map-001：limit 越界（0 与 10001）校验失败；10000 边界合法。
    #[test]
    fn limit_out_of_range_fails_validation() {
        for bad in [0, 10_001] {
            let dto: MapRequestDto = serde_json::from_value(serde_json::json!({
                "url": "https://a.com", "limit": bad
            }))
            .expect("parse");
            assert!(
                dto.validate().is_err(),
                "limit={bad} must fail validation"
            );
        }
        let ok: MapRequestDto = serde_json::from_value(serde_json::json!({
            "url": "https://a.com", "limit": 10_000
        }))
        .expect("parse");
        assert!(ok.validate().is_ok(), "limit=10000 boundary must pass");
    }

    /// R-map-001：patterns 可选字段正常反序列化。
    #[test]
    fn patterns_deserialize() {
        let dto: MapRequestDto = serde_json::from_value(serde_json::json!({
            "url": "https://a.com",
            "include_patterns": ["*/blog/*"],
            "exclude_patterns": ["*/tag/*"],
            "limit": 50
        }))
        .expect("parse");
        assert_eq!(dto.include_patterns.as_deref(), Some(&["*/blog/*".to_string()][..]));
        assert_eq!(dto.exclude_patterns.as_deref(), Some(&["*/tag/*".to_string()][..]));
        assert_eq!(dto.limit, Some(50));
    }
}
