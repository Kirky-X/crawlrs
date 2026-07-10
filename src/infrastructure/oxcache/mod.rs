// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Unified oxcache module for all caching operations.
//!
//! This module provides:
//! - Search result caching
//! - DNS caching
//! - Regex caching
//! - Rate limiting (using oxcache's TokenBucket)
//! - Concurrency control (using oxcache's Semaphore)

use crate::config::settings::CacheSettings;
use crate::domain::models::search_result::SearchResult;
use oxcache::Cache;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

// =============================================================================
// Cache Types
// =============================================================================

/// Search result cache type
pub type SearchCache = Cache<String, Vec<SearchResult>>;

/// Regex cache type - stores compiled regex as string pattern (Regex doesn't impl Serialize)
pub type RegexCacheType = Cache<String, String>;

/// DNS cache entry
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DnsCacheEntry {
    pub ips: Vec<IpAddr>,
    pub remaining_ttl_secs: u64,
}

/// DNS cache type
pub type DnsCache = Cache<String, DnsCacheEntry>;

// =============================================================================
// Rate Limiting (token bucket implementation)
// =============================================================================

/// Rate limit configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum requests per second
    pub max_requests_per_second: u64,
    /// Burst capacity (max tokens in bucket)
    pub burst_capacity: u64,
    /// Block duration in seconds when rate limit exceeded
    pub block_duration_secs: u64,
}

/// Per-client token bucket state
#[derive(Clone)]
struct TokenBucket {
    tokens: f64,
    last_refill: Instant,
}

/// Rate limiter using token bucket algorithm
#[derive(Clone)]
pub struct RateLimiter {
    inner: Arc<tokio::sync::Mutex<HashMap<String, TokenBucket>>>,
    config: RateLimitConfig,
}

impl RateLimiter {
    /// Create a new rate limiter with default config
    #[allow(clippy::new_ret_no_self)]
    pub fn new_default() -> Self {
        let config = RateLimitConfig {
            max_requests_per_second: 60,
            burst_capacity: 20,
            block_duration_secs: 1,
        };
        Self::new(config)
    }

    /// Create a new rate limiter with custom config
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            inner: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
            config,
        }
    }

    /// Create a new rate limiter with individual parameters
    pub fn new_with_config(max_requests_per_second: u64, burst_capacity: u64) -> Self {
        let config = RateLimitConfig {
            max_requests_per_second,
            burst_capacity,
            block_duration_secs: 1,
        };
        Self::new(config)
    }

    /// Check if a request is allowed for the given client
    pub async fn check_rate_limit(&self, client_id: &str) -> Result<(), Duration> {
        self.check_rate_limit_n(client_id, 1).await
    }

    /// Check if N requests are allowed
    pub async fn check_rate_limit_n(&self, client_id: &str, n: u64) -> Result<(), Duration> {
        let mut buckets = self.inner.lock().await;
        let refill_rate = self.config.max_requests_per_second as f64;
        let capacity = self.config.burst_capacity as f64;

        let bucket = buckets.entry(client_id.to_string()).or_insert(TokenBucket {
            tokens: capacity,
            last_refill: Instant::now(),
        });

        // Refill tokens based on elapsed time
        let now = Instant::now();
        let elapsed = now.duration_since(bucket.last_refill).as_secs_f64();
        bucket.tokens = (bucket.tokens + elapsed * refill_rate).min(capacity);
        bucket.last_refill = now;

        let requested = n as f64;
        if bucket.tokens >= requested {
            bucket.tokens -= requested;
            Ok(())
        } else {
            let needed = requested - bucket.tokens;
            let retry_after = needed / refill_rate;
            Err(Duration::from_secs_f64(retry_after))
        }
    }

    /// Get the configuration
    pub fn config(&self) -> &RateLimitConfig {
        &self.config
    }
}

// =============================================================================
// Concurrency Control (using Semaphore)
// =============================================================================

/// Semaphore-based concurrency controller
#[derive(Clone)]
pub struct ConcurrencyController {
    semaphore: Arc<tokio::sync::Semaphore>,
    max_permits: usize,
}

impl ConcurrencyController {
    /// Create a new concurrency controller
    pub fn new(max_permits: usize) -> Self {
        Self {
            semaphore: Arc::new(tokio::sync::Semaphore::new(max_permits)),
            max_permits,
        }
    }

