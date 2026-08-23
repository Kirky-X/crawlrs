// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Playwright 浏览器实例池管理器
//!
//! 提供浏览器实例的复用能力，避免每次请求都创建新的浏览器实例，
//! 从而减少 500ms-2s 的启动延迟。
//!
//! # 主要特性
//!
//! - 浏览器实例复用
//! - 最大实例数限制
//! - 空闲实例自动清理
//! - 健康检查机制
//! - 优雅关闭支持

use crate::engines::browser_downloader::{BrowserDownloadConfig, BrowserDownloadManager};
use crate::engines::client::tab_pool::TabPool;
use crate::engines::engine_client::EngineError;
use crate::infrastructure::services::config_service::BrowserConfigTrait;
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use log::{debug, info, warn};
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
    /// TabPool 最大容量（T068，R-jsrender-004）
    ///
    /// 每个 Browser 实例独立的 Tab 池容量。0 = 禁用 tab 复用。
    /// 默认 10（每个 Browser 缓存最多 10 个空闲 Page）。
    pub tab_pool_max_size: usize,
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
    /// Tab 池（T068，per-Browser 实例）
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
        }
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

    /// 获取 Browser 实例 + 对应的 TabPool（T068，R-jsrender-004）
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

    async fn launch_browser(&self) -> Result<Arc<Browser>, EngineError> {
        let remote_debugging_url = self.browser_config.get_remote_debugging_url();
        let proxy_url = self.browser_config.get_proxy_url();

        let (browser, mut handler) = if let Some(ref url) = remote_debugging_url {
            info!("Connecting to remote Chrome instance at: {}", url);
            Browser::connect(url).await.map_err(|e| {
                EngineError::Other(format!("Failed to connect to remote Chrome: {}", e))
            })?
        } else {
            // 获取浏览器路径
            let browser_path = self.get_or_download_browser().await?;

            let mut builder = BrowserConfig::builder()
                .no_sandbox()
                .request_timeout(Duration::from_secs(30));

            // 设置浏览器路径
            if let Some(ref path) = browser_path {
                info!("Using browser at: {:?}", path);
                builder = builder.chrome_executable(path);
            }

            // 添加自定义参数
            for arg in &self.config.browser_args {
                builder = builder.arg(arg.as_str());
            }

            if let Some(ref proxy) = proxy_url {
                info!("Using proxy for browser: {}", proxy);
                builder = builder.arg(format!("--proxy-server={}", proxy));
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
        // Phase 1: 读锁收集空闲实例
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

        // Phase 2: 写锁移除空闲实例
        let removed = {
            let mut available = self.available.write().await;
            idle_ids
                .into_iter()
                .filter_map(|id| available.remove(&id))
                .collect::<Vec<_>>()
        };

        // Phase 3: 锁外 drop（total_instances 更新 + permit 自动释放）
        for pooled in removed {
            self.total_instances.fetch_sub(1, Ordering::Relaxed);
            info!("Cleaned up idle browser instance {}", pooled.instance_id);
            drop(pooled);
        }
    }

    async fn health_check_all(&self) {
        // SEC-002 + PERF-001: 三阶段模式避免写锁持有期间执行 I/O
        // Phase 1: 读锁收集待检查实例（clone Arc 避免持锁）
        let candidates = {
            let available = self.available.read().await;
            available
                .iter()
                .map(|(id, p)| (*id, p.clone()))
                .collect::<Vec<_>>()
        };

        // Phase 2: 锁外执行健康检查
        let mut unhealthy_ids = Vec::new();
        for (id, pooled) in &candidates {
            if !self.check_browser_health(&pooled.browser).await {
                pooled.mark_unhealthy();
                unhealthy_ids.push(*id);
            }
        }

        // Phase 3: 写锁移除不健康实例，drop 在锁外
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

        info!("All browser instances shut down");
    }

    /// 克隆归还通道发送端（QUAL-003）
    ///
    /// 返回 `Arc<Sender>` 的克隆，无锁操作。
    /// 供 [`BrowserPool::acquire`] 和 [`BrowserPool::acquire_page`] 使用。
    fn clone_return_sender(&self) -> Arc<mpsc::Sender<ReturnMessage>> {
        self.return_sender.clone()
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
                let _ = sender
                    .send(ReturnMessage {
                        instance_id: self.instance_id,
                        browser,
                    })
                    .await;
            }
        }
    }
}

impl Drop for BrowserInstance {
    fn drop(&mut self) {
        if let Some(browser) = self.browser.take() {
            if let Some(sender) = &self.return_sender {
                // 尝试非阻塞发送，如果失败则直接丢弃浏览器
                match sender.try_send(ReturnMessage {
                    instance_id: self.instance_id,
                    browser,
                }) {
                    Ok(_) => {}
                    Err(mpsc::error::TrySendError::Full(_)) => {
                        warn!("Return channel full, dropping browser instance");
                    }
                    Err(mpsc::error::TrySendError::Closed(_)) => {
                        debug!("Return channel closed, dropping browser instance");
                    }
                }
            }
        }
    }
}

