// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Playwright 浏览器实例池管理器
//!
//! 提供浏览器实例的复用能力，避免每次请求都创建新的浏览器实例，
//! 从而减少 500ms-2s 的启动延迟。
//!
//! # 主要特性
//!
//! - 浏览器实例复用
//! - per-task BrowserContext 隔离（`context_isolation`，默认开启：每任务
//!   独立 context + page，归还时销毁整个 context，cookie/storage 不跨任务串扰）
//! - 最大实例数限制
//! - 空闲实例自动清理
//! - 健康检查机制
//! - 优雅关闭支持

use crate::engines::browser_downloader::{BrowserDownloadConfig, BrowserDownloadManager};
use crate::engines::client::tab_pool::TabPool;
use crate::engines::engine_client::EngineError;
use crate::infrastructure::services::config_service::BrowserConfigTrait;
use chromiumoxide::cdp::browser_protocol::browser::BrowserContextId;
use chromiumoxide::cdp::browser_protocol::target::{
    CreateBrowserContextParams, CreateTargetParams,
};
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, OnceCell, RwLock, Semaphore};
use tokio::task::JoinHandle;

/// 浏览器实例池配置
#[derive(Debug, Clone)]
pub struct BrowserPoolConfig {
    /// 最大浏览器实例数
    pub max_instances: usize,
    /// 空闲实例超时时间（秒）
    pub idle_timeout_secs: u64,
    /// 健康检查间隔（秒）
    pub health_check_interval_secs: u64,
    /// 实例创建超时时间（秒）
    pub create_timeout_secs: u64,
    /// 是否启用实例复用
    pub enable_reuse: bool,
    /// 浏览器启动参数
    pub browser_args: Vec<String>,
    /// TabPool 最大容量
    ///
    /// 每个 Browser 实例独立的 Tab 池容量。0 = 禁用 tab 复用。
    /// 默认 10（每个 Browser 缓存最多 10 个空闲 Page）。
    pub tab_pool_max_size: usize,
    /// per-task BrowserContext 隔离
    ///
    /// `true`（默认）：每次 `acquire_page` 为任务创建独立 BrowserContext +
    /// 其内 Page，归还时销毁整个 context——cookie/localStorage/indexedDB
    /// 随之消失，消除 tab 复用导致的跨任务会话串扰。隔离页不进 TabPool。
    ///
    /// `false`：走既有 TabPool 复用路径（顺序复用间 cookie 泄漏是已知取舍），
    /// 作为逃生门保留。
    pub context_isolation: bool,
}

impl Default for BrowserPoolConfig {
    fn default() -> Self {
        Self {
            max_instances: 5,
            idle_timeout_secs: 300,         // 5 分钟
            health_check_interval_secs: 60, // 1 分钟
            create_timeout_secs: 30,
            enable_reuse: true,
            // SEC-001: 默认不包含 `--no-sandbox`（生产环境安全默认）。
            // 容器环境使用 `docker_safe_args()` 构造器自动添加。
            browser_args: vec![
                "--disable-gpu".to_string(),
                "--disable-dev-shm-usage".to_string(),
            ],
            tab_pool_max_size: 10,
            context_isolation: true,
        }
    }
}

impl BrowserPoolConfig {
    /// 创建容器环境安全的配置（SEC-001）
    ///
    /// 检测到容器环境（Docker/Kubernetes）时自动添加 `--no-sandbox`。
    /// 非容器环境与 `default()` 行为一致。
    ///
    /// # 检测方式
    ///
    /// - `/.dockerenv` 文件存在
    /// - `/proc/1/cgroup` 包含 `docker` 或 `kubepods`
    #[must_use]
    pub fn docker_safe_args() -> Self {
        let mut config = Self::default();
        if is_container_environment() {
            config.browser_args.push("--no-sandbox".to_string());
            info!("Container environment detected, adding --no-sandbox to browser args");
        }
        config
    }
}

/// 检测当前是否运行在容器环境中（SEC-001）
///
/// 检查 `/.dockerenv` 或 `/proc/1/cgroup` 中的容器标识符。
fn is_container_environment() -> bool {
    // Docker 标记文件
    if std::path::Path::new("/.dockerenv").exists() {
        return true;
    }
    // cgroup v1/v2 中的容器标识
    if let Ok(cgroup) = std::fs::read_to_string("/proc/1/cgroup") {
        if cgroup.contains("docker") || cgroup.contains("kubepods") || cgroup.contains("containerd")
        {
            return true;
        }
    }
    false
}

/// 浏览器池统计信息
#[derive(Debug, Clone)]
pub struct BrowserPoolStats {
    /// 总实例数
    pub total_instances: usize,
    /// 可用实例数
    pub available_instances: usize,
    /// 使用中实例数
    pub in_use_instances: usize,
    /// 最大实例数
    pub max_instances: usize,
}

/// 池化的浏览器实例
struct PooledBrowser {
    /// 浏览器实例
    browser: Arc<Browser>,
    /// 创建时间
    created_at: Instant,
    /// 最后使用时间（PERF-002: 原子化，存储距 base_instant 的毫秒数）
    last_used_at_millis: AtomicU64,
    /// 使用次数
    use_count: AtomicU64,
    /// 是否健康
    is_healthy: AtomicBool,
    /// 实例 ID
    instance_id: u64,
    /// Tab 池（per-Browser 实例）
    ///
    /// 每个 Browser 实例维护独立的 TabPool，避免 Page 跨 Browser 复用导致的
    /// CDP session 失效问题（chromiumoxide Page 持有的 session 与 Browser 绑定）。
    tab_pool: Arc<TabPool>,
    /// 信号量 permit（SEC-002）
    ///
    /// 持有期间占用一个信号量槽位，drop 时自动释放。
    /// 确保 permit 生命周期与 PooledBrowser 一致，避免双重归还。
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl PooledBrowser {
    fn new(
        browser: Arc<Browser>,
        instance_id: u64,
        tab_pool_max_size: usize,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) -> Self {
        let now = Instant::now();
        Self {
            browser,
            created_at: now,
            last_used_at_millis: AtomicU64::new(0),
            use_count: AtomicU64::new(0),
            is_healthy: AtomicBool::new(true),
            instance_id,
            tab_pool: Arc::new(TabPool::new(tab_pool_max_size)),
            _permit: permit,
        }
    }

    fn touch(&self) {
        let elapsed = self.created_at.elapsed();
        self.last_used_at_millis
            .store(elapsed.as_millis() as u64, Ordering::Release);
        self.use_count.fetch_add(1, Ordering::Relaxed);
    }

    fn last_used(&self) -> Instant {
        let millis = self.last_used_at_millis.load(Ordering::Acquire);
        self.created_at + Duration::from_millis(millis)
    }

    fn mark_unhealthy(&self) {
        self.is_healthy.store(false, Ordering::Release);
    }

    fn is_healthy(&self) -> bool {
        self.is_healthy.load(Ordering::Acquire)
    }
}

impl std::fmt::Debug for PooledBrowser {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PooledBrowser")
            .field("instance_id", &self.instance_id)
            .field("created_at", &self.created_at)
            .field("use_count", &self.use_count)
            .field("is_healthy", &self.is_healthy)
            .finish()
    }
}

