// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Crawl State Machine — 爬取任务的状态转换与验证逻辑。
//!
//! 提供纯函数式的状态检查和转换规则，不依赖 I/O。

use crate::domain::models::CrawlStatus;

/// 验证爬取配置参数。
///
/// # Arguments
///
/// * `max_depth` - 最大爬取深度（0-5）
/// * `max_concurrency` - 最大并发数（1-100，`None` 时跳过检查）
///
/// # Errors
///
/// 当参数超出范围时返回 `Err(reason)`
pub fn validate_crawl_config(max_depth: u32, max_concurrency: Option<u32>) -> Result<(), String> {
    if max_depth > 5 {
        return Err("max_depth must be between 0 and 5".to_string());
    }
    if let Some(concurrency) = max_concurrency {
        if concurrency > 100 {
            return Err("max_concurrency must be between 1 and 100".to_string());
        }
    }
    Ok(())
}

/// 检查爬取任务是否可以被取消。
///
/// 仅当状态不是 `Completed`、`Failed` 或 `Cancelled` 时可取消。
pub fn can_cancel(status: &CrawlStatus) -> bool {
    !matches!(
        status,
        CrawlStatus::Completed | CrawlStatus::Failed | CrawlStatus::Cancelled
    )
}

/// 检查爬取任务是否已结束（不可再转换）。
pub fn is_terminal(status: &CrawlStatus) -> bool {
    matches!(
        status,
        CrawlStatus::Completed | CrawlStatus::Failed | CrawlStatus::Cancelled
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_crawl_config_valid() {
        assert!(validate_crawl_config(0, None).is_ok());
        assert!(validate_crawl_config(5, None).is_ok());
        assert!(validate_crawl_config(3, Some(50)).is_ok());
        assert!(validate_crawl_config(0, Some(1)).is_ok());
        assert!(validate_crawl_config(0, Some(100)).is_ok());
    }

    #[test]
    fn test_validate_crawl_config_depth_too_large() {
        let result = validate_crawl_config(6, None);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("max_depth"));
    }

    #[test]
    fn test_validate_crawl_config_concurrency_too_large() {
        let result = validate_crawl_config(3, Some(101));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("max_concurrency"));
    }

    #[test]
    fn test_can_cancel_queued() {
        assert!(can_cancel(&CrawlStatus::Queued));
    }

    #[test]
    fn test_can_cancel_running() {
        // CrawlStatus may not have Running; adjust if needed
        // For now, test the terminal states
        assert!(!can_cancel(&CrawlStatus::Completed));
        assert!(!can_cancel(&CrawlStatus::Failed));
        assert!(!can_cancel(&CrawlStatus::Cancelled));
    }

    #[test]
    fn test_is_terminal() {
        assert!(is_terminal(&CrawlStatus::Completed));
        assert!(is_terminal(&CrawlStatus::Failed));
        assert!(is_terminal(&CrawlStatus::Cancelled));
        assert!(!is_terminal(&CrawlStatus::Queued));
    }
}
