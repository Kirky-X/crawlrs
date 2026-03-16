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
    /// 基于Redis的锁
    RedisLock,
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

    #[cfg(feature = "rate-limiting")]
    #[error("Redis连接错误: 服务暂时不可用")]
    RedisError,

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