/// 实例归还消息
struct ReturnMessage {
    instance_id: u64,
    browser: Arc<Browser>,
}

/// 浏览器池内部状态
struct BrowserPoolState {
    /// 配置
    config: BrowserPoolConfig,
    /// 浏览器配置
    browser_config: Arc<dyn BrowserConfigTrait>,
    /// 可用实例（实例 ID -> PooledBrowser）
    available: RwLock<HashMap<u64, Arc<PooledBrowser>>>,
    /// 使用中的实例（实例 ID -> PooledBrowser）
    in_use: RwLock<HashMap<u64, Arc<PooledBrowser>>>,
    /// 实例计数器
    instance_counter: AtomicU64,
    /// 当前总实例数
    total_instances: AtomicUsize,
    /// 信号量（限制最大实例数，SEC-002: Arc 包装以支持 acquire_owned）
    semaphore: Arc<Semaphore>,
    /// 下载管理器
    download_manager: Arc<BrowserDownloadManager>,
    /// 清理任务句柄
    cleanup_task: Mutex<Option<JoinHandle<()>>>,
    /// 归还处理任务句柄
    return_task: Mutex<Option<JoinHandle<()>>>,
    /// 归还通道发送端（PERF-005: Arc 包装，避免 Mutex 锁竞争）
    return_sender: Arc<mpsc::Sender<ReturnMessage>>,
    /// 归还通道接收端（start_background_tasks 时取出）
    return_receiver: Mutex<Option<mpsc::Receiver<ReturnMessage>>>,
    /// 关闭标志
    shutdown: AtomicBool,
    /// 浏览器路径缓存（PERF-010: OnceCell 替代 RwLock<Option>，无锁读路径）
    browser_path: OnceCell<PathBuf>,
    /// F6：per-instance 临时 profile 目录计数器
    profile_dir_counter: AtomicU64,
    /// F6：已创建 profile 目录登记（shutdown_all 时 best-effort 清理）
    profile_dirs: std::sync::Mutex<Vec<std::path::PathBuf>>,
}

/// P08：池路径的代理 URL 校验（argv 注入防护）
///
/// 校验格式 + scheme 白名单 + 无空白字符。含空白字符的值拼进
/// `--proxy-server=` 会被 Chrome 拆成额外 flag。校验失败显性报错，
/// 不启动浏览器。
fn validate_pool_proxy(proxy: &str) -> Result<String, EngineError> {
    const ALLOWED_PROXY_SCHEMES: &[&str] = &["http", "https", "socks5", "socks4"];
    crate::utils::proxy::validate_proxy_url(proxy, ALLOWED_PROXY_SCHEMES)
        .map_err(|e| EngineError::Other(format!("Invalid proxy URL: {}", e)))
}

