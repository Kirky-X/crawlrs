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
use uuid::Uuid;

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

impl fmt::Display for TaskType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            TaskType::Scrape => write!(f, "scrape"),
            TaskType::Crawl => write!(f, "crawl"),
            TaskType::Extract => write!(f, "extract"),
        }
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
