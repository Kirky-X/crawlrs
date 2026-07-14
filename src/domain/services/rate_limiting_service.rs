// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 限流策略类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RateLimitStrategy {
    /// 令牌桶算法
    TokenBucket,
    /// 漏桶算法
    LeakyBucket,
    /// 固定窗口计数器
    FixedWindow,
    /// 滑动窗口计数器
    SlidingWindow,
}

/// 并发控制策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConcurrencyStrategy {
    /// 分布式信号量
    DistributedSemaphore,
    /// 信号量
    Semaphore,
    /// 数据库级别的并发控制
    DatabaseLevel,
}

/// 限流配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// 限流策略
    pub strategy: RateLimitStrategy,
    /// 每秒允许的请求数
    pub requests_per_second: u32,
    /// 每分钟允许的请求数
    pub requests_per_minute: u32,
    /// 每小时允许的请求数
    pub requests_per_hour: u32,
    /// 令牌桶容量（如果使用令牌桶算法）
    pub bucket_capacity: Option<u32>,
    /// 是否启用限流
    pub enabled: bool,
}

impl RateLimitConfig {
    /// 验证配置的有效性
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.requests_per_second == 0 {
            return Err(ValidationError::ZeroRate(
                "requests_per_second cannot be zero",
            ));
        }
        if self.requests_per_minute == 0 {
            return Err(ValidationError::ZeroRate(
                "requests_per_minute cannot be zero",
            ));
        }
        if self.requests_per_hour == 0 {
            return Err(ValidationError::ZeroRate(
                "requests_per_hour cannot be zero",
            ));
        }
        // 确保速率一致：每秒速率不应超过每分钟速率/60，每分钟速率不应超过每小时速率/60
        if self.requests_per_second > self.requests_per_minute / 60 {
            return Err(ValidationError::InconsistentRates(
                "requests_per_second exceeds requests_per_minute / 60".to_string(),
            ));
        }
        if self.requests_per_minute > self.requests_per_hour / 60 {
            return Err(ValidationError::InconsistentRates(
                "requests_per_minute exceeds requests_per_hour / 60".to_string(),
            ));
        }
        if let Some(capacity) = self.bucket_capacity {
            if capacity == 0 {
                return Err(ValidationError::ZeroCapacity(
                    "bucket_capacity cannot be zero".to_string(),
                ));
            }
        }
        Ok(())
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            strategy: RateLimitStrategy::TokenBucket,
            requests_per_second: 10,
            requests_per_minute: 100,
            requests_per_hour: 1000,
            bucket_capacity: Some(100),
            enabled: true,
        }
    }
}

/// 并发控制配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConcurrencyConfig {
    /// 并发控制策略
    pub strategy: ConcurrencyStrategy,
    /// 最大并发数
    pub max_concurrent_tasks: u32,
    /// 团队级别的最大并发数
    pub max_concurrent_per_team: u32,
    /// 锁的超时时间（秒）
    pub lock_timeout_seconds: u64,
    /// 是否启用并发控制
    pub enabled: bool,
}

impl ConcurrencyConfig {
    /// 验证配置的有效性
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.max_concurrent_tasks == 0 {
            return Err(ValidationError::ZeroCapacity(
                "max_concurrent_tasks cannot be zero".to_string(),
            ));
        }
        if self.max_concurrent_per_team == 0 {
            return Err(ValidationError::ZeroCapacity(
                "max_concurrent_per_team cannot be zero".to_string(),
            ));
        }
        if self.lock_timeout_seconds == 0 {
            return Err(ValidationError::InvalidTimeout(
                "lock_timeout_seconds cannot be zero".to_string(),
            ));
        }
        if self.lock_timeout_seconds < 60 {
            return Err(ValidationError::InvalidTimeout(
                "lock_timeout_seconds should be at least 60 seconds".to_string(),
            ));
        }
        if self.max_concurrent_per_team > self.max_concurrent_tasks {
            return Err(ValidationError::InconsistentRates(
                "max_concurrent_per_team exceeds max_concurrent_tasks".to_string(),
            ));
        }
        Ok(())
    }
}

impl Default for ConcurrencyConfig {
    fn default() -> Self {
        Self {
            strategy: ConcurrencyStrategy::DistributedSemaphore,
            max_concurrent_tasks: 100,
            max_concurrent_per_team: 10,
            lock_timeout_seconds: 300, // 5分钟
            enabled: true,
        }
    }
}

