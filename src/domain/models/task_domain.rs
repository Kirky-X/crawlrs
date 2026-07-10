// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Task domain types and business logic
//!
//! This module contains only domain logic (enums, errors, helper methods).
//! The database entity definition is in task_entity.rs.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

/// 任务类型枚举
///
/// 定义了系统中支持的不同类型的任务，每种类型对应不同的
/// 处理逻辑和业务规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    /// 网页抓取任务，抓取单个网页的内容
    #[default]
    Scrape,
    /// 网站爬取任务，爬取整个网站或多个页面
    Crawl,
    /// 内容提取任务，从已抓取的内容中提取特定信息
    Extract,
}

impl TaskType {
    /// 返回任务类型的字符串表示
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskType::Scrape => "scrape",
            TaskType::Crawl => "crawl",
            TaskType::Extract => "extract",
        }
    }
}

impl fmt::Display for TaskType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

impl FromStr for TaskType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "scrape" => Ok(TaskType::Scrape),
            "crawl" => Ok(TaskType::Crawl),
            "extract" => Ok(TaskType::Extract),
            _ => Err(()),
        }
    }
}

/// 任务状态枚举
///
/// 表示任务在其生命周期中的不同状态，用于跟踪任务的执行进度。
/// 状态转换遵循以下流程：
/// Queued → Active → Completed/Failed/Cancelled
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    /// 已入队，任务已创建但尚未开始执行
    #[default]
    Queued,
    /// 活跃中，任务正在被执行
    Active,
    /// 已完成，任务成功执行完成
    Completed,
    /// 已失败，任务执行失败且已达到最大重试次数
    Failed,
    /// 已取消，任务被取消执行
    Cancelled,
}

impl fmt::Display for TaskStatus {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TaskStatus::Queued => write!(f, "queued"),
            TaskStatus::Active => write!(f, "active"),
            TaskStatus::Completed => write!(f, "completed"),
            TaskStatus::Failed => write!(f, "failed"),
            TaskStatus::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl FromStr for TaskStatus {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "queued" => Ok(TaskStatus::Queued),
            "active" => Ok(TaskStatus::Active),
            "completed" => Ok(TaskStatus::Completed),
            "failed" => Ok(TaskStatus::Failed),
            "cancelled" => Ok(TaskStatus::Cancelled),
            _ => Err(()),
        }
    }
}

/// 领域错误类型
///
/// 表示在领域层可能发生的各种错误情况，包括状态转换错误、
/// 验证失败、引擎相关错误和资源限制等。
#[derive(Error, Debug)]
pub enum DomainError {
    /// 无效的状态转换，当任务状态转换不符合业务规则时发生
    #[error("Invalid state transition from {from:?} to {to:?}")]
    InvalidStateTransition { from: TaskStatus, to: TaskStatus },

    /// 验证错误，当输入数据不符合领域规则时发生
    #[error("Validation error: {0}")]
    ValidationError(String),

    /// 引擎错误，当底层执行引擎出现问题时发生
    #[error("Engine error: {0}")]
    EngineError(String),

    /// 资源限制错误，当超出系统资源限制时发生
    #[error("Resource limit exceeded: {0}")]
    ResourceLimitExceeded(String),

    /// 未找到错误，当请求的资源不存在时发生
    #[error("Not found: {0}")]
    NotFound(String),

    /// 权限错误，当操作没有足够权限时发生
    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    /// 速率限制错误，当请求超出速率限制时发生
    #[error("Rate limit exceeded: {0}")]
    RateLimitExceeded(String),

    /// 锁定错误，当任务锁定失败或冲突时发生
    #[error("Lock error: {0}")]
    LockError(String),

    /// 过期错误，当任务已过期无法执行时发生
    #[error("Task expired")]
    TaskExpired,

    /// URL验证错误
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// SSRF防护触发
    #[error("SSRF protection triggered: {0}")]
    SsrfProtection(String),

    /// 超时错误，当请求超时时发生
    #[error("Timeout error: {0}")]
    TimeoutError(String),

    /// 网络错误，当网络连接问题时发生
    #[error("Network error: {0}")]
    NetworkError(String),

    /// 安全错误，当安全检查失败时发生
    #[error("Security error: {0}")]
    SecurityError(String),

    /// 爬取错误，当爬取过程中发生错误时发生
    #[error("Crawl error: {0}")]
    CrawlError(String),

    /// 数据库错误
    #[error("Database error: {0}")]
    DatabaseError(String),
}