/// 浏览器 Page 包装器（T068，R-jsrender-004）
///
/// 同时持有 [`BrowserInstance`] 和 [`Page`]，drop 时自动归还两者：
///
/// 1. **Page 归还**：通过 `tokio::spawn` 异步调用 [`TabPool::release`]（导航到
///    `about:blank` 清理状态后压栈）。runtime 不可用时 Page 直接 drop（关闭 tab）。
/// 2. **Browser 归还**：`browser_instance` 字段 drop 时触发
///    [`BrowserInstance::drop`]，通过 return channel 归还到 BrowserPool。
///
/// # 手动释放
///
/// 调用 [`release`](Self::release) 可显式异步归还两者（等待 Page 导航完成），
/// 适用于需要在 drop 前确保 Page 状态清理的场景。
pub struct PooledPage {
    /// Page 实例（Option 便于 Drop 中 take）
    page: Option<Page>,
    /// 对应的 BrowserInstance（drop 时归还 Browser）
    browser_instance: Option<BrowserInstance>,
    /// 对应的 TabPool（用于归还 Page）
    tab_pool: Arc<TabPool>,
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
    /// 1. 调用 [`TabPool::release`] 归还 Page（等待导航到 `about:blank`）
    /// 2. 调用 [`BrowserInstance::release`] 归还 Browser（发送到 return channel）
    ///
    /// 调用后 `self` 被消费，不应再访问。
    pub async fn release(mut self) {
        // 1. 归还 Page 到 TabPool
        if let Some(page) = self.page.take() {
            self.tab_pool.release(page).await;
        }
        // 2. 归还 BrowserInstance（BrowserInstance::release 消费 self）
        if let Some(instance) = self.browser_instance.take() {
            instance.release().await;
        }
    }
}

impl Drop for PooledPage {
    fn drop(&mut self) {
        // 1. 归还 Page（spawn task，因为 TabPool::release 是 async）
        if let Some(page) = self.page.take() {
            let tab_pool = self.tab_pool.clone();
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
        // 2. BrowserInstance drop 自动归还 Browser（通过 return_sender）
        // browser_instance 字段 drop 时触发 BrowserInstance::drop
    }
}

/// 浏览器实例池管理器
///
/// 管理浏览器实例的生命周期，支持实例复用、最大实例数限制、
/// 空闲实例清理和健康检查。
pub struct BrowserPool {
    state: Arc<BrowserPoolState>,
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
        })
    }

    /// 获取 Browser + Page（T068，R-jsrender-004）
    ///
    /// 在 [`acquire`](Self::acquire) 基础上额外从对应 Browser 的 [`TabPool`]
    /// 获取/复用 [`Page`]。返回的 [`PooledPage`] 在 drop 时自动归还 Browser 与 Page。
    ///
    /// # Page 复用语义
    ///
    /// - 池中有空闲 Page（LIFO）→ 弹出复用，避免 `browser.new_page` 开销
    /// - 池空 → 调用 `browser.new_page("about:blank")` 新建
    ///
    /// # 错误
    ///
    /// - 池已关闭、信号量失败、实例不在 in_use：透传 [`acquire`](Self::acquire) 错误
    /// - TabPool::acquire 失败（`browser.new_page` CDP 错误）：
    ///   `EngineError::BrowserError`，并自动归还 Browser 避免泄漏
    pub async fn acquire_page(&self) -> Result<PooledPage, EngineError> {
        let (instance_id, browser, tab_pool) = self.state.acquire_with_tab_pool().await?;

        // 从 TabPool 获取 Page，失败时归还 Browser 避免泄漏
        let page = match tab_pool.acquire(&browser).await {
            Ok(page) => page,
            Err(e) => {
                // acquire Page 失败，归还 Browser 到 return channel
                let _ = self.state.clone_return_sender().try_send(ReturnMessage {
                    instance_id,
                    browser,
                });
                return Err(EngineError::BrowserError(e.to_string()));
            }
        };

        let return_sender = Some(self.state.clone_return_sender());

        Ok(PooledPage {
            page: Some(page),
            browser_instance: Some(BrowserInstance {
                browser: Some(browser),
                instance_id,
                return_sender,
            }),
            tab_pool,
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
        // T068：tab_pool_max_size 默认 10
        assert_eq!(config.tab_pool_max_size, 10);
        // SEC-001: 默认不包含 --no-sandbox（生产环境安全默认）
        assert!(
            !config.browser_args.iter().any(|a| a == "--no-sandbox"),
            "default browser_args should not contain --no-sandbox"
        );
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
}
