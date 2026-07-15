// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::engines::client::reqwest::ReqwestEngine;
use crate::engines::engine_client::{EngineClient, HttpMethod, ScrapeOptions, ScrapeRequest};
use crate::engines::router::{EngineRouter, EngineRouterTrait};
use crate::impl_basic_error_conversions;
use crate::infrastructure::oxcache::CacheService;
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

/// Robots.txt检查器
#[derive(Clone)]
pub struct RobotsChecker {
    /// HTTP客户端 (Arc 包装，支持依赖注入)
    engine_client: Arc<EngineClient>,

    /// 内存缓存
    memory_cache: Arc<Mutex<HashMap<String, CachedRobots>>>,

    /// 缓存服务（可选，用于持久化缓存）
    cache_service: Option<Arc<dyn CacheService>>,

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
        Ok(matcher.one_agent_allowed_by_robots(&content, user_agent, url.path()))
    }

    async fn get_crawl_delay(&self, url_str: &str, user_agent: &str) -> Result<Option<Duration>> {
        let content = self.get_robots_content(url_str).await?;
        Ok(self.parse_crawl_delay(&content, user_agent))
    }
}

impl RobotsChecker {
    /// 创建新的Robots检查器实例（通过依赖注入）
    ///
    /// # Arguments
    ///
    /// * `http_client` - HTTP 客户端（通过依赖注入）
    /// * `cache_service` - 缓存服务（可选，用于持久化缓存）
    /// * `cache_stats` - 缓存统计（可选，用于追踪缓存命中率）
    ///
    /// # Returns
    ///
    /// 返回新的Robots检查器实例
    pub fn new(
        http_client: Arc<reqwest::Client>,
        cache_service: Option<Arc<dyn CacheService>>,
        cache_stats: Option<Arc<CacheStats>>,
    ) -> Self {
        let engine_client = Self::create_engine_client(http_client);
        Self {
            engine_client,
            memory_cache: Arc::new(Mutex::new(HashMap::with_capacity(256))),
            cache_service,
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

        // 2. Check cache service
        let cache_key = format!("robots_cache:{}", robots_url);
        if let Some(ref cache_service) = self.cache_service {
            if let Ok(Some(content)) = cache_service.get(&cache_key).await {
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

        // 5. Update cache service
        if let Some(ref cache_service) = self.cache_service {
            let _ = cache_service.set(&cache_key, &content, 86400).await; // Cache for 24 hours
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

#[cfg(test)]
mod tests {
    // Copyright (c) 2025 Kirky.X
    use super::*;
    use crate::presentation::helpers::ssrf::SsrfError;

    // ========== CacheStats tests ==========

    #[test]
    fn test_cache_stats_default_is_zero() {
        let stats = CacheStats::default();
        assert_eq!(stats.hits(), 0, "default hits should be 0");
        assert_eq!(stats.misses(), 0, "default misses should be 0");
    }

    #[test]
    fn test_cache_stats_record_hit_increments() {
        let stats = CacheStats::default();
        stats.record_hit();
        assert_eq!(stats.hits(), 1);
        assert_eq!(stats.misses(), 0);
    }

    #[test]
    fn test_cache_stats_record_miss_increments() {
        let stats = CacheStats::default();
        stats.record_miss();
        assert_eq!(stats.hits(), 0);
        assert_eq!(stats.misses(), 1);
    }

    #[test]
    fn test_cache_stats_multiple_hits_and_misses() {
        let stats = CacheStats::default();
        for _ in 0..5 {
            stats.record_hit();
        }
        for _ in 0..3 {
            stats.record_miss();
        }
        assert_eq!(stats.hits(), 5);
        assert_eq!(stats.misses(), 3);
    }

    #[test]
    fn test_cache_stats_shared_via_arc() {
        // CacheStats uses Arc<AtomicU64>, so cloning the stats shares the same counters.
        let stats = CacheStats::default();
        let clone = stats.clone();

        stats.record_hit();
        clone.record_miss();

        // Both references should see the same counters.
        assert_eq!(stats.hits(), 1);
        assert_eq!(stats.misses(), 1);
        assert_eq!(clone.hits(), 1);
        assert_eq!(clone.misses(), 1);
    }

    #[test]
    fn test_cache_stats_clone_is_shallow() {
        let stats = CacheStats::default();
        let clone = stats.clone();

        // Record on the original; the clone should reflect the change.
        stats.record_hit();
        stats.record_hit();
        assert_eq!(
            clone.hits(),
            2,
            "clone should share the same atomic counter"
        );
    }

    #[test]
    fn test_cache_stats_independent_instances() {
        let stats_a = CacheStats::default();
        let stats_b = CacheStats::default();

        stats_a.record_hit();
        stats_b.record_miss();

        assert_eq!(stats_a.hits(), 1, "stats_a should have 1 hit");
        assert_eq!(stats_a.misses(), 0, "stats_a should have 0 misses");
        assert_eq!(stats_b.hits(), 0, "stats_b should have 0 hits");
        assert_eq!(stats_b.misses(), 1, "stats_b should have 1 miss");
    }

    // ========== RobotsCheckerError Display tests ==========

    #[test]
    fn test_robots_checker_error_cache_lock_display() {
        let err = RobotsCheckerError::CacheLockError("lock poisoned".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("缓存锁获取失败"));
        assert!(msg.contains("lock poisoned"));
    }

    #[test]
    fn test_robots_checker_error_url_parse_display() {
        let err = RobotsCheckerError::UrlParseError("bad url".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("URL解析失败"));
        assert!(msg.contains("bad url"));
    }

    #[test]
    fn test_robots_checker_error_validation_display() {
        let err = RobotsCheckerError::ValidationError("invalid input".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("验证失败"));
        assert!(msg.contains("invalid input"));
    }

    // ========== From<SsrfError> tests ==========

    #[test]
    fn test_from_ssrf_error_converts_to_validation_error() {
        let ssrf_err = SsrfError::InvalidScheme {
            scheme: "ftp".to_string(),
        };
        let robots_err: RobotsCheckerError = ssrf_err.into();
        match robots_err {
            RobotsCheckerError::ValidationError(msg) => {
                assert!(
                    msg.contains("ftp"),
                    "error message should contain the scheme, got: {}",
                    msg
                );
            }
            other => panic!("expected ValidationError variant, got {:?}", other),
        }
    }

    #[test]
    fn test_from_ssrf_error_blocked_hostname() {
        let ssrf_err = SsrfError::BlockedHostname {
            hostname: "localhost".to_string(),
        };
        let robots_err: RobotsCheckerError = ssrf_err.into();
        match robots_err {
            RobotsCheckerError::ValidationError(msg) => {
                assert!(msg.contains("localhost"));
            }
            other => panic!("expected ValidationError variant, got {:?}", other),
        }
    }

    // ========== impl_basic_error_conversions tests ==========

    #[test]
    fn test_from_string_creates_validation_error() {
        let err: RobotsCheckerError = "something went wrong".to_string().into();
        match err {
            RobotsCheckerError::ValidationError(msg) => {
                assert_eq!(msg, "something went wrong");
            }
            other => panic!("expected ValidationError variant, got {:?}", other),
        }
    }

    #[test]
    fn test_from_str_creates_validation_error() {
        let err: RobotsCheckerError = "bad input".into();
        match err {
            RobotsCheckerError::ValidationError(msg) => {
                assert_eq!(msg, "bad input");
            }
            other => panic!("expected ValidationError variant, got {:?}", other),
        }
    }

    #[test]
    fn test_from_anyhow_error_creates_validation_error() {
        let anyhow_err = anyhow::anyhow!("anyhow failure");
        let err: RobotsCheckerError = anyhow_err.into();
        match err {
            RobotsCheckerError::ValidationError(msg) => {
                assert!(msg.contains("anyhow failure"));
            }
            other => panic!("expected ValidationError variant, got {:?}", other),
        }
    }

    // ========== RobotsChecker construction & parse_crawl_delay tests ==========

    fn make_checker() -> RobotsChecker {
        let http_client = Arc::new(reqwest::Client::new());
        RobotsChecker::new(http_client, None, None)
    }

    #[test]
    fn test_checker_new_creates_instance_with_defaults() {
        let checker = make_checker();
        let (hits, misses) = checker.get_cache_stats();
        assert_eq!(hits, 0, "new checker should have 0 cache hits");
        assert_eq!(misses, 0, "new checker should have 0 cache misses");
    }

    #[test]
    fn test_checker_new_with_custom_cache_stats() {
        let http_client = Arc::new(reqwest::Client::new());
        let stats = Arc::new(CacheStats::default());
        stats.record_hit();
        stats.record_miss();

        let checker = RobotsChecker::new(http_client, None, Some(stats));
        let (hits, misses) = checker.get_cache_stats();
        assert_eq!(hits, 1, "should use the provided cache stats");
        assert_eq!(misses, 1);
    }

    // ----- parse_crawl_delay tests -----

    #[test]
    fn test_parse_crawl_delay_wildcard_agent() {
        let checker = make_checker();
        let content = "User-agent: *\nCrawl-delay: 5\n";
        let delay = checker.parse_crawl_delay(content, "MyBot/1.0");
        assert_eq!(
            delay,
            Some(Duration::from_secs(5)),
            "wildcard agent should match any user agent"
        );
    }

    #[test]
    fn test_parse_crawl_delay_specific_agent_match() {
        let checker = make_checker();
        let content = "User-agent: crawlrs-bot\nCrawl-delay: 10\n";
        let delay = checker.parse_crawl_delay(content, "crawlrs-bot/1.0");
        assert_eq!(
            delay,
            Some(Duration::from_secs(10)),
            "specific agent should match when user_agent contains it"
        );
    }

    #[test]
    fn test_parse_crawl_delay_specific_agent_no_match() {
        let checker = make_checker();
        let content = "User-agent: otherbot\nCrawl-delay: 10\n";
        let delay = checker.parse_crawl_delay(content, "crawlrs-bot/1.0");
        assert_eq!(
            delay, None,
            "should return None when the user agent does not match"
        );
    }

    #[test]
    fn test_parse_crawl_delay_no_directive() {
        let checker = make_checker();
        let content = "User-agent: *\nDisallow: /private\n";
        let delay = checker.parse_crawl_delay(content, "MyBot/1.0");
        assert_eq!(
            delay, None,
            "should return None when no Crawl-delay is present"
        );
    }

    #[test]
    fn test_parse_crawl_delay_empty_content() {
        let checker = make_checker();
        let delay = checker.parse_crawl_delay("", "MyBot/1.0");
        assert_eq!(delay, None, "empty content should return None");
    }

    #[test]
    fn test_parse_crawl_delay_specific_overrides_wildcard() {
        // When a specific agent block appears after the wildcard block,
        // the specific agent's delay should take precedence.
        let checker = make_checker();
        let content = "\
User-agent: *
Crawl-delay: 1
User-agent: crawlrs-bot
Crawl-delay: 30
";
        let delay = checker.parse_crawl_delay(content, "crawlrs-bot/1.0");
        assert_eq!(
            delay,
            Some(Duration::from_secs(30)),
            "specific agent delay should override wildcard delay"
        );
    }

    #[test]
    fn test_parse_crawl_delay_wildcard_when_no_specific_match() {
        // When a specific agent block exists but doesn't match,
        // the wildcard block should still apply.
        let checker = make_checker();
        let content = "\
User-agent: *
Crawl-delay: 2
User-agent: otherbot
Crawl-delay: 30
";
        let delay = checker.parse_crawl_delay(content, "crawlrs-bot/1.0");
        assert_eq!(
            delay,
            Some(Duration::from_secs(2)),
            "wildcard delay should apply when no specific agent matches"
        );
    }

    #[test]
    fn test_parse_crawl_delay_invalid_value_ignored() {
        let checker = make_checker();
        let content = "User-agent: *\nCrawl-delay: not-a-number\n";
        let delay = checker.parse_crawl_delay(content, "MyBot/1.0");
        assert_eq!(
            delay, None,
            "invalid Crawl-delay value should be ignored (return None)"
        );
    }

    #[test]
    fn test_parse_crawl_delay_skips_comments_and_empty_lines() {
        let checker = make_checker();
        let content = "\
# This is a comment
User-agent: *

# Another comment
Crawl-delay: 7
";
        let delay = checker.parse_crawl_delay(content, "MyBot/1.0");
        assert_eq!(
            delay,
            Some(Duration::from_secs(7)),
            "comments and empty lines should be skipped"
        );
    }

    #[test]
    fn test_parse_crawl_delay_case_insensitive_user_agent() {
        // The matching logic uses to_lowercase() for comparison.
        let checker = make_checker();
        let content = "User-agent: CrawlRS-Bot\nCrawl-delay: 3\n";
        let delay = checker.parse_crawl_delay(content, "crawlrs-bot/1.0");
        assert_eq!(
            delay,
            Some(Duration::from_secs(3)),
            "user agent matching should be case-insensitive"
        );
    }

    #[test]
    fn test_parse_crawl_delay_case_insensitive_directive() {
        // The directive parsing uses to_lowercase() for the directive name.
        let checker = make_checker();
        let content = "user-agent: *\ncrawl-delay: 4\n";
        let delay = checker.parse_crawl_delay(content, "MyBot/1.0");
        assert_eq!(
            delay,
            Some(Duration::from_secs(4)),
            "directive names should be case-insensitive"
        );
    }

    #[test]
    fn test_parse_crawl_delay_fractional_seconds() {
        let checker = make_checker();
        let content = "User-agent: *\nCrawl-delay: 0.5\n";
        let delay = checker.parse_crawl_delay(content, "MyBot/1.0");
        assert_eq!(
            delay,
            Some(Duration::from_secs_f64(0.5)),
            "fractional crawl-delay should be parsed correctly"
        );
    }

    #[test]
    fn test_parse_crawl_delay_multiple_blocks_same_agent() {
        // When the same agent appears in multiple blocks, the last matching
        // Crawl-delay in the current block should be used.
        let checker = make_checker();
        let content = "\
User-agent: *
Crawl-delay: 1
User-agent: *
Crawl-delay: 8
";
        let delay = checker.parse_crawl_delay(content, "MyBot/1.0");
        // The second block resets delay to None then sets it to 8.
        assert_eq!(
            delay,
            Some(Duration::from_secs(8)),
            "last matching Crawl-delay should be used"
        );
    }

    #[test]
    fn test_parse_crawl_delay_only_directive_without_agent() {
        // Crawl-delay without a preceding User-agent directive should be ignored
        // because current_agent_matched starts as false.
        let checker = make_checker();
        let content = "Crawl-delay: 5\n";
        let delay = checker.parse_crawl_delay(content, "MyBot/1.0");
        assert_eq!(
            delay, None,
            "Crawl-delay without a matching User-agent block should be ignored"
        );
    }

    // ========== Cache pre-population tests for is_allowed & get_crawl_delay ==========
    // These tests pre-populate the memory cache to exercise the cache-hit path
    // and the robots.txt matching logic without making real HTTP requests.

    async fn populate_robots_cache(checker: &RobotsChecker, robots_url: &str, content: &str) {
        let mut cache = checker.memory_cache.lock().await;
        cache.insert(
            robots_url.to_string(),
            CachedRobots {
                content: content.to_string(),
                expires_at: Instant::now() + Duration::from_secs(3600),
            },
        );
    }

    #[tokio::test]
    async fn test_is_allowed_with_cached_robots_allows_non_disallowed_path() {
        let checker = make_checker();
        let content = "User-agent: *\nDisallow: /private\n";
        populate_robots_cache(&checker, "https://example.com:443/robots.txt", content).await;

        let allowed = checker
            .is_allowed("https://example.com/page", "MyBot/1.0")
            .await
            .expect("should succeed with cached content");
        assert!(allowed, "non-disallowed path should be allowed");
    }

    #[tokio::test]
    async fn test_is_allowed_with_cached_robots_blocks_disallowed_path() {
        let checker = make_checker();
        let content = "User-agent: *\nDisallow: /private\n";
        populate_robots_cache(&checker, "https://example.com:443/robots.txt", content).await;

        let allowed = checker
            .is_allowed("https://example.com/private", "MyBot/1.0")
            .await
            .expect("should succeed with cached content");
        assert!(!allowed, "disallowed path should be blocked");
    }

    #[tokio::test]
    async fn test_is_allowed_with_cached_robots_blocks_disallowed_subpath() {
        let checker = make_checker();
        let content = "User-agent: *\nDisallow: /private\n";
        populate_robots_cache(&checker, "https://example.com:443/robots.txt", content).await;

        let allowed = checker
            .is_allowed("https://example.com/private/secret", "MyBot/1.0")
            .await
            .expect("should succeed with cached content");
        assert!(
            !allowed,
            "subpath of disallowed path should also be blocked"
        );
    }

    #[tokio::test]
    async fn test_is_allowed_with_cached_empty_robots_allows_all() {
        let checker = make_checker();
        populate_robots_cache(&checker, "https://example.com:443/robots.txt", "").await;

        let allowed = checker
            .is_allowed("https://example.com/anything", "MyBot/1.0")
            .await
            .expect("should succeed with empty robots.txt");
        assert!(allowed, "empty robots.txt should allow all paths");
    }

    #[tokio::test]
    async fn test_is_allowed_with_cached_robots_specific_agent_block() {
        let checker = make_checker();
        // Use a specific user agent (without version suffix) that matches
        // the robots.txt User-agent directive. The robotstxt crate's
        // extract_user_agent extracts [a-zA-Z_-] characters from the
        // robots.txt directive, then compares with the caller's user_agent
        // using eq_ignore_ascii_case. So the caller must pass the product
        // name only (e.g. "BadBot", not "BadBot/1.0").
        let content = "User-agent: BadBot\nDisallow: /\n";
        populate_robots_cache(&checker, "https://example.com:443/robots.txt", content).await;

        // BadBot should be blocked everywhere by Disallow: /
        let bad_allowed = checker
            .is_allowed("https://example.com/page", "BadBot")
            .await
            .expect("should succeed");
        assert!(!bad_allowed, "BadBot should be blocked on all paths");

        // With only a BadBot-specific group and no * group, other bots
        // should be allowed (no applicable rules).
        let other_content = "User-agent: BadBot\nDisallow: /\nUser-agent: *\nAllow: /\n";
        populate_robots_cache(
            &checker,
            "https://example.com:443/robots.txt",
            other_content,
        )
        .await;
        let good_allowed = checker
            .is_allowed("https://example.com/page", "GoodBot")
            .await
            .expect("should succeed");
        assert!(good_allowed, "GoodBot should be allowed via * group");
    }

    #[tokio::test]
    async fn test_get_crawl_delay_with_cached_robots() {
        let checker = make_checker();
        let content = "User-agent: *\nCrawl-delay: 5\n";
        populate_robots_cache(&checker, "https://example.com:443/robots.txt", content).await;

        let delay = checker
            .get_crawl_delay("https://example.com/page", "MyBot/1.0")
            .await
            .expect("should succeed with cached content");
        assert_eq!(
            delay,
            Some(Duration::from_secs(5)),
            "should return the cached crawl delay"
        );
    }

    #[tokio::test]
    async fn test_get_crawl_delay_with_cached_no_delay_directive() {
        let checker = make_checker();
        let content = "User-agent: *\nDisallow: /private\n";
        populate_robots_cache(&checker, "https://example.com:443/robots.txt", content).await;

        let delay = checker
            .get_crawl_delay("https://example.com/page", "MyBot/1.0")
            .await
            .expect("should succeed");
        assert_eq!(
            delay, None,
            "should return None when no Crawl-delay directive"
        );
    }

    // ========== Cache hit statistics tests ==========

    #[tokio::test]
    async fn test_is_allowed_cache_hit_increments_stats() {
        let http_client = Arc::new(reqwest::Client::new());
        let stats = Arc::new(CacheStats::default());
        let checker = RobotsChecker::new(http_client, None, Some(stats.clone()));

        populate_robots_cache(
            &checker,
            "https://example.com:443/robots.txt",
            "User-agent: *\nDisallow: /private\n",
        )
        .await;

        let (hits_before, misses_before) = checker.get_cache_stats();
        assert_eq!(hits_before, 0);
        assert_eq!(misses_before, 0);

        checker
            .is_allowed("https://example.com/page", "MyBot")
            .await
            .expect("should succeed");

        let (hits_after, misses_after) = checker.get_cache_stats();
        assert_eq!(hits_after, 1, "cache hit should increment hits");
        assert_eq!(misses_after, 0, "no misses should occur for cache hit");
    }

    #[tokio::test]
    async fn test_get_crawl_delay_cache_hit_increments_stats() {
        let http_client = Arc::new(reqwest::Client::new());
        let stats = Arc::new(CacheStats::default());
        let checker = RobotsChecker::new(http_client, None, Some(stats.clone()));

        populate_robots_cache(
            &checker,
            "https://example.com:443/robots.txt",
            "User-agent: *\nCrawl-delay: 3\n",
        )
        .await;

        checker
            .get_crawl_delay("https://example.com/page", "MyBot")
            .await
            .expect("should succeed");

        let (hits, misses) = checker.get_cache_stats();
        assert_eq!(hits, 1, "cache hit should be recorded");
        assert_eq!(misses, 0);
    }

    // ========== Error path tests ==========

    #[tokio::test]
    async fn test_is_allowed_invalid_url_returns_error() {
        let checker = make_checker();
        let result = checker.is_allowed("not-a-valid-url", "MyBot").await;
        assert!(result.is_err(), "invalid URL should return an error");
    }

    #[tokio::test]
    async fn test_is_allowed_url_without_host_returns_error() {
        let checker = make_checker();
        let result = checker.is_allowed("file:///etc/passwd", "MyBot").await;
        assert!(result.is_err(), "URL without host should return an error");
    }

    #[tokio::test]
    async fn test_get_crawl_delay_invalid_url_returns_error() {
        let checker = make_checker();
        let result = checker.get_crawl_delay("not-a-valid-url", "MyBot").await;
        assert!(result.is_err(), "invalid URL should return an error");
    }

    #[tokio::test]
    async fn test_get_crawl_delay_url_without_host_returns_error() {
        let checker = make_checker();
        let result = checker.get_crawl_delay("file:///etc/passwd", "MyBot").await;
        assert!(result.is_err(), "URL without host should return an error");
    }

    // ========== create_engine_client test ==========

    #[test]
    fn test_create_engine_client_creates_valid_client() {
        let http_client = Arc::new(reqwest::Client::new());
        let engine_client = RobotsChecker::create_engine_client(http_client);
        assert_eq!(
            engine_client.engine_count(),
            1,
            "exactly one engine (reqwest) should be registered"
        );
        let names = engine_client.registered_engines();
        assert!(
            names.iter().any(|n| n == "reqwest"),
            "registered engines should contain 'reqwest', got {:?}",
            names
        );
    }

    // ========== Mock infrastructure for HTTP fetch & cache_service path tests ==========
    //
    // The following mocks allow testing get_robots_content() paths that
    // normally require real HTTP requests or a real cache service backend.
    // A public IP (8.8.8.8) is used as the target host so SSRF validation
    // passes without requiring DNS resolution (IP literals are parsed
    // directly by lookup_host).

    use crate::engines::engine_client::{
        EngineError, InternalScrapeRequest, InternalScrapeResponse,
    };
    use crate::engines::router::EngineStats;
    use std::future::Future;
    use std::pin::Pin;

    /// In-memory mock CacheService for testing the cache_service hit/miss paths.
    struct MockCacheService {
        data: std::sync::Mutex<HashMap<String, String>>,
        set_count: AtomicU64,
    }

    impl MockCacheService {
        fn new() -> Self {
            Self {
                data: std::sync::Mutex::new(HashMap::new()),
                set_count: AtomicU64::new(0),
            }
        }

        fn with_entry(key: &str, value: &str) -> Self {
            let s = Self::new();
            s.data
                .lock()
                .unwrap()
                .insert(key.to_string(), value.to_string());
            s
        }

        fn set_count(&self) -> u64 {
            self.set_count.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl CacheService for MockCacheService {
        fn get(
            &self,
            key: &str,
        ) -> Pin<Box<dyn Future<Output = Result<Option<String>>> + Send + '_>> {
            let data = self.data.lock().unwrap().get(key).cloned();
            Box::pin(async move { Ok(data) })
        }

        fn set(
            &self,
            _key: &str,
            _value: &str,
            _ttl_seconds: u64,
        ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            self.set_count.fetch_add(1, Ordering::Relaxed);
            Box::pin(async move { Ok(()) })
        }

        fn delete(&self, key: &str) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
            self.data.lock().unwrap().remove(key);
            Box::pin(async move { Ok(()) })
        }

        fn exists(&self, key: &str) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + '_>> {
            let exists = self.data.lock().unwrap().contains_key(key);
            Box::pin(async move { Ok(exists) })
        }
    }

    /// Mock EngineRouterTrait that returns a canned response or error,
    /// allowing get_robots_content() to be tested without real HTTP requests.
    struct MockEngineRouter {
        response: Option<InternalScrapeResponse>,
        error: Option<String>,
        call_count: AtomicU64,
    }

    impl MockEngineRouter {
        fn with_response(status_code: u16, content: &str) -> Self {
            Self {
                response: Some(InternalScrapeResponse {
                    status_code,
                    content: content.to_string(),
                    screenshot: None,
                    content_type: "text/plain".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 0,
                }),
                error: None,
                call_count: AtomicU64::new(0),
            }
        }

        fn with_error(msg: &str) -> Self {
            Self {
                response: None,
                error: Some(msg.to_string()),
                call_count: AtomicU64::new(0),
            }
        }

        fn call_count(&self) -> u64 {
            self.call_count.load(Ordering::Relaxed)
        }
    }

    #[async_trait]
    impl EngineRouterTrait for MockEngineRouter {
        async fn route(
            &self,
            _request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            self.call_count.fetch_add(1, Ordering::Relaxed);
            if let Some(resp) = &self.response {
                Ok(resp.clone())
            } else {
                Err(EngineError::RequestFailed(
                    self.error.clone().unwrap_or_default(),
                ))
            }
        }

        async fn aggregate(
            &self,
            request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            self.route(request).await
        }

        fn get_engine_stats(&self) -> HashMap<String, EngineStats> {
            HashMap::new()
        }

        fn reset_engine_stats(&self, _engine_name: &str) {}

        fn registered_engines(&self) -> Vec<String> {
            vec!["mock".to_string()]
        }
    }

    /// Build a RobotsChecker backed by a mock engine router, with short
    /// retry delays so tests run fast.
    fn make_checker_with_mock_router(router: Arc<dyn EngineRouterTrait>) -> RobotsChecker {
        let engine_client = Arc::new(EngineClient::with_router(router));
        RobotsChecker {
            engine_client,
            memory_cache: Arc::new(Mutex::new(HashMap::with_capacity(256))),
            cache_service: None,
            retry_policy: RetryPolicy {
                max_retries: 3,
                initial_backoff: Duration::from_millis(1),
                max_backoff: Duration::from_millis(10),
                ..Default::default()
            },
            cache_stats: Arc::new(CacheStats::default()),
        }
    }

    // ========== Cache service path tests (lines 184, 187-200, 179) ==========

    #[tokio::test]
    async fn test_cache_service_hit_returns_content_without_http() {
        // When memory cache misses but cache_service has the content,
        // the content should be returned without making an HTTP request.
        let robots_url = "http://8.8.8.8:80/robots.txt";
        let cache_key = format!("robots_cache:{}", robots_url);
        let mock_cache = Arc::new(MockCacheService::with_entry(
            &cache_key,
            "User-agent: *\nDisallow: /private\n",
        ));
        let checker = RobotsChecker {
            engine_client: Arc::new(EngineClient::new()),
            memory_cache: Arc::new(Mutex::new(HashMap::new())),
            cache_service: Some(mock_cache as Arc<dyn CacheService>),
            retry_policy: RetryPolicy::default(),
            cache_stats: Arc::new(CacheStats::default()),
        };

        let allowed = checker
            .is_allowed("http://8.8.8.8/page", "MyBot")
            .await
            .expect("should succeed via cache_service");
        assert!(allowed, "non-disallowed path should be allowed");

        // cache_service hit should be recorded
        let (hits, misses) = checker.get_cache_stats();
        assert_eq!(hits, 1, "cache_service hit should increment hits");
        assert_eq!(misses, 1, "memory cache miss should increment misses");

        // Memory cache should be updated from cache_service content
        let mem_cache = checker.memory_cache.lock().await;
        let entry = mem_cache
            .get(robots_url)
            .expect("memory cache should be populated from cache_service");
        assert!(entry.content.contains("Disallow: /private"));
        assert!(entry.expires_at > Instant::now());
    }

    #[tokio::test]
    async fn test_expired_memory_cache_removed_then_cache_service_hit() {
        // An expired memory cache entry should be removed (line 179),
        // then cache_service is consulted (lines 184, 187-200).
        let robots_url = "http://8.8.8.8:80/robots.txt";
        let cache_key = format!("robots_cache:{}", robots_url);
        let mock_cache = Arc::new(MockCacheService::with_entry(
            &cache_key,
            "User-agent: *\nDisallow: /\n",
        ));
        let checker = RobotsChecker {
            engine_client: Arc::new(EngineClient::new()),
            memory_cache: Arc::new(Mutex::new(HashMap::new())),
            cache_service: Some(mock_cache as Arc<dyn CacheService>),
            retry_policy: RetryPolicy::default(),
            cache_stats: Arc::new(CacheStats::default()),
        };

        // Pre-populate memory cache with an EXPIRED entry.
        {
            let mut cache = checker.memory_cache.lock().await;
            cache.insert(
                robots_url.to_string(),
                CachedRobots {
                    content: "old-expired-content".to_string(),
                    expires_at: Instant::now() - Duration::from_secs(1),
                },
            );
        }

        let allowed = checker
            .is_allowed("http://8.8.8.8/page", "MyBot")
            .await
            .expect("should succeed via cache_service after expired memory cache");
        assert!(!allowed, "Disallow: / should block all paths");

        // Memory cache should now hold the fresh content from cache_service.
        let mem_cache = checker.memory_cache.lock().await;
        let entry = mem_cache
            .get(robots_url)
            .expect("memory cache should have fresh entry");
        assert_eq!(entry.content, "User-agent: *\nDisallow: /\n");
        assert!(
            entry.expires_at > Instant::now(),
            "new entry should not be expired"
        );
    }

    // ========== HTTP fetch path tests (lines 205-279) ==========
    //
    // These tests use a mock EngineRouter so no real HTTP request is made.
    // The target host is 8.8.8.8 (a public IP) so SSRF validation passes
    // without DNS resolution.

    #[tokio::test]
    async fn test_http_fetch_success_returns_content() {
        // 200 response → content stored and returned.
        let router = Arc::new(MockEngineRouter::with_response(
            200,
            "User-agent: *\nDisallow: /private\n",
        ));
        let checker = make_checker_with_mock_router(router);

        let allowed = checker
            .is_allowed("http://8.8.8.8/page", "MyBot")
            .await
            .expect("should succeed via HTTP fetch");
        assert!(allowed, "non-disallowed path should be allowed");

        // Memory cache should be updated after fetch.
        let mem_cache = checker.memory_cache.lock().await;
        let entry = mem_cache
            .get("http://8.8.8.8:80/robots.txt")
            .expect("memory cache should be updated after HTTP fetch");
        assert!(entry.content.contains("Disallow: /private"));
    }

    #[tokio::test]
    async fn test_http_fetch_404_returns_empty_content() {
        // 404 → empty content → allow all.
        let router = Arc::new(MockEngineRouter::with_response(404, "Not Found"));
        let checker = make_checker_with_mock_router(router);

        let allowed = checker
            .is_allowed("http://8.8.8.8/anything", "MyBot")
            .await
            .expect("should succeed with empty content on 404");
        assert!(allowed, "404 should default to allowing all paths");

        let mem_cache = checker.memory_cache.lock().await;
        let entry = mem_cache
            .get("http://8.8.8.8:80/robots.txt")
            .expect("memory cache should have empty content");
        assert_eq!(entry.content, "", "404 should store empty content");
    }

    #[tokio::test]
    async fn test_http_fetch_403_returns_empty_content() {
        // Other non-500 status (e.g. 403) → empty content → allow all.
        let router = Arc::new(MockEngineRouter::with_response(403, "Forbidden"));
        let checker = make_checker_with_mock_router(router);

        let allowed = checker
            .is_allowed("http://8.8.8.8/anything", "MyBot")
            .await
            .expect("should succeed with empty content on 403");
        assert!(allowed, "403 should default to allowing all paths");
    }

    #[tokio::test]
    async fn test_http_fetch_500_retries_then_defaults_empty() {
        // 500 (server error) triggers retries; after all retries the
        // content defaults to empty.
        let router = Arc::new(MockEngineRouter::with_response(
            500,
            "Internal Server Error",
        ));
        let router_ref = router.clone();
        let checker = make_checker_with_mock_router(router);

        let allowed = checker
            .is_allowed("http://8.8.8.8/anything", "MyBot")
            .await
            .expect("should succeed with empty content after retries");
        assert!(
            allowed,
            "server errors should default to allowing all paths"
        );

        // Should have retried max_retries (3) times.
        assert_eq!(
            router_ref.call_count(),
            3,
            "should retry 3 times on 500 errors"
        );

        let mem_cache = checker.memory_cache.lock().await;
        let entry = mem_cache
            .get("http://8.8.8.8:80/robots.txt")
            .expect("memory cache should have entry");
        assert_eq!(
            entry.content, "",
            "content should be empty after failed retries"
        );
    }

    #[tokio::test]
    async fn test_http_fetch_request_error_defaults_empty() {
        // Request failure (e.g. connection refused) triggers retries;
        // after all retries the content defaults to empty.
        let router = Arc::new(MockEngineRouter::with_error("connection refused"));
        let router_ref = router.clone();
        let checker = make_checker_with_mock_router(router);

        let allowed = checker
            .is_allowed("http://8.8.8.8/anything", "MyBot")
            .await
            .expect("should succeed with empty content after request errors");
        assert!(
            allowed,
            "request errors should default to allowing all paths"
        );

        assert_eq!(
            router_ref.call_count(),
            3,
            "should retry 3 times on request errors"
        );
    }

    #[tokio::test]
    async fn test_http_fetch_updates_cache_service() {
        // After a successful HTTP fetch, cache_service.set should be called
        // to persist the content.
        let router = Arc::new(MockEngineRouter::with_response(
            200,
            "User-agent: *\nDisallow: /private\n",
        ));
        let mock_cache = Arc::new(MockCacheService::new());
        let checker = RobotsChecker {
            engine_client: Arc::new(EngineClient::with_router(router)),
            memory_cache: Arc::new(Mutex::new(HashMap::new())),
            cache_service: Some(mock_cache.clone() as Arc<dyn CacheService>),
            retry_policy: RetryPolicy {
                max_retries: 3,
                initial_backoff: Duration::from_millis(1),
                max_backoff: Duration::from_millis(10),
                ..Default::default()
            },
            cache_stats: Arc::new(CacheStats::default()),
        };

        checker
            .is_allowed("http://8.8.8.8/page", "MyBot")
            .await
            .expect("should succeed");

        assert_eq!(
            mock_cache.set_count(),
            1,
            "cache_service.set should be called once after HTTP fetch"
        );
    }

    #[tokio::test]
    async fn test_full_flow_first_fetch_then_cache_hit() {
        // First call: cache miss → HTTP fetch → memory cache populated.
        // Second call: memory cache hit → no HTTP fetch needed.
        let router = Arc::new(MockEngineRouter::with_response(
            200,
            "User-agent: *\nDisallow: /private\n",
        ));
        let router_ref = router.clone();
        let checker = make_checker_with_mock_router(router);

        // First call — should fetch via HTTP.
        let allowed1 = checker
            .is_allowed("http://8.8.8.8/page", "MyBot")
            .await
            .expect("first call should succeed");
        assert!(allowed1, "first call: non-disallowed path allowed");
        assert_eq!(
            router_ref.call_count(),
            1,
            "first call should trigger HTTP fetch"
        );

        // Second call — should hit memory cache (no HTTP fetch).
        let allowed2 = checker
            .is_allowed("http://8.8.8.8/private", "MyBot")
            .await
            .expect("second call should succeed");
        assert!(!allowed2, "second call: disallowed path blocked");
        assert_eq!(
            router_ref.call_count(),
            1,
            "second call should NOT trigger HTTP fetch (cache hit)"
        );

        let (hits, misses) = checker.get_cache_stats();
        assert_eq!(hits, 1, "one cache hit (second call)");
        assert_eq!(misses, 1, "one cache miss (first call)");
    }

    // ========== SSRF failure path test (line 205) ==========

    #[tokio::test]
    async fn test_get_robots_content_ssrf_blocked_returns_error() {
        // A localhost URL should be blocked by SSRF validation (line 205),
        // returning an error before any HTTP fetch is attempted.
        let router = Arc::new(MockEngineRouter::with_response(200, ""));
        let router_ref = router.clone();
        let checker = make_checker_with_mock_router(router);

        let result = checker.is_allowed("http://localhost/page", "MyBot").await;
        assert!(result.is_err(), "localhost URL should be blocked by SSRF");

        // The mock engine should never have been called.
        assert_eq!(
            router_ref.call_count(),
            0,
            "no HTTP fetch should occur when SSRF blocks the URL"
        );
    }
}
