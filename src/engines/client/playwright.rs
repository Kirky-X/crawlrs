// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#![allow(deprecated)]

use crate::engines::traits::{
    EngineError, ScrapeAction, ScrapeRequest, ScrapeResponse, ScraperEngine,
};
use crate::engines::validators;
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::{Browser, BrowserConfig};
use futures::StreamExt;
use std::sync::OnceLock;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::task_local;

task_local! {
    pub static REMOTE_URL_OVERRIDE: String;
}

// Global browser instance to avoid re-launching Chrome on every request.
// This significantly improves performance for browser-based scraping.
// Changed to Arc<Mutex<Option<Arc<Browser>>>> to allow resetting in tests
static BROWSER_INSTANCE: OnceLock<Arc<Mutex<Option<Arc<Browser>>>>> = OnceLock::new();

// Maximum number of recovery attempts
const MAX_RECOVERY_ATTEMPTS: u32 = 3;

// Asynchronously gets or initializes the shared browser instance.
// This function ensures that the browser is launched only once.
pub async fn get_browser() -> Result<Arc<Browser>, EngineError> {
    get_browser_with_recovery(MAX_RECOVERY_ATTEMPTS).await
}

/// Gets or creates browser with automatic recovery on failure
async fn get_browser_with_recovery(max_attempts: u32) -> Result<Arc<Browser>, EngineError> {
    let mut attempts = 0;
    loop {
        attempts += 1;

        match get_or_init_browser().await {
            Ok(browser) => return Ok(browser),
            Err(e) if attempts < max_attempts => {
                tracing::warn!(
                    "Browser initialization attempt {} failed: {}, retrying...",
                    attempts,
                    e
                );
                cleanup_browser().await;
                tokio::time::sleep(Duration::from_secs(1)).await;
            }
            Err(e) => return Err(e),
        }
    }
}

/// Internal function to get or initialize browser
async fn get_or_init_browser() -> Result<Arc<Browser>, EngineError> {
    // Check if we're in test mode and should not reuse browser
    let test_mode = std::env::var("CRAWLRS_TEST_NO_BROWSER_REUSE").is_ok();

    // Get or initialize the browser instance
    let browser_instance = BROWSER_INSTANCE.get_or_init(|| Arc::new(Mutex::new(None)));

    // Try to get the existing browser (clone outside lock to avoid holding across await)
    let browser_to_check = {
        let browser_guard = browser_instance
            .lock()
            .map_err(|e| EngineError::Other(format!("Browser lock poisoned: {}", e)))?;
        browser_guard.as_ref().map(Arc::clone)
    };

    if let Some(browser) = browser_to_check {
        // Check if browser is still healthy (now outside the lock)
        if check_browser_health(&browser).await {
            // In test mode, don't reuse browser to avoid conflicts
            if !test_mode {
                return Ok(browser);
            }
        }
        // Browser is not healthy or in test mode, drop it
    }

    // Need to create a new browser
    let remote_debugging_url = REMOTE_URL_OVERRIDE
        .try_with(|url| url.clone())
        .ok()
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
        builder = builder.arg("--disable-gpu").arg("--disable-dev-shm-usage");

        Browser::launch(
            builder
                .build()
                .map_err(|e| EngineError::Other(e.to_string()))?,
        )
        .await
        .map_err(|e| EngineError::Other(e.to_string()))?
    };

    // Spawn a handler to process browser events
    tokio::spawn(async move {
        while let Some(h) = handler.next().await {
            // Continue processing even if there's an error
            // This prevents the handler from stopping unexpectedly
            if let Err(e) = h {
                tracing::debug!("Browser handler event error (continuing): {:?}", e);
            }
        }
    });

    let browser = Arc::new(browser);

    // Store the browser in the global instance
    {
        let mut browser_guard = browser_instance
            .lock()
            .map_err(|e| EngineError::Other(format!("Browser lock poisoned: {}", e)))?;
        *browser_guard = Some(Arc::clone(&browser));
    }

    Ok(browser)
}

/// Check if browser is still healthy and can be used
pub async fn check_browser_health(browser: &Browser) -> bool {
    // Try to create a new page to check if browser is still responsive
    match browser.new_page("about:blank").await {
        Ok(page) => {
            // Close the test page
            let _ = page.close().await;
            true
        }
        Err(_) => false,
    }
}

/// Reset the global browser instance
/// Note: OnceLock cannot be reset, so this is a no-op
/// Tests should use different remote URLs or run separately
pub fn reset_browser() {
    // No-op: OnceLock cannot be reset
    // Tests should run with --test-threads=1 or use unique remote URLs
}

