// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

/// Fire Engine TLS测试模块
///
/// 测试Fire Engine TLS引擎的功能和评分机制
/// 验证TLS指纹对抗引擎的正确性

#[cfg(test)]
mod tests {
    use crawlrs::engines::fire_engine_tls::FireEngineTls;
    use crawlrs::engines::traits::{ScrapeRequest, ScraperEngine};
    use std::collections::HashMap;
    use std::time::Duration;

    #[tokio::test]
    async fn test_fire_engine_tls_support_score() {
        let engine = FireEngineTls::new();

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

        // 需要截图的请求应该获得0分（完全不支持）
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
        assert_eq!(engine.support_score(&screenshot_request), 0);

        // 需要TLS指纹的请求应该获得最高分数
        let tls_request = ScrapeRequest {
            url: "https://example.com".to_string(),
            headers: HashMap::new(),
            timeout: Duration::from_secs(10),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: true,
            use_fire_engine: false,
        };
        assert_eq!(engine.support_score(&tls_request), 100);
    }

    #[tokio::test]
    async fn test_fire_engine_tls_name() {
        let engine = FireEngineTls::new();
        assert_eq!(engine.name(), "fire_engine_tls");
    }
}