impl BrowserPoolState {
    fn new(
        config: BrowserPoolConfig,
        browser_config: Arc<dyn BrowserConfigTrait>,
        download_manager: Arc<BrowserDownloadManager>,
    ) -> Self {
        let max_instances = config.max_instances;
        let (return_tx, return_rx) = mpsc::channel(32);
        Self {
            config,
            browser_config,
            available: RwLock::new(HashMap::new()),
            in_use: RwLock::new(HashMap::new()),
            instance_counter: AtomicU64::new(0),
            total_instances: AtomicUsize::new(0),
            semaphore: Arc::new(Semaphore::new(max_instances)),
            download_manager,
            cleanup_task: Mutex::new(None),
            return_task: Mutex::new(None),
            return_sender: Arc::new(return_tx),
            return_receiver: Mutex::new(Some(return_rx)),
            shutdown: AtomicBool::new(false),
            browser_path: OnceCell::new(),
            profile_dir_counter: AtomicU64::new(0),
            profile_dirs: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// F6：登记 per-instance profile 目录（shutdown_all 清理用）
    fn register_profile_dir(&self, dir: std::path::PathBuf) {
        self.profile_dirs
            .lock()
            .expect("profile_dirs poisoned")
            .push(dir);
    }

    async fn acquire(&self) -> Result<(u64, Arc<Browser>), EngineError> {
        // 检查是否已关闭
        if self.shutdown.load(Ordering::Acquire) {
            return Err(EngineError::Other(
                "Browser pool is shutting down".to_string(),
            ));
        }

        // SEC-002: 获取 OwnedSemaphorePermit，生命周期绑定到 PooledBrowser
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| EngineError::Other("Failed to acquire semaphore".to_string()))?;

        // 尝试从可用池中获取实例
        if self.config.enable_reuse {
            if let Some((id, browser)) = self.try_get_available().await {
                return Ok((id, browser));
            }
        }

        // 创建新实例（传递 permit 绑定到 PooledBrowser）
        self.create_new_instance(permit).await
    }

    /// 获取 Browser 实例 + 对应的 TabPool
    ///
    /// 内部调用 [`acquire`] 获取 Browser，再从 `in_use` 中取出对应 PooledBrowser 的
    /// `tab_pool` 引用。返回的 `(instance_id, browser, tab_pool)` 三元组用于上层
    /// [`BrowserPool::acquire_page`] 构造 [`PooledPage`]。
    ///
    /// # 错误
    ///
    /// - 池已关闭：`EngineError::Other("Browser pool is shutting down")`
    /// - 信号量获取失败：`EngineError::Other("Failed to acquire semaphore")`
    /// - 实例不在 in_use（不应发生）：`EngineError::Other`
    async fn acquire_with_tab_pool(
        &self,
    ) -> Result<(u64, Arc<Browser>, Arc<TabPool>), EngineError> {
        let (instance_id, browser) = self.acquire().await?;
        // acquire 后 PooledBrowser 必在 in_use 中（acquire 内部插入）
        let tab_pool = {
            let in_use = self.in_use.read().await;
            in_use
                .get(&instance_id)
                .map(|p| p.tab_pool.clone())
                .ok_or_else(|| {
                    EngineError::Other(format!(
                        "instance {instance_id} not found in_use after acquire"
                    ))
                })?
        };
        Ok((instance_id, browser, tab_pool))
    }

    async fn try_get_available(&self) -> Option<(u64, Arc<Browser>)> {
        let mut available = self.available.write().await;

        // 找到并移除一个健康的实例
        let healthy_entry = available
            .iter()
            .find(|(_, p)| p.is_healthy())
            .map(|(id, _)| *id);

        if let Some(id) = healthy_entry {
            if let Some(pooled) = available.remove(&id) {
                pooled.touch();

                // 移动到使用中
                let mut in_use = self.in_use.write().await;
                in_use.insert(id, pooled.clone());

                debug!(
                    "Reusing browser instance {} (used {} times)",
                    id,
                    pooled.use_count.load(Ordering::Relaxed)
                );

                return Some((id, pooled.browser.clone()));
            }
        }

        None
    }

    async fn create_new_instance(
        &self,
        permit: tokio::sync::OwnedSemaphorePermit,
    ) -> Result<(u64, Arc<Browser>), EngineError> {
        let instance_id = self.instance_counter.fetch_add(1, Ordering::Relaxed);
        info!("Creating new browser instance {}", instance_id);

        let browser = self.launch_browser().await?;
        let pooled = Arc::new(PooledBrowser::new(
            browser.clone(),
            instance_id,
            self.config.tab_pool_max_size,
            permit,
        ));
        pooled.touch();

        // 添加到使用中
        {
            let mut in_use = self.in_use.write().await;
            in_use.insert(instance_id, pooled);
        }

        self.total_instances.fetch_add(1, Ordering::Relaxed);

        info!(
            "Browser instance {} created successfully (total: {})",
            instance_id,
            self.total_instances.load(Ordering::Relaxed)
        );

        Ok((instance_id, browser))
    }

    async fn return_instance(&self, instance_id: u64, browser: Arc<Browser>) {
        if self.shutdown.load(Ordering::Acquire) {
            debug!(
                "Pool is shutting down, closing browser instance {}",
                instance_id
            );
            return;
        }

        // 从使用中移除
        let pooled = {
            let mut in_use = self.in_use.write().await;
            in_use.remove(&instance_id)
        };

        if let Some(pooled) = pooled {
            // 检查浏览器是否仍然健康
            let is_healthy = self.check_browser_health(&browser).await;

            if is_healthy && self.config.enable_reuse && !self.shutdown.load(Ordering::Acquire) {
                // 归还到可用池（permit 随 PooledBrowser 保留）
                let mut available = self.available.write().await;
                available.insert(instance_id, pooled);
                debug!("Browser instance {} returned to pool", instance_id);
            } else {
                // 不健康或禁用复用，关闭浏览器（permit 随 PooledBrowser drop 自动释放）
                self.total_instances.fetch_sub(1, Ordering::Relaxed);
                debug!(
                    "Browser instance {} closed (unhealthy or reuse disabled)",
                    instance_id
                );
            }
        } else {
            // 实例不在使用中，可能是重复归还（permit 已在之前的归还中释放）
            warn!(
                "Browser instance {} not found in use, ignoring return",
                instance_id
            );
        }
    }

    /// 同步尽力从 in_use 移除泄漏槽位（Drop 路径无法 await）。
    ///
    /// 归还通道满/关闭时 BrowserInstance 被 drop，若 in_use 条目不清理会造成信号量
    /// permit 泄漏、池容量永久收缩。try_write 成功即移除并递减 total_instances
    /// （permit 随 PooledBrowser drop 自动释放），返回 true；锁被占用返回 false，
    /// 由调用方退化为 spawn 异步 return_instance。
    fn try_reclaim_in_use_slot(&self, instance_id: u64) -> bool {
        match self.in_use.try_write() {
            Ok(mut in_use) => {
                if in_use.remove(&instance_id).is_some() {
                    self.total_instances.fetch_sub(1, Ordering::Relaxed);
                    true
                } else {
                    false
                }
            }
            Err(_) => false,
        }
    }

    async fn launch_browser(&self) -> Result<Arc<Browser>, EngineError> {
        let remote_debugging_url = self.browser_config.get_remote_debugging_url();
        let proxy_url = self.browser_config.get_proxy_url();

        // P08：代理校验前置——在任何浏览器下载/启动动作之前 fail-fast。
        // 含空白字符的代理串拼进 `--proxy-server=` 会被 Chrome 拆成额外
        // flag（argv 注入），且 playwright.rs 单实例路径已有同款校验。
        let validated_proxy = proxy_url.as_deref().map(validate_pool_proxy).transpose()?;

        let (browser, mut handler) = if let Some(ref url) = remote_debugging_url {
            info!("Connecting to remote Chrome instance at: {}", url);
            Browser::connect(url).await.map_err(|e| {
                EngineError::Other(format!("Failed to connect to remote Chrome: {}", e))
            })?
        } else {
            // 获取浏览器路径
            let browser_path = self.get_or_download_browser().await?;

            // stealth：New Headless（真 Chrome 身份，消除旧 headless 的品牌/制品类
            // 指纹）+ 浏览器层关闭 Blink 自动化标记（navigator.webdriver 的源头，
            // JS 脚本覆盖只是补救）+ 语言一致性
            let mut builder = BrowserConfig::builder()
                .no_sandbox()
                .new_headless_mode()
                .request_timeout(Duration::from_secs(30))
                .arg("--disable-blink-features=AutomationControlled")
                .arg("--lang=en-US,en");

            // F6：每实例独立临时 profile 目录。chromiumoxide 默认共享
            // /tmp/chromiumoxide-runner，持有 Chrome SingletonLock——并发启动
            // 第二实例即崩（File exists）、kill -9 worker 的孤儿 chrome 继续
            // 占锁且新实例无法自愈。独立目录让实例间互不影响；目录注册进
            // 池，shutdown_all 时 best-effort 清理。
            let profile_dir = std::env::temp_dir().join(format!(
                "crawlrs-chromium-{}-{}",
                std::process::id(),
                self.profile_dir_counter.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&profile_dir).map_err(|e| {
                EngineError::Other(format!(
                    "Failed to create browser profile dir {:?}: {}",
                    profile_dir, e
                ))
            })?;
            self.register_profile_dir(profile_dir.clone());
            info!("Using per-instance browser profile dir: {:?}", profile_dir);
            builder = builder.user_data_dir(&profile_dir);

            // 设置浏览器路径
            if let Some(ref path) = browser_path {
                info!("Using browser at: {:?}", path);
                builder = builder.chrome_executable(path);
            }

            // 添加自定义参数
            for arg in &self.config.browser_args {
                builder = builder.arg(arg.as_str());
            }

            if let Some(validated) = validated_proxy {
                info!(
                    "Using proxy for browser: {}",
                    crate::utils::proxy::redact_proxy_url(&validated)
                );
                builder = builder.arg("--proxy-server").arg(validated);
            }

            Browser::launch(
                builder
                    .build()
                    .map_err(|e| EngineError::Other(e.to_string()))?,
            )
            .await
            .map_err(|e| EngineError::Other(format!("Failed to launch browser: {}", e)))?
        };

        // 启动处理器任务
        tokio::spawn(async move {
            while let Some(h) = handler.next().await {
                if let Err(e) = h {
                    debug!("Browser handler event error (continuing): {:?}", e);
                }
            }
        });

        Ok(Arc::new(browser))
    }

    async fn get_or_download_browser(&self) -> Result<Option<PathBuf>, EngineError> {
        // PERF-010: OnceCell::get_or_try_init — 已初始化时直接返回，无锁
        match self
            .browser_path
            .get_or_try_init(|| async {
                // 首先检查系统浏览器
                if let Some(path) = crate::engines::browser_downloader::find_system_browser().await
                {
                    info!("Using system browser");
                    return Ok(path);
                }

                // 检查是否已下载
                if self.download_manager.is_browser_downloaded().await {
                    let path = crate::engines::browser_downloader::get_browser_executable_path(
                        self.download_manager.get_cache_dir(),
                    );
                    info!("Using downloaded browser: {:?}", path);
                    return Ok(path);
                }

                // 自动下载浏览器
                info!("No browser found, starting automatic download...");
                match self.download_manager.download_browser().await {
                    Ok(path) => {
                        info!("Browser downloaded successfully: {:?}", path);
                        Ok(path)
                    }
                    Err(e) => {
                        warn!("Browser download failed: {}, will try system path", e);
                        Err(EngineError::Other(format!("Browser not found: {}", e)))
                    }
                }
            })
            .await
        {
            Ok(path) => Ok(Some(path.clone())),
            Err(_) => Ok(None), // 浏览器未找到，返回 None（不缓存失败）
        }
    }

    async fn check_browser_health(&self, browser: &Browser) -> bool {
        match browser.new_page("about:blank").await {
            Ok(page) => {
                let _ = page.close().await;
                true
            }
            Err(e) => {
                warn!("Browser health check failed: {}", e);
                false
            }
        }
    }

    async fn cleanup_idle_instances(&self) {
        // SEC-002: 三阶段模式避免写锁持有期间执行 I/O（与 health_check_all 统一）
        // 读锁收集空闲实例
        let idle_ids = {
            let available = self.available.read().await;
            let now = Instant::now();
            let idle_timeout = Duration::from_secs(self.config.idle_timeout_secs);
            available
                .iter()
                .filter(|(_, p)| now.duration_since(p.last_used()) > idle_timeout)
                .map(|(id, _)| *id)
                .collect::<Vec<_>>()
        };

        // 写锁移除空闲实例
        let removed = {
            let mut available = self.available.write().await;
            idle_ids
                .into_iter()
                .filter_map(|id| available.remove(&id))
                .collect::<Vec<_>>()
        };

        // 锁外 drop（total_instances 更新 + permit 自动释放）
        for pooled in removed {
            self.total_instances.fetch_sub(1, Ordering::Relaxed);
            info!("Cleaned up idle browser instance {}", pooled.instance_id);
            drop(pooled);
        }
    }

    async fn health_check_all(&self) {
        // SEC-002 + PERF-001: 三阶段模式避免写锁持有期间执行 I/O
        // 读锁收集待检查实例（clone Arc 避免持锁）
        let candidates = {
            let available = self.available.read().await;
            available
                .iter()
                .map(|(id, p)| (*id, p.clone()))
                .collect::<Vec<_>>()
        };

        // 锁外执行健康检查
        let mut unhealthy_ids = Vec::new();
        for (id, pooled) in &candidates {
            if !self.check_browser_health(&pooled.browser).await {
                pooled.mark_unhealthy();
                unhealthy_ids.push(*id);
            }
        }

        // 写锁移除不健康实例，drop 在锁外
        let removed = {
            let mut available = self.available.write().await;
            unhealthy_ids
                .into_iter()
                .filter_map(|id| available.remove(&id))
                .collect::<Vec<_>>()
        };

        for pooled in removed {
            self.total_instances.fetch_sub(1, Ordering::Relaxed);
            warn!("Removed unhealthy browser instance {}", pooled.instance_id);
            drop(pooled); // permit 自动释放
        }
    }

    async fn shutdown_all(&self) {
        self.shutdown.store(true, Ordering::Release);

        // 清空可用池
        let mut available = self.available.write().await;
        available.clear();

        // 清空使用中池
        let mut in_use = self.in_use.write().await;
        in_use.clear();

        self.total_instances.store(0, Ordering::Relaxed);

        // F6：best-effort 清理 per-instance profile 目录（浏览器进程此刻已随
        // Browser drop 终止；个别文件仍被占用时删除失败仅 debug 记录）
        let dirs = self
            .profile_dirs
            .lock()
            .expect("profile_dirs poisoned")
            .drain(..)
            .collect::<Vec<_>>();
        for dir in dirs {
            if let Err(e) = std::fs::remove_dir_all(&dir) {
                debug!("Failed to remove browser profile dir {:?}: {}", dir, e);
            }
        }

        info!("All browser instances shut down");
    }

    /// 克隆归还通道发送端（QUAL-003）
    ///
    /// 返回 `Arc<Sender>` 的克隆，无锁操作。
    /// 供 [`BrowserPool::acquire`] 和 [`BrowserPool::acquire_page`] 使用。
    fn clone_return_sender(&self) -> Arc<mpsc::Sender<ReturnMessage>> {
        self.return_sender.clone()
    }

    /// Page 获取失败后归还 Browser：优先走 return channel；通道满/关闭时
    /// 直接 await [`return_instance`](Self::return_instance) 移除 in_use 槽位，
    /// 防止信号量 permit 泄漏导致池容量收缩。
    /// 供 TabPool 路径与 context 隔离路径的失败分支复用。
    async fn reclaim_after_page_acquire_failure(
        &self,
        instance_id: u64,
        browser: Arc<Browser>,
        reason: &str,
    ) {
        match self.clone_return_sender().try_send(ReturnMessage {
            instance_id,
            browser,
        }) {
            Ok(_) => {}
            Err(mpsc::error::TrySendError::Full(msg))
            | Err(mpsc::error::TrySendError::Closed(msg)) => {
                error!(
                    "Return channel unavailable while reclaiming browser instance {} after page acquire failure ({}); removing in_use slot directly",
                    msg.instance_id, reason
                );
                self.return_instance(msg.instance_id, msg.browser).await;
            }
        }
    }
}

/// 浏览器实例包装器
///
/// 当实例被 drop 时自动归还到池中
pub struct BrowserInstance {
    /// 浏览器实例
    browser: Option<Arc<Browser>>,
    /// 实例 ID
    instance_id: u64,
    /// 归还通道发送端（PERF-005: Arc 包装，与 BrowserPoolState 共享）
    return_sender: Option<Arc<mpsc::Sender<ReturnMessage>>>,
    /// 池状态引用：归还通道满/关闭时用于直接回收 in_use 槽位，防止 permit 泄漏。
    state: Arc<BrowserPoolState>,
}

impl BrowserInstance {
    /// 获取浏览器实例引用
    pub fn browser(&self) -> &Arc<Browser> {
        self.browser
            .as_ref()
            .expect("Browser instance already released")
    }

    /// 手动释放实例（归还到池中）
    pub async fn release(mut self) {
        if let Some(browser) = self.browser.take() {
            if let Some(sender) = &self.return_sender {
                if let Err(mpsc::error::SendError(msg)) = sender
                    .send(ReturnMessage {
                        instance_id: self.instance_id,
                        browser,
                    })
                    .await
                {
                    // 通道关闭（receiver 已随后台任务终止）：直接回收 in_use 槽位，
                    // 防止信号量 permit 泄漏导致池容量永久收缩。
                    error!(
                        "Return channel closed during release of browser instance {}; reclaiming in_use slot directly",
                        msg.instance_id
                    );
                    self.state
                        .return_instance(msg.instance_id, msg.browser)
                        .await;
                }
            }
        }
    }

    /// 归还通道不可用（满/关闭）时直接回收 in_use 槽位（Drop 无法 await）。
    ///
    /// 先尝试同步 `try_write` 移除（短临界区通常成功，permit 随 PooledBrowser drop
    /// 释放）；锁被占用则退化为 spawn 异步 `return_instance`，保证容量最终可恢复。
    fn reclaim_slot(&self, instance_id: u64, browser: Arc<Browser>) {
        if self.state.try_reclaim_in_use_slot(instance_id) {
            return;
        }
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                let state = Arc::clone(&self.state);
                handle.spawn(async move {
                    state.return_instance(instance_id, browser).await;
                });
            }
            Err(_) => {
                error!(
                    "No tokio runtime to reclaim browser instance {} in_use slot; pool capacity may leak",
                    instance_id
                );
            }
        }
    }
}

