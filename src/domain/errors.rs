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
    use std::error::Error as _;

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

    #[test]
    fn test_crawl_config_error_without_source() {
        let error = DomainError::crawl_config("missing field");
        let msg = error.to_string();
        assert!(msg.contains("爬虫配置无效"));
        assert!(msg.contains("missing field"));
        // source 为 None 时不应 panic
        assert!(error.source().is_none());
    }

    #[test]
    fn test_crawl_config_error_with_source() {
        let source_err = std::io::Error::new(std::io::ErrorKind::Other, "io failed");
        let error = DomainError::CrawlConfigError {
            message: "wrapped".to_string(),
            source: Some(Box::new(source_err)),
        };
        let msg = error.to_string();
        assert!(msg.contains("爬虫配置无效"));
        assert!(msg.contains("wrapped"));
        // source 链应返回内部错误
        let src = error.source();
        assert!(src.is_some());
        assert!(src.unwrap().to_string().contains("io failed"));
    }

    #[test]
    fn test_crawl_depth_exceeded() {
        let error = DomainError::CrawlDepthExceeded {
            max: 5,
            requested: 10,
        };
        let msg = error.to_string();
        assert!(msg.contains("爬取深度超出限制"));
        assert!(msg.contains("5"));
        assert!(msg.contains("10"));
    }

    #[test]
    fn test_crawl_depth_exceeded_boundary_zero() {
        let error = DomainError::CrawlDepthExceeded {
            max: 0,
            requested: 1,
        };
        let msg = error.to_string();
        assert!(msg.contains("最大深度 0"));
        assert!(msg.contains("请求深度 1"));
    }

    #[test]
    fn test_path_filtered() {
        let error = DomainError::PathFiltered {
            path: "/admin/*".to_string(),
        };
        let msg = error.to_string();
        assert!(msg.contains("URL路径被过滤规则排除"));
        assert!(msg.contains("/admin/*"));
    }

    #[test]
    fn test_path_filtered_empty_string() {
        let error = DomainError::PathFiltered {
            path: String::new(),
        };
        let msg = error.to_string();
        assert!(msg.contains("URL路径被过滤规则排除"));
        // 空字符串边界值：仍能渲染
        assert!(!msg.is_empty());
    }

    #[test]
    fn test_invalid_task_state() {
        let error = DomainError::InvalidTaskState {
            current: "Running".to_string(),
            expected: "Pending".to_string(),
        };
        let msg = error.to_string();
        assert!(msg.contains("任务状态无效"));
        assert!(msg.contains("Running"));
        assert!(msg.contains("Pending"));
    }

    #[test]
    fn test_task_expired() {
        let created_at = chrono::Utc::now();
        let error = DomainError::TaskExpired {
            created_at,
            timeout_seconds: 60,
        };
        let msg = error.to_string();
        assert!(msg.contains("任务已过期"));
        // Display 格式可能不是 RFC3339，验证日期组成部分（年月日）存在
        assert!(msg.contains(&created_at.format("%Y").to_string()));
        assert!(msg.contains("60"));
    }

    #[test]
    fn test_task_expired_boundary_zero_timeout() {
        let created_at = chrono::Utc::now();
        let error = DomainError::TaskExpired {
            created_at,
            timeout_seconds: 0,
        };
        let msg = error.to_string();
        assert!(msg.contains("超时时间 0秒"));
    }

    #[test]
    fn test_team_not_found() {
        let team_id = uuid::Uuid::new_v4();
        let error = DomainError::TeamNotFound { team_id };
        let msg = error.to_string();
        assert!(msg.contains("团队不存在"));
        assert!(msg.contains(&team_id.to_string()));
    }

    #[test]
    fn test_team_not_found_nil_uuid() {
        let team_id = uuid::Uuid::nil();
        let error = DomainError::TeamNotFound { team_id };
        let msg = error.to_string();
        assert!(msg.contains("团队不存在"));
        assert!(msg.contains("00000000-0000-0000-0000-000000000000"));
    }

    #[test]
    fn test_concurrency_limit_exceeded() {
        let error = DomainError::ConcurrencyLimitExceeded {
            current: 11,
            limit: 10,
        };
        let msg = error.to_string();
        assert!(msg.contains("团队并发限制"));
        assert!(msg.contains("11"));
        assert!(msg.contains("10"));
    }

    #[test]
    fn test_concurrency_limit_exceeded_boundary_zero() {
        let error = DomainError::ConcurrencyLimitExceeded {
            current: 0,
            limit: 0,
        };
        let msg = error.to_string();
        assert!(msg.contains("当前 0"));
        assert!(msg.contains("限制 0"));
    }

    #[test]
    fn test_domain_blacklisted() {
        let error = DomainError::DomainBlacklisted {
            domain: "example.com".to_string(),
        };
        let msg = error.to_string();
        assert!(msg.contains("URL在黑名单中"));
        assert!(msg.contains("example.com"));
    }

    #[test]
    fn test_robots_forbidden() {
        let error = DomainError::RobotsForbidden {
            url: "https://example.com/private".to_string(),
        };
        let msg = error.to_string();
        assert!(msg.contains("URL被robots.txt禁止"));
        assert!(msg.contains("https://example.com/private"));
    }

    #[test]
    fn test_webhook_delivery_failed() {
        let error = DomainError::WebhookDeliveryFailed {
            url: "https://hook.example.com".to_string(),
            status: 503,
        };
        let msg = error.to_string();
        assert!(msg.contains("Webhook投递失败"));
        assert!(msg.contains("https://hook.example.com"));
        assert!(msg.contains("503"));
    }

    #[test]
    fn test_webhook_delivery_failed_boundary_status() {
        let error = DomainError::WebhookDeliveryFailed {
            url: "https://hook.example.com".to_string(),
            status: u16::MAX,
        };
        let msg = error.to_string();
        assert!(msg.contains(&u16::MAX.to_string()));
    }

    #[test]
    fn test_invalid_webhook_url() {
        let error = DomainError::InvalidWebhookUrl {
            url: "ftp://not-webhook".to_string(),
        };
        let msg = error.to_string();
        assert!(msg.contains("无效的webhook URL"));
        assert!(msg.contains("ftp://not-webhook"));
    }

    #[test]
    fn test_llm_extraction_failed() {
        let error = DomainError::LLMExtractionFailed {
            model: "gpt-4".to_string(),
            message: "rate limited".to_string(),
        };
        let msg = error.to_string();
        assert!(msg.contains("LLM提取失败"));
        assert!(msg.contains("gpt-4"));
        assert!(msg.contains("rate limited"));
    }

    #[test]
    fn test_invalid_css_selector() {
        let error = DomainError::InvalidCssSelector {
            selector: "div >".to_string(),
        };
        let msg = error.to_string();
        assert!(msg.contains("CSS选择器无效"));
        assert!(msg.contains("div >"));
    }

    #[test]
    fn test_validation_helper_constructor() {
        let error = DomainError::validation("email", "格式不正确");
        match error {
            DomainError::ValidationError { field, message } => {
                assert_eq!(field, "email");
                assert_eq!(message, "格式不正确");
            }
            _ => panic!("expected ValidationError variant"),
        }
    }

    #[test]
    fn test_crawl_config_helper_constructor() {
        let error = DomainError::crawl_config("bad config");
        match error {
            DomainError::CrawlConfigError { message, source } => {
                assert_eq!(message, "bad config");
                assert!(source.is_none());
            }
            _ => panic!("expected CrawlConfigError variant"),
        }
    }

    #[test]
    fn test_insufficient_credits_boundary_zero() {
        let error = DomainError::InsufficientCredits {
            required: 0,
            available: 0,
        };
        let msg = error.to_string();
        assert!(msg.contains("需要 0"));
        assert!(msg.contains("可用 0"));
    }

    #[test]
    fn test_insufficient_credits_negative_values() {
        let error = DomainError::InsufficientCredits {
            required: -1,
            available: -10,
        };
        let msg = error.to_string();
        assert!(msg.contains("-1"));
        assert!(msg.contains("-10"));
    }

    #[test]
    fn test_all_variants_implement_std_error() {
        // 确保 DomainError 实现 std::error::Error trait
        fn assert_error<T: std::error::Error>(_: &T) {}
        let errors: Vec<DomainError> = vec![
            DomainError::crawl_config("a"),
            DomainError::CrawlDepthExceeded {
                max: 1,
                requested: 2,
            },
            DomainError::PathFiltered { path: "p".into() },
            DomainError::TaskNotFound {
                task_id: uuid::Uuid::nil(),
            },
            DomainError::InvalidTaskState {
                current: "a".into(),
                expected: "b".into(),
            },
            DomainError::TaskExpired {
                created_at: chrono::Utc::now(),
                timeout_seconds: 1,
            },
            DomainError::TeamNotFound {
                team_id: uuid::Uuid::nil(),
            },
            DomainError::InsufficientCredits {
                required: 1,
                available: 0,
            },
            DomainError::ConcurrencyLimitExceeded {
                current: 1,
                limit: 0,
            },
            DomainError::InvalidUrl { url: "u".into() },
            DomainError::DomainBlacklisted { domain: "d".into() },
            DomainError::RobotsForbidden { url: "u".into() },
            DomainError::WebhookDeliveryFailed {
                url: "u".into(),
                status: 500,
            },
            DomainError::InvalidWebhookUrl { url: "u".into() },
            DomainError::LLMExtractionFailed {
                model: "m".into(),
                message: "e".into(),
            },
            DomainError::InvalidCssSelector {
                selector: "s".into(),
            },
            DomainError::ValidationError {
                field: "f".into(),
                message: "m".into(),
            },
        ];
        for err in &errors {
            assert_error(err);
        }
    }
}