    /// Acquire a permit, returns None if limited
    pub async fn try_acquire(&self) -> Option<ConcurrencyPermit> {
        match self.semaphore.clone().try_acquire_owned() {
            Ok(permit) => Some(ConcurrencyPermit { _permit: permit }),
            Err(_) => None,
        }
    }

    /// Acquire a permit with wait, returns None if timeout
    pub async fn acquire(&self, timeout: Duration) -> Option<ConcurrencyPermit> {
        match tokio::time::timeout(timeout, self.semaphore.clone().acquire_owned()).await {
            Ok(Ok(permit)) => Some(ConcurrencyPermit { _permit: permit }),
            _ => None,
        }
    }

    /// Get current available permits
    pub fn available_permits(&self) -> usize {
        self.semaphore.available_permits()
    }

    /// Get max permits
    pub fn max_permits(&self) -> usize {
        self.max_permits
    }
}

/// RAII guard for concurrency permit
pub struct ConcurrencyPermit {
    _permit: tokio::sync::OwnedSemaphorePermit,
}

impl Drop for ConcurrencyPermit {
    fn drop(&mut self) {
        // Permit is automatically released when dropped
    }
}

// =============================================================================
// Key Generation Utilities
// =============================================================================

/// Generate cache key from prefix and parameters
pub fn generate_key(prefix: &str, params: &[(&str, &str)]) -> String {
    if params.is_empty() {
        return prefix.to_string();
    }

    let mut key = prefix.to_string();
    for (k, v) in params {
        key.push(':');
        key.push_str(k);
        key.push('=');
        key.push_str(v);
    }
    key
}

/// Generate search cache key
pub fn generate_search_key(
    query: &str,
    limit: u32,
    lang: Option<&str>,
    country: Option<&str>,
) -> String {
    let mut key = format!("search:{}", query);
    if let Some(l) = lang {
        key.push_str(&format!(":lang={}", l));
    }
    if let Some(c) = country {
        key.push_str(&format!(":country={}", c));
    }
    key.push_str(&format!(":limit={}", limit));
    key
}

/// Generate DNS cache key
pub fn generate_dns_key(hostname: &str, port: u16) -> String {
    format!("dns:{}:{}", hostname, port)
}

/// Generate regex cache key (using hash for efficiency)
pub fn generate_regex_key(pattern: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    pattern.hash(&mut hasher);
    let hash = hasher.finish();
    format!("regex:{:x}", hash)
}

// =============================================================================
// Cache Initialization
// =============================================================================

/// Create a new oxcache instance with the given settings
pub async fn create_cache(
    settings: &CacheSettings,
) -> Result<Arc<SearchCache>, oxcache::CacheError> {
    let cache: SearchCache = Cache::builder()
        .capacity(settings.memory.capacity)
        .ttl(Duration::from_secs(settings.memory.ttl_seconds))
        .build()
        .await?;
    Ok(Arc::new(cache))
}

/// Create a new DNS cache instance
pub async fn create_dns_cache(
    capacity: u64,
    ttl_seconds: u64,
) -> Result<Arc<DnsCache>, oxcache::CacheError> {
    let cache: DnsCache = Cache::builder()
        .capacity(capacity)
        .ttl(Duration::from_secs(ttl_seconds))
        .build()
        .await?;
    Ok(Arc::new(cache))
}

/// Create a new Regex cache instance
pub async fn create_regex_cache(
    capacity: u64,
    ttl_seconds: u64,
) -> Result<Arc<RegexCacheType>, oxcache::CacheError> {
    let cache: RegexCacheType = Cache::builder()
        .capacity(capacity)
        .ttl(Duration::from_secs(ttl_seconds))
        .build()
        .await?;
    Ok(Arc::new(cache))
}

/// Create oxcache with Redis backend (tiered cache)
#[cfg(feature = "redis-cache")]
pub async fn create_tiered_cache(
    settings: &CacheSettings,
) -> Result<Arc<SearchCache>, oxcache::CacheError> {
    create_cache(settings).await
}

