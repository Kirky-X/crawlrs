// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 浏览器自动下载管理器
//!
//! 提供 Chrome/Chromium 浏览器的自动下载功能，降低用户使用门槛。
//!
//! 注意：当前版本使用系统浏览器检测，chromiumoxide_fetcher 的完整支持将在后续版本中添加。

use std::path::PathBuf;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;
use tracing::info;

/// 下载状态
#[derive(Debug, Clone, PartialEq)]
pub enum DownloadStatus {
    NotDownloaded,
    Downloading,
    Downloaded,
    Failed(String),
}

/// 浏览器下载错误
#[derive(Error, Debug)]
pub enum BrowserDownloadError {
    #[error("下载失败: {0}")]
    DownloadFailed(String),

    #[error("浏览器目录创建失败: {0}")]
    DirectoryCreationFailed(String),

    #[error("浏览器可执行文件不存在: {0}")]
    ExecutableNotFound(PathBuf),

    #[error("权限被拒绝: {0}")]
    PermissionDenied(String),
}

/// 浏览器下载配置
#[derive(Clone, Debug)]
pub struct BrowserDownloadConfig {
    /// 下载目录
    pub download_dir: PathBuf,
    /// 下载超时时间（秒）
    pub timeout_seconds: u64,
    /// 是否在启动时自动下载
    pub auto_download: bool,
}

impl Default for BrowserDownloadConfig {
    fn default() -> Self {
        Self {
            download_dir: get_default_browser_path(),
            timeout_seconds: 300, // 5分钟
            auto_download: true,
        }
    }
}

/// 获取默认浏览器路径
fn get_default_browser_path() -> PathBuf {
    let mut path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."));
    path.push(".cache");
    path.push("crawlrs");
    path.push("chromium");
    path
}

/// 获取浏览器可执行文件路径
pub fn get_browser_executable_path(cache_dir: &PathBuf) -> PathBuf {
    let mut path = cache_dir.clone();
    // 根据平台选择可执行文件名
    #[cfg(target_os = "windows")]
    {
        path.push("chrome-win");
        path.push("chrome.exe");
    }
    #[cfg(target_os = "macos")]
    {
        path.push("chrome-mac");
        path.push("Chromium.app");
        path.push("Contents");
        path.push("MacOS");
        path.push("Chromium");
    }
    #[cfg(target_os = "linux")]
    {
        path.push("chrome-linux");
        path.push("chrome");
    }
    path
}

/// 浏览器下载管理器
///
/// 管理 Chrome/Chromium 浏览器的自动下载和缓存
#[derive(Clone, Debug)]
pub struct BrowserDownloadManager {
    /// 配置
    config: Arc<BrowserDownloadConfig>,
    /// 下载状态
    status: Arc<RwLock<DownloadStatus>>,
}

impl BrowserDownloadManager {
    /// 创建新的下载管理器
    pub fn new(config: BrowserDownloadConfig) -> Self {
        Self {
            config: Arc::new(config),
            status: Arc::new(RwLock::new(DownloadStatus::NotDownloaded)),
        }
    }

    /// 创建使用默认配置的下载管理器
    pub fn with_defaults() -> Self {
        Self::new(BrowserDownloadConfig::default())
    }

    /// 获取当前下载状态
    pub async fn get_status(&self) -> DownloadStatus {
        let status = self.status.read().await;
        status.clone()
    }

    /// 检查浏览器是否已下载
    pub async fn is_browser_downloaded(&self) -> bool {
        let executable_path = get_browser_executable_path(&self.config.download_dir);
        executable_path.exists() && executable_path.is_file()
    }

