// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::infrastructure::cache::redis_client::RedisClient;
use anyhow::Result;
use thiserror::Error;
use tracing::error;

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

    /// 检查API密钥的请求速率是否超出限制
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
        let current_requests = self
            .redis_client
            .incr(&key)
            .await
            .map_err(|e| RateLimitError::InternalError(format!("Redis INCR failed: {}", e)))?;

        // Set expiry for the key if it's a new counter (i.e., current_requests == 1)
        // This ensures the key expires after one minute, resetting the rate limit.
        if current_requests == 1 {
            self.redis_client.expire(&key, 60).await.map_err(|e| {
                RateLimitError::InternalError(format!("Redis EXPIRE failed: {}", e))
            })?;
        }

        let limit = self.get_rate_limit(api_key).await?;

        if current_requests > limit.into() {
            return Err(RateLimitError::TooManyRequests);
        }

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
        match self.redis_client.get(&key).await {
            Ok(Some(limit_str)) => limit_str.parse::<u32>().map_err(|e| {
                RateLimitError::InternalError(format!("Failed to parse rate limit: {}", e))
            }),
            Ok(None) => Ok(self.default_limit_per_minute),
            Err(e) => Err(RateLimitError::InternalError(format!(
                "Redis GET failed: {}",
                e
            ))),
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