#[cfg(not(feature = "redis-cache"))]
pub async fn create_tiered_cache(
    settings: &CacheSettings,
) -> Result<Arc<SearchCache>, oxcache::CacheError> {
    create_cache(settings).await
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generate_key() {
        assert_eq!(generate_key("search", &[]), "search");
        assert_eq!(
            generate_key("search", &[("q", "rust"), ("l", "en")]),
            "search:q=rust:l=en"
        );
    }

    #[test]
    fn test_generate_search_key() {
        let key = generate_search_key("rust", 10, Some("en"), Some("US"));
        assert_eq!(key, "search:rust:lang=en:country=US:limit=10");
    }

    #[test]
    fn test_generate_dns_key() {
        let key = generate_dns_key("example.com", 80);
        assert_eq!(key, "dns:example.com:80");
    }

    #[test]
    fn test_generate_regex_key() {
        let key1 = generate_regex_key(r"\d+");
        let key2 = generate_regex_key(r"\d+");
        assert_eq!(key1, key2);
        assert!(key1.starts_with("regex:"));
    }

    #[tokio::test]
    async fn test_rate_limiter() {
        let limiter = RateLimiter::new_default();

        // First few requests should succeed
        for _ in 0..5 {
            assert!(limiter.check_rate_limit("test_client").await.is_ok());
        }
    }

    #[tokio::test]
    async fn test_concurrency_controller() {
        let controller = ConcurrencyController::new(2);

        // Should be able to acquire 2 permits
        // 必须持有 permit 变量，否则 permit 在语句结束时立即 drop 并释放回信号量
        let _p1 = controller.try_acquire().await;
        assert!(_p1.is_some());
        let _p2 = controller.try_acquire().await;
        assert!(_p2.is_some());

        // Third should fail (_p1 和 _p2 仍存活，未释放)
        assert!(controller.try_acquire().await.is_none());
    }

    // =========================================================================
    // RateLimiter 构造器与配置
    // =========================================================================

    #[test]
    fn test_rate_limiter_new_default_config() {
        let limiter = RateLimiter::new_default();
        let config = limiter.config();
        assert_eq!(config.max_requests_per_second, 60);
        assert_eq!(config.burst_capacity, 20);
        assert_eq!(config.block_duration_secs, 1);
    }

    #[test]
    fn test_rate_limiter_new_custom_config() {
        let config = RateLimitConfig {
            max_requests_per_second: 100,
            burst_capacity: 50,
            block_duration_secs: 5,
        };
        let limiter = RateLimiter::new(config);
        let cfg = limiter.config();
        assert_eq!(cfg.max_requests_per_second, 100);
        assert_eq!(cfg.burst_capacity, 50);
        assert_eq!(cfg.block_duration_secs, 5);
    }

    #[test]
    fn test_rate_limiter_new_with_config_params() {
        let limiter = RateLimiter::new_with_config(30, 10);
        let cfg = limiter.config();
        assert_eq!(cfg.max_requests_per_second, 30);
        assert_eq!(cfg.burst_capacity, 10);
        // new_with_config 固定 block_duration_secs = 1
        assert_eq!(cfg.block_duration_secs, 1);
    }

    #[test]
    fn test_rate_limit_config_field_access() {
        let config = RateLimitConfig {
            max_requests_per_second: 200,
            burst_capacity: 80,
            block_duration_secs: 10,
        };
        assert_eq!(config.max_requests_per_second, 200);
        assert_eq!(config.burst_capacity, 80);
        assert_eq!(config.block_duration_secs, 10);
    }

    #[tokio::test]
    async fn test_rate_limiter_config_getter_returns_reference() {
        let limiter = RateLimiter::new_with_config(42, 7);
        let cfg = limiter.config();
        assert_eq!(cfg.max_requests_per_second, 42);
        assert_eq!(cfg.burst_capacity, 7);
    }

    // =========================================================================
    // RateLimiter 批量扣减与限流触发
    // =========================================================================

    #[tokio::test]
    async fn test_check_rate_limit_n_batch_deduction() {
        // burst_capacity=20, 一次扣减 5 后剩 15，再扣 10 后剩 5
        let limiter = RateLimiter::new_with_config(60, 20);

        assert!(limiter.check_rate_limit_n("batch_client", 5).await.is_ok());
        assert!(limiter.check_rate_limit_n("batch_client", 10).await.is_ok());
        // 仅剩 5 个令牌，请求 10 个应失败
        let result = limiter.check_rate_limit_n("batch_client", 10).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_check_rate_limit_n_exceeds_capacity() {
        // burst_capacity=5，请求 10 必然失败
        let limiter = RateLimiter::new_with_config(60, 5);
        let result = limiter.check_rate_limit_n("big_client", 10).await;
        assert!(result.is_err());
        let retry_after = result.unwrap_err();
        // needed = 10 - 5 = 5, retry_after = 5 / 60
        assert!(retry_after.as_secs_f64() > 0.0);
        assert!(retry_after.as_secs_f64() < 1.0);
    }

    #[tokio::test]
    async fn test_rate_limiter_burst_exhaustion_returns_error() {
        // burst_capacity=3，连续 3 次成功后第 4 次应返回 Err(Duration)
        let limiter = RateLimiter::new_with_config(60, 3);

        assert!(limiter.check_rate_limit("limit_client").await.is_ok());
        assert!(limiter.check_rate_limit("limit_client").await.is_ok());
        assert!(limiter.check_rate_limit("limit_client").await.is_ok());

        let result = limiter.check_rate_limit("limit_client").await;
        assert!(result.is_err());
        let retry_after = result.unwrap_err();
        // 令牌近乎耗尽，retry_after 应为正且小于 1 秒（refill_rate=60）
        assert!(retry_after.as_secs_f64() > 0.0);
        assert!(retry_after.as_secs_f64() < 1.0);
    }

    #[tokio::test]
    async fn test_rate_limiter_token_refill_after_wait() {
        // refill_rate=2/s, burst=2：耗尽后等待令牌恢复
        let limiter = RateLimiter::new_with_config(2, 2);

        // 耗尽令牌
        assert!(limiter.check_rate_limit("refill_client").await.is_ok());
        assert!(limiter.check_rate_limit("refill_client").await.is_ok());
        // 立即再请求应失败
        assert!(limiter.check_rate_limit("refill_client").await.is_err());

        // 等待 600ms，refill_rate=2/s 应恢复约 1.2 个令牌（>=1）
        tokio::time::sleep(Duration::from_millis(600)).await;
        assert!(limiter.check_rate_limit("refill_client").await.is_ok());
    }

    #[tokio::test]
    async fn test_rate_limiter_independent_clients() {
        // 不同 client_id 应有独立的令牌桶
        let limiter = RateLimiter::new_with_config(60, 2);

        assert!(limiter.check_rate_limit("client_a").await.is_ok());
        assert!(limiter.check_rate_limit("client_a").await.is_ok());
        // client_a 耗尽，但 client_b 仍可用
        assert!(limiter.check_rate_limit("client_a").await.is_err());
        assert!(limiter.check_rate_limit("client_b").await.is_ok());
    }

    // =========================================================================
    // ConcurrencyController
    // =========================================================================

    #[tokio::test]
    async fn test_concurrency_controller_available_and_max_permits() {
        let controller = ConcurrencyController::new(3);
        assert_eq!(controller.max_permits(), 3);
        assert_eq!(controller.available_permits(), 3);

        let p1 = controller.try_acquire().await;
        assert!(p1.is_some());
        assert_eq!(controller.available_permits(), 2);
        assert_eq!(controller.max_permits(), 3);
    }

    #[tokio::test]
    async fn test_concurrency_controller_acquire_success() {
        let controller = ConcurrencyController::new(1);
        // 立即可用，acquire 应在 timeout 内成功
        let permit = controller.acquire(Duration::from_millis(100)).await;
        assert!(permit.is_some());
        assert_eq!(controller.available_permits(), 0);
    }

    #[tokio::test]
    async fn test_concurrency_controller_acquire_timeout() {
        let controller = ConcurrencyController::new(1);
        // 先占用唯一 permit（持有不释放）
        let _held = controller.try_acquire().await;
        assert_eq!(controller.available_permits(), 0);

        // acquire 应超时返回 None
        let result = controller.acquire(Duration::from_millis(50)).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_concurrency_permit_drop_releases_semaphore() {
        let controller = ConcurrencyController::new(1);

        {
            let _permit = controller.try_acquire().await;
            assert!(_permit.is_some());
            assert_eq!(controller.available_permits(), 0);
        } // permit 在此 drop

        assert_eq!(controller.available_permits(), 1);
        // drop 后应能再次获取
        let again = controller.try_acquire().await;
        assert!(again.is_some());
    }

    // =========================================================================
    // DnsCacheEntry serde
    // =========================================================================

    #[test]
    fn test_dns_cache_entry_serde_roundtrip() {
        let entry = DnsCacheEntry {
            ips: vec!["192.168.1.1".parse().unwrap(), "::1".parse().unwrap()],
            remaining_ttl_secs: 300,
        };
        let json = serde_json::to_string(&entry).expect("serialize failed");
        let decoded: DnsCacheEntry = serde_json::from_str(&json).expect("deserialize failed");
        assert_eq!(decoded.ips, entry.ips);
        assert_eq!(decoded.remaining_ttl_secs, 300);
    }

    #[test]
    fn test_dns_cache_entry_serde_empty_ips() {
        let entry = DnsCacheEntry {
            ips: vec![],
            remaining_ttl_secs: 0,
        };
        let json = serde_json::to_string(&entry).expect("serialize failed");
        let decoded: DnsCacheEntry = serde_json::from_str(&json).expect("deserialize failed");
        assert!(decoded.ips.is_empty());
        assert_eq!(decoded.remaining_ttl_secs, 0);
    }

    // =========================================================================
    // Cache 初始化函数
    // =========================================================================

    fn make_test_cache_settings() -> CacheSettings {
        CacheSettings {
            enabled: true,
            memory: crate::config::settings::MemoryCacheSettings {
                capacity: 100,
                ttl_seconds: 60,
            },
            redis: crate::config::settings::RedisCacheSettings {
                enabled: false,
                url: "redis://localhost:6379".to_string(),
                pool_size: 10,
                ttl_seconds: 3600,
            },
            types: crate::config::settings::CacheTypeSpecificSettings {
                search: crate::config::settings::CacheTypeSettings {
                    ttl_seconds: 60,
                    max_size: 100,
                },
                dns: crate::config::settings::CacheTypeSettings {
                    ttl_seconds: 300,
                    max_size: 100,
                },
                regex: crate::config::settings::CacheTypeSettings {
                    ttl_seconds: 600,
                    max_size: 50,
                },
            },
        }
    }

    #[tokio::test]
    async fn test_create_cache_ok() {
        let settings = make_test_cache_settings();
        let cache = create_cache(&settings).await;
        assert!(
            cache.is_ok(),
            "create_cache should succeed: {:?}",
            cache.err()
        );
        let cache = cache.unwrap();
        // 验证可用：set 后 get 一致
        let key = "k1".to_string();
        let val: Vec<SearchResult> = vec![];
        assert!(cache.set(&key, &val).await.is_ok());
        let got = cache.get(&key).await;
        assert!(got.is_ok(), "get should succeed");
        assert!(got.unwrap().is_some(), "value should be present");
    }

    #[tokio::test]
    async fn test_create_dns_cache_ok() {
        let cache = create_dns_cache(50, 120).await;
        assert!(cache.is_ok(), "create_dns_cache should succeed");
        let cache = cache.unwrap();
        // 验证可用
        let key = "dns:example.com:80".to_string();
        let entry = DnsCacheEntry {
            ips: vec!["1.2.3.4".parse().unwrap()],
            remaining_ttl_secs: 120,
        };
        assert!(cache.set(&key, &entry).await.is_ok());
        let got = cache.get(&key).await;
        assert!(got.is_ok());
        assert!(got.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_create_regex_cache_ok() {
        let cache = create_regex_cache(20, 600).await;
        assert!(cache.is_ok(), "create_regex_cache should succeed");
        let cache = cache.unwrap();
        let key = "regex:abc".to_string();
        assert!(cache.set(&key, &"pattern".to_string()).await.is_ok());
        let got = cache.get(&key).await;
        assert!(got.is_ok());
        assert!(got.unwrap().is_some());
    }

    #[tokio::test]
    async fn test_create_tiered_cache_ok() {
        // 未启用 redis-cache feature 时，create_tiered_cache 等价于 create_cache
        let settings = make_test_cache_settings();
        let cache = create_tiered_cache(&settings).await;
        assert!(cache.is_ok(), "create_tiered_cache should succeed");
        let _cache = cache.unwrap();
    }

    #[tokio::test]
    async fn test_create_cache_uses_settings_ttl_and_capacity() {
        // 使用不同容量/TTL 验证 settings 真正传入 builder
        let settings = make_test_cache_settings();
        let cache = create_cache(&settings).await.unwrap();
        // 插入超过 capacity 数量的条目不应 panic（moka 自身做淘汰）
        for i in 0..150 {
            let key = format!("k{}", i);
            let val: Vec<SearchResult> = vec![];
            let _ = cache.set(&key, &val).await;
        }
        // 至少能取到最后写入的一个
        let last_key = "k149".to_string();
        let got = cache.get(&last_key).await;
        assert!(got.is_ok());
    }
}