    /// 浏览器自动下载（简化实现）
    ///
    /// 当前版本检测系统浏览器并提示用户。
    /// 完整的自动下载功能需要 chromiumoxide_fetcher 的稳定支持。
    pub async fn download_browser(&self) -> Result<PathBuf, BrowserDownloadError> {
        info!("开始检查浏览器...");

        // 更新状态为下载中
        {
            let mut status = self.status.write().await;
            *status = DownloadStatus::Downloading;
        }

        // 首先检查系统浏览器
        if let Some(path) = find_system_browser().await {
            let mut status = self.status.write().await;
            *status = DownloadStatus::Downloaded;
            return Ok(path);
        }

        // 检查是否已下载
        if self.is_browser_downloaded().await {
            let path = get_browser_executable_path(&self.config.download_dir);
            let mut status = self.status.write().await;
            *status = DownloadStatus::Downloaded;
            return Ok(path);
        }

        // 创建下载目录
        if let Err(e) = tokio::fs::create_dir_all(&self.config.download_dir).await {
            let err_msg = format!("创建下载目录失败: {}", e);
            let mut status = self.status.write().await;
            *status = DownloadStatus::Failed(err_msg.clone());
            return Err(BrowserDownloadError::DirectoryCreationFailed(err_msg));
        }

        // 尝试使用 chromiumoxide_fetcher 下载
        if let Ok(path) = self.try_fetcher_download().await {
            let mut status = self.status.write().await;
            *status = DownloadStatus::Downloaded;
            return Ok(path);
        }

        // 如果自动下载失败，返回错误提示
        let err_msg = String::from(
            "未找到 Chrome/Chromium 浏览器。请安装以下任一浏览器后重试：\n\
             - Google Chrome (https://www.google.com/chrome/)\n\
             - Chromium (https://www.chromium.org/getting-involved/download-chromium/)\n\
             或确保浏览器在系统 PATH 中。",
        );
        let mut status = self.status.write().await;
        *status = DownloadStatus::Failed(err_msg.clone());
        Err(BrowserDownloadError::DownloadFailed(err_msg))
    }

    /// 尝试使用 chromiumoxide_fetcher 下载
    async fn try_fetcher_download(&self) -> Result<PathBuf, BrowserDownloadError> {
        // 动态检测是否可以使用 fetcher
        // 如果 chromiumoxide_fetcher 可用，尝试下载
        #[cfg(feature = "chromiumoxide_fetcher")]
        {
            match self.do_fetcher_download().await {
                Ok(path) => return Ok(path),
                Err(e) => {
                    tracing::warn!("fetcher 下载失败: {}", e);
                }
            }
        }

        Err(BrowserDownloadError::DownloadFailed(
            "Fetcher 不可用".to_string(),
        ))
    }

    /// 实际的 fetcher 下载实现（仅在特性启用时编译）
    #[cfg(feature = "chromiumoxide_fetcher")]
    async fn do_fetcher_download(&self) -> Result<PathBuf, BrowserDownloadError> {
        use chromiumoxide_fetcher::BrowserFetcher;

        let fetcher = BrowserFetcher::builder()
            .with_download_dir(&self.config.download_dir)
            .build()
            .map_err(|e| BrowserDownloadError::DownloadFailed(e.to_string()))?;

        let revision = fetcher
            .fetch()
            .await
            .map_err(|e| BrowserDownloadError::DownloadFailed(e.to_string()))?;

        info!(
            "下载 Chrome 版本: {} (大小: {:.2} MB)",
            revision.version,
            revision.download_size / 1024.0 / 1024.0
        );

        fetcher
            .download(&revision)
            .await
            .map_err(|e| BrowserDownloadError::DownloadFailed(e.to_string()))?;

        let executable_path = get_browser_executable_path(&self.config.download_dir);
        Ok(executable_path)
    }

    /// 下载并返回浏览器路径（如果需要的话）
    pub async fn get_or_download_browser(&self) -> Result<PathBuf, BrowserDownloadError> {
        self.download_browser().await
    }

    /// 清理下载的浏览器
    pub async fn cleanup(&self) -> Result<(), BrowserDownloadError> {
        if self.config.download_dir.exists() {
            tokio::fs::remove_dir_all(&self.config.download_dir)
                .await
                .map_err(|e| BrowserDownloadError::DownloadFailed(format!("清理失败: {}", e)))?;
            let mut status = self.status.write().await;
            *status = DownloadStatus::NotDownloaded;
            info!("已清理下载的浏览器");
        }
        Ok(())
    }

    /// 获取浏览器缓存目录
    pub fn get_cache_dir(&self) -> &PathBuf {
        &self.config.download_dir
    }
}

/// 检查系统是否有可用的 Chrome/Chromium
pub async fn find_system_browser() -> Option<PathBuf> {
    // 检查常见路径
    let common_paths = [
        // Linux
        PathBuf::from("/usr/bin/google-chrome"),
        PathBuf::from("/usr/bin/chromium"),
        PathBuf::from("/usr/bin/chromium-browser"),
        // macOS
        PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
        PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"),
        // Windows
        PathBuf::from(r"C:\Program Files\Google\Chrome\Application\chrome.exe"),
        PathBuf::from(r"C:\Program Files (x86)\Google\Chrome\Application\chrome.exe"),
    ];

    for path in &common_paths {
        if path.exists() {
            tracing::info!("找到系统浏览器: {:?}", path);
            return Some(path.clone());
        }
    }

    None
}
