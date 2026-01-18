// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Domain层错误类型
//!
//! 定义业务逻辑层面的错误类型

use thiserror::Error;

/// Domain层错误类型
///
/// 表示业务逻辑层面的错误，这些错误是可预期的，
/// 应该由调用方处理（如返回给用户）
#[derive(Error, Debug)]
pub enum DomainError {
    // ==================== 爬虫配置错误 ====================
    #[error("爬虫配置无效: {message}")]
    CrawlConfigError {
        message: String,
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    #[error("爬取深度超出限制: 最大深度 {max}, 请求深度 {requested}")]
    CrawlDepthExceeded { max: u32, requested: u32 },

    #[error("URL路径被过滤规则排除: {path}")]
    PathFiltered { path: String },

    // ==================== 任务错误 ====================
    #[error("任务未找到: {task_id}")]
    TaskNotFound { task_id: uuid::Uuid },

    #[error("任务状态无效: 当前状态 {current}, 期望状态 {expected}")]
    InvalidTaskState { current: String, expected: String },

    #[error("任务已过期: 创建于 {created_at}, 超时时间 {timeout_seconds}秒")]
    TaskExpired {
        created_at: chrono::DateTime<chrono::Utc>,
        timeout_seconds: u64,
    },

    // ==================== 团队和配额错误 ====================
    #[error("团队不存在: {team_id}")]
    TeamNotFound { team_id: uuid::Uuid },

    #[error("积分不足: 需要 {required}, 可用 {available}")]
    InsufficientCredits { required: i64, available: i64 },

    #[error("团队并发限制: 当前 {current}, 限制 {limit}")]
    ConcurrencyLimitExceeded { current: usize, limit: usize },

    // ==================== URL验证错误 ====================
    #[error("无效的URL: {url}")]
    InvalidUrl { url: String },

    #[error("URL在黑名单中: {domain}")]
    DomainBlacklisted { domain: String },

    #[error("URL被robots.txt禁止: {url}")]
    RobotsForbidden { url: String },

    // ==================== Webhook错误 ====================
    #[error("Webhook投递失败: {url}, 状态码 {status}")]
    WebhookDeliveryFailed { url: String, status: u16 },

    #[error("无效的webhook URL: {url}")]
    InvalidWebhookUrl { url: String },

    // ==================== 提取错误 ====================
    #[error("LLM提取失败: {model}, 错误: {message}")]
    LLMExtractionFailed { model: String, message: String },

    #[error("CSS选择器无效: {selector}")]
    InvalidCssSelector { selector: String },

    // ==================== 验证错误 ====================
    #[error("输入验证失败: {field} - {message}")]
    ValidationError { field: String, message: String },
}

// 辅助构造函数
impl DomainError {
    pub fn crawl_config(message: impl Into<String>) -> Self {
        DomainError::CrawlConfigError {
            message: message.into(),
            source: None,
        }
    }

    pub fn validation(field: impl Into<String>, message: impl Into<String>) -> Self {
        DomainError::ValidationError {
            field: field.into(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insufficient_credits_error() {
        let error = DomainError::InsufficientCredits {
            required: 100,
            available: 50,
        };

        let msg = error.to_string();
        assert!(msg.contains("积分不足"));
        assert!(msg.contains("100"));
        assert!(msg.contains("50"));
    }

    #[test]
    fn test_validation_error() {
        let error = DomainError::validation("url", "Invalid format");

        let msg = error.to_string();
        assert!(msg.contains("输入验证失败"));
        assert!(msg.contains("url"));
        assert!(msg.contains("Invalid format"));
    }

    #[test]
    fn test_task_not_found() {
        let task_id = uuid::Uuid::new_v4();
        let error = DomainError::TaskNotFound { task_id };

        let msg = error.to_string();
        assert!(msg.contains("任务未找到"));
        assert!(msg.contains(&task_id.to_string()));
    }

    #[test]
    fn test_invalid_url() {
        let error = DomainError::InvalidUrl {
            url: "not-a-url".to_string(),
        };

        let msg = error.to_string();
        assert!(msg.contains("无效的URL"));
        assert!(msg.contains("not-a-url"));
    }
}