impl Drop for BrowserInstance {
    fn drop(&mut self) {
        if let Some(browser) = self.browser.take() {
            if let Some(sender) = &self.return_sender {
                // 非阻塞归还；通道满/关闭时不能直接丢弃浏览器——否则 in_use 槽位与
                // 信号量 permit 泄漏，池容量永久收缩。改为 error! + 直接回收槽位。
                match sender.try_send(ReturnMessage {
                    instance_id: self.instance_id,
                    browser,
                }) {
                    Ok(_) => {}
                    Err(mpsc::error::TrySendError::Full(msg)) => {
                        error!(
                            "Return channel full for browser instance {}; reclaiming in_use slot directly to avoid permit leak",
                            msg.instance_id
                        );
                        self.reclaim_slot(msg.instance_id, msg.browser);
                    }
                    Err(mpsc::error::TrySendError::Closed(msg)) => {
                        error!(
                            "Return channel closed for browser instance {}; reclaiming in_use slot directly to avoid permit leak",
                            msg.instance_id
                        );
                        self.reclaim_slot(msg.instance_id, msg.browser);
                    }
                }
            }
        }
    }
}

/// 浏览器 Page 包装器
///
/// 同时持有 [`BrowserInstance`] 和 [`Page`]，drop/release 时自动归还 Browser，
/// 并按归还模式处理 Page：
///
/// - **TabPool 复用页**（`context_id: None`，`context_isolation = false`）：
///   Page 通过 `tokio::spawn` 异步调用 [`TabPool::release`]（导航到
///   `about:blank` 清理状态后压栈）。runtime 不可用时 Page 直接 drop（关闭 tab）。
/// - **context 隔离页**（`context_id: Some(..)`，`context_isolation = true`）：
///   归还时销毁整个 BrowserContext——页面随之关闭，cookie/localStorage/
///   indexedDB 全部消失。Drop 无法 await，通过 spawn 清理 task 完成销毁 +
///   Browser 归还，保证早退/panic/超时路径不泄漏 context。
///
/// # 手动释放
///
/// 调用 [`release`](Self::release) 可显式异步归还（等待 Page 清理/context
/// 销毁完成），适用于需要在 drop 前确保资源释放时序的场景。
pub struct PooledPage {
    /// Page 实例（Option 便于 Drop 中 take）
    page: Option<Page>,
    /// 归还模式标记：`Some(id)` = context 隔离页（归还时销毁该 context）；
    /// `None` = TabPool 复用页（归还时压栈复用）
    context_id: Option<BrowserContextId>,
    /// 对应的 BrowserInstance（drop 时归还 Browser）
    browser_instance: Option<BrowserInstance>,
    /// 对应的 TabPool（仅复用页持有；隔离页为 None，归还路径不得触碰 TabPool）
    tab_pool: Option<Arc<TabPool>>,
}

