// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! `OxcacheService` — implementation of [`CacheService`] backed by `oxcache::Cache`.
//!
//! Wraps a `Cache<String, String>` instance and provides async get/set/delete/exists
//! operations with TTL support.

use std::sync::Arc;
use std::time::Duration;

use crate::common::metrics_shim::counter;
#[cfg(feature = "cache-redis")]
use oxcache::backend::RedisBackend;
use oxcache::Cache;

use super::CacheService;

/// oxcache-backed implementation of [`CacheService`].
///
/// Stores `String` values keyed by `String`. TTL is applied per-entry via
/// `oxcache::Cache::set_with_ttl`: `ttl_seconds == 0` uses the build-time
/// default TTL, any other value expires after that many seconds.
pub struct OxcacheService {
    cache: Arc<Cache<String, String>>,
    /// 缓存类型标签（用于 metric 标签区分 search/dns/regex 等）
    cache_type: &'static str,
    /// 构建期默认 TTL：`set` 的 `ttl_seconds=0` 落到该值，使 memory
    /// （moka per-entry TTL）与 redis（`SET PX`）两后端 get/set/ttl
    /// 语义一致（R-redis-shared-cache-002）。`Duration::ZERO` 表示未配置
    /// 默认（[`Self::new`] 路径），0 交还后端自身语义。
    default_ttl: Duration,
}

impl OxcacheService {
    /// Create a new `OxcacheService` from an existing cache instance.
    ///
    /// 该路径未配置默认 TTL（`default_ttl = ZERO`）：`set` 的
    /// `ttl_seconds=0` 由后端自身语义决定（memory 为 builder 级默认 TTL）。
    /// 需要 redis 语义一致或默认 TTL 时用 [`Self::build`] /
    /// [`Self::build_with_backend`]。
    pub fn new(cache: Arc<Cache<String, String>>) -> Self {
        Self {
            cache,
            cache_type: "generic",
            default_ttl: Duration::ZERO,
        }
    }

    /// 创建带类型标签的 OxcacheService（用于 metric 标签区分）
    pub fn with_cache_type(cache: Arc<Cache<String, String>>, cache_type: &'static str) -> Self {
        Self {
            cache,
            cache_type,
            default_ttl: Duration::ZERO,
        }
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
        Ok(Self {
            cache: Arc::new(cache),
            cache_type: "generic",
            default_ttl,
        })
    }

    /// 构建带类型标签的 OxcacheService
    pub async fn build_with_type(
        capacity: u64,
        default_ttl: Duration,
        cache_type: &'static str,
    ) -> anyhow::Result<Self> {
        let cache = Cache::builder()
            .capacity(capacity)
            .ttl(default_ttl)
            .build()
            .await
            .map_err(|e| anyhow::anyhow!("failed to build oxcache: {e}"))?;
        Ok(Self {
            cache: Arc::new(cache),
            cache_type,
            default_ttl,
        })
    }

    /// 按 `[cache] backend` 配置构建 memory/redis 缓存实例
    /// （R-redis-shared-cache-002）。
    ///
    /// - `memory`（或空串）：进程内 moka 后端，行为与 [`Self::build`] 一致；
    /// - `redis`：[`oxcache::backend::RedisBackend`]，多实例共享；
    ///   需编译 `cache-redis` feature 且提供非空 `redis_url`，缺一即
    ///   返回错误（fail-closed，与启动期 `CacheSettings::validate_backend`
    ///   同语义，双保险）。
    ///
    /// # TLS 约束
    ///
    /// `RedisBackend` builder 默认强制 `rediss://`（TLS）。开发环境的非
    /// TLS `redis://` 连接必须显式设置环境变量
    /// `OXCACHE_ALLOW_INSECURE_REDIS=I_UNDERSTAND_THE_RISKS`
    /// （或 `development-only`）放行，否则构建报错。
    ///
    /// # Redis 不可用
    ///
    /// redis 实例不可达时构建返回 `Err`（连接期失败），上游启动流程
    /// `init_cache_service` 将其向上传播为启动失败——固定为**拒绝**
    /// （fail-closed），不降级到 memory 后端。
    ///
    /// redis 后端不受 `capacity` 约束（无逐出上限）；`default_ttl` 作为
    /// `ttl_seconds=0` 的默认值经 `SET PX` 与 memory 后端同语义生效。
    pub async fn build_with_backend(
        backend: &str,
        redis_url: Option<&str>,
        capacity: u64,
        default_ttl: Duration,
    ) -> anyhow::Result<Self> {
        match backend {
            "memory" | "" => Self::build(capacity, default_ttl).await,
            "redis" => {
                let url = redis_url
                    .map(str::trim)
                    .filter(|u| !u.is_empty())
                    .ok_or_else(|| {
                        anyhow::anyhow!("cache backend 'redis' requires a non-empty redis_url")
                    })?;
                let cache = Self::build_redis_cache(url).await?;
                Ok(Self {
                    cache,
                    cache_type: "generic",
                    default_ttl,
                })
            }
            other => Err(anyhow::anyhow!(
                "unknown cache backend '{other}' (expected 'memory' or 'redis')"
            )),
        }
    }