/// 限流与并发控制结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RateLimitResult {
    /// 允许通过
    Allowed,
    /// 被拒绝
    Denied { reason: String },
    /// 需要等待（包含等待时间）
    RetryAfter { retry_after_seconds: u64 },
}

/// 并发控制结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConcurrencyResult {
    /// 允许执行
    Allowed,
    /// 被拒绝（并发限制已达到）
    Denied { reason: String },
    /// 任务已加入积压队列
    Queued { backlog_id: Uuid },
}

// === 拆分后的接口 ===

/// 限流服务接口（核心限流功能）
#[async_trait]
pub trait RateLimitService: Send + Sync {
    /// 检查API限流
    async fn check_rate_limit(
        &self,
        api_key: &str,
        endpoint: &str,
    ) -> Result<RateLimitResult, RateLimitingError>;

    /// 获取团队的限流配置
    async fn get_team_rate_limit_config(
        &self,
        team_id: Uuid,
    ) -> Result<RateLimitConfig, RateLimitingError>;

    /// 更新团队的限流配置
    async fn update_team_rate_limit_config(
        &self,
        team_id: Uuid,
        config: RateLimitConfig,
    ) -> Result<(), RateLimitingError>;

    /// 清理过期的限流记录
    async fn cleanup_expired_rate_limits(&self) -> Result<u64, RateLimitingError>;
}

/// 并发控制服务接口
#[async_trait]
pub trait ConcurrencyControlService: Send + Sync {
    /// 检查团队并发限制
    async fn check_team_concurrency(
        &self,
        team_id: Uuid,
        task_id: Uuid,
    ) -> Result<ConcurrencyResult, RateLimitingError>;

    /// 释放团队并发槽位
    async fn release_team_concurrency_slot(
        &self,
        team_id: Uuid,
        task_id: Uuid,
    ) -> Result<(), RateLimitingError>;

    /// 获取团队的当前并发数
    async fn get_team_current_concurrency(&self, team_id: Uuid) -> Result<u32, RateLimitingError>;

    /// 获取团队的并发配置
    async fn get_team_concurrency_config(
        &self,
        team_id: Uuid,
    ) -> Result<ConcurrencyConfig, RateLimitingError>;

    /// 更新团队的并发配置
    async fn update_team_concurrency_config(
        &self,
        team_id: Uuid,
        config: ConcurrencyConfig,
    ) -> Result<(), RateLimitingError>;
}

/// 积压任务服务接口
#[async_trait]
pub trait BacklogService: Send + Sync {
    /// 处理积压任务
    async fn process_backlog_tasks(&self, team_id: Uuid) -> Result<u32, RateLimitingError>;
}

/// 配额/积分服务接口
#[async_trait]
pub trait QuotaService: Send + Sync {
    /// 检查并扣除团队配额（Credits）
    async fn check_and_deduct_quota(
        &self,
        team_id: Uuid,
        amount: i64,
        transaction_type: crate::domain::models::CreditsTransactionType,
        description: String,
        reference_id: Option<Uuid>,
    ) -> Result<(), RateLimitingError>;

    /// 获取团队配额余额
    async fn get_quota_balance(&self, team_id: Uuid) -> Result<i64, RateLimitingError>;
}

/// 组合接口：提供所有限流与并发控制功能（向后兼容）
#[async_trait]
pub trait RateLimitingService:
    RateLimitService + ConcurrencyControlService + BacklogService + QuotaService + Send + Sync
{
}

/// 限流与并发控制错误类型
#[derive(Debug, thiserror::Error)]
pub enum RateLimitingError {
    #[error("限流已达到上限: {0}")]
    RateLimitExceeded(String),

    #[error("并发限制已达到上限: {0}")]
    ConcurrencyLimitExceeded(String),

    #[error("配置错误: {0}")]
    ConfigurationError(String),

    #[error("数据库操作失败，请稍后重试")]
    DatabaseError,

    #[error("积分系统暂时不可用")]
    CreditsError,