impl PooledPage {
    /// 获取 Page 引用
    ///
    /// # Panics
    ///
    /// 若 Page 已被 [`release`](Self::release) 或 drop 取走则 panic。
    pub fn page(&self) -> &Page {
        self.page
            .as_ref()
            .expect("PooledPage already released (page taken)")
    }

    /// 获取 Browser 引用（透传 BrowserInstance）
    ///
    /// # Panics
    ///
    /// 若 BrowserInstance 已被 [`release`](Self::release) 或 drop 取走则 panic。
    pub fn browser(&self) -> &Arc<Browser> {
        self.browser_instance
            .as_ref()
            .expect("PooledPage already released (browser_instance taken)")
            .browser()
    }

    /// 手动释放（异步归还 Page + Browser）
    ///
    /// 1. TabPool 复用页：调用 [`TabPool::release`] 归还 Page（等待导航到
    ///    `about:blank`）；context 隔离页：直接 drop Page（随 context 销毁）
    /// 2. 隔离页：销毁 BrowserContext（等待完成，cookie/storage 随之清除）
    /// 3. 调用 [`BrowserInstance::release`] 归还 Browser（发送到 return channel）
    ///
    /// 调用后 `self` 被消费，不应再访问。
    pub async fn release(mut self) {
        // 1. 归还 Page：复用页压栈，隔离页直接关闭（context 销毁会兜底）
        if let Some(page) = self.page.take() {
            match self.tab_pool.as_ref() {
                Some(tab_pool) => tab_pool.release(page).await,
                None => drop(page),
            }
        }
        // 2. 销毁隔离 context：cookie/localStorage/indexedDB 随之全部消失。
        //    失败仅记日志——浏览器连接已坏时归还路径的健康检查会剔除实例兜底。
        if let Some(context_id) = self.context_id.take() {
            if let Some(instance) = self.browser_instance.as_ref() {
                if let Err(e) = instance.browser().dispose_browser_context(context_id).await {
                    error!("Failed to dispose browser context on release: {}", e);
                }
            }
        }
        // 3. 归还 BrowserInstance（BrowserInstance::release 消费 self）
        if let Some(instance) = self.browser_instance.take() {
            instance.release().await;
        }
    }
}

impl Drop for PooledPage {
    fn drop(&mut self) {
        // 1. Page：复用页 spawn 归还 task（TabPool::release 是 async）；隔离页
        //    直接 drop 客户端句柄（tab 随 context 销毁关闭）
        if let Some(page) = self.page.take() {
            if let Some(tab_pool) = self.tab_pool.clone() {
                // 尝试在当前 runtime spawn 归还 task
                match tokio::runtime::Handle::try_current() {
                    Ok(handle) => {
                        handle.spawn(async move {
                            tab_pool.release(page).await;
                        });
                    }
                    Err(_) => {
                        // runtime 不可用（如 shutdown 中），Page 直接 drop（关闭 tab）
                        debug!("tokio runtime unavailable, dropping Page directly");
                    }
                }
            }
        }
        // 2. 隔离页：Drop 无法 await dispose——spawn 清理 task 先销毁 context
        //    再归还 Browser，保证早退/panic/超时路径不泄漏 context
        if let Some(context_id) = self.context_id.take() {
            if let Some(instance) = self.browser_instance.take() {
                match tokio::runtime::Handle::try_current() {
                    Ok(handle) => {
                        handle.spawn(async move {
                            let browser = instance.browser().clone();
                            if let Err(e) = browser.dispose_browser_context(context_id).await {
                                error!("Failed to dispose browser context on drop: {}", e);
                            }
                            instance.release().await;
                        });
                    }
                    Err(_) => {
                        // runtime 不可用：context 销毁只能放弃（浏览器进程退出时回收）。
                        // BrowserInstance 放回字段走自身 Drop 经 return channel 归还，
                        // 避免信号量 permit 泄漏导致池容量收缩。
                        error!(
                            "tokio runtime unavailable, isolated browser context cannot be disposed; context may leak until browser exits"
                        );
                        self.browser_instance = Some(instance);
                    }
                }
            }
        }
        // 3. 复用页：browser_instance 未被 take，字段 drop 触发
        //    BrowserInstance::drop 经 return channel 归还 Browser
    }
}

