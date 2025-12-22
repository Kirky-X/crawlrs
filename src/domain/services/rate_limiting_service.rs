// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
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

/// 限流结果
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

/// 限流与并发控制服务接口
#[async_trait]
pub trait RateLimitingService: Send + Sync {
    /// 检查API限流
    async fn check_rate_limit(
        &self,
        api_key: &str,
        endpoint: &str,
    ) -> Result<RateLimitResult, RateLimitingError>;

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

    /// 获取团队的限流配置
    async fn get_team_rate_limit_config(
        &self,
        team_id: Uuid,
    ) -> Result<RateLimitConfig, RateLimitingError>;

    /// 获取团队的并发配置
    async fn get_team_concurrency_config(
        &self,
        team_id: Uuid,
    ) -> Result<ConcurrencyConfig, RateLimitingError>;

    /// 更新团队的限流配置
    async fn update_team_rate_limit_config(
        &self,
        team_id: Uuid,
        config: RateLimitConfig,
    ) -> Result<(), RateLimitingError>;

    /// 更新团队的并发配置
    async fn update_team_concurrency_config(
        &self,
        team_id: Uuid,
        config: ConcurrencyConfig,
    ) -> Result<(), RateLimitingError>;

    /// 清理过期的限流记录
    async fn cleanup_expired_rate_limits(&self) -> Result<u64, RateLimitingError>;

    /// 处理积压任务
    async fn process_backlog_tasks(&self, team_id: Uuid) -> Result<u32, RateLimitingError>;

    /// 检查并扣除团队配额（Credits）
    async fn check_and_deduct_quota(
        &self,
        team_id: Uuid,
        amount: i64,
        transaction_type: crate::domain::models::credits::CreditsTransactionType,
        description: String,
        reference_id: Option<Uuid>,
    ) -> Result<(), RateLimitingError>;

    /// 获取团队配额余额
    async fn get_quota_balance(&self, team_id: Uuid) -> Result<i64, RateLimitingError>;
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

    #[error("Redis连接错误: {0}")]
    RedisError(#[from] redis::RedisError),

    #[error("数据库错误: {0}")]
    DatabaseError(#[from] crate::domain::repositories::task_repository::RepositoryError),

    #[error("积分系统错误: {0}")]
    CreditsError(#[from] crate::domain::repositories::credits_repository::CreditsRepositoryError),

    #[error("其他错误: {0}")]
    Other(#[from] anyhow::Error),
}
