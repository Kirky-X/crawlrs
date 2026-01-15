// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use std::sync::Arc;

use async_trait::async_trait;
use chrono::Utc;
#[cfg(feature = "rate-limiting")]
use redis::AsyncCommands;
use sha2::{Digest, Sha256};
use tracing::debug;
use uuid::Uuid;

use crate::domain::repositories::{
    credits_repository::CreditsRepository, task_repository::TaskRepository,
    tasks_backlog_repository::TasksBacklogRepository,
};
use crate::domain::services::rate_limiting_service::{
    ConcurrencyConfig, ConcurrencyResult, RateLimitConfig, RateLimitResult, RateLimitingError,
    RateLimitingService,
};
#[cfg(feature = "rate-limiting")]
use crate::infrastructure::cache::redis_client::RedisClient;

/// 限流与并发控制服务实现
///
/// 该服务实现了基于Redis的分布式限流和并发控制机制
/// 支持令牌桶限流算法和分布式信号量并发控制
#[cfg(feature = "rate-limiting")]
pub struct RateLimitingServiceImpl {
    redis: Arc<RedisClient>,
    task_repository: Arc<dyn TaskRepository>,
    tasks_backlog_repository: Arc<dyn TasksBacklogRepository>,
    credits_repository: Arc<dyn CreditsRepository>,
    config: RateLimitingConfig,
}

/// 限流服务配置
#[derive(Debug, Clone)]
pub struct RateLimitingConfig {
    /// Redis键前缀
    pub redis_key_prefix: String,
    /// 限流配置
    pub rate_limit: RateLimitConfig,
    /// 并发控制配置
    pub concurrency: ConcurrencyConfig,
    /// 积压任务处理间隔（秒）
    pub backlog_process_interval_seconds: u64,
    /// 限流记录过期时间（秒）
    pub rate_limit_ttl_seconds: u64,
}

impl Default for RateLimitingConfig {
    fn default() -> Self {
        Self {
            redis_key_prefix: "crawlrs:ratelimit".to_string(),
            rate_limit: RateLimitConfig::default(),
            concurrency: ConcurrencyConfig::default(),
            backlog_process_interval_seconds: 30,
            rate_limit_ttl_seconds: 3600, // 1小时
        }
    }
}

#[cfg(feature = "rate-limiting")]
impl RateLimitingServiceImpl {
    pub fn new(
        redis: Arc<RedisClient>,
        task_repository: Arc<dyn TaskRepository>,
        tasks_backlog_repository: Arc<dyn TasksBacklogRepository>,
        credits_repository: Arc<dyn CreditsRepository>,
        config: RateLimitingConfig,
    ) -> Self {
        Self {
            redis,
            task_repository,
            tasks_backlog_repository,
            credits_repository,
            config,
        }
    }

    /// 获取Redis连接
    async fn get_redis_conn(&self) -> Result<redis::aio::MultiplexedConnection, RateLimitingError> {
        self.redis.get_connection().await.map_err(|e| {
            RateLimitingError::Other(anyhow::anyhow!("Redis connection failed: {}", e))
        })
    }

    /// 构建Redis键
    fn build_redis_key(&self, suffix: &str) -> String {
        format!("{}:{}", self.config.redis_key_prefix, suffix)
    }

    /// 对API Key进行哈希处理以用于Redis键
    /// 避免API Key明文出现在Redis中，防止日志泄露
    fn hash_api_key_for_redis(&self, api_key: &str) -> String {
        let mut hasher = Sha256::new();
        hasher.update(api_key);
        let result = hasher.finalize();
        format!("{:x}", result)
    }

    /// 构建API限流键
    fn build_api_rate_limit_key(&self, api_key: &str, endpoint: &str, window: &str) -> String {
        let hashed_key = self.hash_api_key_for_redis(api_key);
        self.build_redis_key(&format!("api:{}:{}:{}", hashed_key, endpoint, window))
    }

    /// 构建团队信号量键
    fn build_team_semaphore_key(&self, team_id: Uuid) -> String {
        self.build_redis_key(&format!("team:{}:semaphore", team_id))
    }

