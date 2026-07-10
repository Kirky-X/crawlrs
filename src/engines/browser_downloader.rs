// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 浏览器自动下载管理器
//!
//! 提供 Chrome/Chromium 浏览器的自动下载功能，降低用户使用门槛。
//!
//! 注意：当前版本使用系统浏览器检测，chromiumoxide_fetcher 的完整支持将在后续版本中添加。

use log::info;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

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
    let mut path = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    path.push(".cache");
    path.push("crawlrs");
    path.push("chromium");
    path
}

/// 获取浏览器可执行文件路径
pub fn get_browser_executable_path(cache_dir: &Path) -> PathBuf {
    let mut path = cache_dir.to_path_buf();
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
        // 如果 browser-download 特性启用，尝试下载
        #[cfg(feature = "browser-download")]
        {
            match self.do_fetcher_download().await {
                Ok(path) => return Ok(path),
                Err(e) => {
                    log::warn!("fetcher 下载失败: {}", e);
                }
            }
        }

        Err(BrowserDownloadError::DownloadFailed(
            "Fetcher 不可用".to_string(),
        ))
    }

    /// 实际的 fetcher 下载实现（仅在 browser-download 特性启用时编译）
    #[cfg(feature = "browser-download")]
    async fn do_fetcher_download(&self) -> Result<PathBuf, BrowserDownloadError> {
        // chromiumoxide_fetcher API 需要正确的使用方式
        // 完整实现需要参考 crates.io 上的最新文档
        Err(BrowserDownloadError::DownloadFailed(
            "Browser download feature requires full implementation".to_string(),
        ))
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
            log::info!("找到系统浏览器: {:?}", path);
            return Some(path.clone());
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // === DownloadStatus tests ===

    #[test]
    fn test_download_status_not_downloaded() {
        let status = DownloadStatus::NotDownloaded;
        assert_eq!(status, DownloadStatus::NotDownloaded);
        assert_ne!(status, DownloadStatus::Downloading);
    }

    #[test]
    fn test_download_status_downloading() {
        let status = DownloadStatus::Downloading;
        assert_eq!(status, DownloadStatus::Downloading);
        assert_ne!(status, DownloadStatus::Downloaded);
    }

    #[test]
    fn test_download_status_downloaded() {
        let status = DownloadStatus::Downloaded;
        assert_eq!(status, DownloadStatus::Downloaded);
        assert_ne!(status, DownloadStatus::NotDownloaded);
    }

    #[test]
    fn test_download_status_failed_with_message() {
        let status = DownloadStatus::Failed("network error".to_string());
        match status {
            DownloadStatus::Failed(msg) => assert_eq!(msg, "network error"),
            other => panic!("Expected Failed, got {:?}", other),
        }
    }

    #[test]
    fn test_download_status_clone() {
        let status = DownloadStatus::Failed("error".to_string());
        let cloned = status.clone();
        assert_eq!(status, cloned);
    }

    #[test]
    fn test_download_status_debug() {
        let status = DownloadStatus::Downloaded;
        let debug_str = format!("{:?}", status);
        assert_eq!(debug_str, "Downloaded");
    }

    // === BrowserDownloadError tests ===

    #[test]
    fn test_browser_download_error_download_failed() {
        let err = BrowserDownloadError::DownloadFailed("timeout".to_string());
        assert_eq!(err.to_string(), "下载失败: timeout");
    }

    #[test]
    fn test_browser_download_error_directory_creation_failed() {
        let err = BrowserDownloadError::DirectoryCreationFailed("permission denied".to_string());
        assert_eq!(err.to_string(), "浏览器目录创建失败: permission denied");
    }

    #[test]
    fn test_browser_download_error_executable_not_found() {
        let path = PathBuf::from("/usr/bin/chrome");
        let err = BrowserDownloadError::ExecutableNotFound(path.clone());
        assert_eq!(
            err.to_string(),
            format!("浏览器可执行文件不存在: {}", path.display())
        );
    }

    #[test]
    fn test_browser_download_error_permission_denied() {
        let err = BrowserDownloadError::PermissionDenied("/root/.cache".to_string());
        assert_eq!(err.to_string(), "权限被拒绝: /root/.cache");
    }

    // === BrowserDownloadConfig tests ===

    #[test]
    fn test_browser_download_config_default() {
        let config = BrowserDownloadConfig::default();
        assert_eq!(config.timeout_seconds, 300);
        assert!(config.auto_download);
        // download_dir should end with .cache/crawlrs/chromium
        assert!(config.download_dir.ends_with("chromium"));
    }

    #[test]
    fn test_browser_download_config_custom() {
        let config = BrowserDownloadConfig {
            download_dir: PathBuf::from("/tmp/custom-browser"),
            timeout_seconds: 600,
            auto_download: false,
        };
        assert_eq!(config.download_dir, PathBuf::from("/tmp/custom-browser"));
        assert_eq!(config.timeout_seconds, 600);
        assert!(!config.auto_download);
    }

    #[test]
    fn test_browser_download_config_clone() {
        let config = BrowserDownloadConfig {
            download_dir: PathBuf::from("/tmp/test"),
            timeout_seconds: 100,
            auto_download: true,
        };
        let cloned = config.clone();
        assert_eq!(config.download_dir, cloned.download_dir);
        assert_eq!(config.timeout_seconds, cloned.timeout_seconds);
        assert_eq!(config.auto_download, cloned.auto_download);
    }

    // === get_browser_executable_path tests ===

    #[test]
    fn test_get_browser_executable_path_returns_path_under_cache() {
        let cache_dir = PathBuf::from("/tmp/some-cache");
        let exe_path = get_browser_executable_path(&cache_dir);

        // The path should start with the cache directory
        assert!(exe_path.starts_with(&cache_dir));

        // Platform-specific assertions
        #[cfg(target_os = "linux")]
        {
            assert!(exe_path.ends_with("chrome"));
            assert!(exe_path.to_string_lossy().contains("chrome-linux"));
        }
        #[cfg(target_os = "windows")]
        {
            assert!(exe_path.ends_with("chrome.exe"));
            assert!(exe_path.to_string_lossy().contains("chrome-win"));
        }
        #[cfg(target_os = "macos")]
        {
            assert!(exe_path.ends_with("Chromium"));
            assert!(exe_path.to_string_lossy().contains("chrome-mac"));
        }
    }

    #[test]
    fn test_get_browser_executable_path_with_empty_dir() {
        let cache_dir = PathBuf::new();
        let exe_path = get_browser_executable_path(&cache_dir);
        // Should still produce a valid relative path
        #[cfg(target_os = "linux")]
        assert!(exe_path.ends_with("chrome"));
    }

    // === BrowserDownloadManager creation tests ===

    #[test]
    fn test_browser_download_manager_new() {
        let config = BrowserDownloadConfig {
            download_dir: PathBuf::from("/tmp/test-browser"),
            timeout_seconds: 60,
            auto_download: false,
        };
        let manager = BrowserDownloadManager::new(config);
        // Verify the manager is created (get_cache_dir confirms config is stored)
        assert_eq!(manager.get_cache_dir(), &PathBuf::from("/tmp/test-browser"));
    }

    #[test]
    fn test_browser_download_manager_with_defaults() {
        let manager = BrowserDownloadManager::with_defaults();
        let cache_dir = manager.get_cache_dir();
        assert!(cache_dir.ends_with("chromium"));
    }

    // === get_status tests ===

    #[tokio::test]
    async fn test_get_status_initial_not_downloaded() {
        let manager = BrowserDownloadManager::with_defaults();
        let status = manager.get_status().await;
        assert_eq!(status, DownloadStatus::NotDownloaded);
    }

    #[tokio::test]
    async fn test_get_status_initial_after_custom_config() {
        let config = BrowserDownloadConfig {
            download_dir: PathBuf::from("/nonexistent/path/test"),
            timeout_seconds: 60,
            auto_download: false,
        };
        let manager = BrowserDownloadManager::new(config);
        let status = manager.get_status().await;
        assert_eq!(status, DownloadStatus::NotDownloaded);
    }

    // === is_browser_downloaded tests ===

    #[tokio::test]
    async fn test_is_browser_downloaded_nonexistent_path() {
        let config = BrowserDownloadConfig {
            download_dir: PathBuf::from("/nonexistent/path/that/does/not/exist"),
            timeout_seconds: 60,
            auto_download: false,
        };
        let manager = BrowserDownloadManager::new(config);
        assert!(!manager.is_browser_downloaded().await);
    }

    #[tokio::test]
    async fn test_is_browser_downloaded_temp_empty_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = BrowserDownloadConfig {
            download_dir: temp_dir.path().to_path_buf(),
            timeout_seconds: 60,
            auto_download: false,
        };
        let manager = BrowserDownloadManager::new(config);
        // No executable in empty temp dir
        assert!(!manager.is_browser_downloaded().await);
    }

    // === get_cache_dir tests ===

    #[test]
    fn test_get_cache_dir_custom() {
        let custom_path = PathBuf::from("/custom/cache/dir");
        let config = BrowserDownloadConfig {
            download_dir: custom_path.clone(),
            timeout_seconds: 60,
            auto_download: false,
        };
        let manager = BrowserDownloadManager::new(config);
        assert_eq!(manager.get_cache_dir(), &custom_path);
    }

    // === cleanup tests ===

    #[tokio::test]
    async fn test_cleanup_nonexistent_dir() {
        let config = BrowserDownloadConfig {
            download_dir: PathBuf::from("/nonexistent/path/for/cleanup/test"),
            timeout_seconds: 60,
            auto_download: false,
        };
        let manager = BrowserDownloadManager::new(config);
        // cleanup on nonexistent dir should succeed (no-op)
        let result = manager.cleanup().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cleanup_existing_temp_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = BrowserDownloadConfig {
            download_dir: temp_dir.path().to_path_buf(),
            timeout_seconds: 60,
            auto_download: false,
        };
        let manager = BrowserDownloadManager::new(config);

        // Create a file inside the dir to verify it exists
        let test_file = temp_dir.path().join("test_file.txt");
        tokio::fs::write(&test_file, "test").await.unwrap();
        assert!(test_file.exists());

        // Cleanup should remove the directory
        let result = manager.cleanup().await;
        assert!(result.is_ok());
        assert!(!temp_dir.path().exists());

        // Status should be reset to NotDownloaded
        let status = manager.get_status().await;
        assert_eq!(status, DownloadStatus::NotDownloaded);
    }

    #[tokio::test]
    async fn test_cleanup_then_status_is_not_downloaded() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = BrowserDownloadConfig {
            download_dir: temp_dir.path().to_path_buf(),
            timeout_seconds: 60,
            auto_download: false,
        };
        let manager = BrowserDownloadManager::new(config);

        manager.cleanup().await.unwrap();
        let status = manager.get_status().await;
        assert_eq!(status, DownloadStatus::NotDownloaded);
    }

    // === find_system_browser tests ===

    #[tokio::test]
    async fn test_find_system_browser_returns_option() {
        let result = find_system_browser().await;
        // We can't assert specific result since it depends on the system
        // Just verify it doesn't panic
        let _ = result;
    }

    // === download_browser tests (using temp dir) ===

    #[tokio::test]
    async fn test_download_browser_no_system_browser_returns_error_or_path() {
        // Use a temp directory that definitely doesn't have a browser
        let temp_dir = tempfile::tempdir().unwrap();
        let config = BrowserDownloadConfig {
            download_dir: temp_dir.path().to_path_buf(),
            timeout_seconds: 5,
            auto_download: false,
        };
        let manager = BrowserDownloadManager::new(config);

        let result = manager.download_browser().await;

        // The result depends on whether a system browser is found:
        // - If system browser found: Ok(path)
        // - If no system browser: Err (fetcher not available without browser-download feature)
        match result {
            Ok(path) => {
                // System browser was found, path should exist
                assert!(path.exists());
                let status = manager.get_status().await;
                assert_eq!(status, DownloadStatus::Downloaded);
            }
            Err(e) => {
                // No system browser found, fetcher not available
                let status = manager.get_status().await;
                match status {
                    DownloadStatus::Failed(msg) => {
                        assert!(!msg.is_empty());
                    }
                    _ => panic!("Expected Failed status, got {:?}", status),
                }
                // Verify the error is DownloadFailed
                assert!(matches!(e, BrowserDownloadError::DownloadFailed(_)));
            }
        }
    }

    #[tokio::test]
    async fn test_download_browser_sets_status_downloading() {
        // This test verifies that after calling download_browser, status is not
        // NotDownloaded (it should be Downloaded or Failed)
        let temp_dir = tempfile::tempdir().unwrap();
        let config = BrowserDownloadConfig {
            download_dir: temp_dir.path().to_path_buf(),
            timeout_seconds: 5,
            auto_download: false,
        };
        let manager = BrowserDownloadManager::new(config);

        let _ = manager.download_browser().await;
        let status = manager.get_status().await;
        assert_ne!(status, DownloadStatus::NotDownloaded);
        assert_ne!(status, DownloadStatus::Downloading);
    }

    // === get_or_download_browser tests ===

    #[tokio::test]
    async fn test_get_or_download_browser_calls_download() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config = BrowserDownloadConfig {
            download_dir: temp_dir.path().to_path_buf(),
            timeout_seconds: 5,
            auto_download: false,
        };
        let manager = BrowserDownloadManager::new(config);

        let result = manager.get_or_download_browser().await;
        // Should behave same as download_browser
        if let Ok(path) = result {
            assert!(path.exists());
        }
    }

    // === Manager clone tests ===

    #[test]
    fn test_browser_download_manager_clone() {
        let config = BrowserDownloadConfig {
            download_dir: PathBuf::from("/tmp/clone-test"),
            timeout_seconds: 120,
            auto_download: true,
        };
        let manager = BrowserDownloadManager::new(config);
        let cloned = manager.clone();

        // Both should point to the same cache dir
        assert_eq!(manager.get_cache_dir(), cloned.get_cache_dir());
    }

    #[test]
    fn test_browser_download_manager_debug() {
        let manager = BrowserDownloadManager::with_defaults();
        let debug_str = format!("{:?}", manager);
        // Should contain "BrowserDownloadManager"
        assert!(debug_str.contains("BrowserDownloadManager"));
    }
}
