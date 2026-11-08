// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

use crate::engines::browser_downloader::{BrowserDownloadConfig, BrowserDownloadManager};
use crate::engines::client::playwright_pool::{
    get_global_pool, BrowserPool, BrowserPoolConfig, PooledPage,
};
use crate::engines::client::ua_metadata::{build_ua_fingerprint, cdp_accept_language};
use crate::engines::engine_client::{
    EngineError, InternalCapturedResponse, InternalPageAction, InternalScrapeRequest,
    InternalScrapeResponse, InternalScreenshotConfig, ScraperEngine,
};
use crate::infrastructure::services::config_service::BrowserConfigTrait;
use crate::utils::proxy::{redact_proxy_url, validate_proxy_url};
use crate::utils::ua_pool::{UaPool, UaProfile};
use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use chromiumoxide::cdp::browser_protocol::emulation::{
    SetDeviceMetricsOverrideParams, SetUserAgentOverrideParams,
};
use chromiumoxide::cdp::browser_protocol::network::{
    DisableParams as NetworkDisableParams, EnableParams as NetworkEnableParams,
    EventLoadingFinished, EventResponseReceived, GetResponseBodyParams, ResourceType,
};
use chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat;
use chromiumoxide::{Browser, BrowserConfig};
use futures::StreamExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::engines::xhr_capture::{
    process_body, url_matches_capture_patterns, BodyOutcome, PendingXhr, RecordOutcome,
    XhrCaptureBuffer, XhrCaptureConfig,
};

mod interception;
mod session;

/// PlaywrightEngine 默认 MRT（30 秒，对应 `EngineTimeoutSettings::cdp_seconds`）。
///
/// CDP/浏览器引擎涉及完整浏览器启动 + JS 渲染，
/// 30 秒覆盖绝大多数页面（含 network idle 等待）。生产环境应通过
/// [`PlaywrightEngine::with_mrt`] 从 `Settings.timeouts.engines.cdp_seconds` 注入。
const DEFAULT_PLAYWRIGHT_MRT_SECONDS: u64 = 30;

/// `WaitFor::wait` 的超时上限
///
/// `Selector` / `DomStable` 模式轮询直到满足条件，需要一个上限防止无限阻塞。
/// 取 `min(request.timeout, WAIT_TIMEOUT_CAP)` 确保不会用尽整个请求超时预算，
/// 为后续操作（screenshot 等）留出时间。`NetworkIdle` 模式只 sleep 500ms，不受此限制。
const WAIT_TIMEOUT_CAP: Duration = Duration::from_secs(10);

/// CDP Fetch 拦截事件处理 task 的守卫。
///
/// 持有 `tokio::spawn` 返回的 [`tokio::task::JoinHandle`]，在守卫 drop 时 abort 该 task。
/// scrape 的成功返回、`?` 早退、以及 `tokio::time::timeout` 超时取消（整个
/// future 被 drop）三种退出路径都会触发守卫 drop，从而终止拦截 task，
/// 防止其残留在归还到 TabPool 的复用页面上继续对下一请求发出
/// Continue/FailRequest。
struct InterceptGuard(Option<tokio::task::JoinHandle<()>>);

impl InterceptGuard {
    fn new() -> Self {
        Self(None)
    }

    fn set(&mut self, handle: tokio::task::JoinHandle<()>) {
        // 若已有旧 handle（理论上不会），先 abort 再替换
        if let Some(old) = self.0.take() {
            old.abort();
        }
        self.0 = Some(handle);
    }
}

impl Drop for InterceptGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.0.take() {
            handle.abort();
        }
    }
}

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
    /// CDP handler 任务的 JoinHandle，cleanup 时 abort + await 防止任务泄漏
    handler_handle: Arc<Mutex<Option<tokio::task::JoinHandle<()>>>>,
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
            handler_handle: Arc::new(Mutex::new(None)),
        }
    }
}

#[async_trait]
impl BrowserManagerTrait for PlaywrightBrowserManagerComponent {
    async fn get_browser(&self) -> Result<Arc<Browser>, EngineError> {
        self.get_browser_with_recovery(3).await
    }