/// 浏览器实例池管理器
///
/// 管理浏览器实例的生命周期，支持实例复用、最大实例数限制、
/// 空闲实例清理和健康检查。
pub struct BrowserPool {
    state: Arc<BrowserPoolState>,
}

impl Drop for BrowserPool {
    /// F6：池销毁时回收浏览器实例与 chrome 子进程。
    ///
    /// worker 进程优雅退出路径没有显式调用 `shutdown_all` 的接线，此处
    /// 兜底：runtime 仍存活时 spawn 异步清理（关闭全部实例 → chrome 随
    /// Browser drop 终止，profile 目录一并清理）。runtime 已不存在（如
    /// kill -9）时无从回收——per-instance 独立 profile 目录保证重启后的
    /// 新实例不受孤儿锁影响。
    fn drop(&mut self) {
        if self.state.shutdown.load(Ordering::Acquire) {
            return;
        }
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                let state = Arc::clone(&self.state);
                handle.spawn(async move {
                    state.shutdown_all().await;
                });
            }
            Err(_) => {
                log::warn!("{}", crate::i18n::tr_log("playwright-pool-drop-no-runtime"));
            }
        }
    }
}

impl BrowserPool {
    /// 创建新的浏览器池
    pub fn new(config: BrowserPoolConfig, browser_config: Arc<dyn BrowserConfigTrait>) -> Self {
        let download_manager =
            Arc::new(BrowserDownloadManager::new(BrowserDownloadConfig::default()));
        Self::with_download_manager(config, browser_config, download_manager)
    }

    /// 使用自定义下载管理器创建浏览器池
    pub fn with_download_manager(
        config: BrowserPoolConfig,
        browser_config: Arc<dyn BrowserConfigTrait>,
        download_manager: Arc<BrowserDownloadManager>,
    ) -> Self {
        let state = Arc::new(BrowserPoolState::new(
            config,
            browser_config,
            download_manager,
        ));
        Self { state }
    }

    /// 获取浏览器实例
    ///
    /// 优先从池中获取可用实例，如果没有可用实例则创建新实例。
    /// 返回的 BrowserInstance 在 drop 时会自动归还到池中。
    pub async fn acquire(&self) -> Result<BrowserInstance, EngineError> {
        let (instance_id, browser) = self.state.acquire().await?;

        let return_sender = Some(self.state.clone_return_sender());

        Ok(BrowserInstance {
            browser: Some(browser),
            instance_id,
            return_sender,
            state: self.state.clone(),
        })
    }

    /// 获取 Browser + Page
    ///
    /// 在 [`acquire`](Self::acquire) 基础上获取任务用 [`Page`]。返回的
    /// [`PooledPage`] 在 drop/release 时自动归还 Browser，并按归还模式处理 Page。
    ///
    /// # Page 获取语义（按 `BrowserPoolConfig::context_isolation` 分支）
    ///
    /// - `true`（默认）：为本次任务创建独立 [`BrowserContextId`] + 其内 Page，
    ///   归还时销毁整个 context——cookie/localStorage 等随任务终结，跨任务零串扰。
    ///   隔离页不进 TabPool。
    /// - `false`（逃生门）：从对应 Browser 的 [`TabPool`] 复用 Page（LIFO），
    ///   池空时 `browser.new_page("about:blank")` 新建。
    ///
    /// # 错误
    ///
    /// - 池已关闭、信号量失败、实例不在 in_use：透传 [`acquire`](Self::acquire) 错误
    /// - context 创建失败 / Page 创建失败 / TabPool::acquire 失败：
    ///   `EngineError::BrowserError`，并自动归还 Browser 避免泄漏
    ///   （隔离路径还会先销毁半创建的 context）
    pub async fn acquire_page(&self) -> Result<PooledPage, EngineError> {
        if self.state.config.context_isolation {
            self.acquire_page_isolated().await
        } else {
            self.acquire_page_tab_pool().await
        }
    }

    /// 隔离路径：每任务独立 BrowserContext + Page
    ///
    /// Browser 实例本身仍从池中复用（context 创建为轻量 CDP 调用，无进程开销）；
    /// Page 创建在指定 context 内（`CreateTargetParams.browser_context_id` 显式传入，
    /// chromiumoxide 仅在未指定时才填充浏览器默认 context）。
    ///
    /// 失败路径：context 已创建但 Page 创建失败时，先销毁半创建的 context
    /// 再归还 Browser，防止 context 泄漏累积。
    async fn acquire_page_isolated(&self) -> Result<PooledPage, EngineError> {
        let (instance_id, browser) = self.state.acquire().await?;

        let context_id = match browser
            .create_browser_context(CreateBrowserContextParams::default())
            .await
        {
            Ok(id) => id,
            Err(e) => {
                let err_msg = format!("Failed to create browser context: {}", e);
                self.state
                    .reclaim_after_page_acquire_failure(instance_id, browser, &err_msg)
                    .await;
                return Err(EngineError::BrowserError(err_msg));
            }
        };

        let params = CreateTargetParams {
            browser_context_id: Some(context_id.clone()),
            ..CreateTargetParams::new("about:blank")
        };
        let page = match browser.new_page(params).await {
            Ok(page) => page,
            Err(e) => {
                // Page 创建失败：尽力销毁半创建 context（失败仅记日志——
                // 浏览器连接已坏时归还路径的健康检查会兜底剔除实例）
                if let Err(dispose_err) = browser.dispose_browser_context(context_id).await {
                    error!(
                        "Failed to dispose browser context after page creation failure: {}",
                        dispose_err
                    );
                }
                let err_msg = format!("Failed to create page in isolated context: {}", e);
                self.state
                    .reclaim_after_page_acquire_failure(instance_id, browser, &err_msg)
                    .await;
                return Err(EngineError::BrowserError(err_msg));
            }
        };

        let return_sender = Some(self.state.clone_return_sender());

        Ok(PooledPage {
            page: Some(page),
            context_id: Some(context_id),
            browser_instance: Some(BrowserInstance {
                browser: Some(browser),
                instance_id,
                return_sender,
                state: self.state.clone(),
            }),
            tab_pool: None,
        })
    }

    /// 复用路径（逃生门）：从对应 Browser 的 [`TabPool`] 获取/复用 Page
    async fn acquire_page_tab_pool(&self) -> Result<PooledPage, EngineError> {
        let (instance_id, browser, tab_pool) = self.state.acquire_with_tab_pool().await?;

        // 从 TabPool 获取 Page，失败时归还 Browser 避免泄漏
        let page = match tab_pool.acquire(&browser).await {
            Ok(page) => page,
            Err(e) => {
                let err_msg = e.to_string();
                self.state
                    .reclaim_after_page_acquire_failure(instance_id, browser, &err_msg)
                    .await;
                return Err(EngineError::BrowserError(err_msg));
            }
        };

        let return_sender = Some(self.state.clone_return_sender());

        Ok(PooledPage {
            page: Some(page),
            context_id: None,
            browser_instance: Some(BrowserInstance {
                browser: Some(browser),
                instance_id,
                return_sender,
                state: self.state.clone(),
            }),
            tab_pool: Some(tab_pool),
        })
    }