    /// 实现令牌桶限流算法
    async fn check_token_bucket_rate_limit(
        &self,
        key: String,
        capacity: u32,
        refill_rate: f64,
        window_seconds: u64,
    ) -> Result<RateLimitResult, RateLimitingError> {
        let mut conn = self.get_redis_conn().await?;

        let script = r#"
            local key = KEYS[1]
            local capacity = tonumber(ARGV[1])
            local refill_rate = tonumber(ARGV[2])
            local window = tonumber(ARGV[3])
            local now = tonumber(ARGV[4])
            
            local tokens_key = key .. ":tokens"
            local last_refill_key = key .. ":last_refill"
            
            -- 获取当前令牌数和上次填充时间
            local tokens = redis.call("GET", tokens_key) or capacity
            local last_refill = tonumber(redis.call("GET", last_refill_key)) or now
            
            tokens = tonumber(tokens)
            
            -- 计算需要填充的令牌数
            local time_passed = now - last_refill
            local tokens_to_add = time_passed * refill_rate
            tokens = math.min(capacity, tokens + tokens_to_add)
            
            -- 尝试消耗一个令牌
            if tokens >= 1 then
                tokens = tokens - 1
                
                -- 更新令牌数和填充时间
                redis.call("SET", tokens_key, tokens)
                redis.call("SET", last_refill_key, now)
                redis.call("EXPIRE", tokens_key, window)
                redis.call("EXPIRE", last_refill_key, window)
                
                return {1, 0}  -- 允许通过，无需等待
            else
                -- 计算需要等待的时间
                local wait_time = (1 - tokens) / refill_rate
                return {0, math.ceil(wait_time)}
            end
        "#;

        let now = chrono::Utc::now().timestamp();
        let result: Vec<i64> = redis::Script::new(script)
            .key(&key)
            .arg(capacity)
            .arg(refill_rate)
            .arg(window_seconds)
            .arg(now)
            .invoke_async(&mut conn)
            .await
            .map_err(RateLimitingError::RedisError)?;

        if result[0] == 1 {
            Ok(RateLimitResult::Allowed)
        } else {
            Ok(RateLimitResult::RetryAfter {
                retry_after_seconds: result[1] as u64,
            })
        }
    }

    /// 实现分布式信号量
    async fn acquire_semaphore(
        &self,
        key: String,
        max_concurrent: u32,
        timeout_seconds: u64,
    ) -> Result<bool, RateLimitingError> {
        let mut conn = self.get_redis_conn().await?;

        let script = r#"
            local key = KEYS[1]
            local max_concurrent = tonumber(ARGV[1])
            local timeout = tonumber(ARGV[2])
            local now = tonumber(ARGV[3])
            local token = ARGV[4]
            
            -- 清理过期的信号量
            redis.call("ZREMRANGEBYSCORE", key, 0, now - timeout)
            
            -- 获取当前并发数
            local current = redis.call("ZCARD", key)
            
            -- 检查是否可以获得信号量
            if current < max_concurrent then
                redis.call("ZADD", key, now, token)
                redis.call("EXPIRE", key, timeout * 2)
                return 1
            else
                return 0
            end
        "#;

        let now = chrono::Utc::now().timestamp();
        let token = Uuid::new_v4().to_string();

        let result: i64 = redis::Script::new(script)
            .key(&key)
            .arg(max_concurrent)
            .arg(timeout_seconds)
            .arg(now)
            .arg(&token)
            .invoke_async(&mut conn)
            .await
            .map_err(RateLimitingError::RedisError)?;

        Ok(result == 1)
    }

    /// 释放分布式信号量
    async fn release_semaphore(&self, key: String, token: String) -> Result<(), RateLimitingError> {
        let mut conn = self.get_redis_conn().await?;

        conn.zrem::<_, _, ()>(&key, token)
            .await
            .map_err(RateLimitingError::RedisError)?;

        Ok(())
    }

    /// 获取当前并发数
    async fn get_current_concurrency(&self, key: String) -> Result<u32, RateLimitingError> {
        let mut conn = self.get_redis_conn().await?;

        let script = r#"
            local key = KEYS[1]
            local timeout = tonumber(ARGV[1])
            local now = tonumber(ARGV[2])
            
            -- 清理过期的信号量
            redis.call("ZREMRANGEBYSCORE", key, 0, now - timeout)
            
            -- 返回当前并发数
            return redis.call("ZCARD", key)
        "#;

        let now = chrono::Utc::now().timestamp();
        let current: i64 = redis::Script::new(script)
            .key(&key)
            .arg(self.config.concurrency.lock_timeout_seconds)
            .arg(now)
            .invoke_async(&mut conn)
            .await
            .map_err(RateLimitingError::RedisError)?;

        Ok(current as u32)
    }

