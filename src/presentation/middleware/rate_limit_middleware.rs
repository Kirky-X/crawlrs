// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::impl_basic_error_conversions;
#[cfg(feature = "rate-limiting")]
use crate::infrastructure::cache::redis_client::RedisClient;
use anyhow::Result;
use thiserror::Error;

/// 原子速率限制 Lua 脚本
///
/// 确保 INCR 和 EXPIRE 操作原子性执行，防止竞态条件
const ATOMIC_RATE_LIMIT_SCRIPT: &str = r#"
local key = KEYS[1]
local limit = tonumber(ARGV[1])
local ttl = tonumber(ARGV[2])

local current = redis.call('INCR', key)
if current == 1 then
    redis.call('EXPIRE', key, ttl)
end

if current > limit then
    return 0
end
return current
"#;

/// 速率限制错误类型
#[derive(Error, Debug)]
pub enum RateLimitError {
    /// 请求过多错误
    #[error("Too many requests")]
    TooManyRequests,

    /// 内部服务器错误
    #[error("Internal server error: {0}")]
    InternalError(String),
}

impl_basic_error_conversions!(RateLimitError, InternalError);

/// 速率限制器
pub struct RateLimiter {
    /// Redis客户端
    redis_client: RedisClient,

    /// 默认每分钟限制请求数
    // Default rate limit: 100 requests per minute
    default_limit_per_minute: u32,
}

impl RateLimiter {
    /// 创建新的速率限制器实例
    ///
    /// # 参数
    ///
    /// * `redis_client` - Redis客户端实例
    /// * `default_limit_per_minute` - 默认每分钟请求数限制
    ///
    /// # 返回值
    ///
    /// 返回新的速率限制器实例
    pub fn new(redis_client: RedisClient, default_limit_per_minute: u32) -> Self {
        Self {
            redis_client,
            default_limit_per_minute,
        }
    }

    /// Get a reference to the Redis client
    pub fn redis_client(&self) -> &RedisClient {
        &self.redis_client
    }

    /// Get a clone of the Redis client
    pub fn redis_client_clone(&self) -> RedisClient {
        self.redis_client.clone()
    }

    /// 检查API密钥的请求速率是否超出限制（原子操作）
    ///
    /// # 参数
    ///
    /// * `api_key` - API密钥
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 请求未超出限制
    /// * `Err(RateLimitError)` - 请求超出限制或发生错误
    pub async fn check(&self, api_key: &str) -> Result<(), RateLimitError> {
        let key = format!("rate_limit:{}", api_key);
        let limit = self.get_rate_limit(api_key).await?;
        let ttl = 60; // 60 seconds TTL

        // 使用 Lua 脚本实现原子 INCR + EXPIRE
        let result: i64 = self
            .redis_client
            .eval(
                ATOMIC_RATE_LIMIT_SCRIPT,
                &[&key],
                &[&limit.to_string(), &ttl.to_string()],
            )
            .await
            .map_err(|e| RateLimitError::InternalError(format!("Lua script failed: {}", e)))?
            .parse()
            .map_err(|e| RateLimitError::InternalError(format!("Failed to parse result: {}", e)))?;

        let current_requests = result as u32;
        let key_prefix = &api_key[..std::cmp::min(8, api_key.len())];

        if current_requests == 0 {
            tracing::warn!(
                "RateLimiter: API Key starting with {} exceeded limit. Current: {}, Limit: {}",
                key_prefix,
                current_requests,
                limit
            );
            return Err(RateLimitError::TooManyRequests);
        }

        tracing::debug!(
            "RateLimiter: API Key starting with {} - Current: {}, Limit: {}",
            key_prefix,
            current_requests,
            limit
        );

        Ok(())
    }

    /// 获取API密钥的速率限制配置
    ///
    /// # 参数
    ///
    /// * `api_key` - API密钥
    ///
    /// # 返回值
    ///
    /// * `Ok(u32)` - 速率限制值
    /// * `Err(RateLimitError)` - 获取配置失败
    async fn get_rate_limit(&self, api_key: &str) -> Result<u32, RateLimitError> {
        let key = format!("rate_limit_config:{}", api_key);
        tracing::debug!("[RateLimiter] Getting rate limit for key: {}", key);

        match self.redis_client.get(&key).await {
            Ok(Some(limit_str)) => {
                tracing::debug!("[RateLimiter] Found config string: {}", limit_str);

                // Try to parse as JSON first (new format: {"requests_per_minute": N, ...})
                if limit_str.starts_with('{') {
                    match serde_json::from_str::<serde_json::Value>(&limit_str) {
                        Ok(json) => {
                            tracing::debug!("[RateLimiter] Parsed JSON config: {:?}", json);
                            let limit = json
                                .get("requests_per_minute")
                                .and_then(|v| v.as_u64())
                                .map(|v| v as u32)
                                .unwrap_or(self.default_limit_per_minute);
                            tracing::debug!("[RateLimiter] Extracted limit from JSON: {}", limit);
                            Ok(limit)
                        }
                        Err(e) => {
                            tracing::warn!("[RateLimiter] Failed to parse JSON config: {}", e);
                            // Fall back to parsing as plain number
                            limit_str.parse::<u32>().map_err(|e| {
                                RateLimitError::InternalError(format!(
                                    "Failed to parse rate limit: {}",
                                    e
                                ))
                            })
                        }
                    }
                } else {
                    // Plain number format (legacy)
                    tracing::debug!("[RateLimiter] Using legacy plain number format");
                    limit_str.parse::<u32>().map_err(|e| {
                        RateLimitError::InternalError(format!("Failed to parse rate limit: {}", e))
                    })
                }
            }
            Ok(None) => {
                tracing::debug!(
                    "[RateLimiter] No config found, using default: {}",
                    self.default_limit_per_minute
                );
                Ok(self.default_limit_per_minute)
            }
            Err(e) => {
                tracing::error!("[RateLimiter] Redis GET failed: {}", e);
                Err(RateLimitError::InternalError(format!(
                    "Redis GET failed: {}",
                    e
                )))
            }
        }
    }

    /// 注册API密钥的速率限制配置
    ///
    /// # 参数
    ///
    /// * `api_key` - API密钥
    /// * `rpm` - 每分钟请求数限制
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 注册成功
    /// * `Err(RateLimitError)` - 注册失败
    pub async fn register_key(&self, api_key: String, rpm: u32) -> Result<(), RateLimitError> {
        let key = format!("rate_limit_config:{}", api_key);
        self.redis_client
            .set_forever(&key, &rpm.to_string())
            .await
            .map_err(|e| RateLimitError::InternalError(format!("Redis SET failed: {}", e)))?;
        Ok(())
    }
}