    async fn cleanup(&self) {
        // 先 abort CDP handler 任务，再关闭浏览器
        // 取出 handle 后立即释放锁，避免 MutexGuard 跨越 await
        let handler_to_abort = {
            let mut handler_guard = self
                .handler_handle
                .lock()
                .expect("handler_handle mutex poisoned");
            handler_guard.take()
        };
        if let Some(handle) = handler_to_abort {
            handle.abort();
            // 给 handler 任务一点时间完成清理
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), async {
                let _ = handle.await;
            })
            .await;
            log::debug!("CDP handler task aborted and awaited");
        }

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
        if let Some(browser) = self.cached_browser_if_healthy().await {
            return Ok(browser);
        }

        // 需要创建新的浏览器
        let (browser, handler) = self.connect_or_launch_browser().await?;

        // 启动处理器任务
        // 存储 JoinHandle 到 manager，cleanup 时 abort + await
        self.install_handler(handler);

        let browser = Arc::new(browser);

        // 存储浏览器实例
        self.store_browser(&browser);

        Ok(browser)
    }

    /// 复用已缓存的健康浏览器（测试模式下总是重建）
    async fn cached_browser_if_healthy(&self) -> Option<Arc<Browser>> {
        let test_mode = self.config.is_test_mode();

        let browser_to_check = {
            let browser_guard = self.browser.lock().expect("Browser mutex poisoned");
            browser_guard.as_ref().map(Arc::clone)
        };

        if let Some(browser) = browser_to_check {
            if self.check_health(&browser).await && !test_mode {
                return Some(browser);
            }
        }
        None
    }

    /// 存储浏览器实例（供后续复用）
    fn store_browser(&self, browser: &Arc<Browser>) {
        let mut browser_guard = self.browser.lock().expect("Browser mutex poisoned");
        *browser_guard = Some(Arc::clone(browser));
    }

    /// 连接远端 Chrome 或启动本地浏览器（含浏览器自动下载与 proxy URL 严格校验）
    async fn connect_or_launch_browser(
        &self,
    ) -> Result<(Browser, chromiumoxide::Handler), EngineError> {
        let remote_debugging_url = self.config.get_remote_debugging_url();

        let proxy_url = self.config.get_proxy_url();

        let (browser, handler) = if let Some(ref url) = remote_debugging_url {
            log::info!("Connecting to remote Chrome instance at: {}", url);
            Browser::connect(url).await.map_err(|e| {
                // Chrome 连接失败是可重试的基础设施错误（浏览器池可能在重启），
                // 分类为 BrowserError 允许 fallback，而非不可重试的 Other
                EngineError::BrowserError(format!("Failed to connect to remote Chrome: {}", e))
            })?
        } else {
            // 尝试自动下载浏览器（如果需要）
            let browser_path = self.download_browser_if_needed().await?;
            Self::launch_local_browser(browser_path, proxy_url).await?
        };
        Ok((browser, handler))
    }

    /// 启动本地浏览器（含 proxy URL 严格校验与安全传参）
    async fn launch_local_browser(
        browser_path: Option<PathBuf>,
        proxy_url: Option<String>,
    ) -> Result<(Browser, chromiumoxide::Handler), EngineError> {
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
            // 严格校验 proxy URL 防止命令行参数注入
            //
            // 原漏洞：`format!("--proxy-server={}", proxy)` 若 proxy 含空格或特殊字符，
            // Chrome 可能解析为多个 argv（如 "http://x --enable-bad-flag" 被拆为
            // `--proxy-server=http://x` + `--enable-bad-flag`）。
            //
            // 1. `validate_proxy_url` 严格校验 URL 格式 + scheme 白名单 + 无空白字符
            // 2. `arg("--proxy-server").arg(validated)` 分离传递 flag 与值，
            // 从根本上消除单字符串拼接导致的 argv 拆分风险
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

        let (browser, handler) = Browser::launch(
            builder
                .build()
                .map_err(|e| EngineError::Other(e.to_string()))?,
        )
        .await
        .map_err(|e| EngineError::Other(e.to_string()))?;
        Ok((browser, handler))
    }

    /// 启动 handler 事件循环 task 并登记到 manager：
    /// 已有旧 handler 先 abort；存储 JoinHandle 到 manager，cleanup 时 abort + await
    fn install_handler(&self, mut handler: chromiumoxide::Handler) {
        let handler_task = tokio::spawn(async move {
            while let Some(h) = handler.next().await {
                if let Err(e) = h {
                    log::debug!("Browser handler event error (continuing): {:?}", e);
                }
            }
        });
        {
            let mut guard = self
                .handler_handle
                .lock()
                .expect("handler_handle mutex poisoned");
            // 如果已有旧 handler，先 abort
            if let Some(old) = guard.take() {
                old.abort();
            }
            *guard = Some(handler_task);
        }
    }

    /// 下载浏览器（如果需要）
    async fn download_browser_if_needed(&self) -> Result<Option<PathBuf>, EngineError> {
        // 首先检查系统是否有浏览器
        if let Some(path) = crate::engines::browser_downloader::find_system_browser().await {
            log::info!("Using system browser");
            return Ok(Some(path));
        }

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
    /// UA 池每次请求从池中选取一致的 UA + viewport
    ua_pool: UaPool,
    /// 单引擎最大响应时间（MRT）。
    ///
    /// router 顺序 fallback 路径用 `min(remaining, mrt)` 包裹单引擎调用，
    /// 超 MRT 即切下一引擎。注入自 `Settings.timeouts.engines.cdp_seconds`（默认 30 秒）。
    mrt: Duration,
    /// XHR/Fetch 后台响应捕获配置（F6）。
    ///
    /// `None` = 不捕获：整条 Network 监听代码不执行（零开销）。
    /// bootstrap 从 `PlaywrightEngineSettings.capture_xhr*` 映射注入。
    xhr_capture: Option<XhrCaptureConfig>,
}

impl PlaywrightEngine {
    /// 创建新的 Playwright 引擎（使用全局浏览器池）
    pub fn new() -> Self {
        Self::with_mrt(Duration::from_secs(DEFAULT_PLAYWRIGHT_MRT_SECONDS))
    }

    /// 创建带 MRT 配置的 Playwright 引擎。
    ///
    /// 生产环境应从 `settings.timeouts.engines.cdp_seconds` 注入 `mrt`。
    #[must_use]
    pub fn with_mrt(mrt: Duration) -> Self {
        Self {
            pool: None,
            ua_pool: UaPool::default(),
            mrt,
            xhr_capture: None,
        }
    }

    /// 创建带有自定义浏览器池的 Playwright 引擎
    pub fn with_pool(pool: BrowserPool) -> Self {
        Self::with_pool_and_mrt(pool, Duration::from_secs(DEFAULT_PLAYWRIGHT_MRT_SECONDS))
    }

    /// 创建带有自定义浏览器池 + MRT 配置的 Playwright 引擎。
    ///
    /// 生产环境应从 `settings.timeouts.engines.cdp_seconds` 注入 `mrt`。
    #[must_use]
    pub fn with_pool_and_mrt(pool: BrowserPool, mrt: Duration) -> Self {
        Self {
            pool: Some(pool),
            ua_pool: UaPool::default(),
            mrt,
            xhr_capture: None,
        }
    }

    /// 启用 XHR/Fetch 后台响应捕获（F6）。
    ///
    /// bootstrap 从 `PlaywrightEngineSettings` 映射 [`XhrCaptureConfig`] 注入；
    /// 未调用时保持 `None`，scrape 全程无 Network 监听（零开销）。
    #[must_use]
    pub fn with_xhr_capture(mut self, config: XhrCaptureConfig) -> Self {
        self.xhr_capture = Some(config);
        self
    }

    /// 获取 XHR 捕获配置引用（用于测试验证）
    #[must_use]
    pub fn xhr_capture_config(&self) -> Option<&XhrCaptureConfig> {
        self.xhr_capture.as_ref()
    }

    /// 获取 UA 池引用（用于测试验证）
    #[must_use]
    pub fn ua_pool(&self) -> &UaPool {
        &self.ua_pool
    }

    /// 获取引擎级 MRT（用于测试验证）。
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

        if let Some(pool) = get_global_pool() {
            return pool.clone();
        }

