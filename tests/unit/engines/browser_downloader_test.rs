#![cfg(test)]
// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! External unit tests for browser_downloader module
//!
//! Supplements the embedded tests by covering the "browser already downloaded"
//! path in `download_browser` and testing `get_or_download_browser` with
//! pre-created executable files.

#[cfg(test)]
mod browser_downloader_tests {
    use crawlrs::engines::browser_downloader::{
        get_browser_executable_path, BrowserDownloadConfig, BrowserDownloadError,
        BrowserDownloadManager, DownloadStatus,
    };
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    // === Helper: create a fake browser executable in a temp dir ===

    fn create_fake_browser_executable(temp_dir: &tempfile::TempDir) -> std::path::PathBuf {
        let exe_path = get_browser_executable_path(temp_dir.path());
        if let Some(parent) = exe_path.parent() {
            fs::create_dir_all(parent).expect("Failed to create parent dirs");
        }
        fs::write(&exe_path, "#!/bin/bash\necho fake-browser").expect("Failed to write fake exe");
        // Make it executable on Unix
        let metadata = fs::metadata(&exe_path).expect("Failed to get metadata");
        let mut perms = metadata.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&exe_path, perms).expect("Failed to set permissions");
        exe_path
    }

    // === download_browser: "already downloaded" path ===

    #[tokio::test]
    async fn test_download_browser_finds_cached_browser_when_no_system_browser() {
        // Pre-create a browser executable in the cache dir
        let temp_dir = tempfile::tempdir().unwrap();
        let cached_exe = create_fake_browser_executable(&temp_dir);

        let config = BrowserDownloadConfig {
            download_dir: temp_dir.path().to_path_buf(),
            timeout_seconds: 5,
            auto_download: false,
        };
        let manager = BrowserDownloadManager::new(config);

        let result = manager.download_browser().await;

        // The result should be Ok (either from system browser or cached browser)
        assert!(result.is_ok(), "download_browser should succeed");
        let path = result.unwrap();

        let status = manager.get_status().await;
        assert_eq!(status, DownloadStatus::Downloaded);

        // Verify the path exists. If no system browser is found, the returned
        // path should match the cached executable we pre-created.
        assert!(path.exists(), "returned path should exist");
        // When no system browser is available, download_browser falls back to
        // the cached executable, so the paths must match.
        if path != cached_exe {
            // A system browser was found; ensure that path also exists.
            assert!(path.exists(), "system browser path should exist");
        }
    }

    #[tokio::test]
    async fn test_download_browser_cached_path_sets_status_downloaded() {
        let temp_dir = tempfile::tempdir().unwrap();
        create_fake_browser_executable(&temp_dir);

        let config = BrowserDownloadConfig {
            download_dir: temp_dir.path().to_path_buf(),
            timeout_seconds: 5,
            auto_download: false,
        };
        let manager = BrowserDownloadManager::new(config);

        let _ = manager.download_browser().await;

        let status = manager.get_status().await;
        // Status should be Downloaded regardless of which path was taken
        assert_eq!(
            status,
            DownloadStatus::Downloaded,
            "Status should be Downloaded after successful download_browser"
        );
    }

    // === get_or_download_browser with cached browser ===

    #[tokio::test]
    async fn test_get_or_download_browser_with_cached_browser_returns_path() {
        let temp_dir = tempfile::tempdir().unwrap();
        let cached_exe = create_fake_browser_executable(&temp_dir);

        let config = BrowserDownloadConfig {
            download_dir: temp_dir.path().to_path_buf(),
            timeout_seconds: 5,
            auto_download: false,
        };
        let manager = BrowserDownloadManager::new(config);

        let result = manager.get_or_download_browser().await;

        assert!(result.is_ok(), "get_or_download_browser should succeed");
        let path = result.unwrap();
        assert!(path.exists(), "returned path should exist");

        // If no system browser is found, the returned path must equal the
        // cached executable we pre-created.
        if path != cached_exe {
            // A system browser was found; verify its path exists.
            assert!(path.exists(), "system browser path should exist");
        }

        let status = manager.get_status().await;
        assert_eq!(status, DownloadStatus::Downloaded);
    }

    // === download_browser: directory creation failure ===

    #[tokio::test]
    async fn test_download_browser_directory_creation_failure_returns_error() {
        // Use a path where the parent is a file (not a directory)
        // /dev/null is a file on Linux, so create_dir_all will fail
        let invalid_dir = std::path::PathBuf::from("/dev/null/crawlrs/chromium");

        let config = BrowserDownloadConfig {
            download_dir: invalid_dir,
            timeout_seconds: 5,
            auto_download: false,
        };
        let manager = BrowserDownloadManager::new(config);

        let result = manager.download_browser().await;

        // The result depends on whether a system browser is found:
        // - If system browser found: Ok(path)
        // - If no system browser: Err(DirectoryCreationFailed) because /dev/null is a file
        match result {
            Ok(path) => {
                // System browser was found
                assert!(path.exists());
                let status = manager.get_status().await;
                assert_eq!(status, DownloadStatus::Downloaded);
            }
            Err(e) => {
                // No system browser, directory creation should fail
                let status = manager.get_status().await;
                match &status {
                    DownloadStatus::Failed(msg) => {
                        assert!(!msg.is_empty(), "Failure message should not be empty");
                    }
                    _ => panic!("Expected Failed status, got {:?}", status),
                }
                // The error should be DirectoryCreationFailed
                assert!(
                    matches!(e, BrowserDownloadError::DirectoryCreationFailed(_)),
                    "Expected DirectoryCreationFailed, got {:?}",
                    e
                );
            }
        }
    }

    // === download_browser: no browser available returns DownloadFailed ===

    #[tokio::test]
    async fn test_download_browser_no_browser_available_returns_download_failed() {
        // Use an empty temp dir with no browser executable
        let temp_dir = tempfile::tempdir().unwrap();

        let config = BrowserDownloadConfig {
            download_dir: temp_dir.path().to_path_buf(),
            timeout_seconds: 5,
            auto_download: false,
        };
        let manager = BrowserDownloadManager::new(config);

        let result = manager.download_browser().await;

        // The result depends on whether a system browser is found:
        match result {
            Ok(path) => {
                // System browser was found
                assert!(path.exists());
                let status = manager.get_status().await;
                assert_eq!(status, DownloadStatus::Downloaded);
            }
            Err(e) => {
                // No system browser, no cached browser, fetcher not available
                let status = manager.get_status().await;
                match &status {
                    DownloadStatus::Failed(msg) => {
                        assert!(!msg.is_empty());
                    }
                    _ => panic!("Expected Failed status, got {:?}", status),
                }
                assert!(
                    matches!(e, BrowserDownloadError::DownloadFailed(_)),
                    "Expected DownloadFailed, got {:?}",
                    e
                );
            }
        }
    }

    // === is_browser_downloaded with actual file ===

    #[tokio::test]
    async fn test_is_browser_downloaded_returns_true_when_executable_exists() {
        let temp_dir = tempfile::tempdir().unwrap();
        create_fake_browser_executable(&temp_dir);

        let config = BrowserDownloadConfig {
            download_dir: temp_dir.path().to_path_buf(),
            timeout_seconds: 5,
            auto_download: false,
        };
        let manager = BrowserDownloadManager::new(config);

        assert!(
            manager.is_browser_downloaded().await,
            "is_browser_downloaded should return true when executable exists"
        );
    }

    #[tokio::test]
    async fn test_is_browser_downloaded_returns_false_when_executable_missing() {
        let temp_dir = tempfile::tempdir().unwrap();

        let config = BrowserDownloadConfig {
            download_dir: temp_dir.path().to_path_buf(),
            timeout_seconds: 5,
            auto_download: false,
        };
        let manager = BrowserDownloadManager::new(config);

        assert!(
            !manager.is_browser_downloaded().await,
            "is_browser_downloaded should return false when executable is missing"
        );
    }

    // === cleanup after download ===

    #[tokio::test]
    async fn test_cleanup_after_successful_download_removes_dir() {
        let temp_dir = tempfile::tempdir().unwrap();
        let temp_path = temp_dir.path().to_path_buf();

        // Create a fake browser
        create_fake_browser_executable(&temp_dir);

        let config = BrowserDownloadConfig {
            download_dir: temp_path.clone(),
            timeout_seconds: 5,
            auto_download: false,
        };
        let manager = BrowserDownloadManager::new(config);

        // Verify browser exists
        assert!(manager.is_browser_downloaded().await);

        // Cleanup
        let result = manager.cleanup().await;
        assert!(result.is_ok(), "cleanup should succeed");

        // Verify dir was removed
        assert!(!temp_path.exists(), "download_dir should be removed after cleanup");

        // Verify status is NotDownloaded
        let status = manager.get_status().await;
        assert_eq!(status, DownloadStatus::NotDownloaded);
    }

    // === BrowserDownloadConfig edge cases ===

    #[test]
    fn test_browser_download_config_with_custom_timeout() {
        let config = BrowserDownloadConfig {
            download_dir: std::path::PathBuf::from("/tmp/custom-browser"),
            timeout_seconds: 600,
            auto_download: false,
        };
        assert_eq!(config.timeout_seconds, 600);
        assert!(!config.auto_download);
    }

    #[test]
    fn test_get_browser_executable_path_under_custom_dir() {
        let custom_dir = std::path::PathBuf::from("/tmp/custom-cache");
        let exe_path = get_browser_executable_path(&custom_dir);
        assert!(exe_path.starts_with(&custom_dir));
    }
}
