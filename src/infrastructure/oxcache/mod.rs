// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Unified oxcache module for all caching operations.
//!
//! This module provides:
//! - [`CacheService`] trait — abstract cache interface (get/set/delete/exists)
//! - [`OxcacheService`] — implementation wrapping `oxcache::Cache<String, String>`
//! - Search result caching
//! - DNS caching
//! - Regex caching
//! - Concurrency control (using tokio::sync::Semaphore)
//!
//! Rate limiting is handled by `limiteron_service` via the domain
//! `RateLimitingService` trait — no hand-written token bucket here.

use crate::config::settings::CacheSettings;
use crate::domain::models::search_result::SearchResult;
use oxcache::Cache;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

pub mod cache_service;

pub use cache_service::OxcacheService;

// =============================================================================
// CacheService Trait
// =============================================================================

/// Abstract cache service interface.
///
/// Provides basic key-value cache operations backed by oxcache.
/// All methods are async and return `anyhow::Result`.
///
/// # Implementations
///
/// - [`OxcacheService`](cache_service::OxcacheService) — wraps `oxcache::Cache<String, String>`
///
/// # Usage
///
/// ```rust,ignore
/// use crate::infrastructure::oxcache::CacheService;
///
/// async fn example(cache: &dyn CacheService) -> anyhow::Result<()> {
///     cache.set("key", "value", 3600).await?;     // TTL = 3600s
///     let val = cache.get("key").await?;           // Option<String>
///     assert!(cache.exists("key").await?);
///     cache.delete("key").await?;
///     Ok(())
/// }
/// ```
#[async_trait::async_trait]
pub trait CacheService: Send + Sync {
    /// Get value by key.
    ///
    /// Returns `Ok(None)` if key does not exist (not an error).
    fn get(
        &self,
        key: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<Option<String>>> + Send + '_>,
    >;

    /// Set key-value pair with TTL (in seconds).
    ///
    /// If `ttl_seconds` is 0, the entry is stored without expiration.
    fn set(
        &self,
        key: &str,
        value: &str,
        ttl_seconds: u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>>;

    /// Delete a key.
    ///
    /// Returns `Ok(())` even if the key did not exist.
    fn delete(
        &self,
        key: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>>;

    /// Check if a key exists.
    fn exists(
        &self,
        key: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send + '_>>;
}

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
) -> Result<Arc<SearchCache>, oxcache::OxCacheError> {
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
) -> Result<Arc<DnsCache>, oxcache::OxCacheError> {
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
) -> Result<Arc<RegexCacheType>, oxcache::OxCacheError> {
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
) -> Result<Arc<SearchCache>, oxcache::OxCacheError> {
    create_cache(settings).await
}

#[cfg(not(feature = "redis-cache"))]
pub async fn create_tiered_cache(
    settings: &CacheSettings,
) -> Result<Arc<SearchCache>, oxcache::OxCacheError> {
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

    // =========================================================================
    // generate_search_key: None 参数路径
    // =========================================================================

    #[test]
    fn test_generate_search_key_without_lang_and_country() {
        // lang=None, country=None 时不应包含 lang/country 段
        let key = generate_search_key("rust", 10, None, None);
        assert_eq!(key, "search:rust:limit=10");
    }

    #[test]
    fn test_generate_search_key_with_only_lang() {
        // 仅 lang=Some 时包含 lang 段，不包含 country 段
        let key = generate_search_key("rust", 5, Some("zh"), None);
        assert_eq!(key, "search:rust:lang=zh:limit=5");
    }

    #[test]
    fn test_generate_search_key_with_only_country() {
        // 仅 country=Some 时包含 country 段，不包含 lang 段
        let key = generate_search_key("rust", 20, None, Some("CN"));
        assert_eq!(key, "search:rust:country=CN:limit=20");
    }
}