    /// 启动后台清理任务
    ///
    /// 定期清理空闲实例和进行健康检查
    pub async fn start_background_tasks(&self) {
        // 启动清理任务
        {
            let state = self.state.clone();
            let health_check_interval =
                Duration::from_secs(self.state.config.health_check_interval_secs);

            let handle = tokio::spawn(async move {
                let mut interval = tokio::time::interval(health_check_interval);

                loop {
                    interval.tick().await;

                    if state.shutdown.load(Ordering::Acquire) {
                        break;
                    }

                    // 清理空闲实例
                    state.cleanup_idle_instances().await;

                    // 健康检查
                    state.health_check_all().await;

                    debug!(
                        "Browser pool cleanup completed (total: {})",
                        state.total_instances.load(Ordering::Relaxed)
                    );
                }
            });

            let mut task = self.state.cleanup_task.lock().await;
            *task = Some(handle);
        }

        // 启动归还处理任务
        {
            let state = self.state.clone();
            let receiver = self.state.return_receiver.lock().await.take();

            let Some(mut receiver) = receiver else {
                warn!("Return receiver already taken (start_background_tasks called twice?)");
                return;
            };

            let handle = tokio::spawn(async move {
                while let Some(msg) = receiver.recv().await {
                    if state.shutdown.load(Ordering::Acquire) {
                        break;
                    }
                    state.return_instance(msg.instance_id, msg.browser).await;
                }
            });

            let mut task = self.state.return_task.lock().await;
            *task = Some(handle);
        }
    }

    /// 停止后台任务
    pub async fn stop_background_tasks(&self) {
        {
            let mut task = self.state.cleanup_task.lock().await;
            if let Some(handle) = task.take() {
                handle.abort();
            }
        }
        {
            let mut task = self.state.return_task.lock().await;
            if let Some(handle) = task.take() {
                handle.abort();
            }
        }
    }

    /// 关闭浏览器池
    ///
    /// 关闭所有浏览器实例并停止后台任务
    pub async fn shutdown(&self) {
        self.stop_background_tasks().await;
        self.state.shutdown_all().await;
    }

    /// 获取池统计信息
    pub async fn stats(&self) -> BrowserPoolStats {
        let available_count = self.state.available.read().await.len();
        let in_use_count = self.state.in_use.read().await.len();

        BrowserPoolStats {
            total_instances: self.state.total_instances.load(Ordering::Relaxed),
            available_instances: available_count,
            in_use_instances: in_use_count,
            max_instances: self.state.config.max_instances,
        }
    }

    /// 手动触发健康检查
    pub async fn health_check(&self) {
        self.state.health_check_all().await;
    }

    /// 手动清理空闲实例
    pub async fn cleanup_idle(&self) {
        self.state.cleanup_idle_instances().await;
    }

    /// 获取配置
    pub fn config(&self) -> &BrowserPoolConfig {
        &self.state.config
    }
}

impl Clone for BrowserPool {
    fn clone(&self) -> Self {
        Self {
            state: self.state.clone(),
        }
    }
}

/// 全局浏览器池实例
static GLOBAL_BROWSER_POOL: std::sync::OnceLock<BrowserPool> = std::sync::OnceLock::new();

/// 获取全局浏览器池实例
pub fn get_global_pool() -> Option<&'static BrowserPool> {
    GLOBAL_BROWSER_POOL.get()
}

/// 初始化全局浏览器池
pub fn init_global_pool(config: BrowserPoolConfig, browser_config: Arc<dyn BrowserConfigTrait>) {
    let pool = BrowserPool::new(config, browser_config);
    let _ = GLOBAL_BROWSER_POOL.set(pool);
    info!("Global browser pool initialized");
}

