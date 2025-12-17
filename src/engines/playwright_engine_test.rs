// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

#[cfg(test)]
mod tests {
    use crate::engines::playwright_engine::PlaywrightEngine;
    use crate::engines::traits::{ScrapeRequest, ScraperEngine};
    use std::collections::HashMap;
    use std::time::Duration;

    #[tokio::test]
    async fn test_playwright_engine_support_score() {
        let engine = PlaywrightEngine;
        
        // 基础请求应该获得中等分数
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
        assert_eq!(engine.support_score(&basic_request), 50);

        // 需要JS的请求应该获得最高分数
        let js_request = ScrapeRequest {
            url: "https://example.com".to_string(),
            headers: HashMap::new(),
            timeout: Duration::from_secs(10),
            needs_js: true,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
        };
        assert_eq!(engine.support_score(&js_request), 100);

        // 需要截图的请求应该获得最高分数
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
        assert_eq!(engine.support_score(&screenshot_request), 100);

        // 移动端请求应该获得中等分数
        let mobile_request = ScrapeRequest {
            url: "https://example.com".to_string(),
            headers: HashMap::new(),
            timeout: Duration::from_secs(10),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: true,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
        };
        assert_eq!(engine.support_score(&mobile_request), 60);
    }

    #[tokio::test]
    async fn test_playwright_engine_name() {
        let engine = PlaywrightEngine;
        assert_eq!(engine.name(), "playwright");
    }
}