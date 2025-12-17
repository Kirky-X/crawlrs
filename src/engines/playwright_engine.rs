// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::engines::traits::{EngineError, ScrapeRequest, ScrapeResponse, ScraperEngine};
use crate::engines::validators;
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::{Browser, BrowserConfig};
use futures::StreamExt;
use std::time::Instant;

/// Playwright引擎
///
/// 基于chromiumoxide实现的浏览器自动化抓取引擎
pub struct PlaywrightEngine; // Using chromiumoxide as Rust alternative to Playwright

#[async_trait]
impl ScraperEngine for PlaywrightEngine {
    /// 执行浏览器自动化抓取
    ///
    /// # 参数
    ///
    /// * `request` - 抓取请求
    ///
    /// # 返回值
    ///
    /// * `Ok(ScrapeResponse)` - 抓取响应
    /// * `Err(EngineError)` - 抓取过程中出现的错误
    async fn scrape(&self, request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
        // SSRF protection
        validators::validate_url(&request.url)
            .await
            .map_err(|e| EngineError::Other(format!("SSRF protection: {}", e)))?;

        // Only run if specifically requested for JS or screenshot
        if !request.needs_js && !request.needs_screenshot {
            return Err(EngineError::AllEnginesFailed);
        }

        let start = Instant::now();
        let timeout_duration = request.timeout;

        // Wrap the entire operation in a timeout
        tokio::time::timeout(timeout_duration, async {
            // Check if we should connect to a remote Chrome instance
            let remote_debugging_url = std::env::var("CHROMIUM_REMOTE_DEBUGGING_URL").ok();
            let (mut browser, mut handler) = if let Some(ref url) = remote_debugging_url {
                // Connect to existing Chrome instance via remote debugging
                tracing::info!("Connecting to remote Chrome instance at: {}", url);
                Browser::connect(url)
                    .await
                    .map_err(|e| EngineError::Other(format!("Failed to connect to remote Chrome: {}", e)))?
            } else {
                // Launch new Chrome instance
                // Configure browser for production/container environment
                let mut builder = BrowserConfig::builder();
                builder = builder.no_sandbox()
                    .request_timeout(timeout_duration);

                // Handle proxy (Chromiumoxide uses args for proxy)
                if let Some(proxy_url) = &request.proxy {
                    builder = builder.arg(format!("--proxy-server={}", proxy_url));
                }

                // Handle TLS verification
                if request.skip_tls_verification {
                    builder = builder
                        .arg("--ignore-certificate-errors")
                        .arg("--allow-insecure-localhost");
                }

                // Set window size if mobile
                if request.mobile {
                    // Default to iPhone 12 Pro dimensions
                    builder = builder.viewport(chromiumoxide::handler::viewport::Viewport {
                        width: 390,
                        height: 844,
                        device_scale_factor: Some(3.0),
                        emulating_mobile: true,
                        is_landscape: false,
                        has_touch: true,
                    });
                } else {
                    // Desktop viewport
                    builder = builder.viewport(chromiumoxide::handler::viewport::Viewport {
                        width: 1920,
                        height: 1080,
                        device_scale_factor: Some(1.0),
                        emulating_mobile: false,
                        is_landscape: true,
                        has_touch: false,
                    });
                }

                Browser::launch(builder.build().map_err(|e| EngineError::Other(e.to_string()))?)
                    .await
                    .map_err(|e| EngineError::Other(e.to_string()))?
            };

            // Spawn handler
            let handle = tokio::spawn(async move {
                while let Some(h) = handler.next().await {
                    if h.is_err() {
                        break;
                    }
                }
            });

            // Create new page and navigate
            let page = browser.new_page("about:blank").await
                .map_err(|e| EngineError::Other(e.to_string()))?;

            // Set user agent if mobile
            if request.mobile {
                page.set_user_agent("Mozilla/5.0 (iPhone; CPU iPhone OS 14_4 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0.3 Mobile/15E148 Safari/604.1").await
                    .map_err(|e| EngineError::Other(e.to_string()))?;
            }

            // Navigate and wait for load
            // goto waits for the load event by default
            page.goto(&request.url).await
                .map_err(|e| EngineError::Other(e.to_string()))?;

            // Extra wait for network idle if needed (simple delay for now as network idle is complex in some versions)
            // Or rely on goto's implicit wait.
            // For now, let's verify if we have a valid response

            let status_code = 200; // Chromiumoxide goto returns Page, not Response directly in this version pattern

            let content = page.content().await
                .map_err(|e| EngineError::Other(e.to_string()))?;

            // Handle screenshot if requested
            let mut screenshot = None;
            if request.needs_screenshot {
                let config = request.screenshot_config.clone().unwrap_or(crate::engines::traits::ScreenshotConfig {
                    full_page: true,
                    selector: None,
                    quality: Some(80),
                    format: Some("jpeg".to_string()),
                });

                let format = match config.format.as_deref() {
                    Some("png") => CaptureScreenshotFormat::Png,
                    _ => CaptureScreenshotFormat::Jpeg,
                };

                let params = chromiumoxide::page::ScreenshotParams::builder()
                    .format(format)
                    .quality(config.quality.unwrap_or(80) as i64)
                    .full_page(config.full_page)
                    .build();

                let screenshot_bytes = if let Some(selector) = &config.selector {
                    // Find element and screenshot
                    let element = page.find_element(selector).await
                        .map_err(|e| EngineError::Other(format!("Element not found: {}", e)))?;

                    // Create new format instance for element screenshot since original was moved
                    let element_format = match config.format.as_deref() {
                        Some("png") => CaptureScreenshotFormat::Png,
                        _ => CaptureScreenshotFormat::Jpeg,
                    };

                    element.screenshot(element_format).await
                        .map_err(|e| EngineError::Other(format!("Element screenshot failed: {}", e)))?
                } else {
                    // Page screenshot
                    page.screenshot(params).await
                        .map_err(|e| EngineError::Other(format!("Page screenshot failed: {}", e)))?
                };

                screenshot = Some(BASE64.encode(screenshot_bytes));
            }

            browser.close().await
                .map_err(|e| EngineError::Other(e.to_string()))?;

            handle.await.ok();

            Ok(ScrapeResponse {
                status_code,
                content,
                screenshot,
                content_type: "text/html".to_string(),
                headers: std::collections::HashMap::new(), // Playwright headers not implemented yet
                response_time_ms: start.elapsed().as_millis() as u64,
            })
        })
            .await
            .map_err(|_| EngineError::Timeout)?
    }

