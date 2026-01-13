// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use anyhow::Result;
use reqwest::Client;
use robotstxt::DefaultMatcher;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use thiserror::Error;
use url::Url;

use async_trait::async_trait;

/// Robots.txt 检查器错误
#[derive(Error, Debug)]
pub enum RobotsCheckerError {
    #[error("缓存锁获取失败: {0}")]
    CacheLockError(String),

    #[error("URL解析失败: {0}")]
    UrlParseError(String),

    #[error("验证失败: {0}")]
    ValidationError(String),
}

impl From<anyhow::Error> for RobotsCheckerError {
    fn from(err: anyhow::Error) -> Self {
        RobotsCheckerError::ValidationError(err.to_string())
    }
}

/// Robots.txt缓存统计
struct CacheStats {
    pub hits: AtomicU64,
    pub misses: AtomicU64,
}

impl Default for CacheStats {
    fn default() -> Self {
        Self {
            hits: AtomicU64::new(0),
            misses: AtomicU64::new(0),
        }
    }
}

use once_cell::sync::Lazy;

static CACHE_STATS: Lazy<CacheStats> = Lazy::new(CacheStats::default);

/// Robots.txt检查器接口
#[async_trait]
pub trait RobotsCheckerTrait: Send + Sync {
    /// 检查URL是否被允许访问
    async fn is_allowed(&self, url_str: &str, user_agent: &str) -> Result<bool>;
    /// 获取爬取延迟
    async fn get_crawl_delay(&self, url_str: &str, user_agent: &str) -> Result<Option<Duration>>;
}

/// 缓存的Robots.txt内容
#[derive(Clone)]
struct CachedRobots {
    /// 内容
    content: String,

    /// 过期时间
    expires_at: Instant,
}

use crate::infrastructure::cache::redis_client::RedisClient;
use crate::utils::retry_policy::RetryPolicy;

/// Robots.txt检查器
#[derive(Clone)]
pub struct RobotsChecker {
    /// HTTP客户端
    client: Client,

    /// 内存缓存
    memory_cache: Arc<Mutex<HashMap<String, CachedRobots>>>,

    /// Redis客户端
    redis_client: Option<Arc<RedisClient>>,

    /// 重试策略
    retry_policy: RetryPolicy,
}

#[async_trait]
impl RobotsCheckerTrait for RobotsChecker {
    async fn is_allowed(&self, url_str: &str, user_agent: &str) -> Result<bool> {
        let content = self.get_robots_content(url_str).await?;
        let url = Url::parse(url_str)?;
        let mut matcher = DefaultMatcher::default();
        Ok(matcher.one_agent_allowed_by_robots(user_agent, url.path(), &content))
    }

    async fn get_crawl_delay(&self, url_str: &str, user_agent: &str) -> Result<Option<Duration>> {
        let content = self.get_robots_content(url_str).await?;
        Ok(self.parse_crawl_delay(&content, user_agent))
    }
}

impl Default for RobotsChecker {
    fn default() -> Self {
        Self::new(None)
    }
}

impl RobotsChecker {
    /// 创建新的Robots检查器实例
    ///
    /// # 返回值
    ///
    /// 返回新的Robots检查器实例
    pub fn new(redis_client: Option<Arc<RedisClient>>) -> Self {
        Self {
            client: Client::new(),
            memory_cache: Arc::new(Mutex::new(HashMap::with_capacity(256))),
            redis_client,
            retry_policy: RetryPolicy {
                max_retries: 5,
                initial_backoff: Duration::from_secs(2),
                max_backoff: Duration::from_secs(10),
                ..Default::default()
            },
        }
    }

