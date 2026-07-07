// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::engines::client::reqwest::ReqwestEngine;
use crate::engines::engine_client::{EngineClient, HttpMethod, ScrapeOptions, ScrapeRequest};
use crate::engines::router::{EngineRouter, EngineRouterTrait};
use crate::impl_basic_error_conversions;
#[cfg(feature = "redis-cache")]
use crate::infrastructure::cache::redis_client::RedisClient;
use crate::utils::retry_policy::RetryPolicy;
use anyhow::Result;
use robotstxt::DefaultMatcher;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::sync::Mutex;
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

impl_basic_error_conversions!(RobotsCheckerError, ValidationError);

impl From<crate::presentation::helpers::ssrf::SsrfError> for RobotsCheckerError {
    fn from(err: crate::presentation::helpers::ssrf::SsrfError) -> Self {
        RobotsCheckerError::ValidationError(err.to_string())
    }
}

/// Robots.txt缓存统计
#[derive(Default, Clone)]
pub struct CacheStats {
    pub hits: Arc<AtomicU64>,
    pub misses: Arc<AtomicU64>,
}

impl CacheStats {
    /// 获取缓存命中次数
    pub fn hits(&self) -> u64 {
        self.hits.load(Ordering::Relaxed)
    }

    /// 获取缓存未命中次数
    pub fn misses(&self) -> u64 {
        self.misses.load(Ordering::Relaxed)
    }

    /// 记录缓存命中
    pub fn record_hit(&self) {
        self.hits.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录缓存未命中
    pub fn record_miss(&self) {
        self.misses.fetch_add(1, Ordering::Relaxed);
    }
}

use shaku::Interface;

/// Robots.txt检查器接口
#[async_trait]
pub trait RobotsCheckerTrait: Interface + Send + Sync {
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

/// Robots.txt检查器
#[derive(Clone)]
pub struct RobotsChecker {
    /// HTTP客户端 (Arc 包装，支持依赖注入)
    engine_client: Arc<EngineClient>,

    /// 内存缓存
    memory_cache: Arc<Mutex<HashMap<String, CachedRobots>>>,

    /// Redis客户端
    #[cfg(feature = "redis-cache")]
    redis_client: Option<Arc<RedisClient>>,

    /// 重试策略
    retry_policy: RetryPolicy,

    /// 缓存统计
    cache_stats: Arc<CacheStats>,
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

#[cfg(feature = "redis-cache")]
impl RobotsChecker {
    /// 创建新的Robots检查器实例（通过依赖注入）
    ///
    /// # Arguments
    ///
    /// * `http_client` - HTTP 客户端（通过依赖注入）
    /// * `redis_client` - Redis 客户端（可选，用于缓存）
    /// * `cache_stats` - 缓存统计（可选，用于追踪缓存命中率）
    ///
    /// # Returns
    ///
    /// 返回新的Robots检查器实例
    pub fn new(
        http_client: Arc<reqwest::Client>,
        redis_client: Option<Arc<RedisClient>>,
        cache_stats: Option<Arc<CacheStats>>,
    ) -> Self {
        let engine_client = Self::create_engine_client(http_client);
        Self {
            engine_client,
            memory_cache: Arc::new(Mutex::new(HashMap::with_capacity(256))),
            redis_client,
            retry_policy: RetryPolicy {
                max_retries: 5,
                initial_backoff: Duration::from_secs(2),
                max_backoff: Duration::from_secs(10),
                ..Default::default()
            },
            cache_stats: cache_stats.unwrap_or_else(|| Arc::new(CacheStats::default())),
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
            let mut cache = self.memory_cache.lock().await;
            if let Some(cached) = cache.get(&robots_url) {
                if cached.expires_at > Instant::now() {
                    self.cache_stats.record_hit();
                    return Ok(cached.content.clone());
                } else {
                    cache.remove(&robots_url);
                }
            }
        }

        self.cache_stats.record_miss();

        // 2. Check Redis cache
        let redis_key = format!("robots_cache:{}", robots_url);
        if let Some(ref redis) = self.redis_client {
            if let Ok(Some(content)) = redis.get(&redis_key).await {
                // Update memory cache
                let mut cache = self.memory_cache.lock().await;
                cache.insert(
                    robots_url.clone(),
                    CachedRobots {
                        content: content.clone(),
                        expires_at: Instant::now() + Duration::from_secs(3600),
                    },
                );
                self.cache_stats.record_hit();
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
            let mut headers = HashMap::new();
            headers.insert("User-Agent".to_string(), "crawlrs-bot/1.0".to_string());

            let request = ScrapeRequest::new(&robots_url).with_options(
                ScrapeOptions::builder()
                    .method(HttpMethod::Get)
                    .headers(headers)
                    .timeout(Duration::from_secs(5))
                    .build(),
            );

            let response = self.engine_client.scrape(&request).await;

            match response {
                Ok(resp) => {
                    if resp.is_success() {
                        content = resp.content;
                        last_error = None;
                        break;
                    } else if resp.status_code == 404 {
                        content = "".to_string();
                        last_error = None;
                        break;
                    } else if resp.status_code >= 500 {
                        last_error = Some(anyhow::anyhow!("Server error: {}", resp.status_code));
                    } else {
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
            log::warn!("Failed to fetch robots.txt from {}: {}", robots_url, err);
            // Default to empty content on persistent error
            content = "".to_string();
        }

        // 4. Update memory cache
        {
            let mut cache = self.memory_cache.lock().await;
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
    pub fn get_cache_stats(&self) -> (u64, u64) {
        (self.cache_stats.hits(), self.cache_stats.misses())
    }
}

impl RobotsChecker {
    fn create_engine_client(http_client: Arc<reqwest::Client>) -> Arc<EngineClient> {
        let reqwest_engine = ReqwestEngine::new(http_client);
        let router: Arc<dyn EngineRouterTrait> =
            Arc::new(EngineRouter::new(vec![Arc::new(reqwest_engine)]));
        Arc::new(EngineClient::with_router(router))
    }
}