    #[error("其他错误: {0}")]
    Other(#[from] anyhow::Error),
}

/// 配置验证错误类型
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("速率限制不能为零: {0}")]
    ZeroRate(&'static str),

    #[error("容量不能为零: {0}")]
    ZeroCapacity(String),

    #[error("超时时间无效: {0}")]
    InvalidTimeout(String),

    #[error("速率配置不一致: {0}")]
    InconsistentRates(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== RateLimitStrategy tests ==========

    #[test]
    fn test_rate_limit_strategy_serde_all_variants() {
        for strategy in [
            RateLimitStrategy::TokenBucket,
            RateLimitStrategy::LeakyBucket,
            RateLimitStrategy::FixedWindow,
            RateLimitStrategy::SlidingWindow,
        ] {
            let json = serde_json::to_string(&strategy).expect("serialize");
            let back: RateLimitStrategy = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(strategy, back, "roundtrip should preserve: {}", json);
        }
    }

    #[test]
    fn test_rate_limit_strategy_equality() {
        assert_eq!(
            RateLimitStrategy::TokenBucket,
            RateLimitStrategy::TokenBucket
        );
        assert_ne!(
            RateLimitStrategy::TokenBucket,
            RateLimitStrategy::LeakyBucket
        );
        assert_ne!(
            RateLimitStrategy::FixedWindow,
            RateLimitStrategy::SlidingWindow
        );
    }

    #[test]
    fn test_rate_limit_strategy_clone_copy() {
        let s1 = RateLimitStrategy::TokenBucket;
        let s2 = s1; // Copy
        assert_eq!(s1, s2);
        let s3 = s1;
        assert_eq!(s1, s3);
    }

    // ========== ConcurrencyStrategy tests ==========

    #[test]
    fn test_concurrency_strategy_serde_all_variants() {
        for strategy in [
            ConcurrencyStrategy::DistributedSemaphore,
            ConcurrencyStrategy::Semaphore,
            ConcurrencyStrategy::DatabaseLevel,
        ] {
            let json = serde_json::to_string(&strategy).expect("serialize");
            let back: ConcurrencyStrategy = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(strategy, back, "roundtrip should preserve: {}", json);
        }
    }

    #[test]
    fn test_concurrency_strategy_equality() {
        assert_eq!(
            ConcurrencyStrategy::DistributedSemaphore,
            ConcurrencyStrategy::DistributedSemaphore
        );
        assert_ne!(
            ConcurrencyStrategy::DistributedSemaphore,
            ConcurrencyStrategy::Semaphore
        );
        assert_ne!(
            ConcurrencyStrategy::Semaphore,
            ConcurrencyStrategy::DatabaseLevel
        );
    }

    // ========== RateLimitConfig::default tests ==========

    #[test]
    fn test_rate_limit_config_default_values() {
        let config = RateLimitConfig::default();
        assert_eq!(config.strategy, RateLimitStrategy::TokenBucket);
        assert_eq!(config.requests_per_second, 10);
        assert_eq!(config.requests_per_minute, 100);
        assert_eq!(config.requests_per_hour, 1000);
        assert_eq!(config.bucket_capacity, Some(100));
        assert!(config.enabled);
    }

    #[test]
    fn test_rate_limit_config_default_has_expected_values() {
        let config = RateLimitConfig::default();
        assert_eq!(config.strategy, RateLimitStrategy::TokenBucket);
        assert_eq!(config.requests_per_second, 10);
        assert_eq!(config.requests_per_minute, 100);
        assert_eq!(config.requests_per_hour, 1000);
        assert_eq!(config.bucket_capacity, Some(100));
        assert!(config.enabled);
    }

    // ========== RateLimitConfig::validate tests ==========

    #[test]
    fn test_rate_limit_config_validate_success() {
        let config = RateLimitConfig {
            strategy: RateLimitStrategy::TokenBucket,
            requests_per_second: 1,
            requests_per_minute: 100,
            requests_per_hour: 6000,
            bucket_capacity: Some(50),
            enabled: true,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_rate_limit_config_validate_success_without_bucket_capacity() {
        let config = RateLimitConfig {
            strategy: RateLimitStrategy::FixedWindow,
            requests_per_second: 1,
            requests_per_minute: 60,
            requests_per_hour: 3600,
            bucket_capacity: None,
            enabled: true,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_rate_limit_config_validate_zero_requests_per_second() {
        let config = RateLimitConfig {
            requests_per_second: 0,
            ..Default::default()
        };
        let err = config.validate().expect_err("should fail");
        assert!(matches!(err, ValidationError::ZeroRate(_)));
        assert!(err.to_string().contains("requests_per_second"));
    }

    #[test]
    fn test_rate_limit_config_validate_zero_requests_per_minute() {
        let config = RateLimitConfig {
            requests_per_minute: 0,
            ..Default::default()
        };
        let err = config.validate().expect_err("should fail");
        assert!(matches!(err, ValidationError::ZeroRate(_)));
        assert!(err.to_string().contains("requests_per_minute"));
    }

    #[test]
    fn test_rate_limit_config_validate_zero_requests_per_hour() {
        let config = RateLimitConfig {
            requests_per_hour: 0,
            ..Default::default()
        };
        let err = config.validate().expect_err("should fail");
        assert!(matches!(err, ValidationError::ZeroRate(_)));
        assert!(err.to_string().contains("requests_per_hour"));
    }

    #[test]
    fn test_rate_limit_config_validate_per_second_exceeds_per_minute() {
        // requests_per_second > requests_per_minute / 60
        let config = RateLimitConfig {
            strategy: RateLimitStrategy::TokenBucket,
            requests_per_second: 5,
            requests_per_minute: 100, // 100/60 = 1.66, 5 > 1.66
            requests_per_hour: 6000,
            bucket_capacity: Some(100),
            enabled: true,
        };
        let err = config.validate().expect_err("should fail");
        assert!(matches!(err, ValidationError::InconsistentRates(_)));
        assert!(err
            .to_string()
            .contains("requests_per_second exceeds requests_per_minute"));
    }

    #[test]
    fn test_rate_limit_config_validate_per_minute_exceeds_per_hour() {
        // requests_per_minute > requests_per_hour / 60
        let config = RateLimitConfig {
            strategy: RateLimitStrategy::TokenBucket,
            requests_per_second: 1,
            requests_per_minute: 100,
            requests_per_hour: 6000, // 6000/60 = 100, 100 > 100 is false, need to push minute higher
            bucket_capacity: Some(100),
            enabled: true,
        };
        // The above should pass; construct one that fails
        let config_fail = RateLimitConfig {
            strategy: RateLimitStrategy::TokenBucket,
            requests_per_second: 1,
            requests_per_minute: 200,
            requests_per_hour: 6000, // 6000/60 = 100, 200 > 100
            bucket_capacity: Some(100),
            enabled: true,
        };
        let err = config_fail.validate().expect_err("should fail");
        assert!(matches!(err, ValidationError::InconsistentRates(_)));
        assert!(err
            .to_string()
            .contains("requests_per_minute exceeds requests_per_hour"));
        // Sanity: the valid one passes
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_rate_limit_config_validate_zero_bucket_capacity() {
        // Use consistent rates so validation reaches the bucket_capacity check
        let mut config = RateLimitConfig {
            strategy: RateLimitStrategy::TokenBucket,
            requests_per_second: 1,
            requests_per_minute: 100,
            requests_per_hour: 6000,
            bucket_capacity: Some(50),
            enabled: true,
        };
        config.bucket_capacity = Some(0);
        let err = config.validate().expect_err("should fail");
        assert!(matches!(err, ValidationError::ZeroCapacity(_)));
        assert!(err.to_string().contains("bucket_capacity cannot be zero"));
    }

    #[test]
    fn test_rate_limit_config_serde_roundtrip() {
        let config = RateLimitConfig::default();
        let json = serde_json::to_string(&config).expect("serialize");
        let back: RateLimitConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.strategy, config.strategy);
        assert_eq!(back.requests_per_second, config.requests_per_second);
        assert_eq!(back.requests_per_minute, config.requests_per_minute);
        assert_eq!(back.requests_per_hour, config.requests_per_hour);
        assert_eq!(back.bucket_capacity, config.bucket_capacity);
        assert_eq!(back.enabled, config.enabled);
    }

    // ========== ConcurrencyConfig::default tests ==========

    #[test]
    fn test_concurrency_config_default_values() {
        let config = ConcurrencyConfig::default();
        assert_eq!(config.strategy, ConcurrencyStrategy::DistributedSemaphore);
        assert_eq!(config.max_concurrent_tasks, 100);
        assert_eq!(config.max_concurrent_per_team, 10);
        assert_eq!(config.lock_timeout_seconds, 300);
        assert!(config.enabled);
    }

    #[test]
    fn test_concurrency_config_default_passes_validation() {
        let config = ConcurrencyConfig::default();
        assert!(config.validate().is_ok(), "default config should be valid");
    }

    // ========== ConcurrencyConfig::validate tests ==========

    #[test]
    fn test_concurrency_config_validate_success() {
        let config = ConcurrencyConfig {
            strategy: ConcurrencyStrategy::Semaphore,
            max_concurrent_tasks: 50,
            max_concurrent_per_team: 10,
            lock_timeout_seconds: 120,
            enabled: true,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_concurrency_config_validate_zero_max_concurrent_tasks() {
        let config = ConcurrencyConfig {
            max_concurrent_tasks: 0,
            ..Default::default()
        };
        let err = config.validate().expect_err("should fail");
        assert!(matches!(err, ValidationError::ZeroCapacity(_)));
        assert!(err
            .to_string()
            .contains("max_concurrent_tasks cannot be zero"));
    }

    #[test]
    fn test_concurrency_config_validate_zero_max_concurrent_per_team() {
        let config = ConcurrencyConfig {
            max_concurrent_per_team: 0,
            ..Default::default()
        };
        let err = config.validate().expect_err("should fail");
        assert!(matches!(err, ValidationError::ZeroCapacity(_)));
        assert!(err
            .to_string()
            .contains("max_concurrent_per_team cannot be zero"));
    }

    #[test]
    fn test_concurrency_config_validate_zero_lock_timeout() {
        let config = ConcurrencyConfig {
            lock_timeout_seconds: 0,
            ..Default::default()
        };
        let err = config.validate().expect_err("should fail");
        assert!(matches!(err, ValidationError::InvalidTimeout(_)));
        assert!(err
            .to_string()
            .contains("lock_timeout_seconds cannot be zero"));
    }

    #[test]
    fn test_concurrency_config_validate_lock_timeout_below_minimum() {
        let config = ConcurrencyConfig {
            lock_timeout_seconds: 30,
            ..Default::default()
        };
        let err = config.validate().expect_err("should fail");
        assert!(matches!(err, ValidationError::InvalidTimeout(_)));
        assert!(err.to_string().contains("at least 60 seconds"));
    }

    #[test]
    fn test_concurrency_config_validate_lock_timeout_exactly_60_is_valid() {
        let config = ConcurrencyConfig {
            lock_timeout_seconds: 60,
            ..Default::default()
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_concurrency_config_validate_per_team_exceeds_total() {
        let config = ConcurrencyConfig {
            strategy: ConcurrencyStrategy::DistributedSemaphore,
            max_concurrent_tasks: 10,
            max_concurrent_per_team: 20, // > max_concurrent_tasks
            lock_timeout_seconds: 120,
            enabled: true,
        };
        let err = config.validate().expect_err("should fail");
        assert!(matches!(err, ValidationError::InconsistentRates(_)));
        assert!(err
            .to_string()
            .contains("max_concurrent_per_team exceeds max_concurrent_tasks"));
    }

    #[test]
    fn test_concurrency_config_validate_per_team_equals_total_is_valid() {
        let config = ConcurrencyConfig {
            strategy: ConcurrencyStrategy::DistributedSemaphore,
            max_concurrent_tasks: 10,
            max_concurrent_per_team: 10, // == max_concurrent_tasks is OK
            lock_timeout_seconds: 120,
            enabled: true,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_concurrency_config_serde_roundtrip() {
        let config = ConcurrencyConfig::default();
        let json = serde_json::to_string(&config).expect("serialize");
        let back: ConcurrencyConfig = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.strategy, config.strategy);
        assert_eq!(back.max_concurrent_tasks, config.max_concurrent_tasks);
        assert_eq!(back.max_concurrent_per_team, config.max_concurrent_per_team);
        assert_eq!(back.lock_timeout_seconds, config.lock_timeout_seconds);
        assert_eq!(back.enabled, config.enabled);
    }

    // ========== RateLimitResult tests ==========

    #[test]
    fn test_rate_limit_result_allowed_equality() {
        assert_eq!(RateLimitResult::Allowed, RateLimitResult::Allowed);
    }

    #[test]
    fn test_rate_limit_result_denied_carries_reason() {
        let r = RateLimitResult::Denied {
            reason: "too many requests".to_string(),
        };
        match r {
            RateLimitResult::Denied { reason } => {
                assert_eq!(reason, "too many requests");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_rate_limit_result_retry_after_carries_seconds() {
        let r = RateLimitResult::RetryAfter {
            retry_after_seconds: 30,
        };
        match r {
            RateLimitResult::RetryAfter {
                retry_after_seconds,
            } => {
                assert_eq!(retry_after_seconds, 30);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_rate_limit_result_variants_not_equal() {
        assert_ne!(
            RateLimitResult::Allowed,
            RateLimitResult::Denied {
                reason: "x".to_string(),
            }
        );
    }

    // ========== ConcurrencyResult tests ==========

    #[test]
    fn test_concurrency_result_allowed_equality() {
        assert_eq!(ConcurrencyResult::Allowed, ConcurrencyResult::Allowed);
    }

    #[test]
    fn test_concurrency_result_denied_carries_reason() {
        let r = ConcurrencyResult::Denied {
            reason: "limit reached".to_string(),
        };
        match r {
            ConcurrencyResult::Denied { reason } => {
                assert_eq!(reason, "limit reached");
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_concurrency_result_queued_carries_backlog_id() {
        let id = Uuid::new_v4();
        let r = ConcurrencyResult::Queued { backlog_id: id };
        match r {
            ConcurrencyResult::Queued { backlog_id } => {
                assert_eq!(backlog_id, id);
            }
            _ => panic!("wrong variant"),
        }
    }

    #[test]
    fn test_concurrency_result_variants_not_equal() {
        assert_ne!(
            ConcurrencyResult::Allowed,
            ConcurrencyResult::Queued {
                backlog_id: Uuid::new_v4(),
            }
        );
    }

    // ========== RateLimitingError Display tests ==========

    #[test]
    fn test_rate_limiting_error_rate_limit_exceeded_display() {
        let err = RateLimitingError::RateLimitExceeded("100/min".to_string());
        assert!(err.to_string().contains("限流已达到上限"));
        assert!(err.to_string().contains("100/min"));
    }

    #[test]
    fn test_rate_limiting_error_concurrency_limit_exceeded_display() {
        let err = RateLimitingError::ConcurrencyLimitExceeded("5 tasks".to_string());
        assert!(err.to_string().contains("并发限制已达到上限"));
        assert!(err.to_string().contains("5 tasks"));
    }

    #[test]
    fn test_rate_limiting_error_configuration_error_display() {
        let err = RateLimitingError::ConfigurationError("bad config".to_string());
        assert!(err.to_string().contains("配置错误"));
        assert!(err.to_string().contains("bad config"));
    }

    #[test]
    fn test_rate_limiting_error_database_error_display() {
        let err = RateLimitingError::DatabaseError;
        assert!(err.to_string().contains("数据库操作失败"));
    }

    #[test]
    fn test_rate_limiting_error_credits_error_display() {
        let err = RateLimitingError::CreditsError;
        assert!(err.to_string().contains("积分系统暂时不可用"));
    }

    #[test]
    fn test_rate_limiting_error_other_from_anyhow() {
        let err = RateLimitingError::Other(anyhow::anyhow!("something broke"));
        assert!(err.to_string().contains("其他错误"));
        assert!(err.to_string().contains("something broke"));
    }

    // ========== ValidationError Display tests ==========

    #[test]
    fn test_validation_error_zero_rate_display() {
        let err = ValidationError::ZeroRate("requests_per_second cannot be zero");
        assert!(err.to_string().contains("速率限制不能为零"));
        assert!(err
            .to_string()
            .contains("requests_per_second cannot be zero"));
    }

    #[test]
    fn test_validation_error_zero_capacity_display() {
        let err = ValidationError::ZeroCapacity("bucket_capacity cannot be zero".to_string());
        assert!(err.to_string().contains("容量不能为零"));
        assert!(err.to_string().contains("bucket_capacity cannot be zero"));
    }

    #[test]
    fn test_validation_error_invalid_timeout_display() {
        let err = ValidationError::InvalidTimeout("timeout too short".to_string());
        assert!(err.to_string().contains("超时时间无效"));
        assert!(err.to_string().contains("timeout too short"));
    }

    #[test]
    fn test_validation_error_inconsistent_rates_display() {
        let err = ValidationError::InconsistentRates("mismatch".to_string());
        assert!(err.to_string().contains("速率配置不一致"));
        assert!(err.to_string().contains("mismatch"));
    }

    // ========== RateLimitingError From<anyhow::Error> test ==========

    #[test]
    fn test_rate_limiting_error_from_anyhow_preserves_message() {
        let anyhow_err = anyhow::anyhow!("custom failure");
        let err: RateLimitingError = anyhow_err.into();
        match err {
            RateLimitingError::Other(inner) => {
                assert!(inner.to_string().contains("custom failure"));
            }
            other => panic!("expected Other variant, got {:?}", other),
        }
    }
}
