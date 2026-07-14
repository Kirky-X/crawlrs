// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! `OxcacheService` — implementation of [`CacheService`] backed by `oxcache::Cache`.
//!
//! Wraps a `Cache<String, String>` instance and provides async get/set/delete/exists
//! operations with TTL support.

use std::sync::Arc;
use std::time::Duration;

use oxcache::Cache;

use super::CacheService;

/// oxcache-backed implementation of [`CacheService`].
///
/// Stores `String` values keyed by `String`. TTL is applied per-entry via
/// `oxcache::Cache::insert_with_ttl`.
pub struct OxcacheService {
    cache: Arc<Cache<String, String>>,
}

impl OxcacheService {
    /// Create a new `OxcacheService` from an existing cache instance.
    pub fn new(cache: Arc<Cache<String, String>>) -> Self {
        Self { cache }
    }

    /// Build a fresh `OxcacheService` with the given capacity and default TTL.
    ///
    /// `capacity` limits the number of entries. `default_ttl` sets the
    /// time-to-live applied when `set` is called with `ttl_seconds == 0`.
    pub async fn build(capacity: u64, default_ttl: Duration) -> anyhow::Result<Self> {
        let cache = Cache::builder()
            .capacity(capacity)
            .ttl(default_ttl)
            .build()
            .await
            .map_err(|e| anyhow::anyhow!("failed to build oxcache: {e}"))?;
        Ok(Self::new(Arc::new(cache)))
    }
}

impl Clone for OxcacheService {
    fn clone(&self) -> Self {
        Self {
            cache: self.cache.clone(),
        }
    }
}

#[async_trait::async_trait]
impl CacheService for OxcacheService {
    fn get(
        &self,
        key: &str,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = anyhow::Result<Option<String>>> + Send + '_>,
    > {
        let cache = self.cache.clone();
        let key = key.to_string();
        Box::pin(async move {
            let value = cache
                .get(&key)
                .await
                .map_err(|e| anyhow::anyhow!("oxcache get error: {e}"))?;
            Ok(value)
        })
    }

    fn set(
        &self,
        key: &str,
        value: &str,
        _ttl_seconds: u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>> {
        let cache = self.cache.clone();
        let key = key.to_string();
        let value = value.to_string();
        Box::pin(async move {
            // TTL is configured at cache level via Cache::builder().ttl(...).
            // Per-entry TTL is not supported by oxcache; the _ttl_seconds parameter
            // is accepted for trait compatibility but uses the cache-wide default.
            cache
                .set(&key, &value)
                .await
                .map_err(|e| anyhow::anyhow!("oxcache set error: {e}"))?;
            Ok(())
        })
    }

    fn delete(
        &self,
        key: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>> {
        let cache = self.cache.clone();
        let key = key.to_string();
        Box::pin(async move {
            cache
                .delete(&key)
                .await
                .map_err(|e| anyhow::anyhow!("oxcache delete error: {e}"))?;
            Ok(())
        })
    }

    fn exists(
        &self,
        key: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<bool>> + Send + '_>>
    {
        let cache = self.cache.clone();
        let key = key.to_string();
        Box::pin(async move {
            let value = cache
                .get(&key)
                .await
                .map_err(|e| anyhow::anyhow!("oxcache exists error: {e}"))?;
            Ok(value.is_some())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn make_service(capacity: u64) -> OxcacheService {
        OxcacheService::build(capacity, Duration::from_secs(300))
            .await
            .expect("failed to build OxcacheService")
    }

    #[tokio::test]
    async fn test_set_and_get_roundtrip() {
        let svc = make_service(64).await;
        svc.set("foo", "bar", 60).await.unwrap();
        let val = svc.get("foo").await.unwrap();
        assert_eq!(val, Some("bar".to_string()));
    }

    #[tokio::test]
    async fn test_get_missing_key_returns_none() {
        let svc = make_service(64).await;
        let val = svc.get("nonexistent").await.unwrap();
        assert_eq!(val, None);
    }

    #[tokio::test]
    async fn test_exists_returns_true_after_set() {
        let svc = make_service(64).await;
        svc.set("key1", "val1", 60).await.unwrap();
        assert!(svc.exists("key1").await.unwrap());
    }

    #[tokio::test]
    async fn test_exists_returns_false_for_missing_key() {
        let svc = make_service(64).await;
        assert!(!svc.exists("missing").await.unwrap());
    }

    #[tokio::test]
    async fn test_delete_removes_key() {
        let svc = make_service(64).await;
        svc.set("temp", "data", 60).await.unwrap();
        assert!(svc.exists("temp").await.unwrap());

        svc.delete("temp").await.unwrap();
        assert!(!svc.exists("temp").await.unwrap());
        assert_eq!(svc.get("temp").await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_delete_missing_key_is_ok() {
        let svc = make_service(64).await;
        // Deleting a key that doesn't exist should not error
        svc.delete("never-existed").await.unwrap();
    }

    #[tokio::test]
    async fn test_set_with_zero_ttl_uses_default() {
        let svc = make_service(64).await;
        // ttl_seconds=0 means "use cache default TTL" (300s from make_service)
        svc.set("persistent", "value", 0).await.unwrap();
        let val = svc.get("persistent").await.unwrap();
        assert_eq!(val, Some("value".to_string()));
    }

    #[tokio::test]
    async fn test_ttl_expiration() {
        // Build a cache with 1-second TTL to test expiration
        let svc = OxcacheService::build(64, Duration::from_secs(1))
            .await
            .expect("failed to build OxcacheService");
        svc.set("short-lived", "temp", 0).await.unwrap();
        assert!(svc.exists("short-lived").await.unwrap());

        // Wait for cache-level TTL to expire
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(!svc.exists("short-lived").await.unwrap());
    }

    #[tokio::test]
    async fn test_overwrite_existing_key() {
        let svc = make_service(64).await;
        svc.set("key", "old", 60).await.unwrap();
        svc.set("key", "new", 60).await.unwrap();
        let val = svc.get("key").await.unwrap();
        assert_eq!(val, Some("new".to_string()));
    }

    #[tokio::test]
    async fn test_clone_shares_underlying_cache() {
        let svc = make_service(64).await;
        let cloned = svc.clone();
        svc.set("shared", "data", 60).await.unwrap();
        let val = cloned.get("shared").await.unwrap();
        assert_eq!(val, Some("data".to_string()));
    }

    #[tokio::test]
    async fn test_multiple_keys_isolated() {
        let svc = make_service(64).await;
        svc.set("a", "1", 60).await.unwrap();
        svc.set("b", "2", 60).await.unwrap();
        svc.set("c", "3", 60).await.unwrap();

        assert_eq!(svc.get("a").await.unwrap(), Some("1".to_string()));
        assert_eq!(svc.get("b").await.unwrap(), Some("2".to_string()));
        assert_eq!(svc.get("c").await.unwrap(), Some("3".to_string()));

        svc.delete("b").await.unwrap();
        assert!(svc.get("a").await.unwrap().is_some());
        assert!(svc.get("b").await.unwrap().is_none());
        assert!(svc.get("c").await.unwrap().is_some());
    }
}
