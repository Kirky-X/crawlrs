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
use crate::engines::intercept::{InterceptController, ResourceKind, BLOCK_REASON};
use crate::engines::validators;
use crate::infrastructure::services::config_service::BrowserConfigTrait;
use crate::utils::proxy::{redact_proxy_url, validate_proxy_url};
use crate::utils::ua_pool::UaPool;
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chromiumoxide::cdp::browser_protocol::emulation::SetDeviceMetricsOverrideParams;
use chromiumoxide::cdp::browser_protocol::fetch::{
    ContinueRequestParams, EnableParams, EventRequestPaused, FailRequestParams,
};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::{Browser, BrowserConfig};
use futures::StreamExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// PlaywrightEngine 默认 MRT（30 秒，对应 `EngineTimeoutSettings::cdp_seconds`）。
///
/// design.md §14 / T060：CDP/浏览器引擎涉及完整浏览器启动 + JS 渲染，
/// 30 秒覆盖绝大多数页面（含 network idle 等待）。生产环境应通过
/// [`PlaywrightEngine::with_mrt`] 从 `Settings.timeouts.engines.cdp_seconds` 注入。
const DEFAULT_PLAYWRIGHT_MRT_SECONDS: u64 = 30;

/// `WaitFor::wait` 的超时上限（T069，R-jsrender-004）
///
/// `Selector` / `DomStable` 模式轮询直到满足条件，需要一个上限防止无限阻塞。
/// 取 `min(request.timeout, WAIT_TIMEOUT_CAP)` 确保不会用尽整个请求超时预算，
/// 为后续操作（screenshot 等）留出时间。`NetworkIdle` 模式只 sleep 500ms，不受此限制。
const WAIT_TIMEOUT_CAP: Duration = Duration::from_secs(10);

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
                builder = builder.chrome_executable(path);
            }

            builder = builder.arg("--disable-gpu").arg("--disable-dev-shm-usage");

            if let Some(ref proxy) = proxy_url {
                // 安全审查 H-1：严格校验 proxy URL 防止命令行参数注入
                //
                // 原漏洞：`format!("--proxy-server={}", proxy)` 若 proxy 含空格或特殊字符，
                // Chrome 可能解析为多个 argv（如 "http://x --enable-bad-flag" 被拆为
                // `--proxy-server=http://x` + `--enable-bad-flag`）。
                //
                // 修复：
                // 1. `validate_proxy_url` 严格校验 URL 格式 + scheme 白名单 + 无空白字符
                // 2. `arg("--proxy-server").arg(validated)` 分离传递 flag 与值，
                //    从根本上消除单字符串拼接导致的 argv 拆分风险
                // 3. `redact_proxy_url` 脱敏日志输出，避免 user:pass 凭证泄露
                const ALLOWED_PROXY_SCHEMES: &[&str] = &["http", "https", "socks5", "socks4"];
                let validated = validate_proxy_url(proxy, ALLOWED_PROXY_SCHEMES)
                    .map_err(|e| EngineError::Other(format!("Invalid proxy URL: {}", e)))?;
                log::info!(
                    "Using proxy for Playwright: {}",
                    redact_proxy_url(&validated)
                );
                builder = builder.arg("--proxy-server").arg(validated);
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
            log::info!("Using system browser");
            return Ok(Some(path));
        }

        // 检查是否已下载
        if self.download_manager.is_browser_downloaded().await {
            let path = crate::engines::browser_downloader::get_browser_executable_path(
                self.download_manager.get_cache_dir(),
            );
            log::info!("Using downloaded browser: {:?}", path);
            return Ok(Some(path));
        }

        // 自动下载浏览器
        log::info!("No browser detected, starting auto-download...");
        match self.download_manager.download_browser().await {
            Ok(path) => {
                log::info!("Browser downloaded successfully: {:?}", path);
                Ok(Some(path))
            }
            Err(e) => {
                log::warn!(
                    "Browser download failed: {}, falling back to system path",
                    e
                );
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
    /// UA 池（R-identity-001）：每次请求从池中选取一致的 UA + viewport
    ua_pool: UaPool,
    /// 单引擎最大响应时间（MRT，design.md §14 / T060）。
    ///
    /// router 顺序 fallback 路径用 `min(remaining, mrt)` 包裹单引擎调用，
    /// 超 MRT 即切下一引擎。注入自 `Settings.timeouts.engines.cdp_seconds`（默认 30 秒）。
    mrt: Duration,
}

impl PlaywrightEngine {
    /// 创建新的 Playwright 引擎（使用全局浏览器池）
    pub fn new() -> Self {
        Self::with_mrt(Duration::from_secs(DEFAULT_PLAYWRIGHT_MRT_SECONDS))
    }

    /// 创建带 MRT 配置的 Playwright 引擎（T060/T061）。
    ///
    /// 生产环境应从 `settings.timeouts.engines.cdp_seconds` 注入 `mrt`。
    #[must_use]
    pub fn with_mrt(mrt: Duration) -> Self {
        Self {
            pool: None,
            ua_pool: UaPool::default(),
            mrt,
        }
    }

    /// 创建带有自定义浏览器池的 Playwright 引擎
    pub fn with_pool(pool: BrowserPool) -> Self {
        Self::with_pool_and_mrt(pool, Duration::from_secs(DEFAULT_PLAYWRIGHT_MRT_SECONDS))
    }

    /// 创建带有自定义浏览器池 + MRT 配置的 Playwright 引擎（T060/T061）。
    ///
    /// 生产环境应从 `settings.timeouts.engines.cdp_seconds` 注入 `mrt`。
    #[must_use]
    pub fn with_pool_and_mrt(pool: BrowserPool, mrt: Duration) -> Self {
        Self {
            pool: Some(pool),
            ua_pool: UaPool::default(),
            mrt,
        }
    }

    /// 获取 UA 池引用（用于测试验证 R-identity-001）
    #[must_use]
    pub fn ua_pool(&self) -> &UaPool {
        &self.ua_pool
    }

    /// 获取引擎级 MRT（用于测试验证 T060）。
    ///
    /// 返回构造时注入的 `mrt`（默认 30 秒，对应 `cdp_seconds`）。
    #[must_use]
    pub fn mrt(&self) -> Duration {
        self.mrt
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
        // SEC-001: 使用 docker_safe_args() 确保容器环境下自动添加 --no-sandbox
        let config = BrowserPoolConfig::docker_safe_args();
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
            // T068 / R-jsrender-004：从池中获取 Browser + Page
            // Page 优先从 TabPool 复用（LIFO），池空时调用 browser.new_page
            let pooled_page = pool.acquire_page().await?;
            // Page 是 Arc-based，clone 廉价；pooled_page 持有原始 Page 用于归还
            let page: chromiumoxide::page::Page = pooled_page.page().clone();

            // Note: Page 在 pooled_page drop 时自动归还到 TabPool
            // （TabPool::release 导航到 about:blank 清理状态后压栈复用）。
            // Browser 也在 pooled_page drop 时归还到 BrowserPool。
            // 错误路径下 Page 可能不可用，TabPool::release 会 drop 它（关闭 tab）。

            // R-identity-001: 从 UaPool 取一致的 UA + viewport profile
            // 替换原固定移动 UA 分支；mobile 和 desktop 都从池中取 profile
            let profile = self.ua_pool.pick(request.mobile);

            // Set User-Agent（所有请求都设，不只 mobile）
            page.set_user_agent(profile.ua)
                .await
                .map_err(|e| EngineError::BrowserError(e.to_string()))?;

            // Set viewport to match UA platform（R-identity-001: viewport 与 UA 一致）
            // 用 CDP Emulation.setDeviceMetricsOverride 设置视口尺寸 + mobile 标志
            let viewport_params = SetDeviceMetricsOverrideParams::new(
                profile.viewport.0 as i64,
                profile.viewport.1 as i64,
                1.0,
                profile.mobile,
            );
            page.execute(viewport_params)
                .await
                .map_err(|e| {
                    EngineError::BrowserError(format!("Failed to set viewport: {}", e))
                })?;

            // 设置自定义 Headers
            if !request.headers.is_empty() {
                // 如果 chromiumoxide 的 API 限制太多，我们暂时记录日志并跳过，
                // 或者在未来版本中寻找更底层的 CDP 调用方式
                log::warn!("Custom headers are currently partially supported in PlaywrightEngine due to API constraints");
            }

            // T032 / R-jsrender-002：导航前注入 stealth 脚本（best-effort）
            // 覆盖 navigator.webdriver 等反爬指纹属性，必须在页面脚本执行前生效
            let stealth_injector = crate::engines::js_inject::JsInjector::stealth();
            if let Err(e) = stealth_injector
                .apply(&page, crate::engines::js_inject::InjectPhase::BeforeLoad)
                .await
            {
                log::warn!("Stealth injection failed (best-effort, continue): {}", e);
            }

            // T033 / R-jsrender-003：请求拦截（广告/追踪域名 + 媒体资源）
            // 仅当 block_ads 或 block_media 任一启用时激活 CDP Fetch domain 拦截。
            // 启用后所有请求被暂停，必须由事件处理 task 及时 continue/fail，
            // 否则请求会挂起直至超时。task 在 page 关闭后事件流结束自动退出。
            if request.block_ads || request.block_media {
                let controller = Arc::new(InterceptController::new(
                    request.block_ads,
                    request.block_media,
                ));

                // 启用 Fetch domain（默认拦截所有请求阶段）
                page.execute(EnableParams::default())
                    .await
                    .map_err(|e| {
                        EngineError::BrowserError(format!(
                            "Failed to enable Fetch interception: {}",
                            e
                        ))
                    })?;

                // 订阅 EventRequestPaused 事件流
                let mut events = page
                    .event_listener::<EventRequestPaused>()
                    .await
                    .map_err(|e| {
                        EngineError::BrowserError(format!(
                            "Failed to subscribe EventRequestPaused: {}",
                            e
                        ))
                    })?;

                // 克隆 page + controller 给事件处理 task
                let page_clone = page.clone();
                let controller_clone = Arc::clone(&controller);

                // 事件处理 task：对每个被暂停的请求判断是否拦截
                // 命中黑名单/媒体 → FailRequest(BlockedByClient) + 计数
                // 否则 → ContinueRequest（放行）
                //
                // H-3 重构：CDP `ResourceType` 在边界处通过 `ResourceKind::from` 转换为领域
                // `ResourceKind`，避免 InterceptController 依赖具体 CDP 实现。
                tokio::spawn(async move {
                    while let Some(event) = events.next().await {
                        let url = event.request.url.clone();
                        let kind = ResourceKind::from(event.resource_type.clone());
                        let request_id = event.request_id.clone();
                        if controller_clone.should_block(&url, Some(kind)) {
                            controller_clone.record_block();
                            if let Err(e) = page_clone
                                .execute(FailRequestParams::new(request_id, BLOCK_REASON))
                                .await
                            {
                                log::debug!("FailRequest failed for {}: {}", url, e);
                            }
                        } else if let Err(e) = page_clone
                            .execute(ContinueRequestParams::new(request_id))
                            .await
                        {
                            log::debug!("ContinueRequest failed for {}: {}", url, e);
                        }
                    }
                });
            }

            // Navigate and wait for load
            // goto waits for the load event by default
            page.goto(&request.url).await
                .map_err(|e| EngineError::BrowserError(e.to_string()))?;

            // T032 / R-jsrender-002：页面加载后注入 cleanup 脚本（best-effort）
            // 顺序：consent_popups → overlay_elements → flatten_shadow_dom（design.md §6）
            let cleanup_injector = crate::engines::js_inject::JsInjector::cleanup();
            if let Err(e) = cleanup_injector
                .apply(&page, crate::engines::js_inject::InjectPhase::AfterLoad)
                .await
            {
                log::warn!("Cleanup injection failed (best-effort, continue): {}", e);
            }

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

            // T069 / R-jsrender-004：页面加载后等待策略（替代原 sync_wait_ms 固定 sleep）
            //
            // `request.wait_for` 由调用方通过 `ScrapeOptions.wait_for` 设置；
            // `None` 时使用 `WaitFor::NetworkIdle`（与原默认等待语义一致，sleep 500ms）。
            //
            // 满足条件立即返回，超时返回 `EngineError::BrowserError`。
            // `sync_wait_ms` 不再在 Playwright 引擎中使用（保留供 FlareSolverr 等非浏览器引擎）。
            let wait_strategy = request.wait_for.clone().unwrap_or_default();
            let wait_timeout = request.timeout.min(WAIT_TIMEOUT_CAP);
            wait_strategy.wait(&page, wait_timeout).await?;

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

            // T068：不调用 page.close()，让 pooled_page drop 时归还 Page 到 TabPool
            // （TabPool::release 会导航到 about:blank 清理状态后压栈复用）
            // Browser 也在 pooled_page drop 时归还到 BrowserPool
            drop(pooled_page);

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

    /// T060：覆写 MRT，返回构造时注入的 `mrt`（默认 30 秒）。
    ///
    /// router 顺序 fallback 路径用 `min(remaining, self.mrt)` 包裹单引擎调用，
    /// 超 MRT 即切下一引擎（瀑布式）。
    fn max_response_time(&self) -> Duration {
        self.mrt
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
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
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
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
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
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
        };
        assert_eq!(engine.support_score(&request_basic), 10);
    }

    // === T022 / R-identity-001: UaPool 集成测试 ===

    #[test]
    fn test_playwright_engine_has_non_empty_ua_pool() {
        let engine = PlaywrightEngine::new();
        let pool = engine.ua_pool();
        assert!(
            pool.count(false) >= 20,
            "desktop pool must have >= 20 profiles"
        );
        assert!(
            pool.count(true) >= 20,
            "mobile pool must have >= 20 profiles"
        );
    }

    #[test]
    fn test_playwright_engine_pick_ua_mobile_vs_desktop() {
        let engine = PlaywrightEngine::new();
        let pool = engine.ua_pool();
        // mobile=true 应返回移动 profile
        for _ in 0..10 {
            let p = pool.pick(true);
            assert!(p.mobile, "pick(true) must return mobile profile");
        }
        // mobile=false 应返回桌面 profile
        for _ in 0..10 {
            let p = pool.pick(false);
            assert!(!p.mobile, "pick(false) must return desktop profile");
        }
    }

    #[test]
    fn test_playwright_engine_pick_ua_returns_varied_profiles() {
        // R-identity-001: 多次选取应返回不同 UA（随机性）
        let engine = PlaywrightEngine::new();
        let pool = engine.ua_pool();
        let mut uas = std::collections::HashSet::new();
        for _ in 0..50 {
            uas.insert(pool.pick(false).ua);
        }
        assert!(
            uas.len() >= 2,
            "multiple picks should return varied UAs, got only {} unique in 50 picks",
            uas.len()
        );
    }

    #[test]
    fn test_playwright_engine_viewport_matches_platform() {
        // R-identity-001: viewport 与 UA platform 必须一致
        // - iOS platform → viewport 宽度 ∈ [375, 1366]（iPhone/iPad 范围）
        // - Android platform → viewport 宽度 ∈ [360, 1280]
        // - Windows/macOS/Linux → viewport 宽度 >= 1024
        let engine = PlaywrightEngine::new();
        let pool = engine.ua_pool();
        for p in pool.desktop.iter().chain(pool.mobile.iter()) {
            match p.platform {
                "iOS" => {
                    assert!(
                        (375..=1366).contains(&p.viewport.0),
                        "iOS viewport width {} out of range [375, 1366]: {}",
                        p.viewport.0,
                        p.ua
                    );
                }
                "Android" => {
                    assert!(
                        (360..=1280).contains(&p.viewport.0),
                        "Android viewport width {} out of range [360, 1280]: {}",
                        p.viewport.0,
                        p.ua
                    );
                }
                "Windows" | "macOS" | "Linux" => {
                    assert!(
                        p.viewport.0 >= 1024,
                        "Desktop viewport width {} < 1024: {}",
                        p.viewport.0,
                        p.ua
                    );
                }
                _ => {
                    panic!("unknown platform: {}", p.platform);
                }
            }
        }
    }

    #[test]
    fn test_playwright_engine_ua_not_fixed_mobile_string() {
        // R-identity-001: 引擎的 mobile UA pool 应包含多个 UA
        // 不应全部等于原固定 mobile UA 字符串
        let engine = PlaywrightEngine::new();
        let pool = engine.ua_pool();
        let fixed_mobile_ua = "Mozilla/5.0 (iPhone; CPU iPhone OS 14_4 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0.3 Mobile/15E148 Safari/604.1";
        let non_fixed_count = pool
            .mobile
            .iter()
            .filter(|p| p.ua != fixed_mobile_ua)
            .count();
        assert!(
            non_fixed_count >= 20,
            "most mobile UAs should differ from the old fixed UA, got {} non-fixed",
            non_fixed_count
        );
    }

    #[test]
    fn test_playwright_engine_with_pool_has_ua_pool() {
        // 验证 with_pool 构造路径也初始化 ua_pool
        let config = BrowserPoolConfig::default();
        let browser_config = Arc::new(
            crate::infrastructure::services::config_service::BrowserConfigComponent::default(),
        );
        let pool = BrowserPool::new(config, browser_config);
        let engine = PlaywrightEngine::with_pool(pool);
        assert!(engine.ua_pool().count(false) >= 20);
        assert!(engine.ua_pool().count(true) >= 20);
    }

    #[test]
    fn test_playwright_engine_default_has_ua_pool() {
        let engine = PlaywrightEngine::default();
        assert!(engine.ua_pool().count(false) >= 20);
        assert!(engine.ua_pool().count(true) >= 20);
    }

    #[test]
    fn test_playwright_engine_pick_seeded_stable() {
        // R-identity-001: 同 seed 必须稳定返回同一 profile
        let engine = PlaywrightEngine::new();
        let pool = engine.ua_pool();
        let p1 = pool.pick_seeded(42, true);
        let p2 = pool.pick_seeded(42, true);
        assert_eq!(p1.ua, p2.ua, "same seed must return same mobile profile");
        assert_eq!(
            p1.viewport, p2.viewport,
            "same seed must return same viewport"
        );
    }
}
