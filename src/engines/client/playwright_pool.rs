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
use crate::engines::engine_client::EngineError;
use crate::infrastructure::services::config_service::BrowserConfigTrait;
use chromiumoxide::{Browser, BrowserConfig};
use futures::StreamExt;
use log::{debug, error, info, warn};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{mpsc, Mutex, RwLock, Semaphore};
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
}

impl Default for BrowserPoolConfig {
    fn default() -> Self {
        Self {
            max_instances: 5,
            idle_timeout_secs: 300,         // 5 分钟
            health_check_interval_secs: 60, // 1 分钟
            create_timeout_secs: 30,
            enable_reuse: true,
            browser_args: vec![
                "--disable-gpu".to_string(),
                "--disable-dev-shm-usage".to_string(),
                "--no-sandbox".to_string(),
            ],
        }
    }
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
    /// 最后使用时间
    last_used_at: std::sync::Mutex<Instant>,
    /// 使用次数
    use_count: AtomicU64,
    /// 是否健康
    is_healthy: AtomicBool,
    /// 实例 ID
    instance_id: u64,
}

impl PooledBrowser {
    fn new(browser: Arc<Browser>, instance_id: u64) -> Self {
        let now = Instant::now();
        Self {
            browser,
            created_at: now,
            last_used_at: std::sync::Mutex::new(now),
            use_count: AtomicU64::new(0),
            is_healthy: AtomicBool::new(true),
            instance_id,
        }
    }

    fn touch(&self) {
        let mut last_used = match self.last_used_at.lock() {
            Ok(g) => g,
            Err(e) => {
                log::error!("PooledBrowser last_used_at mutex poisoned: {}", e);
                return;
            }
        };
        *last_used = Instant::now();
        self.use_count.fetch_add(1, Ordering::Relaxed);
    }

    fn last_used(&self) -> Instant {
        match self.last_used_at.lock() {
            Ok(g) => *g,
            Err(e) => {
                log::error!("PooledBrowser last_used_at mutex poisoned: {}", e);
                Instant::now()
            }
        }
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
    /// 信号量（限制最大实例数）
    semaphore: Semaphore,
    /// 下载管理器
    download_manager: Arc<BrowserDownloadManager>,
    /// 清理任务句柄
    cleanup_task: Mutex<Option<JoinHandle<()>>>,
    /// 归还处理任务句柄
    return_task: Mutex<Option<JoinHandle<()>>>,
    /// 归还通道发送端
    return_sender: Mutex<Option<mpsc::Sender<ReturnMessage>>>,
    /// 关闭标志
    shutdown: AtomicBool,
    /// 浏览器路径缓存
    browser_path: RwLock<Option<PathBuf>>,
}

impl BrowserPoolState {
    fn new(
        config: BrowserPoolConfig,
        browser_config: Arc<dyn BrowserConfigTrait>,
        download_manager: Arc<BrowserDownloadManager>,
    ) -> Self {
        let max_instances = config.max_instances;
        Self {
            config,
            browser_config,
            available: RwLock::new(HashMap::new()),
            in_use: RwLock::new(HashMap::new()),
            instance_counter: AtomicU64::new(0),
            total_instances: AtomicUsize::new(0),
            semaphore: Semaphore::new(max_instances),
            download_manager,
            cleanup_task: Mutex::new(None),
            return_task: Mutex::new(None),
            return_sender: Mutex::new(None),
            shutdown: AtomicBool::new(false),
            browser_path: RwLock::new(None),
        }
    }