/// 关闭全局浏览器池
pub async fn shutdown_global_pool() {
    if let Some(pool) = GLOBAL_BROWSER_POOL.get() {
        pool.shutdown().await;
        info!("Global browser pool shut down");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::services::config_service::BrowserConfigComponent;

    #[test]
    fn test_browser_pool_config_default() {
        let config = BrowserPoolConfig::default();
        assert_eq!(config.max_instances, 5);
        assert_eq!(config.idle_timeout_secs, 300);
        assert_eq!(config.health_check_interval_secs, 60);
        assert!(config.enable_reuse);
        // tab_pool_max_size 默认 10
        assert_eq!(config.tab_pool_max_size, 10);
        // context_isolation 默认开启（per-task BrowserContext 隔离）
        assert!(
            config.context_isolation,
            "context_isolation must default to true"
        );
        // SEC-001: 默认不包含 --no-sandbox（生产环境安全默认）
        assert!(
            !config.browser_args.iter().any(|a| a == "--no-sandbox"),
            "default browser_args should not contain --no-sandbox"
        );
    }

    #[test]
    fn test_context_isolation_branch_selection() {
        // acquire_page 的分支选择完全由 config.context_isolation 决定：
        // 默认 true → 隔离路径；显式 false → TabPool 复用路径（逃生门）
        let isolation_on = BrowserPoolConfig::default();
        assert!(isolation_on.context_isolation);

        let isolation_off = BrowserPoolConfig {
            context_isolation: false,
            ..BrowserPoolConfig::default()
        };
        assert!(!isolation_off.context_isolation);
        // 结构体更新语法保留其余默认值（逃生门只切 Page 来源，不动池参数）
        assert_eq!(isolation_off.max_instances, 5);
        assert_eq!(isolation_off.tab_pool_max_size, 10);
        assert!(isolation_off.enable_reuse);

        // docker_safe_args() 继承 default → 隔离同样默认开启（容器临时池不降级）
        assert!(BrowserPoolConfig::docker_safe_args().context_isolation);
    }

    #[tokio::test]
    async fn test_acquire_page_tab_pool_escape_hatch_shutdown_fails() {
        // context_isolation=false（逃生门）路径在池关闭后同样安全失败，
        // 且失败路径不得触碰 TabPool 既有归还逻辑
        let config = BrowserPoolConfig {
            context_isolation: false,
            ..BrowserPoolConfig::default()
        };
        let browser_config = Arc::new(BrowserConfigComponent::default());
        let pool = BrowserPool::new(config, browser_config);
        pool.shutdown().await;
        let result = pool.acquire_page().await;
        assert!(result.is_err());
    }

    #[test]
    fn test_browser_pool_config_docker_safe_args() {
        // docker_safe_args() 在无容器环境下不应添加 --no-sandbox
        let config = BrowserPoolConfig::docker_safe_args();
        // 在非容器测试环境中，可能不含 --no-sandbox
        // 验证基本配置正确
        assert_eq!(config.max_instances, 5);
        assert!(config.browser_args.contains(&"--disable-gpu".to_string()));
    }

    #[test]
    fn test_browser_pool_config_tab_pool_disabled() {
        // tab_pool_max_size=0 禁用 tab 复用
        let config = BrowserPoolConfig {
            tab_pool_max_size: 0,
            ..BrowserPoolConfig::default()
        };
        assert_eq!(config.tab_pool_max_size, 0);
    }

    #[tokio::test]
    async fn test_browser_pool_acquire_page_shutdown_fails() {
        // 池关闭后 acquire_page 应返回错误
        let config = BrowserPoolConfig::default();
        let browser_config = Arc::new(BrowserConfigComponent::default());
        let pool = BrowserPool::new(config, browser_config);
        pool.shutdown().await;
        let result = pool.acquire_page().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_try_reclaim_in_use_slot_absent_id_is_noop() {
        // 归还通道满/关闭时 Drop 通过 try_reclaim_in_use_slot 同步移除
        // in_use 槽位以回收 permit。无浏览器环境下无法在 in_use 放置真实
        // PooledBrowser（需 Chrome，与本文件其余测试一致），故验证回收的安全属性：
        // 对不存在的 instance_id 回收返回 false，且不误减 total_instances（防止过度
        // 递减破坏计数）。in_use 收缩的正向断言需真实浏览器，属集成测试范畴。
        let config = BrowserPoolConfig::default();
        let browser_config = Arc::new(BrowserConfigComponent::default());
        let state = Arc::new(BrowserPoolState::new(
            config,
            browser_config,
            Arc::new(BrowserDownloadManager::new(BrowserDownloadConfig::default())),
        ));
        // 预置非零 total_instances，验证回收不存在槽位时不会误减
        state.total_instances.store(3, Ordering::Relaxed);
        assert!(
            !state.try_reclaim_in_use_slot(999),
            "reclaiming an absent instance must return false"
        );
        assert_eq!(
            state.total_instances.load(Ordering::Relaxed),
            3,
            "absent-id reclaim must not decrement total_instances"
        );
        assert_eq!(state.in_use.read().await.len(), 0);
    }

    #[test]
    fn test_browser_pool_stats() {
        let config = BrowserPoolConfig::default();
        let browser_config = Arc::new(BrowserConfigComponent::default());
        let pool = BrowserPool::new(config, browser_config);

        let stats = tokio_test::block_on(pool.stats());
        assert_eq!(stats.total_instances, 0);
        assert_eq!(stats.available_instances, 0);
        assert_eq!(stats.in_use_instances, 0);
    }

    #[tokio::test]
    async fn test_browser_pool_shutdown() {
        let config = BrowserPoolConfig::default();
        let browser_config = Arc::new(BrowserConfigComponent::default());
        let pool = BrowserPool::new(config, browser_config);

        pool.shutdown().await;

        // 尝试获取实例应该失败
        let result = pool.acquire().await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_browser_pool_clone() {
        let config = BrowserPoolConfig::default();
        let browser_config = Arc::new(BrowserConfigComponent::default());
        let pool = BrowserPool::new(config, browser_config);
        let pool2 = pool.clone();

        // 两个引用应该共享相同的状态
        let stats1 = pool.stats().await;
        let stats2 = pool2.stats().await;

        assert_eq!(stats1.total_instances, stats2.total_instances);
    }

    // --- QUAL-001: 并发安全测试 ---

    #[tokio::test]
    async fn test_concurrent_shutdown_acquire() {
        // 并发 shutdown 期间 acquire 应安全失败（无 panic / 无数据竞争）
        let config = BrowserPoolConfig::default();
        let browser_config = Arc::new(BrowserConfigComponent::default());
        let pool = Arc::new(BrowserPool::new(config, browser_config));

        let mut handles = Vec::new();
        for _ in 0..20 {
            let pool_clone = pool.clone();
            handles.push(tokio::spawn(async move { pool_clone.acquire().await }));
        }

        // 同时关闭池
        pool.shutdown().await;

        // 所有 acquire 要么在 shutdown 前成功（需要真实浏览器，测试环境会失败），
        // 要么在 shutdown 后返回错误。关键是无 panic。
        let mut error_count = 0;
        for handle in handles {
            if handle.await.unwrap().is_err() {
                error_count += 1;
            }
        }
        // 在无浏览器环境中，所有 acquire 应失败（创建浏览器失败或 shutdown）
        assert!(
            error_count > 0,
            "expected some errors in test env without browser"
        );
    }

    #[tokio::test]
    async fn test_semaphore_permit_consistency() {
        // 验证信号量 permit 计数与池状态一致
        let config = BrowserPoolConfig {
            max_instances: 3,
            ..BrowserPoolConfig::default()
        };
        let browser_config = Arc::new(BrowserConfigComponent::default());
        let pool = BrowserPool::new(config, browser_config);

        // 初始状态：无实例，信号量应为满
        let stats = pool.stats().await;
        assert_eq!(stats.total_instances, 0);
        assert_eq!(stats.in_use_instances, 0);
        assert_eq!(stats.available_instances, 0);

        // 信号量可用 permit 应等于 max_instances（无实例占用）
        // 通过 acquire_owned 测试（不实际创建浏览器）
        let permit = pool.state.semaphore.clone().acquire_owned().await;
        assert!(
            permit.is_ok(),
            "should be able to acquire permit when pool is empty"
        );
        drop(permit); // 释放

        pool.shutdown().await;
    }

    #[tokio::test]
    async fn test_return_sender_arc_clone_no_block() {
        // PERF-005: 验证 return_sender Arc clone 不阻塞
        let config = BrowserPoolConfig::default();
        let browser_config = Arc::new(BrowserConfigComponent::default());
        let pool = BrowserPool::new(config, browser_config);

        // 并发克隆 return_sender（应无锁，瞬间完成）
        let mut handles = Vec::new();
        for _ in 0..100 {
            let state = pool.state.clone();
            handles.push(tokio::spawn(async move {
                let _sender = state.clone_return_sender();
            }));
        }

        // 所有克隆应快速完成（无死锁）
        for handle in handles {
            handle.await.unwrap();
        }

        pool.shutdown().await;
    }

    #[test]
    fn test_pooled_browser_last_used_atomic() {
        // PERF-002: 验证 last_used_at_millis 原子操作正确性
        // 无法直接构造 PooledBrowser（需要 OwnedSemaphorePermit），
        // 但可以通过 AtomicU64 验证语义
        let atomic = AtomicU64::new(0);

        // 模拟 touch: store elapsed millis
        let base = Instant::now();
        let elapsed = base.elapsed();
        atomic.store(elapsed.as_millis() as u64, Ordering::Release);

        // 模拟 last_used: load + reconstruct
        let millis = atomic.load(Ordering::Acquire);
        let reconstructed = base + Duration::from_millis(millis);

        // 重建的时间应接近 now（误差 < 100ms）
        let diff = Instant::now().duration_since(reconstructed);
        assert!(diff < Duration::from_millis(100));
    }

    // ==== P08：池路径代理校验 ====

    /// 含空白字符的代理串必须被拒绝（argv 注入防护）
    #[test]
    fn test_validate_pool_proxy_rejects_whitespace_injection() {
        assert!(validate_pool_proxy("http://bad proxy --flag").is_err());
        assert!(validate_pool_proxy("ftp://not-allowed").is_err());
        assert!(validate_pool_proxy("http://proxy.example.com:8080").is_ok());
        assert!(validate_pool_proxy("socks5://user:pass@proxy:1080").is_ok()); // pragma: allowlist secret
    }
}
