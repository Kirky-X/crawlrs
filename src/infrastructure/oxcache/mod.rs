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
}