impl From<anyhow::Error> for DomainError {
    fn from(error: anyhow::Error) -> Self {
        let error_msg = error.to_string().to_lowercase();

        if error_msg.contains("timeout") {
            DomainError::TimeoutError(error_msg)
        } else if error_msg.contains("connection") || error_msg.contains("network") {
            DomainError::NetworkError(error_msg)
        } else if error_msg.contains("parse") || error_msg.contains("validation") {
            DomainError::ValidationError(error_msg)
        } else if error_msg.contains("security") || error_msg.contains("ssrf") {
            DomainError::SecurityError(error_msg)
        } else if error_msg.contains("permission") || error_msg.contains("denied") {
            DomainError::PermissionDenied(error_msg)
        } else if error_msg.contains("database") || error_msg.contains("sql") {
            DomainError::DatabaseError(error_msg)
        } else {
            DomainError::EngineError(error_msg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    // ========== TaskType::as_str tests ==========

    #[test]
    fn test_task_type_as_str_all_variants() {
        assert_eq!(TaskType::Scrape.as_str(), "scrape");
        assert_eq!(TaskType::Crawl.as_str(), "crawl");
        assert_eq!(TaskType::Extract.as_str(), "extract");
    }

    // ========== TaskType Display tests ==========

    #[test]
    fn test_task_type_display_matches_as_str() {
        for ty in [TaskType::Scrape, TaskType::Crawl, TaskType::Extract] {
            assert_eq!(ty.to_string(), ty.as_str(), "Display should match as_str");
        }
    }

    // ========== TaskType FromStr tests ==========

    #[test]
    fn test_task_type_from_str_valid() {
        assert_eq!(
            TaskType::from_str("scrape").expect("valid"),
            TaskType::Scrape
        );
        assert_eq!(TaskType::from_str("crawl").expect("valid"), TaskType::Crawl);
        assert_eq!(
            TaskType::from_str("extract").expect("valid"),
            TaskType::Extract
        );
    }

    #[test]
    fn test_task_type_from_str_invalid_returns_err() {
        assert!(
            TaskType::from_str("unknown").is_err(),
            "unknown type should error"
        );
        assert!(TaskType::from_str("").is_err(), "empty string should error");
        assert!(
            TaskType::from_str("SCRAPE").is_err(),
            "uppercase should error (case-sensitive)"
        );
    }

    #[test]
    fn test_task_type_default_is_scrape() {
        assert_eq!(TaskType::default(), TaskType::Scrape);
    }

    #[test]
    fn test_task_type_serde_roundtrip() {
        for ty in [TaskType::Scrape, TaskType::Crawl, TaskType::Extract] {
            let json = serde_json::to_string(&ty).expect("serialize");
            let back: TaskType = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(ty, back, "roundtrip should preserve: {}", json);
        }
    }

    // ========== TaskStatus Display tests ==========

    #[test]
    fn test_task_status_display_all_variants() {
        assert_eq!(TaskStatus::Queued.to_string(), "queued");
        assert_eq!(TaskStatus::Active.to_string(), "active");
        assert_eq!(TaskStatus::Completed.to_string(), "completed");
        assert_eq!(TaskStatus::Failed.to_string(), "failed");
        assert_eq!(TaskStatus::Cancelled.to_string(), "cancelled");
    }

    // ========== TaskStatus FromStr tests ==========

    #[test]
    fn test_task_status_from_str_valid() {
        assert_eq!(
            TaskStatus::from_str("queued").expect("valid"),
            TaskStatus::Queued
        );
        assert_eq!(
            TaskStatus::from_str("active").expect("valid"),
            TaskStatus::Active
        );
        assert_eq!(
            TaskStatus::from_str("completed").expect("valid"),
            TaskStatus::Completed
        );
        assert_eq!(
            TaskStatus::from_str("failed").expect("valid"),
            TaskStatus::Failed
        );
        assert_eq!(
            TaskStatus::from_str("cancelled").expect("valid"),
            TaskStatus::Cancelled
        );
    }

    #[test]
    fn test_task_status_from_str_invalid_returns_err() {
        assert!(TaskStatus::from_str("unknown").is_err());
        assert!(TaskStatus::from_str("").is_err());
        assert!(TaskStatus::from_str("COMPLETED").is_err(), "case-sensitive");
    }

    #[test]
    fn test_task_status_default_is_queued() {
        assert_eq!(TaskStatus::default(), TaskStatus::Queued);
    }

    #[test]
    fn test_task_status_serde_roundtrip() {
        for status in [
            TaskStatus::Queued,
            TaskStatus::Active,
            TaskStatus::Completed,
            TaskStatus::Failed,
            TaskStatus::Cancelled,
        ] {
            let json = serde_json::to_string(&status).expect("serialize");
            let back: TaskStatus = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(status, back, "roundtrip should preserve: {}", json);
        }
    }

    // ========== DomainError Display tests ==========

    #[test]
    fn test_domain_error_invalid_state_transition_display() {
        let err = DomainError::InvalidStateTransition {
            from: TaskStatus::Queued,
            to: TaskStatus::Completed,
        };
        let msg = err.to_string();
        assert!(msg.contains("Invalid state transition"), "msg: {}", msg);
        assert!(msg.contains("Queued"), "should show from status: {}", msg);
        assert!(msg.contains("Completed"), "should show to status: {}", msg);
    }

    #[test]
    fn test_domain_error_validation_error_display() {
        let err = DomainError::ValidationError("bad input".to_string());
        let msg = err.to_string();
        assert!(msg.contains("Validation error"), "msg: {}", msg);
        assert!(msg.contains("bad input"), "msg: {}", msg);
    }

    #[test]
    fn test_domain_error_engine_error_display() {
        let err = DomainError::EngineError("timeout".to_string());
        assert!(err.to_string().contains("Engine error"));
        assert!(err.to_string().contains("timeout"));
    }

    #[test]
    fn test_domain_error_resource_limit_exceeded_display() {
        let err = DomainError::ResourceLimitExceeded("too many".to_string());
        assert!(err.to_string().contains("Resource limit exceeded"));
        assert!(err.to_string().contains("too many"));
    }

    #[test]
    fn test_domain_error_not_found_display() {
        let err = DomainError::NotFound("task 42".to_string());
        assert!(err.to_string().contains("Not found"));
        assert!(err.to_string().contains("task 42"));
    }

    #[test]
    fn test_domain_error_permission_denied_display() {
        let err = DomainError::PermissionDenied("no access".to_string());
        assert!(err.to_string().contains("Permission denied"));
        assert!(err.to_string().contains("no access"));
    }

    #[test]
    fn test_domain_error_rate_limit_exceeded_display() {
        let err = DomainError::RateLimitExceeded("100/min".to_string());
        assert!(err.to_string().contains("Rate limit exceeded"));
        assert!(err.to_string().contains("100/min"));
    }

    #[test]
    fn test_domain_error_lock_error_display() {
        let err = DomainError::LockError("held".to_string());
        assert!(err.to_string().contains("Lock error"));
        assert!(err.to_string().contains("held"));
    }

    #[test]
    fn test_domain_error_task_expired_display() {
        let err = DomainError::TaskExpired;
        assert_eq!(err.to_string(), "Task expired");
    }

    #[test]
    fn test_domain_error_invalid_url_display() {
        let err = DomainError::InvalidUrl("bad url".to_string());
        assert!(err.to_string().contains("Invalid URL"));
        assert!(err.to_string().contains("bad url"));
    }

    #[test]
    fn test_domain_error_ssrf_protection_display() {
        let err = DomainError::SsrfProtection("internal ip".to_string());
        assert!(err.to_string().contains("SSRF protection triggered"));
        assert!(err.to_string().contains("internal ip"));
    }

    #[test]
    fn test_domain_error_timeout_error_display() {
        let err = DomainError::TimeoutError("30s".to_string());
        assert!(err.to_string().contains("Timeout error"));
        assert!(err.to_string().contains("30s"));
    }

    #[test]
    fn test_domain_error_network_error_display() {
        let err = DomainError::NetworkError("conn refused".to_string());
        assert!(err.to_string().contains("Network error"));
        assert!(err.to_string().contains("conn refused"));
    }

    #[test]
    fn test_domain_error_security_error_display() {
        let err = DomainError::SecurityError("breach".to_string());
        assert!(err.to_string().contains("Security error"));
        assert!(err.to_string().contains("breach"));
    }

    #[test]
    fn test_domain_error_crawl_error_display() {
        let err = DomainError::CrawlError("depth".to_string());
        assert!(err.to_string().contains("Crawl error"));
        assert!(err.to_string().contains("depth"));
    }

    #[test]
    fn test_domain_error_database_error_display() {
        let err = DomainError::DatabaseError("deadlock".to_string());
        assert!(err.to_string().contains("Database error"));
        assert!(err.to_string().contains("deadlock"));
    }

    // ========== From<anyhow::Error> tests ==========

    #[test]
    fn test_from_anyhow_timeout_keyword_maps_to_timeout_error() {
        let err: DomainError = anyhow::anyhow!("request timeout after 30s").into();
        match err {
            DomainError::TimeoutError(msg) => {
                assert!(msg.contains("timeout"), "should contain keyword: {}", msg);
            }
            other => panic!("expected TimeoutError, got {:?}", other),
        }
    }

    #[test]
    fn test_from_anyhow_connection_keyword_maps_to_network_error() {
        let err: DomainError = anyhow::anyhow!("connection refused").into();
        match err {
            DomainError::NetworkError(msg) => {
                assert!(
                    msg.contains("connection"),
                    "should contain keyword: {}",
                    msg
                );
            }
            other => panic!("expected NetworkError, got {:?}", other),
        }
    }

    #[test]
    fn test_from_anyhow_network_keyword_maps_to_network_error() {
        let err: DomainError = anyhow::anyhow!("network unreachable").into();
        match err {
            DomainError::NetworkError(msg) => {
                assert!(msg.contains("network"), "should contain keyword: {}", msg);
            }
            other => panic!("expected NetworkError, got {:?}", other),
        }
    }

    #[test]
    fn test_from_anyhow_parse_keyword_maps_to_validation_error() {
        let err: DomainError = anyhow::anyhow!("failed to parse url").into();
        match err {
            DomainError::ValidationError(msg) => {
                assert!(msg.contains("parse"), "should contain keyword: {}", msg);
            }
            other => panic!("expected ValidationError, got {:?}", other),
        }
    }

    #[test]
    fn test_from_anyhow_validation_keyword_maps_to_validation_error() {
        let err: DomainError = anyhow::anyhow!("validation failed").into();
        match err {
            DomainError::ValidationError(msg) => {
                assert!(
                    msg.contains("validation"),
                    "should contain keyword: {}",
                    msg
                );
            }
            other => panic!("expected ValidationError, got {:?}", other),
        }
    }

    #[test]
    fn test_from_anyhow_security_keyword_maps_to_security_error() {
        let err: DomainError = anyhow::anyhow!("security violation").into();
        match err {
            DomainError::SecurityError(msg) => {
                assert!(msg.contains("security"), "should contain keyword: {}", msg);
            }
            other => panic!("expected SecurityError, got {:?}", other),
        }
    }

    #[test]
    fn test_from_anyhow_ssrf_keyword_maps_to_security_error() {
        let err: DomainError = anyhow::anyhow!("ssrf detected").into();
        match err {
            DomainError::SecurityError(msg) => {
                assert!(msg.contains("ssrf"), "should contain keyword: {}", msg);
            }
            other => panic!("expected SecurityError, got {:?}", other),
        }
    }

    #[test]
    fn test_from_anyhow_permission_keyword_maps_to_permission_denied() {
        let err: DomainError = anyhow::anyhow!("permission denied").into();
        match err {
            DomainError::PermissionDenied(msg) => {
                assert!(
                    msg.contains("permission"),
                    "should contain keyword: {}",
                    msg
                );
            }
            other => panic!("expected PermissionDenied, got {:?}", other),
        }
    }

    #[test]
    fn test_from_anyhow_denied_keyword_maps_to_permission_denied() {
        let err: DomainError = anyhow::anyhow!("access denied").into();
        match err {
            DomainError::PermissionDenied(msg) => {
                assert!(msg.contains("denied"), "should contain keyword: {}", msg);
            }
            other => panic!("expected PermissionDenied, got {:?}", other),
        }
    }

    #[test]
    fn test_from_anyhow_database_keyword_maps_to_database_error() {
        let err: DomainError = anyhow::anyhow!("database locked").into();
        match err {
            DomainError::DatabaseError(msg) => {
                assert!(msg.contains("database"), "should contain keyword: {}", msg);
            }
            other => panic!("expected DatabaseError, got {:?}", other),
        }
    }

    #[test]
    fn test_from_anyhow_sql_keyword_maps_to_database_error() {
        let err: DomainError = anyhow::anyhow!("sql syntax error").into();
        match err {
            DomainError::DatabaseError(msg) => {
                assert!(msg.contains("sql"), "should contain keyword: {}", msg);
            }
            other => panic!("expected DatabaseError, got {:?}", other),
        }
    }

    #[test]
    fn test_from_anyhow_unknown_keyword_maps_to_engine_error() {
        let err: DomainError = anyhow::anyhow!("something went wrong").into();
        match err {
            DomainError::EngineError(msg) => {
                assert!(
                    msg.contains("something went wrong"),
                    "should contain original message: {}",
                    msg
                );
            }
            other => panic!("expected EngineError, got {:?}", other),
        }
    }

    #[test]
    fn test_from_anyhow_message_lowercased() {
        // Verify the conversion lowercases the message (matters for keyword matching)
        let err: DomainError = anyhow::anyhow!("TIMEOUT Occurred").into();
        match err {
            DomainError::TimeoutError(msg) => {
                assert!(
                    msg.contains("timeout"),
                    "message should be lowercased for matching: {}",
                    msg
                );
            }
            other => panic!("expected TimeoutError, got {:?}", other),
        }
    }

    #[test]
    fn test_from_anyhow_timeout_takes_precedence_over_connection() {
        // "timeout" is checked first; a message with both should map to TimeoutError
        let err: DomainError = anyhow::anyhow!("connection timeout").into();
        assert!(
            matches!(err, DomainError::TimeoutError(_)),
            "timeout should take precedence over connection"
        );
    }
}