/// Clean up and close the global browser instance
/// This should be called when shutting down the application
pub async fn cleanup_browser() {
    let browser_instance = BROWSER_INSTANCE.get();

    if let Some(instance) = browser_instance {
        if let Ok(mut guard) = instance.lock() {
            if let Some(browser) = guard.take() {
                // Close all pages and then browser
                tracing::info!("Closing browser instance");
                drop(browser);
            }
        }
        // If lock fails during cleanup, it's best-effort anyway
    }
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
            return Err(EngineError::AllEnginesFailed(
                "PlaywrightEngine only supports JS and screenshot requests".to_string(),
            ));
        }

        let start = Instant::now();
        let timeout_duration = request.timeout;

        // Wrap the entire operation in a timeout
        tokio::time::timeout(timeout_duration, async {
            let browser = get_browser().await?;

            // Create new page and navigate
            let page = browser.new_page("about:blank").await
                .map_err(|e| EngineError::BrowserError(e.to_string()))?;

            // Note: Page is intentionally not closed here to allow for reuse.
            // Browser will be closed when application shuts down.
            // In case of errors, the Page will be dropped automatically.

            // Set user agent if mobile
            if request.mobile {
                page.set_user_agent("Mozilla/5.0 (iPhone; CPU iPhone OS 14_4 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0.3 Mobile/15E148 Safari/604.1").await
                    .map_err(|e| EngineError::BrowserError(e.to_string()))?;
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
                .map_err(|e| EngineError::BrowserError(e.to_string()))?;

            // 执行页面交互动作
            for action in &request.actions {
                match action {
                    ScrapeAction::Wait { milliseconds } => {
                        tokio::time::sleep(Duration::from_millis(*milliseconds)).await;
                    }
                    ScrapeAction::Click { selector } => {
                        page.find_element(selector)
                            .await
                            .map_err(|e| EngineError::BrowserError(format!("Click failed, element not found: {}", e)))?
                            .click()
                            .await
                            .map_err(|e| EngineError::BrowserError(format!("Click failed: {}", e)))?;
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
                            .map_err(|e| EngineError::BrowserError(format!("Scroll failed: {}", e)))?;
                    }
                    ScrapeAction::Screenshot { full_page: _ } => {
                        // 此处动作生成的截图暂不直接返回，仅作为交互过程的一部分
                        // 如果需要保存，可能需要额外的逻辑处理
                    }
                    ScrapeAction::Input { selector, text } => {
                        page.find_element(selector)
                            .await
                            .map_err(|e| EngineError::BrowserError(format!("Input failed, element not found: {}", e)))?
                            .type_str(text)
                            .await
                            .map_err(|e| EngineError::BrowserError(format!("Input failed: {}", e)))?;
                    }
                }
            }

            // 同步等待
            if request.sync_wait_ms > 0 {
                tokio::time::sleep(Duration::from_millis(request.sync_wait_ms as u64)).await;
            }

            // Get final URL after navigation (handles redirects)
            let _final_url = page.url().await
                .ok()
                .flatten()
                .unwrap_or_else(|| request.url.clone());

            // Try to get content-type from document properties
            let content_type = page.evaluate(r#"
                () => document.contentType || document.querySelector('meta[http-equiv="content-type"]')?.getAttribute('content') || 'text/html'
            "#).await
                .map_err(|e| EngineError::BrowserError(e.to_string()))?
                .into_value::<String>()
                .unwrap_or_else(|_| "text/html".to_string())
                .split(';')
                .next()
                .unwrap_or("text/html")
                .trim()
                .to_string();

            // Use 200 as default - getting exact status from browser JS is unreliable
            // For most scraping use cases, 200 is the expected success status
            let status_code = 200;

            let content = page.content().await
                .map_err(|e| EngineError::BrowserError(e.to_string()))?;

            // Build headers from available document information
            let response_headers = {
                let mut headers = std::collections::HashMap::new();
                headers.insert("Content-Type".to_string(), content_type.clone());
                headers
            };

            // Handle screenshot if requested
            let mut screenshot: Option<String> = None;

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
                        .map_err(|e| EngineError::BrowserError(format!("Element not found: {}", e)))?;

                    // Create new format instance for element screenshot since original was moved
                    let element_format = match config.format.as_deref() {
                        Some("png") => CaptureScreenshotFormat::Png,
                        _ => CaptureScreenshotFormat::Jpeg,
                    };

                    element.screenshot(element_format).await
                        .map_err(|e| EngineError::BrowserError(format!("Element screenshot failed: {}", e)))?
                } else {
                    // Page screenshot
                    page.screenshot(params).await
                        .map_err(|e| EngineError::BrowserError(format!("Page screenshot failed: {}", e)))?
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
            .map_err(|_| EngineError::Timeout(timeout_duration))?
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

    // 覆盖能力方法 - Playwright 不专门优化 TLS 指纹

    fn supports_tls_fingerprint(&self) -> bool {
        false
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