        // 创建临时池（不推荐，应该使用全局池）
        // : 使用 docker_safe_args 确保容器环境下自动添加 --no-sandbox
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

/// 浏览器会话指纹一致性设置：UA/客户端提示 + locale + viewport + stealth。
///
/// PlaywrightEngine 与 MllmEngine 共用——任何驱动真实 Chromium 页面的引擎
/// 都必须过这一套，否则等于"接了遥控线没化妆"：引擎壳是指纹中性的裸
/// 浏览器，反爬单请求即可识破。profile 取自 UaPool 的 Chromium 系（P08）；
/// stealth 经 addScriptToEvaluateOnNewDocument 做 target 级持久注入，
/// 幂等守卫防止 tab 复用时脚本份数累积。
///
/// 失败语义：UA/viewport 硬失败（页面不可用=抓取无意义）；locale/stealth
/// best-effort 仅告警（与既定策略一致）。
pub(crate) async fn apply_browser_fingerprint(
    page: &chromiumoxide::Page,
    ua_pool: &UaPool,
    mobile: bool,
) -> Result<(), EngineError> {
    let profile = ua_pool.pick_chromium(mobile);
    apply_ua_and_locale(page, profile).await?;
    apply_viewport_and_stealth(page, profile).await?;
    Ok(())
}

/// UA override + locale 校正（自 [`apply_browser_fingerprint`] 提取）。
///
/// UA override 为硬失败；locale 为 best-effort（CDP 失败仅告警）。
async fn apply_ua_and_locale(
    page: &chromiumoxide::Page,
    profile: &UaProfile,
) -> Result<(), EngineError> {
    // 完整 UA override：UA + acceptLanguage + navigator.platform +
    // userAgentMetadata（Sec-CH-UA* 头与 navigator.userAgentData 的官方
    // 修正通道）；双词表与 brands 模板见 ua_metadata 模块文档
    let fingerprint = build_ua_fingerprint(
        profile.ua,
        profile.sec_ch_ua,
        profile.platform,
        profile.mobile,
    );
    let mut ua_override = SetUserAgentOverrideParams::builder()
        .user_agent(profile.ua)
        .accept_language(cdp_accept_language(profile.accept_language));
    if let Some(navigator_platform) = fingerprint.navigator_platform {
        ua_override = ua_override.platform(navigator_platform);
    }
    if let Some(metadata) = fingerprint.metadata {
        ua_override = ua_override.user_agent_metadata(metadata);
    }
    let ua_override = ua_override.build().map_err(|e| {
        EngineError::BrowserError(format!("Failed to build user agent override: {}", e))
    })?;
    page.execute(ua_override).await.map_err(|e| {
        EngineError::BrowserError(format!("Failed to set user agent override: {}", e))
    })?;

    // locale 覆盖：accept_language 首段标签，校正 navigator.language 与
    // Intl 默认 locale（launch 的进程级 --lang 是固定值的残留缺口）；
    // best-effort：CDP 失败仅告警
    if let Some(locale) = profile.accept_language.split(',').next() {
        if let Err(e) = page
            .execute(
                chromiumoxide::cdp::browser_protocol::emulation::SetLocaleOverrideParams::builder()
                    .locale(locale.trim())
                    .build(),
            )
            .await
        {
            log::warn!("Failed to set locale override: {}", e);
        }
    }
    Ok(())
}

/// viewport 覆盖 + stealth 注入（自 [`apply_browser_fingerprint`] 提取）。
///
/// viewport 为硬失败；stealth best-effort 仅告警。
async fn apply_viewport_and_stealth(
    page: &chromiumoxide::Page,
    profile: &UaProfile,
) -> Result<(), EngineError> {
    // viewport 与 UA platform 一致
    let viewport_params = SetDeviceMetricsOverrideParams::new(
        profile.viewport.0 as i64,
        profile.viewport.1 as i64,
        1.0,
        profile.mobile,
    );
    page.execute(viewport_params)
        .await
        .map_err(|e| EngineError::BrowserError(format!("Failed to set viewport: {}", e)))?;

    // stealth 注入（best-effort）：幂等守卫——CDP 注册 target 级持久、随
    // tab 复用存活，重复注册会让每个 tab 的脚本份数线性累积
    let stealth_injector = crate::engines::js_inject::JsInjector::stealth();
    let already_installed: bool = match page
        .evaluate("() => window.__crawlrs_stealth_installed === true")
        .await
    {
        Ok(v) => v.into_value::<bool>().unwrap_or(false),
        Err(_) => false,
    };
    if !already_installed {
        if let Err(e) = stealth_injector.apply_before_document(page).await {
            log::warn!(
                "{}",
                crate::i18n::tr_log_args(
                    "playwright-stealth-inject-failed",
                    &[("error", fluent_bundle::FluentValue::from(e.to_string()))],
                )
            );
        }
    }
    Ok(())
}

/// 从池中获取 Browser + Page 并应用指纹一致性设置。
///
/// 默认（context_isolation=true）每任务独立 BrowserContext，归还时
/// 整体销毁，cookie/storage 不跨任务串扰；isolation=false 时 Page
/// 从 TabPool 复用（LIFO），池空时调用 browser.new_page。
///
/// Note: pooled_page drop 时自动归还资源——复用页经 TabPool 导航
/// about:blank 后压栈，隔离页随整个 BrowserContext 销毁。
/// Browser 也在 pooled_page drop 时归还到 BrowserPool。
/// 错误路径下 Page 可能不可用，归还路径会 drop 它（关闭 tab）。
async fn acquire_fingerprinted_page(
    pool: &BrowserPool,
    ua_pool: &UaPool,
    request: &InternalScrapeRequest,
) -> Result<(PooledPage, chromiumoxide::page::Page), EngineError> {
    let pooled_page = pool.acquire_page().await?;
    // Page 是 Arc-based，clone 廉价；pooled_page 持有原始 Page 用于归还
    let page: chromiumoxide::page::Page = pooled_page.page().clone();

    // 指纹一致性设置（UA/客户端提示/locale/viewport/stealth）已抽取
    // 为共享函数——MllmEngine 同样驱动真实 Chromium 页面，必须过同
    // 一套（此前 mllm 裸 goto，是全引擎里唯一没化妆的检测点）
    apply_browser_fingerprint(&page, ua_pool, request.mobile).await?;

    Ok((pooled_page, page))
}

/// 页面导航与等待：goto（默认等 load 事件）→ 注入 cleanup 脚本（best-effort）→
/// bot 检测页告警（仍返回内容交由解析器处理）→ 逐个执行页面动作 → 等待策略。
async fn navigate_and_wait(
    page: &chromiumoxide::page::Page,
    request: &InternalScrapeRequest,
) -> Result<(), EngineError> {
    goto_and_inject_cleanup(page, &request.url).await?;

    warn_if_bot_detection_page(page).await?;

    run_page_actions(page, &request.actions).await?;

    // 页面加载后等待策略（替代原 sync_wait_ms 固定 sleep）
    //
    // `request.wait_for` 由调用方通过 `ScrapeOptions.wait_for` 设置；
    // `None` 时使用 `WaitFor::NetworkIdle`（与原默认等待语义一致，sleep 500ms）。
    //
    // 满足条件立即返回，超时返回 `EngineError::BrowserError`。
    // `sync_wait_ms` 不再在 Playwright 引擎中使用（保留供 FlareSolverr 等非浏览器引擎）。
    let wait_strategy = request.wait_for.clone().unwrap_or_default();
    let wait_timeout = request.timeout.min(WAIT_TIMEOUT_CAP);
    wait_strategy.wait(page, wait_timeout).await?;

    Ok(())
}

/// 导航到目标 URL（默认等 load 事件）并注入 cleanup 脚本（best-effort）
async fn goto_and_inject_cleanup(
    page: &chromiumoxide::page::Page,
    url: &str,
) -> Result<(), EngineError> {
    // Navigate and wait for load
    // goto waits for the load event by default
    page.goto(url)
        .await
        .map_err(|e| EngineError::BrowserError(e.to_string()))?;

    // 页面加载后注入 cleanup 脚本（best-effort）
    // 顺序：consent_popups → overlay_elements → flatten_shadow_dom
    let cleanup_injector = crate::engines::js_inject::JsInjector::cleanup();
    if let Err(e) = cleanup_injector
        .apply(page, crate::engines::js_inject::InjectPhase::AfterLoad)
        .await
    {
        log::warn!("Cleanup injection failed (best-effort, continue): {}", e);
    }
    Ok(())
}

/// bot 检测页仅告警（仍返回内容交由解析器处理）
async fn warn_if_bot_detection_page(page: &chromiumoxide::page::Page) -> Result<(), EngineError> {
    // Try to detect if we got a bot detection page
    let content: String = page
        .content()
        .await
        .map_err(|e| EngineError::BrowserError(e.to_string()))?;

    if is_bot_detection_page(&content) {
        log::warn!("Detected bot detection page from Google");
        // Still return the content, let the parser handle it
    }
    Ok(())
}

/// 逐个执行页面动作（任一失败即向上传播）
async fn run_page_actions(
    page: &chromiumoxide::page::Page,
    actions: &[InternalPageAction],
) -> Result<(), EngineError> {
    for action in actions {
        execute_page_action(page, action).await?;
    }
    Ok(())
}

/// bot 检测页特征（Google 告警页等；命中仅告警，仍返回内容交由解析器处理）
fn is_bot_detection_page(content: &str) -> bool {
    content.contains("如果您在几秒钟内没有被重定向")
        || content.contains("Having trouble accessing Google")
        || content.contains("enablejs")
}

/// 执行单个页面动作（wait / click / scroll / screenshot / input）
async fn execute_page_action(
    page: &chromiumoxide::page::Page,
    action: &InternalPageAction,
) -> Result<(), EngineError> {
    match action {
        InternalPageAction::Wait { milliseconds } => {
            tokio::time::sleep(Duration::from_millis(*milliseconds)).await;
        }
        InternalPageAction::Click { selector } => {
            let element: chromiumoxide::element::Element =
                page.find_element(selector).await.map_err(|e| {
                    EngineError::BrowserError(format!("Click failed, element not found: {}", e))
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
            let element: chromiumoxide::element::Element =
                page.find_element(selector).await.map_err(|e| {
                    EngineError::BrowserError(format!("Input failed, element not found: {}", e))
                })?;
            element
                .type_str(text)
                .await
                .map_err(|e| EngineError::BrowserError(format!("Input failed: {}", e)))?;
        }
    }
    Ok(())
}

/// 按请求配置抓取截图（`needs_screenshot=false` 时返回 `None`）。
async fn capture_screenshot(
    page: &chromiumoxide::page::Page,
    request: &InternalScrapeRequest,
) -> Result<Option<String>, EngineError> {
    if !request.needs_screenshot {
        return Ok(None);
    }

    let config = request
        .screenshot_config
        .clone()
        .unwrap_or(InternalScreenshotConfig {
            full_page: true,
            selector: None,
            quality: Some(80),
            format: Some("jpeg".to_string()),
        });

    let format = screenshot_format(config.format.as_deref());

    let screenshot_bytes = if let Some(selector) = &config.selector {
        capture_element_screenshot(page, selector, format).await?
    } else {
        capture_page_screenshot(page, &config, format).await?
    };

    Ok(Some(BASE64.encode(screenshot_bytes)))
}

/// 截图格式映射：`png` → Png，其余 → Jpeg
fn screenshot_format(format: Option<&str>) -> CaptureScreenshotFormat {
    match format {
        Some("png") => CaptureScreenshotFormat::Png,
        _ => CaptureScreenshotFormat::Jpeg,
    }
}

/// 元素截图：按选择器定位元素后截图（元素不存在/截图失败均映射为浏览器错误）。
async fn capture_element_screenshot(
    page: &chromiumoxide::page::Page,
    selector: &str,
    format: CaptureScreenshotFormat,
) -> Result<Vec<u8>, EngineError> {
    // Find element and screenshot
    let element: chromiumoxide::element::Element = page
        .find_element(selector)
        .await
        .map_err(|e| EngineError::BrowserError(format!("Element not found: {}", e)))?;

    element
        .screenshot(format)
        .await
        .map_err(|e| EngineError::BrowserError(format!("Element screenshot failed: {}", e)))
}

/// 整页截图：按配置构建 `ScreenshotParams` 后截图。
async fn capture_page_screenshot(
    page: &chromiumoxide::page::Page,
    config: &InternalScreenshotConfig,
    format: CaptureScreenshotFormat,
) -> Result<Vec<u8>, EngineError> {
    let params = chromiumoxide::page::ScreenshotParams::builder()
        .format(format)
        .quality(config.quality.unwrap_or(80) as i64)
        .full_page(config.full_page)
        .build();

    page.screenshot(params)
        .await
        .map_err(|e| EngineError::BrowserError(format!("Page screenshot failed: {}", e)))
}

/// 提取最终内容并构建响应：XHR 捕获收集 → final URL → content-type →
/// 正文 → 响应头 → 截图 → 归还池化页（drop）→ 组装 [`InternalScrapeResponse`]。
async fn collect_and_build_response(
    page: &chromiumoxide::page::Page,
    request: &InternalScrapeRequest,
    xhr_session: Option<&XhrCaptureSession>,
    pooled_page: PooledPage,
    start: Instant,
) -> Result<InternalScrapeResponse, EngineError> {
    // F6：页面等待完成后统一取体（wait 之后、page 释放之前）。
    // NetworkIdle 等待退出时可能已 best-effort Network.disable（见 wait.rs），
    // 取体前重新 enable（幂等）；完成后 disable，防止 tab 池复用期事件常开。
    let captured_responses = match xhr_session {
        Some(session) => session.collect(page).await,
        None => Vec::new(),
    };

    // Get final URL after navigation (handles redirects)
    let final_url: String = page
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

    let screenshot = capture_screenshot(page, request).await?;

    // 不调用 page.close，让 pooled_page drop 时按归还模式收尾：
    // 复用页归还 TabPool（导航 about:blank 后压栈），隔离页销毁整个
    // BrowserContext。Browser 也在 pooled_page drop 时归还到 BrowserPool
    drop(pooled_page);

    Ok(InternalScrapeResponse {
        js_degraded: false,
        status_code,
        content,
        screenshot,
        content_type: "text/html".to_string(),
        headers: response_headers,
        response_time_ms: start.elapsed().as_millis() as u64,
        // 传递页面导航后的最终 URL（不再丢弃），修复 final_url 全链路失效
        final_url: Some(final_url),
        captured_responses,
    })
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
        session::validate_scrape_request(request).await?;

        let start = Instant::now();
        let timeout_duration = request.timeout;

        let pool = self.get_or_init_pool();

        // Wrap the entire operation in a timeout
        tokio::time::timeout(timeout_duration, self.scrape_session(request, &pool, start))
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

    /// 覆写 MRT，返回构造时注入的 `mrt`（默认 30 秒）。
    ///
    /// router 顺序 fallback 路径用 `min(remaining, self.mrt)` 包裹单引擎调用，
    /// 超 MRT 即切下一引擎（瀑布式）。
    fn max_response_time(&self) -> Duration {
        self.mrt
    }
}

/// F6 XHR/Fetch 捕获会话：单次 scrape 生命周期的捕获状态
///
/// [`XhrCaptureSession::start`] 在导航前启用 Network domain 并启动事件监听
/// task（类型 ∈ {XHR, Fetch} + URL glob 过滤 + max_responses 上限，命中记入
/// 共享缓冲）；页面导航/等待完成后调用 [`XhrCaptureSession::collect`] 逐条
/// 取体。会话 drop 时 abort 监听 task（复用 [`InterceptGuard`] 语义），防止
/// task 残留在归还池的复用页面上。
struct XhrCaptureSession {
    buffer: Arc<Mutex<XhrCaptureBuffer>>,
    _guard: InterceptGuard,
}

impl XhrCaptureSession {
    /// 启动捕获会话（导航前调用）
    ///
    /// 设置失败返回 `None`（fail-open：捕获为增强功能，不阻断抓取）。
    async fn start(page: &chromiumoxide::page::Page, config: &XhrCaptureConfig) -> Option<Self> {
        let (responses, finished) = Self::subscribe_xhr_events(page).await?;

        let buffer = Arc::new(Mutex::new(XhrCaptureBuffer::new(config.max_responses)));
        let handle = spawn_xhr_capture_task(
            responses,
            finished,
            Arc::clone(&buffer),
            config.url_patterns.clone(),
            config.max_bytes,
            page,
        );
        let mut guard = InterceptGuard::new();
        guard.set(handle);
        Some(Self {
            buffer,
            _guard: guard,
        })
    }

    /// 启用 Network domain 并订阅 ResponseReceived / LoadingFinished 事件流。
    ///
    /// 设置失败返回 `None`（fail-open：捕获为增强功能，不阻断抓取）。
    async fn subscribe_xhr_events(
        page: &chromiumoxide::page::Page,
    ) -> Option<(
        chromiumoxide::listeners::EventStream<EventResponseReceived>,
        chromiumoxide::listeners::EventStream<EventLoadingFinished>,
    )> {
        let setup = async {
            page.execute(NetworkEnableParams::default()).await?;
            let responses = page.event_listener::<EventResponseReceived>().await?;
            let finished = page.event_listener::<EventLoadingFinished>().await?;
            Ok::<_, chromiumoxide::error::CdpError>((responses, finished))
        };
        match setup.await {
            Ok(streams) => Some(streams),
            Err(e) => {
                // 对齐 wait.rs 复查 L2：enable 成功而订阅失败时关闭 domain，
                // 防止页面存活期事件常开分发
                let _ = page.execute(NetworkDisableParams::default()).await;
                log::warn!(
                    "{}",
                    crate::i18n::tr_log_args(
                        "playwright-xhr-setup-failed",
                        &[("error", fluent_bundle::FluentValue::from(e.to_string()))],
                    )
                );
                None
            }
        }
    }

    /// 页面等待完成后收集捕获结果（wait 之后、page 释放之前）
    async fn collect(&self, page: &chromiumoxide::page::Page) -> Vec<InternalCapturedResponse> {
        let (captured, stats, pending_left) = {
            let mut buf = self.buffer.lock().expect("xhr capture buffer poisoned");
            (buf.drain_captured(), buf.stats(), buf.pending_len())
        };
        if pending_left > 0 {
            log::debug!(
                "{}",
                crate::i18n::tr_log_args(
                    "playwright-xhr-unfinished-at-collect",
                    &[(
                        "count",
                        fluent_bundle::FluentValue::from(pending_left as i64)
                    )],
                )
            );
        }
        if stats.overflow_skipped > 0
            || stats.oversized_dropped > 0
            || stats.decode_failed > 0
            || stats.fetch_failed > 0
        {
            log::warn!(
                "{}",
                crate::i18n::tr_log_args(
                    "playwright-xhr-dropped",
                    &[
                        (
                            "oversize",
                            fluent_bundle::FluentValue::from(stats.oversized_dropped as i64),
                        ),
                        (
                            "decode_failed",
                            fluent_bundle::FluentValue::from(stats.decode_failed as i64),
                        ),
                        (
                            "fetch_failed",
                            fluent_bundle::FluentValue::from(stats.fetch_failed as i64),
                        ),
                        (
                            "overflow",
                            fluent_bundle::FluentValue::from(stats.overflow_skipped as i64),
                        ),
                    ],
                )
            );
        }
        // 收尾关闭 Network domain（Selector/DomStable 等待路径不会 disable；
        // NetworkIdle 已 disable 时此调用为无害 no-op）
        let _ = page.execute(NetworkDisableParams::default()).await;
        captured
    }
}

/// 启动 XHR 捕获监听 task：单任务 select! 消费双事件流，命中记入共享缓冲。
fn spawn_xhr_capture_task(
    mut responses: chromiumoxide::listeners::EventStream<EventResponseReceived>,
    mut finished: chromiumoxide::listeners::EventStream<EventLoadingFinished>,
    buf_clone: Arc<Mutex<XhrCaptureBuffer>>,
    patterns: Vec<String>,
    max_bytes: u64,
    page: &chromiumoxide::page::Page,
) -> tokio::task::JoinHandle<()> {
    let page_clone = page.clone();
    tokio::spawn(async move {
        // 单任务 select! 消费双事件流，保证 ResponseReceived 与
        // LoadingFinished 的配对无跨流乱序竞态（早到事件经
        // finished_early 集合补偿）。取体必须在 LoadingFinished 到达时
        // 立即执行：NetworkIdle 等待退出会 Network.disable，届时浏览器
        // 已丢弃缓冲资源（getResponseBody 报 No resource with given
        // identifier found）。
        loop {
            tokio::select! {
                Some(event) = responses.next() => {
                    if !matches!(event.r#type, ResourceType::Xhr | ResourceType::Fetch) {
                        continue;
                    }
                    let url = event.response.url.clone();
                    if !url_matches_capture_patterns(&url, &patterns) {
                        continue;
                    }
                    // SSRF 防护：页面 JS 可指路本机/内网/链路本地地址（如
                    // 169.254.169.254 元数据端点），命中即丢弃——捕获通道
                    // 不得成为页面发起的内网数据回传出口
                    if crate::engines::validators::is_internal_url(&url) {
                        log::debug!(
                            "{}",
                            crate::i18n::tr_log_args(
                                "playwright-xhr-internal-blocked",
                                &[("url", fluent_bundle::FluentValue::from(url.as_str()))],
                            )
                        );
                        continue;
                    }
                    let entry = PendingXhr {
                        request_id: event.request_id.as_ref().to_string(),
                        url,
                        status: u16::try_from(event.response.status).unwrap_or(0),
                        mime_type: event.response.mime_type.clone(),
                    };
                    let early_entry = entry.clone();
                    let outcome = buf_clone
                        .lock()
                        .expect("xhr capture buffer poisoned")
                        .record(entry);
                    if outcome == RecordOutcome::AlreadyFinished {
                        // LoadingFinished 早到：立即取体
                        fetch_and_store(&page_clone, &buf_clone, &early_entry, max_bytes)
                            .await;
                    }
                }
                Some(event) = finished.next() => {
                    let request_id = event.request_id.as_ref().to_string();
                    let pending = {
                        let mut buf = buf_clone.lock().expect("xhr capture buffer poisoned");
                        match buf.take_pending(&request_id) {
                            Some(entry) => Some(entry),
                            None => {
                                buf.mark_finished_early(&request_id);
                                None
                            }
                        }
                    };
                    if let Some(entry) = pending {
                        fetch_and_store(&page_clone, &buf_clone, &entry, max_bytes).await;
                    }
                }
                else => break,
            }
        }
    })
}

/// LoadingFinished 后立即取体并入完成区（Network domain 存活期内，body 未被丢弃）
///
/// 单条 CDP 失败仅 debug 记录并计数，不中断抓取（best-effort）。
/// 锁在 CDP 调用返回后才获取，不跨 await 持有。
async fn fetch_and_store(
    page: &chromiumoxide::page::Page,
    buffer: &Mutex<XhrCaptureBuffer>,
    entry: &PendingXhr,
    max_bytes: u64,
) {
    let result = page
        .execute(GetResponseBodyParams::new(entry.request_id.clone()))
        .await;
    let mut buf = buffer.lock().expect("xhr capture buffer poisoned");
    match result {
        Ok(resp) => {
            let returns = resp.result;
            match process_body(returns.body, returns.base64_encoded, max_bytes) {
                BodyOutcome::Kept(body) => {
                    buf.push_captured(InternalCapturedResponse {
                        url: entry.url.clone(),
                        status: entry.status,
                        mime_type: entry.mime_type.clone(),
                        body,
                    });
                }
                BodyOutcome::Oversize => {
                    log::debug!(
                        "{}",
                        crate::i18n::tr_log_args(
                            "playwright-xhr-body-oversize",
                            &[("url", fluent_bundle::FluentValue::from(entry.url.as_str()))],
                        )
                    );
                    buf.note_oversize();
                }
                BodyOutcome::DecodeFailed => {
                    log::debug!(
                        "{}",
                        crate::i18n::tr_log_args(
                            "playwright-xhr-body-decode-failed",
                            &[("url", fluent_bundle::FluentValue::from(entry.url.as_str()))],
                        )
                    );
                    buf.note_decode_failed();
                }
            }
        }
        Err(e) => {
            log::debug!(
                "{}",
                crate::i18n::tr_log_args(
                    "playwright-xhr-fetch-body-failed",
                    &[
                        ("url", fluent_bundle::FluentValue::from(entry.url.as_str())),
                        ("error", fluent_bundle::FluentValue::from(e.to_string())),
                    ],
                )
            );
            buf.note_fetch_failed();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::engine_client::{InternalScrapeRequest, WaitFor};
    use crate::engines::intercept::BLOCK_REASON;
    use chromiumoxide::cdp::browser_protocol::fetch::{
        ContinueRequestParams, EnableParams, EventRequestPaused, FailRequestParams,
    };
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
            needs_mllm: false,
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
            needs_mllm: false,
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
            needs_mllm: false,
        };
        assert_eq!(engine.support_score(&request_basic), 10);
    }

    // UaPool 集成测试

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
        // 多次选取应返回不同 UA（随机性）
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
        // viewport 与 UA platform 必须一致
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
        // 引擎的 mobile UA pool 应包含多个 UA
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
        // 同 seed 必须稳定返回同一 profile
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

    // XHR/Fetch 捕获配置（F6）

    #[test]
    fn test_playwright_engine_xhr_capture_disabled_by_default() {
        // 默认不捕获（零开销），全部构造路径一致
        assert!(PlaywrightEngine::new().xhr_capture_config().is_none());
        assert!(PlaywrightEngine::default().xhr_capture_config().is_none());
    }

    #[test]
    fn test_playwright_engine_xhr_capture_builder() {
        let engine = PlaywrightEngine::new().with_xhr_capture(XhrCaptureConfig {
            url_patterns: vec!["*/api/*".to_string()],
            max_bytes: 1024,
            max_responses: 7,
        });
        let config = engine
            .xhr_capture_config()
            .expect("capture config must be set");
        assert_eq!(config.url_patterns, vec!["*/api/*".to_string()]);
        assert_eq!(config.max_bytes, 1024);
        assert_eq!(config.max_responses, 7);
    }

    // F6 XHR/Fetch 捕获 e2e（需本机 Chrome，`#[ignore]` 门控）
    //
    // scrape 入口强制 SSRF 校验（loopback 被拒，属安全设计不可削弱），无法直达
    // 本地 mock 服务器；因此 e2e 直接驱动 scrape 内部的 XhrCaptureSession（同一
    // 份捕获代码）+ 真实 Chromium 页面，验证 CDP Network 事件流捕获全链路。
    //
    // 运行: cargo test --features full --lib xhr_capture_e2e -- --ignored --test-threads=1

    /// 本地 mock 服务器，返回绑定的 base URL
    ///
    /// 路由：
    /// - GET /page        页面内 `fetch('/api/data')`
    /// - GET /page_multi  页面内依次 `fetch('/api/data?i=1..3')`
    /// - GET /page_mixed  页面内 `fetch('/api/blocked')` + `fetch('/api/data')`
    /// - GET /api/data    JSON `{"ok":true,"msg":"hello xhr"}`
    /// - 其余路径         JSON `{"blocked":true}`（供 Fetch 拦截 FailRequest 用）
    fn spawn_mock_xhr_server() -> String {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind mock server");
        let addr = listener.local_addr().expect("local addr");
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let mut stream = match stream {
                    Ok(s) => s,
                    Err(_) => continue,
                };
                let mut buf = [0u8; 2048];
                let _ = stream.read(&mut buf);
                let req = String::from_utf8_lossy(&buf);
                let (content_type, body): (&str, String) = if req.starts_with("GET /page_multi") {
                    (
                        "text/html; charset=utf-8",
                        "<html><head><title>mock</title></head><body><script>\
                         [1,2,3].forEach(function (i) { fetch('/api/data?i=' + i); });\
                         </script></body></html>"
                            .to_string(),
                    )
                } else if req.starts_with("GET /page_mixed") {
                    (
                        "text/html; charset=utf-8",
                        "<html><head><title>mock</title></head><body><script>\
                         fetch('/api/blocked'); fetch('/api/data');\
                         </script></body></html>"
                            .to_string(),
                    )
                } else if req.starts_with("GET /page") {
                    (
                        "text/html; charset=utf-8",
                        "<html><head><title>mock</title></head><body><script>\
                         fetch('/api/data').then(function (r) { return r.json(); }).then(function (d) { document.title = 'loaded'; });\
                         </script></body></html>"
                            .to_string(),
                    )
                } else if req.starts_with("GET /api/data") {
                    (
                        "application/json",
                        "{\"ok\":true,\"msg\":\"hello xhr\"}".to_string(),
                    )
                } else {
                    ("application/json", "{\"blocked\":true}".to_string())
                };
                let resp = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    content_type,
                    body.len(),
                    body
                );
                let _ = stream.write_all(resp.as_bytes());
                let _ = stream.flush();
            }
        });
        // host 用横线编码的 sslip.io 域名（公网 DNS → 127.0.0.1，Chrome 仍连
        // 本机 mock）：捕获管线含 SSRF 守卫，点分回环 "127." 前缀会被
        // is_localhost 判为 internal 而丢弃全部捕获，e2e 永远拿不到事件。
        format!("http://127-0-0-1.sslip.io:{}", addr.port())
    }

    /// 序列化本模块 e2e 用例的浏览器生命周期（launch→close）。
    ///
    /// chromiumoxide 默认共享 profile 目录（/tmp/chromiumoxide-runner），
    /// 并发启动触发 Chrome ProcessSingleton 冲突（SingletonLock File exists）
    /// 直接 abort。此锁等价实现各用例 ignore 注明的 `--test-threads=1` 契约，
    /// 不再依赖测试运行方式。
    static CHROME_E2E_LOCK: std::sync::OnceLock<tokio::sync::Mutex<()>> =
        std::sync::OnceLock::new();

    /// 启动独立浏览器并打开空白页
    ///
    /// F6 同款修复：每测试唯一临时 profile 目录——chromiumoxide 默认共享
    /// `/tmp/chromiumoxide-runner`，任何一次异常退出的浏览器残留
    /// SingletonLock 都会让后续所有测试启动失败且无法自愈。
    async fn launch_test_page() -> (chromiumoxide::Browser, chromiumoxide::page::Page) {
        static PROFILE_COUNTER: std::sync::OnceLock<std::sync::atomic::AtomicU64> =
            std::sync::OnceLock::new();
        let profile_dir = std::env::temp_dir().join(format!(
            "crawlrs-chromium-e2e-{}-{}",
            std::process::id(),
            PROFILE_COUNTER
                .get_or_init(std::sync::atomic::AtomicU64::default)
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&profile_dir).expect("create e2e profile dir");

        let config = chromiumoxide::BrowserConfig::builder()
            .no_sandbox()
            .arg("--disable-gpu")
            .arg("--disable-dev-shm-usage")
            .user_data_dir(&profile_dir)
            .request_timeout(Duration::from_secs(30))
            .build()
            .expect("browser config");
        let (browser, mut handler) = chromiumoxide::Browser::launch(config)
            .await
            .expect("browser launch");
        tokio::spawn(async move {
            while let Some(h) = handler.next().await {
                if let Err(e) = h {
                    log::debug!(
                        "{}",
                        crate::i18n::tr_log_args(
                            "playwright-handler-event-error",
                            &[("error", fluent_bundle::FluentValue::from(format!("{e:?}")))],
                        )
                    );
                }
            }
        });
        let page = browser.new_page("about:blank").await.expect("new page");
        (browser, page)
    }

    #[tokio::test]
    #[ignore = "需要本机 Chrome：cargo test --features full --lib xhr_capture_e2e -- --ignored --test-threads=1"]
    async fn xhr_capture_e2e_captures_fetch_response() {
        let base = spawn_mock_xhr_server();
        let _chrome_guard = CHROME_E2E_LOCK
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await;
        let (mut browser, page) = launch_test_page().await;
        let config = XhrCaptureConfig {
            url_patterns: vec![],
            max_bytes: 262_144,
            max_responses: 50,
        };
        let session = XhrCaptureSession::start(&page, &config)
            .await
            .expect("capture session must start");
        page.goto(&format!("{}/page", base)).await.expect("goto");
        WaitFor::default()
            .wait(&page, Duration::from_secs(10))
            .await
            .expect("network idle wait");
        let captured = session.collect(&page).await;

        assert!(
            !captured.is_empty(),
            "capture_xhr=true must produce captured responses"
        );
        let data = captured
            .iter()
            .find(|c| c.url.ends_with("/api/data"))
            .expect("/api/data must be captured");
        assert_eq!(data.status, 200);
        assert_eq!(data.mime_type, "application/json");
        assert_eq!(data.body, "{\"ok\":true,\"msg\":\"hello xhr\"}");
        // 文档本身不是 XHR/Fetch，不得混入捕获
        assert!(
            captured.iter().all(|c| !c.url.ends_with("/page")),
            "document must not be captured as XHR, got: {:?}",
            captured
        );
        browser.close().await.ok();
    }

    #[tokio::test]
    #[ignore = "需要本机 Chrome：cargo test --features full --lib xhr_capture_e2e -- --ignored --test-threads=1"]
    async fn xhr_capture_e2e_url_glob_filter_excludes_non_matching() {
        let base = spawn_mock_xhr_server();
        let _chrome_guard = CHROME_E2E_LOCK
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await;
        let (mut browser, page) = launch_test_page().await;
        let config = XhrCaptureConfig {
            url_patterns: vec!["*/nomatch*".to_string()],
            max_bytes: 262_144,
            max_responses: 50,
        };
        let session = XhrCaptureSession::start(&page, &config)
            .await
            .expect("capture session must start");
        page.goto(&format!("{}/page", base)).await.expect("goto");
        WaitFor::default()
            .wait(&page, Duration::from_secs(10))
            .await
            .expect("network idle wait");
        let captured = session.collect(&page).await;
        assert!(
            captured.is_empty(),
            "non-matching XHR must not be captured, got: {:?}",
            captured
        );
        browser.close().await.ok();
    }

    #[tokio::test]
    #[ignore = "需要本机 Chrome：cargo test --features full --lib xhr_capture_e2e -- --ignored --test-threads=1"]
    async fn xhr_capture_e2e_max_responses_caps_collection() {
        let base = spawn_mock_xhr_server();
        let _chrome_guard = CHROME_E2E_LOCK
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await;
        let (mut browser, page) = launch_test_page().await;
        let config = XhrCaptureConfig {
            url_patterns: vec![],
            max_bytes: 262_144,
            max_responses: 2,
        };
        let session = XhrCaptureSession::start(&page, &config)
            .await
            .expect("capture session must start");
        page.goto(&format!("{}/page_multi", base))
            .await
            .expect("goto");
        WaitFor::default()
            .wait(&page, Duration::from_secs(10))
            .await
            .expect("network idle wait");
        let captured = session.collect(&page).await;
        assert_eq!(
            captured.len(),
            2,
            "max_responses=2 must cap captures at 2, got: {:?}",
            captured
        );
        // 第 N+1 条起不捕获：3 个请求只出现 i=1、i=2
        assert!(captured.iter().all(|c| c.url.contains("/api/data?i=")));
        browser.close().await.ok();
    }

    #[tokio::test]
    #[ignore = "需要本机 Chrome：cargo test --features full --lib xhr_capture_e2e -- --ignored --test-threads=1"]
    async fn xhr_capture_e2e_max_bytes_drops_oversize_body() {
        let base = spawn_mock_xhr_server();
        let _chrome_guard = CHROME_E2E_LOCK
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await;
        let (mut browser, page) = launch_test_page().await;
        // /api/data 响应体 26 字节，上限 4 字节 → 整体丢弃
        let config = XhrCaptureConfig {
            url_patterns: vec![],
            max_bytes: 4,
            max_responses: 50,
        };
        let session = XhrCaptureSession::start(&page, &config)
            .await
            .expect("capture session must start");
        page.goto(&format!("{}/page", base)).await.expect("goto");
        WaitFor::default()
            .wait(&page, Duration::from_secs(10))
            .await
            .expect("network idle wait");
        let captured = session.collect(&page).await;
        assert!(
            captured.is_empty(),
            "oversize body must be dropped, got: {:?}",
            captured
        );
        browser.close().await.ok();
    }

    #[tokio::test]
    #[ignore = "需要本机 Chrome：cargo test --features full --lib xhr_capture_e2e -- --ignored --test-threads=1"]
    async fn xhr_capture_e2e_coexists_with_fetch_interception() {
        let base = spawn_mock_xhr_server();
        let _chrome_guard = CHROME_E2E_LOCK
            .get_or_init(|| tokio::sync::Mutex::new(()))
            .lock()
            .await;
        let (mut browser, page) = launch_test_page().await;

        // 启用 Fetch 拦截（与 scrape() 的 block_ads/block_media 同款机制）：
        // 命中 /api/blocked 的请求 FailRequest，其余 ContinueRequest 放行
        page.execute(EnableParams::default())
            .await
            .expect("enable fetch");
        let mut paused = page
            .event_listener::<EventRequestPaused>()
            .await
            .expect("subscribe EventRequestPaused");
        let intercept_page = page.clone();
        let intercept_handle = tokio::spawn(async move {
            while let Some(event) = paused.next().await {
                let url = event.request.url.clone();
                let request_id = event.request_id.clone();
                if url.contains("/api/blocked") {
                    let _ = intercept_page
                        .execute(FailRequestParams::new(request_id, BLOCK_REASON))
                        .await;
                } else {
                    let _ = intercept_page
                        .execute(ContinueRequestParams::new(request_id))
                        .await;
                }
            }
        });

        let config = XhrCaptureConfig {
            url_patterns: vec![],
            max_bytes: 262_144,
            max_responses: 50,
        };
        let session = XhrCaptureSession::start(&page, &config)
            .await
            .expect("capture session must start");
        page.goto(&format!("{}/page_mixed", base))
            .await
            .expect("goto");
        WaitFor::default()
            .wait(&page, Duration::from_secs(10))
            .await
            .expect("network idle wait");
        let captured = session.collect(&page).await;

        // 放行的 XHR 正常捕获；被 FailRequest 的请求无响应，不出现在捕获中
        assert!(
            captured.iter().any(|c| c.url.ends_with("/api/data")),
            "continued XHR must be captured, got: {:?}",
            captured
        );
        assert!(
            captured.iter().all(|c| !c.url.contains("/api/blocked")),
            "failed request must not appear in capture, got: {:?}",
            captured
        );
        intercept_handle.abort();
        browser.close().await.ok();
    }
}