    /// 计算对请求的支持分数
    ///
    /// # 参数
    ///
    /// * `request` - 抓取请求
    ///
    /// # 返回值
    ///
    /// 支持分数（0-100），需要JS或截图的请求返回100分
    fn support_score(&self, request: &ScrapeRequest) -> u8 {
        if request.needs_js || request.needs_screenshot {
            return 100;
        }
        10 // Can do it, but expensive
    }

    /// 获取引擎名称
    ///
    /// # 返回值
    ///
    /// 引擎名称
    fn name(&self) -> &'static str {
        "playwright"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::time::Duration;

    #[test]
    fn test_support_score() {
        let engine = PlaywrightEngine;

        // Test with JS requirement
        let request_js = ScrapeRequest {
            url: "http://example.com".to_string(),
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: true,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
        };
        assert_eq!(engine.support_score(&request_js), 100);

        // Test with Screenshot requirement
        let request_screenshot = ScrapeRequest {
            url: "http://example.com".to_string(),
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: false,
            needs_screenshot: true,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
        };
        assert_eq!(engine.support_score(&request_screenshot), 100);

        // Test with neither (basic request)
        let request_basic = ScrapeRequest {
            url: "http://example.com".to_string(),
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
        };
        assert_eq!(engine.support_score(&request_basic), 10);
    }
}
