// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 浏览器启动与维护 — 从 [`super::BrowserPoolState`] 拆分的 partial impl block
//!
//! 包含远程/本地浏览器启动、下载、健康检查、空闲回收与关闭。

use super::*;

impl BrowserPoolState {
    /// 连接远程 Chrome（自 [`Self::launch_browser`] 提取）
    pub(super) async fn connect_remote_browser(
        url: &str,
    ) -> Result<(Browser, chromiumoxide::Handler), EngineError> {
        info!("Connecting to remote Chrome instance at: {}", url);
        Browser::connect(url)
            .await
            .map_err(|e| EngineError::Other(format!("Failed to connect to remote Chrome: {}", e)))
    }

    /// 启动本地浏览器实例（自 [`Self::launch_browser`] 提取）
    pub(super) async fn launch_local_browser(
        &self,
        validated_proxy: Option<String>,
    ) -> Result<(Browser, chromiumoxide::Handler), EngineError> {
        let browser_path = self.get_or_download_browser().await?;
        let config = self.prepare_launch_config(browser_path.as_ref(), validated_proxy)?;
        Browser::launch(config)
            .await
            .map_err(|e| EngineError::Other(format!("Failed to launch browser: {}", e)))
    }

    /// 构造本地浏览器启动配置（自 [`Self::launch_browser`] 提取）
    fn prepare_launch_config(
        &self,
        browser_path: Option<&PathBuf>,
        validated_proxy: Option<String>,
    ) -> Result<BrowserConfig, EngineError> {
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

        if let Some(path) = browser_path {
            info!("Using browser at: {:?}", path);
            builder = builder.chrome_executable(path);
        }

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

        builder
            .build()
            .map_err(|e| EngineError::Other(e.to_string()))
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

    pub(super) async fn check_browser_health(&self, browser: &Browser) -> bool {
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

    /// 内存水位进入 Critical 时的一次性空闲实例回收（engine-moli）
    ///
    /// 复用 [`Self::cleanup_idle_instances`] 的三阶段回收机制（读锁收集 → 写锁移除 →
    /// 锁外 drop + total_instances 递减），区别仅在不做 idle_timeout 过滤——
    /// Critical 语义下所有 available 实例都属可回收空闲，立即回收而非等超时。
    /// 在运（in_use）实例不受影响：门控只作用于新 acquire，不中断进行中的任务。
    #[cfg(feature = "engine-moli")]
    pub(super) async fn reclaim_idle_instances_now(&self) {
        let removed = {
            let mut available = self.available.write().await;
            available.drain().collect::<Vec<_>>()
        };
        for (_, pooled) in removed {
            self.total_instances.fetch_sub(1, Ordering::Relaxed);
            log::warn!(
                "Reclaimed idle browser instance {} due to Critical memory watermark",
                pooled.instance_id
            );
            drop(pooled);
        }
    }

    pub(super) async fn cleanup_idle_instances(&self) {
        // : 三阶段模式避免写锁持有期间执行 I/O（与 health_check_all 统一）
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

    pub(super) async fn health_check_all(&self) {
        // + PERF-001: 三阶段模式避免写锁持有期间执行 I/O
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

    pub(super) async fn shutdown_all(&self) {
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
    pub(super) fn clone_return_sender(&self) -> Arc<mpsc::Sender<ReturnMessage>> {
        self.return_sender.clone()
    }

    /// Page 获取失败后归还 Browser：优先走 return channel；通道满/关闭时
    /// 直接 await [`return_instance`](Self::return_instance) 移除 in_use 槽位，
    /// 防止信号量 permit 泄漏导致池容量收缩。
    /// 供 TabPool 路径与 context 隔离路径的失败分支复用。
    pub(super) async fn reclaim_after_page_acquire_failure(
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
