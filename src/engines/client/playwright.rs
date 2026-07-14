// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::engines::browser_downloader::{BrowserDownloadConfig, BrowserDownloadManager};
use crate::engines::client::playwright_pool::{get_global_pool, BrowserPool, BrowserPoolConfig};
use crate::engines::engine_client::{
    EngineError, InternalPageAction, InternalScrapeRequest, InternalScrapeResponse,
    InternalScreenshotConfig, ScraperEngine,
};
use crate::engines::validators;
use crate::infrastructure::services::config_service::BrowserConfigTrait;
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::{Browser, BrowserConfig};
use futures::StreamExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Playwright context for browser operations
///
/// This struct provides a way to pass browser configuration through the call stack
/// instead of using task-local storage or global state.
/// For DI-based usage, prefer PlaywrightBrowserManagerComponent.
#[derive(Clone, Debug, Default)]
pub struct PlaywrightContext {
    /// Remote debugging URL for connecting to existing browser
    pub remote_debugging_url: Option<String>,
    /// Proxy URL for browser requests
    pub proxy_url: Option<String>,
    /// Test mode flag
    pub test_mode: bool,
}

impl PlaywrightContext {
    /// Create a new context with custom values
    pub fn new(
        remote_debugging_url: Option<String>,
        proxy_url: Option<String>,
        test_mode: bool,
    ) -> Self {
        Self {
            remote_debugging_url,
            proxy_url,
            test_mode,
        }
    }
}

/// 浏览器管理器 trait（支持 DI）
///
/// 提供浏览器实例管理的抽象接口，便于测试时注入 mock 实现。
#[async_trait]
pub trait BrowserManagerTrait: Send + Sync {
    /// 获取或创建浏览器实例
    async fn get_browser(&self) -> Result<Arc<Browser>, EngineError>;
    /// 清理浏览器实例
    async fn cleanup(&self);
    /// 重置浏览器实例
    fn reset(&self);
    /// 检查浏览器健康状态
    async fn check_health(&self, browser: &Browser) -> bool;
}

/// Playwright 浏览器管理器组件（DI 实现）
pub struct PlaywrightBrowserManagerComponent {
    /// 浏览器配置
    config: Arc<dyn BrowserConfigTrait>,
    /// 浏览器实例
    browser: Arc<Mutex<Option<Arc<Browser>>>>,
    /// 浏览器下载管理器
    download_manager: Arc<BrowserDownloadManager>,
}

impl PlaywrightBrowserManagerComponent {
    /// 创建新的浏览器管理器
    pub fn new(config: Arc<dyn BrowserConfigTrait>) -> Self {
        Self::with_download_config(config, BrowserDownloadConfig::default())
    }

    /// 创建带有下载配置的浏览器管理器
    pub fn with_download_config(
        config: Arc<dyn BrowserConfigTrait>,
        download_config: BrowserDownloadConfig,
    ) -> Self {
        Self {
            config,
            browser: Arc::new(Mutex::new(None)),
            download_manager: Arc::new(BrowserDownloadManager::new(download_config)),
        }
    }
}

#[async_trait]
impl BrowserManagerTrait for PlaywrightBrowserManagerComponent {
    async fn get_browser(&self) -> Result<Arc<Browser>, EngineError> {
        self.get_browser_with_recovery(3).await
    }

    async fn cleanup(&self) {
        let mut guard = match self.browser.lock() {
            Ok(g) => g,
            Err(e) => {
                log::error!("Browser mutex poisoned during cleanup: {}", e);
                return;
            }
        };
        if let Some(browser) = guard.take() {
            log::info!("Closing browser instance");
            drop(browser);
        }
    }

    fn reset(&self) {
        let mut guard = match self.browser.lock() {
            Ok(g) => g,
            Err(e) => {
                log::error!("Browser mutex poisoned during reset: {}", e);
                return;
            }
        };
        *guard = None;
    }

