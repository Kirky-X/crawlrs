// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 浏览器池实例与页面包装 — 从 `playwright_pool` 拆分的类型定义
//!
//! 包含 [`BrowserInstance`]、[`PooledPage`] 与 [`BrowserPool`] 的构造/获取/归还/关闭。

use super::*;

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
        // 失败仅记日志——浏览器连接已坏时归还路径的健康检查会剔除实例兜底。
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

    /// 复用页 Drop 归还（自 [`Drop::drop`] 提取）：runtime 可用时 spawn
    /// [`TabPool::release`] task；runtime 不可用（如 shutdown 中）直接 drop Page。
    fn spawn_tab_return(tab_pool: Arc<TabPool>, page: Page) {
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

    /// 隔离页 Drop 清理（自 [`Drop::drop`] 提取）：spawn 清理 task 先销毁
    /// context 再归还 Browser；runtime 不可用时返回 `Some(instance)`，由调用方
    /// 放回字段走自身 Drop 经 return channel 归还。
    fn spawn_context_cleanup(
        context_id: BrowserContextId,
        instance: BrowserInstance,
    ) -> Option<BrowserInstance> {
        match tokio::runtime::Handle::try_current() {
            Ok(handle) => {
                handle.spawn(async move {
                    let browser = instance.browser().clone();
                    if let Err(e) = browser.dispose_browser_context(context_id).await {
                        error!("Failed to dispose browser context on drop: {}", e);
                    }
                    instance.release().await;
                });
                None
            }
            Err(_) => {
                // runtime 不可用：context 销毁只能放弃（浏览器进程退出时回收）。
                // BrowserInstance 放回字段走自身 Drop 经 return channel 归还，
                // 避免信号量 permit 泄漏导致池容量收缩。
                error!(
                    "tokio runtime unavailable, isolated browser context cannot be disposed; context may leak until browser exits"
                );
                Some(instance)
            }
        }
    }
}

impl Drop for PooledPage {
    fn drop(&mut self) {
        // 1. Page：复用页 spawn 归还 task（TabPool::release 是 async）；隔离页
        // 直接 drop 客户端句柄（tab 随 context 销毁关闭）
        if let Some(page) = self.page.take() {
            if let Some(tab_pool) = self.tab_pool.clone() {
                // 尝试在当前 runtime spawn 归还 task
                Self::spawn_tab_return(tab_pool, page);
            }
        }
        // 2. 隔离页：Drop 无法 await dispose——spawn 清理 task 先销毁 context
        // 再归还 Browser，保证早退/panic/超时路径不泄漏 context
        if let Some(context_id) = self.context_id.take() {
            if let Some(instance) = self.browser_instance.take() {
                // runtime 不可用时 helper 把 instance 交回，放回字段由其自身 Drop
                // 经 return channel 归还
                if let Some(instance) = Self::spawn_context_cleanup(context_id, instance) {
                    self.browser_instance = Some(instance);
                }
            }
        }
        // 3. 复用页：browser_instance 未被 take，字段 drop 触发
        // BrowserInstance::drop 经 return channel 归还 Browser
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
    /// 注入内存水位镜像（engine-moli）
    ///
    /// 由 DI 装配在池构造后调用；`None`（调度器关闭）时池行为与无水位门控一致。
    /// 与 EngineRouter 共享同一 `Arc` 实例（读到的档位恒一致）。
    #[cfg(feature = "engine-moli")]
    pub fn set_memory_watermark(
        &self,
        watermark: Option<Arc<crate::engines::memory_watermark::MemoryWatermark>>,
    ) {
        *self.state.memory.write() = watermark;
    }

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
