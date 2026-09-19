// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! ScrapeWorker 错误类型
//!
//! 定义工作器模块使用的错误类型

use thiserror::Error;

/// 工作器错误类型
#[derive(Error, Debug)]
pub enum ScrapeWorkerError {
    /// 正则表达式编译错误
    #[error("Regex compilation failed: {0}")]
    RegexError(String),

    /// 缓存锁获取失败
    #[error("Regex cache lock acquisition failed")]
    CacheLockError,

    /// 选择器解析错误
    #[error("Selector parse error: {0}")]
    SelectorError(String),

    /// 任务处理错误
    #[error("Task processing error: {0}")]
    TaskError(String),
}

impl From<String> for ScrapeWorkerError {
    fn from(msg: String) -> Self {
        ScrapeWorkerError::TaskError(msg)
    }
}

impl From<regex::Error> for ScrapeWorkerError {
    fn from(e: regex::Error) -> Self {
        ScrapeWorkerError::RegexError(e.to_string())
    }
}

impl From<url::ParseError> for ScrapeWorkerError {
    fn from(e: url::ParseError) -> Self {
        ScrapeWorkerError::TaskError(format!("URL parse error: {}", e))
    }
}

// 注意：scraper crate 的 SelectorError 不是公开类型
// 如果需要处理选择器错误，可以使用 Result 类型的错误信息

#[cfg(test)]
mod tests {
    use super::*;

    // ========== Display / error message tests ==========

    #[test]
    fn test_regex_error_display() {
        let err = ScrapeWorkerError::RegexError("invalid pattern".to_string());
        let msg = format!("{}", err);
        assert!(
            msg.contains("Regex compilation failed"),
            "Display should contain \"Regex compilation failed\""
        );
        assert!(
            msg.contains("invalid pattern"),
            "Display should contain the inner message"
        );
    }

    #[test]
    fn test_cache_lock_error_display() {
        let err = ScrapeWorkerError::CacheLockError;
        let msg = format!("{}", err);
        assert!(
            msg.contains("Regex cache lock acquisition failed"),
            "Display should contain cache lock message"
        );
    }

    #[test]
    fn test_selector_error_display() {
        let err = ScrapeWorkerError::SelectorError("bad selector".to_string());
        let msg = format!("{}", err);
        assert!(
            msg.contains("Selector parse error"),
            "Display should contain \"Selector parse error\""
        );
        assert!(msg.contains("bad selector"));
    }

    #[test]
    fn test_task_error_display() {
        let err = ScrapeWorkerError::TaskError("task failed".to_string());
        let msg = format!("{}", err);
        assert!(
            msg.contains("Task processing error"),
            "Display should contain \"Task processing error\""
        );
        assert!(msg.contains("task failed"));
    }

    // ========== Debug tests ==========

    #[test]
    fn test_regex_error_debug() {
        let err = ScrapeWorkerError::RegexError("dbg".to_string());
        let dbg = format!("{:?}", err);
        assert!(
            dbg.contains("RegexError"),
            "Debug should contain variant name"
        );
    }

    #[test]
    fn test_cache_lock_error_debug() {
        let err = ScrapeWorkerError::CacheLockError;
        let dbg = format!("{:?}", err);
        assert!(
            dbg.contains("CacheLockError"),
            "Debug should contain variant name"
        );
    }

    #[test]
    fn test_selector_error_debug() {
        let err = ScrapeWorkerError::SelectorError("dbg-sel".to_string());
        let dbg = format!("{:?}", err);
        assert!(
            dbg.contains("SelectorError"),
            "Debug should contain variant name"
        );
    }

    #[test]
    fn test_task_error_debug() {
        let err = ScrapeWorkerError::TaskError("dbg-task".to_string());
        let dbg = format!("{:?}", err);
        assert!(
            dbg.contains("TaskError"),
            "Debug should contain variant name"
        );
    }

    // ========== From<String> tests ==========

    #[test]
    fn test_from_string_creates_task_error() {
        let msg = "something went wrong".to_string();
        let err: ScrapeWorkerError = msg.into();
        match err {
            ScrapeWorkerError::TaskError(m) => {
                assert_eq!(
                    m, "something went wrong",
                    "From<String> should preserve message"
                );
            }
            other => panic!("Expected TaskError, got {:?}", other),
        }
    }

    #[test]
    fn test_from_empty_string_creates_task_error() {
        let msg = String::new();
        let err: ScrapeWorkerError = msg.into();
        match err {
            ScrapeWorkerError::TaskError(m) => {
                assert!(
                    m.is_empty(),
                    "empty string should map to TaskError with empty msg"
                );
            }
            other => panic!("Expected TaskError, got {:?}", other),
        }
    }

    // ========== From<regex::Error> tests ==========

    #[test]
    #[allow(clippy::invalid_regex)]
    fn test_from_regex_error_creates_regex_error() {
        // An unclosed parenthesis produces a regex::Error.
        let regex_err = regex::Regex::new("(unclosed").unwrap_err();
        let err: ScrapeWorkerError = regex_err.into();
        match err {
            ScrapeWorkerError::RegexError(msg) => {
                assert!(
                    !msg.is_empty(),
                    "From<regex::Error> should produce non-empty msg"
                );
            }
            other => panic!("Expected RegexError, got {:?}", other),
        }
    }

    // ========== From<url::ParseError> tests ==========

    #[test]
    fn test_from_url_parse_error_creates_task_error() {
        let url_err = url::Url::parse("not a valid url").unwrap_err();
        let err: ScrapeWorkerError = url_err.into();
        match err {
            ScrapeWorkerError::TaskError(msg) => {
                assert!(
                    msg.contains("URL parse error"),
                    "From<url::ParseError> should contain \"URL parse error\" prefix"
                );
            }
            other => panic!("Expected TaskError, got {:?}", other),
        }
    }

    // ========== std::error::Error source tests ==========

    #[test]
    fn test_regex_error_is_std_error() {
        let err = ScrapeWorkerError::RegexError("e".to_string());
        // ScrapeWorkerError derives thiserror::Error, so it implements std::error::Error.
        // The source for these variants is None (no #[source] attribute).
        assert!(std::error::Error::source(&err).is_none());
    }

    #[test]
    fn test_cache_lock_error_source_is_none() {
        let err = ScrapeWorkerError::CacheLockError;
        assert!(std::error::Error::source(&err).is_none());
    }
}