    /// 获取自定义速率限制配置
    async fn get_custom_rate_limit_config(&self, api_key: &str) -> Option<RateLimitConfig> {
        let config_key = format!("{}:config:{}", self.config.redis_key_prefix, api_key);
        let fallback_key = format!("rate_limit_config:{}", api_key);

        tracing::debug!(
            "Looking for rate limit config with keys: primary={}, fallback={}",
            config_key,
            fallback_key
        );

        let mut conn = match self.get_redis_conn().await {
            Ok(conn) => conn,
            Err(e) => {
                tracing::warn!("Failed to get Redis connection: {}", e);
                return None;
            }
        };

        let config_str = match conn.get::<&str, Option<String>>(&config_key).await {
            Ok(Some(s)) => {
                tracing::debug!("Found config with primary key: {}", s);
                Some(s)
            }
            Ok(None) => {
                tracing::debug!("No config found with primary key");
                match conn.get::<&str, Option<String>>(&fallback_key).await {
                    Ok(Some(s)) => {
                        tracing::debug!("Found config with fallback key: {}", s);
                        Some(s)
                    }
                    _ => {
                        tracing::debug!("No config found with fallback key either");
                        None
                    }
                }
            }
            Err(e) => {
                tracing::warn!("Redis error getting config: {}", e);
                None
            }
        };

        let config_value: serde_json::Value = match config_str {
            Some(s) => match serde_json::from_str(&s) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!("Failed to parse config JSON: {}", e);
                    return None;
                }
            },
            None => return None,
        };

        let requests_per_minute = config_value
            .get("requests_per_minute")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let bucket_capacity = config_value
            .get("capacity")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let requests_per_hour = config_value
            .get("requests_per_hour")
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        Some(RateLimitConfig {
            enabled: true,
            strategy: self.config.rate_limit.strategy,
            bucket_capacity,
            requests_per_second: self.config.rate_limit.requests_per_second,
            requests_per_minute: requests_per_minute
                .unwrap_or(self.config.rate_limit.requests_per_minute),
            requests_per_hour: requests_per_hour
                .unwrap_or(self.config.rate_limit.requests_per_hour),
        })
    }
}

#[async_trait]
#[cfg(feature = "rate-limiting")]
impl RateLimitingService for RateLimitingServiceImpl {
    async fn check_rate_limit(
        &self,
        api_key: &str,
        endpoint: &str,
    ) -> Result<RateLimitResult, RateLimitingError> {
        debug!("========== RATE LIMIT CHECK ==========");
        // 使用SHA256哈希代替部分掩码，更安全地记录API key
        let mut hasher = Sha256::new();
        hasher.update(api_key.as_bytes());
        let _api_key_hash = format!("{:x}", hasher.finalize());
        debug!(endpoint);
        debug!(enabled = self.config.rate_limit.enabled);

        if !self.config.rate_limit.enabled {
            debug!("Rate limiting is DISABLED globally, allowing request");
            return Ok(RateLimitResult::Allowed);
        }

        // Try to get custom rate limit config from Redis
        debug!("Attempting to get custom rate limit config from Redis...");
        let custom_config = self.get_custom_rate_limit_config(api_key).await;

        debug!(?custom_config);

        let (bucket_capacity, requests_per_minute, requests_per_hour) = match custom_config {
            Some(config) => {
                debug!("Using custom config");
                (
                    config
                        .bucket_capacity
                        .unwrap_or(self.config.rate_limit.bucket_capacity.unwrap_or(100)),
                    config.requests_per_minute,
                    config.requests_per_hour,
                )
            }
            None => {
                debug!("No custom config found, using defaults");
                (
                    self.config.rate_limit.bucket_capacity.unwrap_or(100),
                    self.config.rate_limit.requests_per_minute,
                    self.config.rate_limit.requests_per_hour,
                )
            }
        };

        debug!(
            bucket_capacity,
            rpm = requests_per_minute,
            rph = requests_per_hour
        );

        // 检查每秒限流
        debug!("Checking per-second rate limit...");
        let per_second_key = self.build_api_rate_limit_key(api_key, endpoint, "per_second");
        let per_second_result = self
            .check_token_bucket_rate_limit(
                per_second_key,
                bucket_capacity,
                self.config.rate_limit.requests_per_second as f64,
                1,
            )
            .await?;

        debug!(?per_second_result);

        if !matches!(per_second_result, RateLimitResult::Allowed) {
            debug!("Per-second rate limit exceeded");
            return Ok(per_second_result);
        }

        // 检查每分钟限流
        debug!("Checking per-minute rate limit...");
        let per_minute_key = self.build_api_rate_limit_key(api_key, endpoint, "per_minute");
        let per_minute_result = self
            .check_token_bucket_rate_limit(
                per_minute_key,
                requests_per_minute,
                requests_per_minute as f64 / 60.0,
                60,
            )
            .await?;

        debug!(?per_minute_result);

        if !matches!(per_minute_result, RateLimitResult::Allowed) {
            debug!("Per-minute rate limit exceeded");
            return Ok(per_minute_result);
        }

        // 检查每小时限流
        debug!("Checking per-hour rate limit...");
        let per_hour_key = self.build_api_rate_limit_key(api_key, endpoint, "per_hour");
        let per_hour_result = self
            .check_token_bucket_rate_limit(
                per_hour_key,
                requests_per_hour,
                requests_per_hour as f64 / 3600.0,
                3600,
            )
            .await?;

        debug!(?per_hour_result);
        debug!("========== RATE LIMIT CHECK COMPLETE ==========");

        Ok(per_hour_result)
    }

