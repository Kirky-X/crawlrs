// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! ScrapeWorker 错误类型
//!
//! 定义工作器模块使用的错误类型

use thiserror::Error;

/// 工作器错误类型
#[derive(Error, Debug)]
pub enum ScrapeWorkerError {
    /// 正则表达式编译错误
    #[error("正则表达式编译错误: {0}")]
    RegexError(String),

    /// 缓存锁获取失败
    #[error("正则表达式缓存锁获取失败")]
    CacheLockError,

    /// 选择器解析错误
    #[error("选择器解析错误: {0}")]
    SelectorError(String),

    /// 任务处理错误
    #[error("任务处理错误: {0}")]
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
        ScrapeWorkerError::TaskError(format!("URL解析错误: {}", e))
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
            msg.contains("正则表达式编译错误"),
            "Display should contain 正则表达式编译错误"
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
            msg.contains("正则表达式缓存锁获取失败"),
            "Display should contain cache lock message"
        );
    }

    #[test]
    fn test_selector_error_display() {
        let err = ScrapeWorkerError::SelectorError("bad selector".to_string());
        let msg = format!("{}", err);
        assert!(
            msg.contains("选择器解析错误"),
            "Display should contain 选择器解析错误"
        );
        assert!(msg.contains("bad selector"));
    }

    #[test]
    fn test_task_error_display() {
        let err = ScrapeWorkerError::TaskError("task failed".to_string());
        let msg = format!("{}", err);
        assert!(
            msg.contains("任务处理错误"),
            "Display should contain 任务处理错误"
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
                    msg.contains("URL解析错误"),
                    "From<url::ParseError> should contain URL解析错误 prefix"
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