    async fn acquire(&self) -> Result<(u64, Arc<Browser>), EngineError> {
        // 检查是否已关闭
        if self.shutdown.load(Ordering::Acquire) {
            return Err(EngineError::Other(
                "Browser pool is shutting down".to_string(),
            ));
        }

        // 获取信号量许可
        let _permit = self
            .semaphore
            .acquire()
            .await
            .map_err(|_| EngineError::Other("Failed to acquire semaphore".to_string()))?;

        // 尝试从可用池中获取实例
        if self.config.enable_reuse {
            if let Some((id, browser)) = self.try_get_available().await {
                return Ok((id, browser));
            }
        }

        // 创建新实例
        self.create_new_instance().await
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

    async fn create_new_instance(&self) -> Result<(u64, Arc<Browser>), EngineError> {
        let instance_id = self.instance_counter.fetch_add(1, Ordering::Relaxed);
        info!("Creating new browser instance {}", instance_id);

        let browser = self.launch_browser().await?;
        let pooled = Arc::new(PooledBrowser::new(browser.clone(), instance_id));
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
                // 归还到可用池
                let mut available = self.available.write().await;
                available.insert(instance_id, pooled);
                debug!("Browser instance {} returned to pool", instance_id);
            } else {
                // 不健康或禁用复用，关闭浏览器
                self.total_instances.fetch_sub(1, Ordering::Relaxed);
                self.semaphore.add_permits(1);
                debug!(
                    "Browser instance {} closed (unhealthy or reuse disabled)",
                    instance_id
                );
            }
        } else {
            // 实例不在使用中，可能是重复归还
            warn!(
                "Browser instance {} not found in use, ignoring return",
                instance_id
            );
            self.semaphore.add_permits(1);
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
            let _browser_path = self.get_or_download_browser().await?;

            let mut builder = BrowserConfig::builder()
                .no_sandbox()
                .request_timeout(Duration::from_secs(30));

            // 添加自定义参数
            for arg in &self.config.browser_args {
                builder = builder.arg(arg);
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
        // 检查缓存
        {
            let path = self.browser_path.read().await;
            if path.is_some() {
                return Ok(path.clone());
            }
        }

        // 首先检查系统浏览器
        if let Some(path) = crate::engines::browser_downloader::find_system_browser().await {
            info!("Using system browser");
            let mut cached = self.browser_path.write().await;
            *cached = Some(path.clone());
            return Ok(Some(path));
        }

        // 检查是否已下载
        if self.download_manager.is_browser_downloaded().await {
            let path = crate::engines::browser_downloader::get_browser_executable_path(
                self.download_manager.get_cache_dir(),
            );
            info!("Using downloaded browser: {:?}", path);
            let mut cached = self.browser_path.write().await;
            *cached = Some(path.clone());
            return Ok(Some(path));
        }

        // 自动下载浏览器
        info!("No browser found, starting automatic download...");
        match self.download_manager.download_browser().await {
            Ok(path) => {
                info!("Browser downloaded successfully: {:?}", path);
                let mut cached = self.browser_path.write().await;
                *cached = Some(path.clone());
                Ok(Some(path))
            }
            Err(e) => {
                warn!("Browser download failed: {}, will try system path", e);
                Ok(None)
            }
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
        let now = Instant::now();
        let idle_timeout = Duration::from_secs(self.config.idle_timeout_secs);

        let mut available = self.available.write().await;
        let mut to_remove = Vec::new();

        for (id, pooled) in available.iter() {
            let idle_duration = now.duration_since(pooled.last_used());
            if idle_duration > idle_timeout {
                to_remove.push(*id);
            }
        }

        for id in to_remove {
            if let Some(pooled) = available.remove(&id) {
                // 关闭浏览器
                drop(pooled);
                self.total_instances.fetch_sub(1, Ordering::Relaxed);
                self.semaphore.add_permits(1);
                info!("Cleaned up idle browser instance {}", id);
            }
        }
    }

    async fn health_check_all(&self) {
        let mut available = self.available.write().await;
        let mut unhealthy = Vec::new();

        for (id, pooled) in available.iter() {
            if !self.check_browser_health(&pooled.browser).await {
                pooled.mark_unhealthy();
                unhealthy.push(*id);
            }
        }

        for id in unhealthy {
            if let Some(pooled) = available.remove(&id) {
                drop(pooled);
                self.total_instances.fetch_sub(1, Ordering::Relaxed);
                self.semaphore.add_permits(1);
                warn!("Removed unhealthy browser instance {}", id);
            }
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
}

/// 浏览器实例包装器
///
/// 当实例被 drop 时自动归还到池中
pub struct BrowserInstance {
    /// 浏览器实例
    browser: Option<Arc<Browser>>,
    /// 实例 ID
    instance_id: u64,
    /// 归还通道发送端
    return_sender: Option<mpsc::Sender<ReturnMessage>>,
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

        // 获取归还通道发送端
        let return_sender = {
            let sender = self.state.return_sender.lock().await;
            sender.clone()
        };

        Ok(BrowserInstance {
            browser: Some(browser),
            instance_id,
            return_sender,
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
            let (sender, mut receiver) = mpsc::channel::<ReturnMessage>(32);

            // 保存发送端到状态中
            {
                let mut return_sender = self.state.return_sender.lock().await;
                *return_sender = Some(sender);
            }

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

    /// 获取归还通道发送端
    async fn get_return_sender(&self) -> Option<mpsc::Sender<ReturnMessage>> {
        let sender = self.state.return_sender.lock().await;
        sender.clone()
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
}
