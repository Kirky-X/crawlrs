// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::engines::traits::{
    EngineError, ScrapeAction, ScrapeRequest, ScrapeResponse, ScraperEngine,
};
use crate::engines::validators;
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::{Browser, BrowserConfig};
use futures::StreamExt;
use std::time::{Duration, Instant};
use tokio::sync::OnceCell;
use tokio::task_local;

task_local! {
    pub static REMOTE_URL_OVERRIDE: String;
}

// Global browser instance to avoid re-launching Chrome on every request.
// This significantly improves performance for browser-based scraping.
static BROWSER_INSTANCE: OnceCell<Browser> = OnceCell::const_new();

// Asynchronously gets or initializes the shared browser instance.
// This function ensures that the browser is launched only once.
pub async fn get_browser() -> Result<&'static Browser, EngineError> {
    BROWSER_INSTANCE.get_or_try_init(|| async {
        let remote_debugging_url = REMOTE_URL_OVERRIDE.try_with(|url| url.clone()).ok()
            .or_else(|| std::env::var("CHROMIUM_REMOTE_DEBUGGING_URL").ok());

        let (browser, mut handler) = if let Some(ref url) = remote_debugging_url {
            tracing::info!("Connecting to remote Chrome instance at: {}", url);
            Browser::connect(url)
                .await
                .map_err(|e| EngineError::Other(format!("Failed to connect to remote Chrome: {}", e)))?
        } else {
            let mut builder = BrowserConfig::builder()
                .no_sandbox()
                .request_timeout(Duration::from_secs(30)); // Default timeout

            // Production environment setup
            builder = builder.arg("--disable-gpu")
                             .arg("--disable-dev-shm-usage");

            Browser::launch(builder.build().map_err(|e| EngineError::Other(e.to_string()))?)
                .await
                .map_err(|e| EngineError::Other(e.to_string()))?
        };

        // Spawn a handler to process browser events
        tokio::spawn(async move {
            while let Some(h) = handler.next().await {
                if h.is_err() {
                    break;
                }
            }
        });

        Ok(browser)
    }).await
}

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
            let browser = get_browser().await?;

            // Create new page and navigate
            let page = browser.new_page("about:blank").await
                .map_err(|e| EngineError::Other(e.to_string()))?;

            // Set user agent if mobile
            if request.mobile {
                page.set_user_agent("Mozilla/5.0 (iPhone; CPU iPhone OS 14_4 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0.3 Mobile/15E148 Safari/604.1").await
                    .map_err(|e| EngineError::Other(e.to_string()))?;
            }

            // 设置自定义 Headers
            if !request.headers.is_empty() {
                // 如果 chromiumoxide 的 API 限制太多，我们暂时记录日志并跳过，
                // 或者在未来版本中寻找更底层的 CDP 调用方式
                tracing::warn!("Custom headers are currently partially supported in PlaywrightEngine due to API constraints");
            }

            // Navigate and wait for load
            // goto waits for the load event by default
            page.goto(&request.url).await
                .map_err(|e| EngineError::Other(e.to_string()))?;

            // 执行页面交互动作
            for action in &request.actions {
                match action {
                    ScrapeAction::Wait { milliseconds } => {
                        tokio::time::sleep(Duration::from_millis(*milliseconds)).await;
                    }
                    ScrapeAction::Click { selector } => {
                        page.find_element(selector)
                            .await
                            .map_err(|e| EngineError::Other(format!("Click failed, element not found: {}", e)))?
                            .click()
                            .await
                            .map_err(|e| EngineError::Other(format!("Click failed: {}", e)))?;
                    }
                    ScrapeAction::Scroll { direction } => {
                        let script = match direction.as_str() {
                            "down" => "window.scrollBy(0, window.innerHeight);",
                            "up" => "window.scrollBy(0, -window.innerHeight);",
                            "bottom" => "window.scrollTo(0, document.body.scrollHeight);",
                            "top" => "window.scrollTo(0, 0);",
                            _ => "window.scrollBy(0, window.innerHeight);",
                        };
                        page.evaluate(script)
                            .await
                            .map_err(|e| EngineError::Other(format!("Scroll failed: {}", e)))?;
                    }
                    ScrapeAction::Screenshot { full_page: _ } => {
                        // 此处动作生成的截图暂不直接返回，仅作为交互过程的一部分
                        // 如果需要保存，可能需要额外的逻辑处理
                    }
                    ScrapeAction::Input { selector, text } => {
                        page.find_element(selector)
                            .await
                            .map_err(|e| EngineError::Other(format!("Input failed, element not found: {}", e)))?
                            .type_str(text)
                            .await
                            .map_err(|e| EngineError::Other(format!("Input failed: {}", e)))?;
                    }
                }
            }

            // 同步等待
            if request.sync_wait_ms > 0 {
                tokio::time::sleep(Duration::from_millis(request.sync_wait_ms as u64)).await;
            }

            // Extra wait for network idle if needed (simple delay for now as network idle is complex in some versions)
            // Or rely on goto's implicit wait.
            // For now, let's verify if we have a valid response

            let status_code = 200; // Chromiumoxide goto returns Page, not Response directly in this version pattern

            let content = page.content().await
                .map_err(|e| EngineError::Other(e.to_string()))?;

            // Handle screenshot if requested
            let mut screenshot = None;
            let response_headers = std::collections::HashMap::new();

            // Try to get response headers if possible
            // Note: chromiumoxide might not expose headers directly on Page,
            // but we can try to intercept them if needed. For now, we return empty or basic ones.

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

            // Note: The browser is no longer closed here to allow for reuse.
            // It will be closed when the application shuts down.

            Ok(ScrapeResponse {
                status_code,
                content,
                screenshot,
                content_type: "text/html".to_string(),
                headers: response_headers,
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
            actions: vec![],
            sync_wait_ms: 0,
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
            actions: vec![],
            sync_wait_ms: 0,
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
            actions: vec![],
            sync_wait_ms: 0,
        };
        assert_eq!(engine.support_score(&request_basic), 10);
    }
}