    /// 构建注入 `RedisBackend` 的 oxcache 实例。
    #[cfg(not(feature = "cache-redis"))]
    async fn build_redis_cache(_redis_url: &str) -> anyhow::Result<Arc<Cache<String, String>>> {
        Err(anyhow::anyhow!(
            "cache backend 'redis' requires the `cache-redis` feature (not compiled in this build)"
        ))
    }

    /// 构建注入 `RedisBackend` 的 oxcache 实例。
    #[cfg(feature = "cache-redis")]
    async fn build_redis_cache(redis_url: &str) -> anyhow::Result<Arc<Cache<String, String>>> {
        let backend = RedisBackend::builder()
            .connection_string(redis_url)
            .build()
            .await
            .map_err(|e| anyhow::anyhow!("failed to build oxcache redis backend: {e}"))?;
        let cache = Cache::builder()
            .backend_arc(Arc::new(backend))
            .build()
            .await
            .map_err(|e| anyhow::anyhow!("failed to build oxcache: {e}"))?;
        Ok(Arc::new(cache))
    }
}

impl Clone for OxcacheService {
    fn clone(&self) -> Self {
        Self {
            cache: self.cache.clone(),
            cache_type: self.cache_type,
            default_ttl: self.default_ttl,
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
        #[allow(unused_variables)]
        let cache_type = self.cache_type;
        Box::pin(async move {
            let value = cache
                .get(&key)
                .await
                .map_err(|e| anyhow::anyhow!("oxcache get error: {e}"))?;
            // Prometheus 缓存命中指标
            #[allow(unused_variables)]
            let result_label = if value.is_some() { "hit" } else { "miss" };
            counter!(
                "crawlrs_cache_hit_total",
                "cache_type" => cache_type.to_string(),
                "result" => result_label.to_string()
            )
            .increment(1);
            Ok(value)
        })
    }

    fn set(
        &self,
        key: &str,
        value: &str,
        ttl_seconds: u64,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send + '_>> {
        let cache = self.cache.clone();
        let key = key.to_string();
        let value = value.to_string();
        Box::pin(async move {
            // 逐键 TTL：`ttl_seconds=0` 落到构建期 default_ttl（未配置时
            // 传 None 交还后端自身语义），否则按秒生效。memory（moka
            // per-entry TTL）与 redis（`SET PX`）经同一换算保持 get/set/ttl
            // 语义一致（R-redis-shared-cache-002）。
            let ttl = if ttl_seconds == 0 {
                (!self.default_ttl.is_zero()).then_some(self.default_ttl)
            } else {
                Some(Duration::from_secs(ttl_seconds))
            };
            cache
                .set_with_ttl(&key, &value, ttl)
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

    /// 双后端共享语义场景（R-redis-shared-cache-002）：get/set/ttl 行为
    /// 在 memory 与 redis 后端下一致。键名带随机后缀，可对共享 redis 实例
    /// 重复运行。
    async fn assert_backend_semantics(svc: OxcacheService) {
        let uid = uuid::Uuid::new_v4().simple().to_string();
        let key = |suffix: &str| format!("crawlrs_test:{uid}:{suffix}");

        // set/get roundtrip
        let k = key("roundtrip");
        svc.set(&k, "bar", 60).await.unwrap();
        assert_eq!(svc.get(&k).await.unwrap(), Some("bar".to_string()));

        // missing key → None
        assert_eq!(svc.get(&key("missing")).await.unwrap(), None);

        // exists 语义
        assert!(svc.exists(&k).await.unwrap());
        assert!(!svc.exists(&key("missing")).await.unwrap());

        // overwrite
        svc.set(&k, "new", 60).await.unwrap();
        assert_eq!(svc.get(&k).await.unwrap(), Some("new".to_string()));

        // delete 移除
        svc.delete(&k).await.unwrap();
        assert_eq!(svc.get(&k).await.unwrap(), None);

        // ttl 语义：显式秒数到期消失（两后端同样 1s 后不可读）
        let k = key("ttl-explicit");
        svc.set(&k, "gone", 1).await.unwrap();
        assert!(svc.exists(&k).await.unwrap());
        tokio::time::sleep(Duration::from_millis(2100)).await;
        assert!(
            svc.get(&k).await.unwrap().is_none(),
            "entry with ttl_seconds=1 must expire on both backends"
        );

        // ttl 语义：ttl_seconds=0 → 构建期默认 TTL（300s），短期内仍可读
        let k = key("ttl-default");
        svc.set(&k, "kept", 0).await.unwrap();
        tokio::time::sleep(Duration::from_millis(2100)).await;
        assert!(
            svc.get(&k).await.unwrap().is_some(),
            "entry with ttl_seconds=0 must use the build-time default TTL on both backends"
        );
    }

    #[tokio::test]
    async fn test_memory_backend_semantics() {
        let svc = OxcacheService::build_with_backend("memory", None, 64, Duration::from_secs(300))
            .await
            .expect("build memory backend");
        assert_backend_semantics(svc).await;
    }

    /// redis 后端语义一致性：需 `REDIS_TEST_URL` 指向可达 redis 实例，
    /// 未设置时跳过（CI 缺省不依赖外部 redis）。
    #[tokio::test]
    async fn test_redis_backend_semantics_match_memory() {
        let Ok(url) = std::env::var("REDIS_TEST_URL") else {
            eprintln!("skip: REDIS_TEST_URL not set; redis backend semantics not exercised");
            return;
        };
        // 非 TLS 开发连接的显式放行（文档化逃生口；rediss:// 不受影响）
        std::env::set_var("OXCACHE_ALLOW_INSECURE_REDIS", "development-only");

        let svc =
            OxcacheService::build_with_backend("redis", Some(&url), 64, Duration::from_secs(300))
                .await
                .expect("build redis backend");
        assert_backend_semantics(svc).await;
    }

    #[tokio::test]
    async fn test_build_with_backend_unknown_value_rejected() {
        let err = match OxcacheService::build_with_backend(
            "memcached",
            None,
            64,
            Duration::from_secs(300),
        )
        .await
        {
            Ok(_) => panic!("unknown backend must be rejected"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("unknown cache backend"),
            "got: {err}"
        );
    }

    /// 未编译 cache-redis：构造层同样 fail-closed（URL 不能绕过编译期能力）
    #[cfg(not(feature = "cache-redis"))]
    #[tokio::test]
    async fn test_build_with_backend_redis_rejected_without_feature() {
        let err = match OxcacheService::build_with_backend(
            "redis",
            Some("rediss://127.0.0.1:6379"),
            64,
            Duration::from_secs(300),
        )
        .await
        {
            Ok(_) => panic!("redis backend requires the cache-redis feature"),
            Err(e) => e,
        };
        assert!(
            err.to_string().contains("cache-redis"),
            "error must mention the cache-redis feature, got: {err}"
        );
    }

    /// 已编译 cache-redis：缺 URL 构造拒绝
    #[cfg(feature = "cache-redis")]
    #[tokio::test]
    async fn test_build_with_backend_redis_requires_url() {
        let err =
            match OxcacheService::build_with_backend("redis", None, 64, Duration::from_secs(300))
                .await
            {
                Ok(_) => panic!("redis backend requires redis_url"),
                Err(e) => e,
            };
        assert!(
            err.to_string().contains("redis_url"),
            "error must mention redis_url, got: {err}"
        );
    }

    /// 已编译 cache-redis：redis 不可达时构建拒绝（固定语义：不降级
    /// memory，R-redis-shared-cache-002）。端口 1 恒为 connection refused，
    /// 不依赖外部服务。
    #[cfg(feature = "cache-redis")]
    #[tokio::test]
    async fn test_build_with_backend_redis_unreachable_rejected() {
        let err = match OxcacheService::build_with_backend(
            "redis",
            Some("redis://127.0.0.1:1"),
            64,
            Duration::from_secs(300),
        )
        .await
        {
            Ok(_) => panic!("unreachable redis must be rejected at build time"),
            Err(e) => e,
        };
        assert!(
            err.to_string().to_lowercase().contains("redis"),
            "error must mention redis, got: {err}"
        );
    }
}