    async fn check_team_concurrency(
        &self,
        team_id: Uuid,
        task_id: Uuid,
    ) -> Result<ConcurrencyResult, RateLimitingError> {
        if !self.config.concurrency.enabled {
            return Ok(ConcurrencyResult::Allowed);
        }

        let semaphore_key = self.build_team_semaphore_key(team_id);
        let _token = format!("{}:{}", team_id, task_id);

        // 尝试获取信号量
        let acquired = self
            .acquire_semaphore(
                semaphore_key.clone(),
                self.config.concurrency.max_concurrent_per_team,
                self.config.concurrency.lock_timeout_seconds,
            )
            .await?;

        if acquired {
            Ok(ConcurrencyResult::Allowed)
        } else {
            // 并发限制已达到，将任务加入积压队列
            let task = self
                .task_repository
                .find_by_id(task_id)
                .await
                .map_err(RateLimitingError::DatabaseError)?;

            if let Some(task) = task {
                let backlog =
                    crate::domain::repositories::tasks_backlog_repository::TasksBacklog::new(
                        task_id,
                        team_id,
                        task.task_type.to_string(),
                        task.priority,
                        task.payload,
                        task.expires_at.map(|dt| dt.with_timezone(&Utc)),
                    );

                let saved_backlog = self
                    .tasks_backlog_repository
                    .create(&backlog)
                    .await
                    .map_err(RateLimitingError::DatabaseError)?;

                Ok(ConcurrencyResult::Queued {
                    backlog_id: saved_backlog.id,
                })
            } else {
                Err(RateLimitingError::DatabaseError(
                    crate::domain::repositories::task_repository::RepositoryError::NotFound,
                ))
            }
        }
    }

    async fn release_team_concurrency_slot(
        &self,
        team_id: Uuid,
        task_id: Uuid,
    ) -> Result<(), RateLimitingError> {
        let semaphore_key = self.build_team_semaphore_key(team_id);
        let token = format!("{}:{}", team_id, task_id);

        self.release_semaphore(semaphore_key, token).await?;

        // 处理积压任务
        self.process_backlog_tasks(team_id).await?;

        Ok(())
    }

    async fn get_team_current_concurrency(&self, team_id: Uuid) -> Result<u32, RateLimitingError> {
        let semaphore_key = self.build_team_semaphore_key(team_id);
        self.get_current_concurrency(semaphore_key).await
    }

    async fn get_team_rate_limit_config(
        &self,
        _team_id: Uuid,
    ) -> Result<RateLimitConfig, RateLimitingError> {
        Ok(self.config.rate_limit.clone())
    }

    async fn get_team_concurrency_config(
        &self,
        _team_id: Uuid,
    ) -> Result<ConcurrencyConfig, RateLimitingError> {
        Ok(self.config.concurrency.clone())
    }

    async fn update_team_rate_limit_config(
        &self,
        _team_id: Uuid,
        _config: RateLimitConfig,
    ) -> Result<(), RateLimitingError> {
        // 这里可以实现团队特定的限流配置更新逻辑
        // 目前返回默认配置
        Ok(())
    }

    async fn update_team_concurrency_config(
        &self,
        _team_id: Uuid,
        _config: ConcurrencyConfig,
    ) -> Result<(), RateLimitingError> {
        // 这里可以实现团队特定的并发配置更新逻辑
        // 目前返回默认配置
        Ok(())
    }