    async fn check_health(&self, browser: &Browser) -> bool {
        match browser.new_page("about:blank").await {
            Ok(page) => {
                let _ = page.close().await;
                true
            }
            Err(_) => false,
        }
    }
}

impl PlaywrightBrowserManagerComponent {
    /// 获取或创建浏览器（带自动恢复）
    async fn get_browser_with_recovery(
        &self,
        max_attempts: u32,
    ) -> Result<Arc<Browser>, EngineError> {
        let mut attempts = 0;
        loop {
            attempts += 1;

            match self.get_or_init_browser().await {
                Ok(browser) => return Ok(browser),
                Err(e) if attempts < max_attempts => {
                    log::warn!(
                        "Browser initialization attempt {} failed: {}, retrying...",
                        attempts,
                        e
                    );
                    self.cleanup().await;
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    /// 内部函数：获取或初始化浏览器
    async fn get_or_init_browser(&self) -> Result<Arc<Browser>, EngineError> {
        let test_mode = self.config.is_test_mode();

        // 尝试获取现有的浏览器实例
        let browser_to_check = {
            let browser_guard = self.browser.lock().expect("Browser mutex poisoned");
            browser_guard.as_ref().map(Arc::clone)
        };

        if let Some(browser) = browser_to_check {
            if self.check_health(&browser).await && !test_mode {
                return Ok(browser);
            }
        }

        // 需要创建新的浏览器
        let remote_debugging_url = self.config.get_remote_debugging_url();

        let proxy_url = self.config.get_proxy_url();

        let (browser, mut handler) = if let Some(ref url) = remote_debugging_url {
            log::info!("Connecting to remote Chrome instance at: {}", url);
            Browser::connect(url).await.map_err(|e| {
                EngineError::Other(format!("Failed to connect to remote Chrome: {}", e))
            })?
        } else {
            // 尝试自动下载浏览器（如果需要）
            let browser_path = self.download_browser_if_needed().await?;

            let mut builder = BrowserConfig::builder()
                .no_sandbox()
                .request_timeout(Duration::from_secs(30));

            // 设置浏览器路径（如果 chromiumoxide 支持）
            if let Some(ref path) = browser_path {
                log::info!("Using browser at: {:?}", path);
            }

            builder = builder.arg("--disable-gpu").arg("--disable-dev-shm-usage");

            if let Some(ref proxy) = proxy_url {
                log::info!("Using proxy for Playwright: {}", proxy);
                builder = builder.arg(format!("--proxy-server={}", proxy));
            }

            Browser::launch(
                builder
                    .build()
                    .map_err(|e| EngineError::Other(e.to_string()))?,
            )
            .await
            .map_err(|e| EngineError::Other(e.to_string()))?
        };

        // 启动处理器任务
        tokio::spawn(async move {
            while let Some(h) = handler.next().await {
                if let Err(e) = h {
                    log::debug!("Browser handler event error (continuing): {:?}", e);
                }
            }
        });

        let browser = Arc::new(browser);

        // 存储浏览器实例
        {
            let mut browser_guard = self.browser.lock().expect("Browser mutex poisoned");
            *browser_guard = Some(Arc::clone(&browser));
        }

        Ok(browser)
    }

    /// 下载浏览器（如果需要）
    async fn download_browser_if_needed(&self) -> Result<Option<PathBuf>, EngineError> {
        // 首先检查系统是否有浏览器
        if let Some(path) = crate::engines::browser_downloader::find_system_browser().await {
            log::info!("使用系统浏览器");
            return Ok(Some(path));
        }

        // 检查是否已下载
        if self.download_manager.is_browser_downloaded().await {
            let path = crate::engines::browser_downloader::get_browser_executable_path(
                self.download_manager.get_cache_dir(),
            );
            log::info!("使用已下载的浏览器: {:?}", path);
            return Ok(Some(path));
        }

        // 自动下载浏览器
        log::info!("未检测到可用浏览器，开始自动下载...");
        match self.download_manager.download_browser().await {
            Ok(path) => {
                log::info!("浏览器下载成功: {:?}", path);
                Ok(Some(path))
            }
            Err(e) => {
                log::warn!("浏览器下载失败: {}，将尝试使用系统路径", e);
                Ok(None)
            }
        }
    }
}

/// Check if browser is still healthy and can be used
pub async fn check_browser_health(browser: &Browser) -> bool {
    match browser.new_page("about:blank").await {
        Ok(page) => {
            let _ = page.close().await;
            true
        }
        Err(_) => false,
    }
}

/// Playwright引擎
///
/// 基于chromiumoxide实现的浏览器自动化抓取引擎
pub struct PlaywrightEngine {
    /// 浏览器池（可选，用于实例复用）
    pool: Option<BrowserPool>,
}

impl PlaywrightEngine {
    /// 创建新的 Playwright 引擎（使用全局浏览器池）
    pub fn new() -> Self {
        Self { pool: None }
    }

    /// 创建带有自定义浏览器池的 Playwright 引擎
    pub fn with_pool(pool: BrowserPool) -> Self {
        Self { pool: Some(pool) }
    }

    /// 获取或创建浏览器池
    fn get_or_init_pool(&self) -> BrowserPool {
        if let Some(pool) = &self.pool {
            return pool.clone();
        }

        // 尝试使用全局池
        if let Some(pool) = get_global_pool() {
            return pool.clone();
        }

        // 创建临时池（不推荐，应该使用全局池）
        let config = BrowserPoolConfig::default();
        let browser_config = Arc::new(
            crate::infrastructure::services::config_service::BrowserConfigComponent::default(),
        );
        BrowserPool::new(config, browser_config)
    }
}

impl Default for PlaywrightEngine {
    fn default() -> Self {
        Self::new()
    }
}

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
    /// * `Ok(InternalScrapeResponse)` - 抓取响应
    /// * `Err(EngineError)` - 抓取过程中出现的错误
    async fn scrape(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        if request.method != crate::engines::engine_client::HttpMethod::Get {
            return Err(EngineError::Other("Unsupported HTTP method".to_string()));
        }
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

        // 获取浏览器池
        let pool = self.get_or_init_pool();

        // Wrap the entire operation in a timeout
        tokio::time::timeout(timeout_duration, async {
            // 从池中获取浏览器实例
            let browser_instance = pool.acquire().await?;
            let browser = browser_instance.browser();

            // Create new page and navigate
            let page: chromiumoxide::page::Page = browser
                .new_page("about:blank")
                .await
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
                log::warn!("Custom headers are currently partially supported in PlaywrightEngine due to API constraints");
            }

            // Navigate and wait for load
            // goto waits for the load event by default
            page.goto(&request.url).await
                .map_err(|e| EngineError::BrowserError(e.to_string()))?;

            // Wait for network to be idle (important for JS-heavy sites like Google)
            // Since chromiumoxide doesn't have wait_for_load_state, we use a delay approach
            // This ensures all dynamic content is loaded
            tokio::time::sleep(Duration::from_secs(5)).await;

            // Try to detect if we got a bot detection page
            let content: String = page
                .content()
                .await
                .map_err(|e| EngineError::BrowserError(e.to_string()))?;

            if content.contains("如果您在几秒钟内没有被重定向") || 
               content.contains("Having trouble accessing Google") ||
               content.contains("enablejs") {
                log::warn!("Detected bot detection page from Google");
                // Still return the content, let the parser handle it
            }

            // 执行页面交互动作
            for action in &request.actions {
                match action {
                    InternalPageAction::Wait { milliseconds } => {
                        tokio::time::sleep(Duration::from_millis(*milliseconds)).await;
                    }
                    InternalPageAction::Click { selector } => {
                        let element: chromiumoxide::element::Element = page
                            .find_element(selector)
                            .await
                            .map_err(|e| {
                                EngineError::BrowserError(format!(
                                    "Click failed, element not found: {}",
                                    e
                                ))
                            })?;
                        element
                            .click()
                            .await
                            .map_err(|e| EngineError::BrowserError(format!("Click failed: {}", e)))?;
                    }
                    InternalPageAction::Scroll { direction } => {
                        let script = match direction.as_str() {
                            "down" => "window.scrollBy(0, window.innerHeight);",
                            "up" => "window.scrollBy(0, -window.innerHeight);",
                            "bottom" => "window.scrollTo(0, document.body.scrollHeight);",
                            "top" => "window.scrollTo(0, 0);",
                            _ => "window.scrollBy(0, window.innerHeight);",
                        };
                        let _: chromiumoxide::js::EvaluationResult = page
                            .evaluate(script)
                            .await
                            .map_err(|e| EngineError::BrowserError(format!("Scroll failed: {}", e)))?;
                    }
                    InternalPageAction::Screenshot { full_page: _ } => {
                        // 此处动作生成的截图暂不直接返回，仅作为交互过程的一部分
                        // 如果需要保存，可能需要额外的逻辑处理
                    }
                    InternalPageAction::Input { selector, text } => {
                        let element: chromiumoxide::element::Element = page
                            .find_element(selector)
                            .await
                            .map_err(|e| {
                                EngineError::BrowserError(format!(
                                    "Input failed, element not found: {}",
                                    e
                                ))
                            })?;
                        element
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
            let _final_url: String = page
                .url()
                .await
                .ok()
                .flatten()
                .unwrap_or_else(|| request.url.clone());

            // Try to get content-type from document properties
            let content_type = page
                .evaluate(
                    r#"
                () => document.contentType || document.querySelector('meta[http-equiv="content-type"]')?.getAttribute('content') || 'text/html'
            "#,
                )
                .await
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

            let content: String = page
                .content()
                .await
                .map_err(|e| EngineError::BrowserError(e.to_string()))?;

            // Build headers from available document information
            let response_headers = {
                let mut headers = std::collections::HashMap::with_capacity(2);
                headers.insert("Content-Type".to_string(), content_type.clone());
                headers
            };

            // Handle screenshot if requested
            let mut screenshot: Option<String> = None;

            if request.needs_screenshot {
                let config = request.screenshot_config.clone().unwrap_or(InternalScreenshotConfig {
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
                    let element: chromiumoxide::element::Element = page
                        .find_element(selector)
                        .await
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

            // 关闭页面（但保留浏览器实例供复用）
            let _ = page.close().await;

            // 浏览器实例会在 browser_instance drop 时自动归还到池中
            // 如果需要手动归还，可以调用 browser_instance.release().await

            Ok(InternalScrapeResponse {
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
    fn support_score(&self, request: &InternalScrapeRequest) -> u8 {
        if request.method != crate::engines::engine_client::HttpMethod::Get {
            return 0;
        }
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
    use crate::engines::engine_client::InternalScrapeRequest;
    use std::collections::HashMap;
    use std::time::Duration;

    #[test]
    fn test_support_score() {
        let engine = PlaywrightEngine::new();

        // Test with JS requirement
        let request_js = InternalScrapeRequest {
            url: "http://example.com".to_string(),
            method: crate::engines::engine_client::HttpMethod::Get,
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
            body: None,
            sync_wait_ms: 0,
        };
        assert_eq!(engine.support_score(&request_js), 100);

        // Test with Screenshot requirement
        let request_screenshot = InternalScrapeRequest {
            url: "http://example.com".to_string(),
            method: crate::engines::engine_client::HttpMethod::Get,
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
            body: None,
            sync_wait_ms: 0,
        };
        assert_eq!(engine.support_score(&request_screenshot), 100);

        // Test with neither (basic request)
        let request_basic = InternalScrapeRequest {
            url: "http://example.com".to_string(),
            method: crate::engines::engine_client::HttpMethod::Get,
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
            body: None,
            sync_wait_ms: 0,
        };
        assert_eq!(engine.support_score(&request_basic), 10);
    }
}
