// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// Fire Engine CDP测试模块
///
/// 测试Fire Engine CDP引擎的功能和评分机制
/// 验证完整浏览器自动化引擎的正确性

#[cfg(test)]
mod tests {
    use crawlrs::engines::fire_engine_cdp::FireEngineCdp;
    use crawlrs::engines::traits::{ScrapeRequest, ScraperEngine};
    use std::collections::HashMap;
    use std::time::Duration;

    #[tokio::test]
    async fn test_fire_engine_cdp_support_score() {
        let engine = FireEngineCdp::new();

        // 基础请求应该获得低分数（成本高）
        let basic_request = ScrapeRequest {
            url: "https://example.com".to_string(),
            headers: HashMap::new(),
            timeout: Duration::from_secs(10),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
        };
        assert_eq!(engine.support_score(&basic_request), 40);

        // 需要截图的请求应该获得较高分数
        let screenshot_request = ScrapeRequest {
            url: "https://example.com".to_string(),
            headers: HashMap::new(),
            timeout: Duration::from_secs(10),
            needs_js: false,
            needs_screenshot: true,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
        };
        assert_eq!(engine.support_score(&screenshot_request), 80);

        // 需要TLS指纹且需要截图的请求应该获得最高分数
        let tls_screenshot_request = ScrapeRequest {
            url: "https://example.com".to_string(),
            headers: HashMap::new(),
            timeout: Duration::from_secs(10),
            needs_js: false,
            needs_screenshot: true,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: true,
            use_fire_engine: false,
        };
        assert_eq!(engine.support_score(&tls_screenshot_request), 100);
    }

    #[tokio::test]
    async fn test_fire_engine_cdp_name() {
        let engine = FireEngineCdp::new();
        assert_eq!(engine.name(), "fire_engine_cdp");
    }
}