    async fn cleanup_expired_rate_limits(&self) -> Result<u64, RateLimitingError> {
        // 清理过期的限流记录
        let mut conn = self.get_redis_conn().await?;

        let pattern = format!("{}:*", self.config.redis_key_prefix);
        let keys: Vec<String> = conn
            .keys(&pattern)
            .await
            .map_err(RateLimitingError::RedisError)?;

        let mut cleaned_count = 0;
        for key in keys {
            let ttl: i64 = conn
                .ttl(&key)
                .await
                .map_err(RateLimitingError::RedisError)?;

            if ttl == -2 {
                // 键已过期
                let _: i64 = conn
                    .del(&key)
                    .await
                    .map_err(RateLimitingError::RedisError)?;
                cleaned_count += 1;
            }
        }

        Ok(cleaned_count)
    }

    async fn process_backlog_tasks(&self, team_id: Uuid) -> Result<u32, RateLimitingError> {
        // 获取待处理的积压任务
        let pending_backlogs = self
            .tasks_backlog_repository
            .get_pending_tasks(Some(team_id), Some(10))
            .await
            .map_err(RateLimitingError::DatabaseError)?;

        let mut processed_count = 0;

        for backlog in pending_backlogs {
            // 检查任务是否已过期
            if backlog.is_expired() {
                let mut expired_backlog = backlog.clone();
                expired_backlog
                    .mark_expired()
                    .map_err(|e| RateLimitingError::Other(anyhow::anyhow!(e)))?;

                self.tasks_backlog_repository
                    .update(&expired_backlog)
                    .await
                    .map_err(RateLimitingError::DatabaseError)?;

                continue;
            }

            // 尝试获取并发槽位
            let semaphore_key = self.build_team_semaphore_key(team_id);
            let _token = format!("{}:{}", team_id, backlog.task_id);

            let acquired = self
                .acquire_semaphore(
                    semaphore_key.clone(),
                    self.config.concurrency.max_concurrent_per_team,
                    self.config.concurrency.lock_timeout_seconds,
                )
                .await?;

            if acquired {
                // 成功获取槽位，创建任务并标记积压任务为已完成
                let task = self
                    .task_repository
                    .find_by_id(backlog.task_id)
                    .await
                    .map_err(RateLimitingError::DatabaseError)?;

                if let Some(mut task) = task {
                    if task.status == crate::domain::models::task::TaskStatus::Queued {
                        task.status = crate::domain::models::task::TaskStatus::Active;
                        task.started_at = Some(chrono::Utc::now().into());
                        // 清除 lock_token 并设置 lock_expires_at，以便新的 Worker 可以获取此任务
                        task.lock_token = None;
                        task.lock_expires_at =
                            Some((chrono::Utc::now() + chrono::Duration::seconds(300)).into());
                        self.task_repository
                            .update(&task)
                            .await
                            .map_err(RateLimitingError::DatabaseError)?;
                    }
                }

                // 更新积压任务状态
                let mut updated_backlog = backlog.clone();
                updated_backlog
                    .mark_completed()
                    .map_err(|e| RateLimitingError::Other(anyhow::anyhow!(e)))?;

                self.tasks_backlog_repository
                    .update(&updated_backlog)
                    .await
                    .map_err(RateLimitingError::DatabaseError)?;

                processed_count += 1;
            } else {
                // 无法获取槽位，保持积压状态
                break;
            }
        }

        Ok(processed_count)
    }

    async fn check_and_deduct_quota(
        &self,
        team_id: Uuid,
        amount: i64,
        transaction_type: crate::domain::models::credits::CreditsTransactionType,
        description: String,
        reference_id: Option<Uuid>,
    ) -> Result<(), RateLimitingError> {
        // 在扣除积分之前先检查余额，确保原子性操作前的预检查
        let balance = self
            .credits_repository
            .get_balance(team_id)
            .await
            .map_err(RateLimitingError::CreditsError)?;

        if balance < amount {
            return Err(RateLimitingError::RateLimitExceeded(format!(
                "Insufficient credits: required {}, available {}",
                amount, balance
            )));
        }

        self.credits_repository
            .deduct_credits(team_id, amount, transaction_type, description, reference_id)
            .await
            .map_err(RateLimitingError::CreditsError)?;

        Ok(())
    }

    async fn get_quota_balance(&self, team_id: Uuid) -> Result<i64, RateLimitingError> {
        self.credits_repository
            .get_balance(team_id)
            .await
            .map_err(RateLimitingError::CreditsError)
    }
}
