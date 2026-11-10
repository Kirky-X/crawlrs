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

mod launch;
mod pool;

pub use pool::{BrowserInstance, BrowserPool, PooledPage};

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
            // : 默认不包含 `--no-sandbox`（生产环境安全默认）。
            // 容器环境使用 `docker_safe_args` 构造器自动添加。
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
    /// 创建容器环境安全的配置
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

/// 检测当前是否运行在容器环境中
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
    /// 信号量 permit
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
    /// 信号量（限制最大实例数，: Arc 包装以支持 acquire_owned）
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
    /// 内存水位镜像（engine-moli）：`None`（未装配）时行为与无水位门控一致。
    /// 构造期自取全局单例，显式 setter 可覆盖（DI 装配顺序：set_global 先于池构造，
    /// 测试可显式注入）。parking_lot 短临界段锁，与仓内既有选型一致。
    #[cfg(feature = "engine-moli")]
    memory: parking_lot::RwLock<Option<Arc<crate::engines::memory_watermark::MemoryWatermark>>>,
    /// Critical 首次进入的一次性空闲回收标记（AtomicBool CAS 保证单次触发）
    #[cfg(feature = "engine-moli")]
    critical_reclaim_done: AtomicBool,
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
        // engine-moli：构造期自取全局水位镜像（与 router 共享同一 Arc；未装配则空）
        #[cfg(feature = "engine-moli")]
        let global_watermark = crate::engines::memory_watermark::get_global();

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
            #[cfg(feature = "engine-moli")]
            memory: parking_lot::RwLock::new(global_watermark),
            #[cfg(feature = "engine-moli")]
            critical_reclaim_done: AtomicBool::new(false),
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
        if self.shutdown.load(Ordering::Acquire) {
            return Err(EngineError::Other(
                "Browser pool is shutting down".to_string(),
            ));
        }

        // 内存水位门控（engine-moli）：Critical 拒绝新 acquire 并一次性回收空闲实例；
        // Tight 只允许复用可用实例、总实例数不增。由路由层 D3/D4 语义兜底降级。
        #[cfg(feature = "engine-moli")]
        if let Some(gated) = self.gate_by_memory_watermark().await {
            return gated;
        }

        // : 获取 OwnedSemaphorePermit，生命周期绑定到 PooledBrowser
        let permit = self
            .semaphore
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| EngineError::Other("Failed to acquire semaphore".to_string()))?;

        if self.config.enable_reuse {
            if let Some((id, browser)) = self.try_get_available().await {
                return Ok((id, browser));
            }
        }

        // 创建新实例（传递 permit 绑定到 PooledBrowser）
        self.create_new_instance(permit).await
    }

    /// 内存水位门控（engine-moli）：返回 `Some(result)` 表示 acquire 结果被
    /// 水位决策短路（Critical 拒绝 / Tight 复用或拒绝）；`None` 表示继续正常路径。
    #[cfg(feature = "engine-moli")]
    async fn gate_by_memory_watermark(&self) -> Option<Result<(u64, Arc<Browser>), EngineError>> {
        use crate::engines::memory_watermark::MemoryLevel;
        let watermark = self.memory.read().clone();
        let watermark = watermark?;
        match watermark.level() {
            MemoryLevel::Critical => {
                if !self.critical_reclaim_done.swap(true, Ordering::AcqRel) {
                    self.reclaim_idle_instances_now().await;
                }
                Some(Err(EngineError::MemoryConstrained(
                    "browser pool acquire rejected under Critical memory watermark".to_string(),
                )))
            }
            MemoryLevel::Tight => {
                // 分层契约：router 是精确策略（needs_js 维度修正分数），池是
                // 粗粒度安全阀（Critical 全拒 + Tight 控容量）。Tight 语义 =
                // 复用在运实例、总实例数不增；但 total_instances==0 的冷池
                // （临时池/进程刚启动）放行首实例——否则 Tight 期间浏览器
                // 渲染被系统性拒绝，Tight 档位形同 Critical。
                if self.config.enable_reuse {
                    if let Some((id, browser)) = self.try_get_available().await {
                        return Some(Ok((id, browser)));
                    }
                }
                if self.total_instances.load(Ordering::Relaxed) > 0 {
                    return Some(Err(EngineError::MemoryConstrained(
                        "no reusable browser instance under Tight memory watermark".to_string(),
                    )));
                }
                None
            }
            MemoryLevel::Abundant => {
                // 水位回落即复位一次性回收标记：下一个 Critical 周期
                // 可再次触发空闲实例立即回收（普通 load/store，无 RMW 开销）
                if self.critical_reclaim_done.load(Ordering::Relaxed) {
                    self.critical_reclaim_done.store(false, Ordering::Relaxed);
                }
                None
            }
        }
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
            Self::connect_remote_browser(url).await?
        } else {
            self.launch_local_browser(validated_proxy).await?
        };

        tokio::spawn(async move {
            while let Some(h) = handler.next().await {
                if let Err(e) = h {
                    debug!("Browser handler event error (continuing): {:?}", e);
                }
            }
        });

        Ok(Arc::new(browser))
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
    // ---- 内存水位门控（engine-moli）：R-engine-scheduling-003 验收 ----

    #[cfg(feature = "engine-moli")]
    mod memory_gate {
        use super::*;
        use crate::engines::memory_watermark::{MemoryLevel, MemoryWatermark};

        fn pool() -> BrowserPool {
            BrowserPool::new(
                BrowserPoolConfig::default(),
                Arc::new(BrowserConfigComponent::default()),
            )
        }

        fn watermark(_available_mib: u64) -> Arc<MemoryWatermark> {
            MemoryWatermark::new(512, 2048)
        }

        #[tokio::test]
        async fn critical_rejects_acquire_without_launching() {
            let pool = pool();
            let wm = watermark(100);
            assert_eq!(wm.apply_sample(Some(100)), MemoryLevel::Critical);
            pool.set_memory_watermark(Some(wm));
            let result = pool.acquire_page().await;
            match result {
                Err(EngineError::MemoryConstrained(msg)) => {
                    assert!(msg.contains("Critical"), "错误文案应指明 Critical：{}", msg);
                }
                other => panic!("期望 MemoryConstrained，实际: {:?}", other.err()),
            }
        }

        #[tokio::test]
        async fn tight_cold_pool_passes_gate_but_running_pool_rejects_new_instance() {
            let pool = pool();
            let wm = watermark(1500);
            assert_eq!(wm.apply_sample(Some(1500)), MemoryLevel::Tight);
            pool.set_memory_watermark(Some(wm));

            // 冷池（total_instances==0）：门控放行首实例——acquire 越过水位门，
            // 失败只能发生在更深的实例创建层（测试环境无 Chrome 可执行文件），
            // 以"错误不是 MemoryConstrained"钉住放行语义。
            let result = pool.acquire_page().await;
            assert!(
                !matches!(result, Err(EngineError::MemoryConstrained(_))),
                "冷池 Tight 应放行首实例，实际被水位拒绝: {:?}",
                result.err()
            );

            // 已有实例（total_instances>0）且无可复用：拒绝（总实例数不增）
            pool.state_for_test()
                .total_instances
                .store(1, Ordering::Relaxed);
            let result = pool.acquire_page().await;
            match result {
                Err(EngineError::MemoryConstrained(msg)) => {
                    assert!(msg.contains("Tight"), "错误文案应指明 Tight：{}", msg);
                }
                other => panic!("期望 MemoryConstrained，实际: {:?}", other.err()),
            }
            assert_eq!(
                pool.state_for_test()
                    .total_instances
                    .load(Ordering::Relaxed),
                1,
                "Tight 下不得创建新实例"
            );
        }

        #[tokio::test]
        async fn pool_shares_global_watermark_instance_with_router() {
            // 装配语义：set_global 后新建池自取同一 Arc（ptr 同源、档位一致）
            let wm = MemoryWatermark::new(512, 2048);
            assert_eq!(wm.apply_sample(Some(1500)), MemoryLevel::Tight);
            crate::engines::memory_watermark::set_global(wm.clone());
            let pool = pool();
            let pool_wm = pool
                .state_for_test()
                .memory
                .read()
                .clone()
                .expect("池构造期应自取全局水位镜像");
            assert!(
                Arc::ptr_eq(&wm, &pool_wm),
                "池与 router 必须共享同一水位 Arc 实例"
            );
            assert_eq!(pool_wm.level(), MemoryLevel::Tight);
        }

        #[tokio::test]
        async fn no_watermark_keeps_legacy_behavior() {
            // 未装配水位：池不拦截（这里只验证不因水位被拒——
            // 真正启动浏览器不在本单测范围，故池先 shutdown 后断言错误类型不是水位错误）
            let pool = pool();
            pool.shutdown().await;
            let result = pool.acquire_page().await;
            assert!(
                !matches!(result, Err(EngineError::MemoryConstrained(_))),
                "无水位装配时不得出现水位错误"
            );
        }
    }

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
        // : 默认不包含 --no-sandbox（生产环境安全默认）
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

        // docker_safe_args 继承 default → 隔离同样默认开启（容器临时池不降级）
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
        // docker_safe_args 在无容器环境下不应添加 --no-sandbox
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

    // QUAL-001: 并发安全测试

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
        let permit = pool
            .state_for_test()
            .semaphore
            .clone()
            .acquire_owned()
            .await;
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
            let state = pool.state_for_test().clone();
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

    // P08：池路径代理校验

    /// 含空白字符的代理串必须被拒绝（argv 注入防护）
    #[test]
    fn test_validate_pool_proxy_rejects_whitespace_injection() {
        assert!(validate_pool_proxy("http://bad proxy --flag").is_err());
        assert!(validate_pool_proxy("ftp://not-allowed").is_err());
        assert!(validate_pool_proxy("http://proxy.example.com:8080").is_ok());
        assert!(validate_pool_proxy("socks5://user:pass@proxy:1080").is_ok()); // pragma: allowlist secret
    }
}