    /// 获取Robots.txt内容（带缓存）
    async fn get_robots_content(&self, url_str: &str) -> Result<String, RobotsCheckerError> {
        let url =
            Url::parse(url_str).map_err(|e| RobotsCheckerError::UrlParseError(e.to_string()))?;
        let host = url
            .host_str()
            .ok_or_else(|| RobotsCheckerError::UrlParseError("Invalid URL: no host".to_string()))?;
        let scheme = url.scheme();
        let port = url.port_or_known_default().unwrap_or(80);

        let robots_url = format!("{}://{}:{}/robots.txt", scheme, host, port);

        // 1. Check memory cache
        {
            let mut cache = self
                .memory_cache
                .lock()
                .map_err(|e| RobotsCheckerError::CacheLockError(e.to_string()))?;
            if let Some(cached) = cache.get(&robots_url) {
                if cached.expires_at > Instant::now() {
                    CACHE_STATS.hits.fetch_add(1, Ordering::Relaxed);
                    return Ok(cached.content.clone());
                } else {
                    cache.remove(&robots_url);
                }
            }
        }

        CACHE_STATS.misses.fetch_add(1, Ordering::Relaxed);

        // 2. Check Redis cache
        let redis_key = format!("robots_cache:{}", robots_url);
        if let Some(ref redis) = self.redis_client {
            if let Ok(Some(content)) = redis.get(&redis_key).await {
                // Update memory cache
                let mut cache = self
                    .memory_cache
                    .lock()
                    .map_err(|e| RobotsCheckerError::CacheLockError(e.to_string()))?;
                cache.insert(
                    robots_url.clone(),
                    CachedRobots {
                        content: content.clone(),
                        expires_at: Instant::now() + Duration::from_secs(3600),
                    },
                );
                CACHE_STATS.hits.fetch_add(1, Ordering::Relaxed);
                return Ok(content);
            }
        }

        // SSRF protection
        crate::engines::validators::validate_url(&robots_url).await?;

        // 3. Fetch robots.txt with retry
        let mut attempt = 0;
        let mut content = String::new();
        let mut last_error = None;

        while attempt < self.retry_policy.max_retries {
            attempt += 1;
            let response = self
                .client
                .get(&robots_url)
                .header("User-Agent", "crawlrs-bot/1.0")
                .timeout(Duration::from_secs(5))
                .send()
                .await;

            match response {
                Ok(resp) => {
                    if resp.status().is_success() {
                        content = resp.text().await.unwrap_or_default();
                        last_error = None;
                        break;
                    } else if resp.status() == reqwest::StatusCode::NOT_FOUND {
                        // 404 is a valid response, meaning no robots.txt
                        content = "".to_string();
                        last_error = None;
                        break;
                    } else if resp.status().is_server_error() {
                        last_error = Some(anyhow::anyhow!("Server error: {}", resp.status()));
                    } else {
                        // Other errors (403, etc.) might be permanent, but we'll treat them as "allow all" for safety or stop
                        content = "".to_string();
                        last_error = None;
                        break;
                    }
                }
                Err(e) => {
                    last_error = Some(anyhow::anyhow!("Request failed: {}", e));
                }
            }

            if attempt < self.retry_policy.max_retries {
                let backoff = self.retry_policy.calculate_backoff(attempt);
                tokio::time::sleep(backoff).await;
            }
        }

        if let Some(err) = last_error {
            tracing::warn!("Failed to fetch robots.txt from {}: {}", robots_url, err);
            // Default to empty content on persistent error
            content = "".to_string();
        }

        // 4. Update memory cache
        {
            let mut cache = self
                .memory_cache
                .lock()
                .map_err(|e| RobotsCheckerError::CacheLockError(e.to_string()))?;
            cache.insert(
                robots_url.clone(),
                CachedRobots {
                    content: content.clone(),
                    expires_at: Instant::now() + Duration::from_secs(3600), // Cache for 1 hour
                },
            );
        }

        // 5. Update Redis cache
        if let Some(ref redis) = self.redis_client {
            let _ = redis.set(&redis_key, &content, 86400).await; // Redis cache for 24 hours
        }

        Ok(content)
    }

    /// 解析Crawl-delay指令
    fn parse_crawl_delay(&self, content: &str, user_agent: &str) -> Option<Duration> {
        // 简单的解析逻辑：查找适用于该 User-Agent 的 Crawl-delay
        // 注意：这是一个简化的实现，不完全符合 RFC 规范，但足以处理大多数情况
        // 逻辑：
        // 1. 找到匹配的 User-agent 块
        // 2. 在块内查找 Crawl-delay

        let mut current_agent_matched = false;
        let mut delay: Option<f64> = None;
        let mut specific_agent_found = false;

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }

            let lower_line = line.to_lowercase();
            if lower_line.starts_with("user-agent:") {
                let agent = line[11..].trim();
                if agent == "*" {
                    current_agent_matched = !specific_agent_found;
                } else if user_agent.to_lowercase().contains(&agent.to_lowercase()) {
                    current_agent_matched = true;
                    specific_agent_found = true;
                    // Reset delay if we found a more specific agent
                    delay = None;
                } else {
                    current_agent_matched = false;
                }
            } else if lower_line.starts_with("crawl-delay:") && current_agent_matched {
                if let Ok(d) = line[12..].trim().parse::<f64>() {
                    delay = Some(d);
                }
            }
        }

        delay.map(Duration::from_secs_f64)
    }

    /// 旧的公开方法，为了兼容性保留
    pub async fn is_allowed(&self, url_str: &str, user_agent: &str) -> Result<bool> {
        RobotsCheckerTrait::is_allowed(self, url_str, user_agent).await
    }

    /// 获取缓存统计信息
    pub fn get_cache_stats() -> (u64, u64) {
        (
            CACHE_STATS.hits.load(Ordering::Relaxed),
            CACHE_STATS.misses.load(Ordering::Relaxed),
        )
    }
}
