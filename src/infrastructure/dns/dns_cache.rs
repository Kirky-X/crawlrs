// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! DNS缓存模块（使用 oxcache + hickory-resolver）

use crate::common::metrics_shim::counter;
use crate::infrastructure::oxcache::{generate_dns_key, DnsCache, DnsCacheEntry};
use dashmap::DashMap;
use hickory_resolver::Resolver;
use log::{debug, warn};
use std::future::Future;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

/// 线程安全的DNS缓存（使用 oxcache + hickory-resolver）
#[derive(Debug, Clone)]
pub struct DnsCacheService {
    cache: Arc<DnsCache>,
    resolver: Resolver<hickory_resolver::net::runtime::TokioRuntimeProvider>,
    default_ttl: Duration,
    /// per-key single-flight：在飞解析的 key → 锁。同 host 并发查询只放行一个
    /// 调用者实际走系统解析，其余调用者等锁释放后直接读缓存，避免 thundering herd。
    in_flight: Arc<DashMap<String, Arc<tokio::sync::Mutex<()>>>>,
}

impl DnsCacheService {
    /// 创建新的DNS缓存
    pub fn new(
        cache: Arc<DnsCache>,
        default_ttl_seconds: u64,
    ) -> Result<Self, hickory_resolver::net::NetError> {
        let resolver = Resolver::builder_tokio()?.build()?;
        Ok(Self {
            cache,
            resolver,
            default_ttl: Duration::from_secs(default_ttl_seconds),
            in_flight: Arc::new(DashMap::new()),
        })
    }

    /// 解析主机名（使用缓存）
    pub async fn lookup_host(
        &self,
        hostname: &str,
        port: u16,
    ) -> Result<Vec<IpAddr>, std::io::Error> {
        let cache_key = generate_dns_key(hostname, port);

        // 尝试从缓存获取
        match self.cache.get(&cache_key).await {
            Ok(Some(entry)) => {
                debug!("DNS cache hit for {}", cache_key);
                // DNS 缓存命中指标
                counter!(
                    "crawlrs_cache_hit_total",
                    "cache_type" => "dns",
                    "result" => "hit"
                )
                .increment(1);
                return Ok(entry.ips);
            }
            Ok(None) => {
                debug!("DNS cache miss for {}, performing lookup", cache_key);
                // DNS 缓存未命中指标
                counter!(
                    "crawlrs_cache_hit_total",
                    "cache_type" => "dns",
                    "result" => "miss"
                )
                .increment(1);
            }
            Err(e) => {
                warn!("DNS cache error for {}: {}", cache_key, e);
                counter!(
                    "crawlrs_cache_hit_total",
                    "cache_type" => "dns",
                    "result" => "miss"
                )
                .increment(1);
            }
        }

        // 缓存未命中：per-key single-flight，同 host 并发查询只放行一个实际解析，
        // 后来者等锁释放后 double-check 缓存命中即返回，不再各自走系统解析。
        let resolver = &self.resolver;
        let cache_for_check = Arc::clone(&self.cache);
        let key_for_check = cache_key.clone();
        let cache_for_store = Arc::clone(&self.cache);
        let key_for_store = cache_key.clone();
        let default_ttl_secs = self.default_ttl.as_secs();
        resolve_single_flight(
            &self.in_flight,
            &cache_key,
            move || async move {
                cache_for_check
                    .get(&key_for_check)
                    .await
                    .ok()
                    .flatten()
                    .map(|entry| entry.ips)
            },
            move || async move {
                let lookup = resolver.lookup_ip(hostname).await.map_err(|e| {
                    std::io::Error::other(format!("DNS resolution failed for {}: {}", hostname, e))
                })?;
                let ips: Vec<IpAddr> = lookup.iter().collect();
                if ips.is_empty() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("No IP addresses resolved for {}", hostname),
                    ));
                }
                Ok(ips)
            },
            move |ips| async move {
                let ip_count = ips.len();
                let entry = DnsCacheEntry {
                    ips,
                    remaining_ttl_secs: default_ttl_secs,
                };
                if let Err(e) = cache_for_store.set(&key_for_store, &entry).await {
                    warn!("Failed to cache DNS result for {}: {}", key_for_store, e);
                } else {
                    debug!(
                        "Cached DNS result for {} with {} IPs, TTL: {}s",
                        key_for_store, ip_count, default_ttl_secs
                    );
                }
            },
        )
        .await
    }

    /// 获取缓存统计信息
    pub async fn get_stats(&self) -> DnsCacheStats {
        // oxcache doesn't expose hit/miss stats directly
        DnsCacheStats {
            total_entries: 0,
            hit_count: 0,
            miss_count: 0,
            hit_rate: 0.0,
            max_entries: 0,
            default_ttl_seconds: self.default_ttl.as_secs(),
        }
    }

    /// 清除所有缓存
    pub async fn clear(&self) {
        // oxcache doesn't support clear operation
        debug!("DNS cache clear requested");
    }

    /// 手动设置缓存条目（用于预热）
    pub async fn set(&self, hostname: &str, port: u16, ips: Vec<IpAddr>, ttl_seconds: u64) {
        let cache_key = generate_dns_key(hostname, port);
        let entry = DnsCacheEntry {
            ips,
            remaining_ttl_secs: ttl_seconds,
        };

        if let Err(e) = self.cache.set(&cache_key, &entry).await {
            warn!("Failed to set DNS cache for {}: {}", cache_key, e);
        } else {
            debug!("Manually set DNS cache for {}", cache_key);
        }
    }

    /// 移除特定主机的缓存
    pub async fn remove(&self, hostname: &str, port: u16) {
        let cache_key = generate_dns_key(hostname, port);
        if let Err(e) = self.cache.delete(&cache_key).await {
            warn!("Failed to remove DNS cache for {}: {}", cache_key, e);
        } else {
            debug!("Removed DNS cache for {}", cache_key);
        }
    }
}

/// per-key single-flight 执行器：同一 `key` 的并发解析只放行一个调用者实际执行。
///
/// 流程：抢 per-key 锁 → double-check 缓存（`cache_get` 返回 `Some` 即命中直接返回）
/// → 未命中则执行 `resolve` 并经 `cache_store` 写回缓存 → 释放锁并清理在飞记录。
/// 排队的后来者拿锁后 double-check 命中前一个调用者的结果，不再重复解析。
///
/// 缓存读取失败视为未命中（与 [`DnsCacheService::lookup_host`] 的既有语义一致）；
/// `resolve` 失败同样清理在飞记录，保证后续调用者可重试，不会永久串行化该 host。
async fn resolve_single_flight<G, GF, R, RF, S, SF>(
    in_flight: &DashMap<String, Arc<tokio::sync::Mutex<()>>>,
    key: &str,
    cache_get: G,
    resolve: R,
    cache_store: S,
) -> Result<Vec<IpAddr>, std::io::Error>
where
    G: FnOnce() -> GF,
    GF: Future<Output = Option<Vec<IpAddr>>>,
    R: FnOnce() -> RF,
    RF: Future<Output = Result<Vec<IpAddr>, std::io::Error>>,
    S: FnOnce(Vec<IpAddr>) -> SF,
    SF: Future<Output = ()>,
{
    // 获取（或创建）该 key 的在飞锁。DashMap 的 entry 守卫在块尾即释放，
    // 绝不跨 await 持有；持锁期间也不再触碰 in_flight 的分片锁。
    let key_lock = {
        let entry = in_flight
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())));
        Arc::clone(entry.value())
    };

    // 首个调用者持锁解析；后来者在此排队等待
    let guard = key_lock.lock().await;

    let result = match cache_get().await {
        Some(ips) => Ok(ips),
        None => {
            let resolved = resolve().await;
            if let Ok(ips) = &resolved {
                cache_store(ips.clone()).await;
            }
            resolved
        }
    };

    drop(guard);
    // 仅当条目仍指向本次获取的锁实例时移除（窗口期内可能已被并发调用者重建），
    // 避免每个查询过的 host 在 map 中永久残留一个锁条目。
    in_flight.remove_if(&key.to_string(), |_, v| Arc::ptr_eq(v, &key_lock));

    result
}

/// DNS缓存统计信息
#[derive(Debug, Clone)]
pub struct DnsCacheStats {
    pub total_entries: usize,
    pub hit_count: u64,
    pub miss_count: u64,
    pub hit_rate: f64,
    pub max_entries: usize,
    pub default_ttl_seconds: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dns_cache_creation() {
        let cache = Arc::new(
            oxcache::Cache::builder()
                .capacity(100)
                .ttl(Duration::from_secs(300))
                .build()
                .await
                .unwrap(),
        );

        let dns_cache = DnsCacheService::new(cache, 300).unwrap();
        assert_eq!(dns_cache.default_ttl.as_secs(), 300);
    }

    #[tokio::test]
    async fn test_generate_dns_key() {
        let key = generate_dns_key("example.com", 80);
        assert_eq!(key, "dns:example.com:80");
    }

    // =========================================================================
    // 辅助函数：构造真实 oxcache 并包装为 DnsCacheService
    // =========================================================================

    async fn make_service(ttl_seconds: u64) -> DnsCacheService {
        let cache = Arc::new(
            oxcache::Cache::builder()
                .capacity(100)
                .ttl(Duration::from_secs(ttl_seconds))
                .build()
                .await
                .expect("cache build failed"),
        );
        DnsCacheService::new(cache, ttl_seconds).expect("failed to create DnsCacheService")
    }

    // =========================================================================
    // DnsCacheService::new 与 default_ttl
    // =========================================================================

    #[tokio::test]
    async fn test_new_sets_default_ttl() {
        let svc = make_service(300).await;
        assert_eq!(svc.default_ttl.as_secs(), 300);
    }

    #[tokio::test]
    async fn test_new_with_different_ttl() {
        let svc = make_service(42).await;
        assert_eq!(svc.default_ttl.as_secs(), 42);
    }

    // =========================================================================
    // set + lookup_host 缓存命中（不触发真实 DNS）
    // =========================================================================

    #[tokio::test]
    async fn test_set_then_lookup_host_cache_hit() {
        let svc = make_service(300).await;

        let ips: Vec<IpAddr> = vec!["203.0.113.5".parse().unwrap()];
        svc.set("cache-hit.example", 443, ips.clone(), 300).await;

        // lookup_host 应命中缓存，不调用真实 DNS
        let resolved = svc.lookup_host("cache-hit.example", 443).await;
        assert!(resolved.is_ok(), "lookup_host should hit cache");
        let resolved = resolved.unwrap();
        assert_eq!(resolved, ips);
    }

    #[tokio::test]
    async fn test_set_multiple_entries_then_lookup_host() {
        let svc = make_service(300).await;

        let ips_a: Vec<IpAddr> = vec!["10.0.0.1".parse().unwrap(), "10.0.0.2".parse().unwrap()];
        let ips_b: Vec<IpAddr> = vec!["192.168.2.1".parse().unwrap()];

        svc.set("host-a.example", 80, ips_a.clone(), 300).await;
        svc.set("host-b.example", 8080, ips_b.clone(), 300).await;

        let resolved_a = svc.lookup_host("host-a.example", 80).await.unwrap();
        assert_eq!(resolved_a, ips_a);
        let resolved_b = svc.lookup_host("host-b.example", 8080).await.unwrap();
        assert_eq!(resolved_b, ips_b);
    }

    // =========================================================================
    // set 后通过 cache.get 验证条目存在
    // =========================================================================

    #[tokio::test]
    async fn test_set_writes_entry_to_cache() {
        let svc = make_service(300).await;
        let ips: Vec<IpAddr> = vec!["198.51.100.7".parse().unwrap()];
        svc.set("write-test.example", 53, ips.clone(), 120).await;

        let key = generate_dns_key("write-test.example", 53);
        let got = svc.cache.get(&key).await;
        assert!(got.is_ok(), "cache.get should succeed");
        let entry = got.unwrap();
        assert!(entry.is_some(), "entry should exist");
        let entry = entry.unwrap();
        assert_eq!(entry.ips, ips);
        assert_eq!(entry.remaining_ttl_secs, 120);
    }

    // =========================================================================
    // remove
    // =========================================================================

    #[tokio::test]
    async fn test_remove_deletes_entry() {
        let svc = make_service(300).await;
        let ips: Vec<IpAddr> = vec!["203.0.113.9".parse().unwrap()];
        svc.set("remove-me.example", 80, ips.clone(), 300).await;

        // 确认存在
        let key = generate_dns_key("remove-me.example", 80);
        assert!(svc.cache.get(&key).await.unwrap().is_some());

        // remove 后应不存在
        svc.remove("remove-me.example", 80).await;
        assert!(svc.cache.get(&key).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_remove_nonexistent_does_not_panic() {
        let svc = make_service(300).await;
        // 删除不存在的条目不应 panic
        svc.remove("never-set.example", 9999).await;
    }

    // =========================================================================
    // lookup_host 缓存命中后 remove 再 lookup_host 应走真实 DNS（会失败/Err）
    // 这里只验证 remove 后缓存确实为空，避免真实 DNS 依赖
    // =========================================================================

    #[tokio::test]
    async fn test_lookup_host_after_remove_cache_miss() {
        let svc = make_service(300).await;
        let ips: Vec<IpAddr> = vec!["203.0.113.10".parse().unwrap()];
        svc.set("miss-after-remove.example", 80, ips, 300).await;

        // 先命中缓存
        assert!(svc
            .lookup_host("miss-after-remove.example", 80)
            .await
            .is_ok());

        // remove 后缓存为空
        svc.remove("miss-after-remove.example", 80).await;
        let key = generate_dns_key("miss-after-remove.example", 80);
        assert!(svc.cache.get(&key).await.unwrap().is_none());
    }

    // =========================================================================
    // get_stats
    // =========================================================================

    #[tokio::test]
    async fn test_get_stats_returns_default_ttl() {
        let svc = make_service(256).await;
        let stats = svc.get_stats().await;
        assert_eq!(stats.default_ttl_seconds, 256);
    }

    #[tokio::test]
    async fn test_get_stats_fields_after_operations() {
        let svc = make_service(300).await;

        // 执行若干操作
        let ips: Vec<IpAddr> = vec!["203.0.113.20".parse().unwrap()];
        svc.set("stats.example", 80, ips, 300).await;
        let _ = svc.lookup_host("stats.example", 80).await;

        let stats = svc.get_stats().await;
        // oxcache 不暴露 hit/miss 统计，这些字段固定为 0
        assert_eq!(stats.total_entries, 0);
        assert_eq!(stats.hit_count, 0);
        assert_eq!(stats.miss_count, 0);
        assert_eq!(stats.hit_rate, 0.0);
        assert_eq!(stats.max_entries, 0);
        assert_eq!(stats.default_ttl_seconds, 300);
    }

    // =========================================================================
    // clear
    // =========================================================================

    #[tokio::test]
    async fn test_clear_does_not_panic() {
        let svc = make_service(300).await;
        let ips: Vec<IpAddr> = vec!["203.0.113.30".parse().unwrap()];
        svc.set("clear.example", 80, ips, 300).await;

        // clear 不应 panic（oxcache 不支持 clear，仅记录日志）
        svc.clear().await;
    }

    // =========================================================================
    // DnsCacheStats 字段访问
    // =========================================================================

    #[test]
    fn test_dns_cache_stats_field_access() {
        let stats = DnsCacheStats {
            total_entries: 10,
            hit_count: 7,
            miss_count: 3,
            hit_rate: 0.7,
            max_entries: 100,
            default_ttl_seconds: 300,
        };
        assert_eq!(stats.total_entries, 10);
        assert_eq!(stats.hit_count, 7);
        assert_eq!(stats.miss_count, 3);
        assert!((stats.hit_rate - 0.7).abs() < f64::EPSILON);
        assert_eq!(stats.max_entries, 100);
        assert_eq!(stats.default_ttl_seconds, 300);
    }

    #[test]
    fn test_dns_cache_stats_default_zeroes() {
        // 验证全零字段的合理性
        let stats = DnsCacheStats {
            total_entries: 0,
            hit_count: 0,
            miss_count: 0,
            hit_rate: 0.0,
            max_entries: 0,
            default_ttl_seconds: 0,
        };
        assert_eq!(stats.total_entries, 0);
        assert_eq!(stats.hit_count, 0);
        assert_eq!(stats.miss_count, 0);
        assert_eq!(stats.hit_rate, 0.0);
        assert_eq!(stats.max_entries, 0);
        assert_eq!(stats.default_ttl_seconds, 0);
    }

    // =========================================================================
    // lookup_host 真实 DNS 解析路径（缓存未命中 → DNS 解析 → 存储到缓存）
    // 覆盖行 46, 54-57, 69-70, 73, 76, 84
    // =========================================================================

    #[tokio::test]
    async fn test_lookup_host_cache_miss_performs_real_dns_resolution() {
        let svc = make_service(300).await;
        // localhost 应该解析为 127.0.0.1 或 ::1
        let result = svc.lookup_host("localhost", 80).await;
        assert!(result.is_ok(), "localhost should resolve");
        let ips = result.unwrap();
        assert!(!ips.is_empty(), "should have at least one IP for localhost");
        // 验证解析的 IP 是 loopback 地址
        let has_loopback = ips.iter().any(|ip| ip.is_loopback());
        assert!(
            has_loopback,
            "localhost should resolve to loopback: {:?}",
            ips
        );

        // 第二次调用应命中缓存（不会再次 DNS 解析）
        let result2 = svc.lookup_host("localhost", 80).await;
        assert!(result2.is_ok());
        let ips2 = result2.unwrap();
        assert_eq!(ips2, ips, "cached result should match first resolution");
    }

    #[tokio::test]
    async fn test_lookup_host_cache_miss_then_remove_then_miss_again() {
        let svc = make_service(300).await;
        // 第一次解析（缓存未命中）
        let result1 = svc.lookup_host("localhost", 80).await;
        assert!(result1.is_ok());
        let ips1 = result1.unwrap();
        assert!(!ips1.is_empty());

        // 确认缓存中有条目
        let key = generate_dns_key("localhost", 80);
        assert!(svc.cache.get(&key).await.unwrap().is_some());

        // remove 后缓存为空
        svc.remove("localhost", 80).await;
        assert!(svc.cache.get(&key).await.unwrap().is_none());

        // 再次解析（缓存未命中，走真实 DNS）
        let result2 = svc.lookup_host("localhost", 80).await;
        assert!(result2.is_ok());
        let ips2 = result2.unwrap();
        assert!(!ips2.is_empty());
    }

    // =========================================================================
    // logger 初始化：覆盖 debug! 宏参数行（line 76, 103, 117, 127）
    // =========================================================================

    use crate::test_utils::ensure_debug_logger;

    #[tokio::test]
    async fn test_set_remove_clear_with_logger_covers_debug_macros() {
        ensure_debug_logger();
        let svc = make_service(300).await;

        let ips: Vec<IpAddr> = vec!["203.0.113.10".parse().unwrap()];
        // set → 覆盖 line 117 debug! 宏
        svc.set("logger-test.example", 443, ips.clone(), 60).await;

        // remove → 覆盖 line 127 debug! 宏
        svc.remove("logger-test.example", 443).await;

        // clear → 覆盖 line 103 debug! 宏
        svc.clear().await;
    }

    #[tokio::test]
    async fn test_lookup_host_with_logger_covers_cache_store_debug_macro() {
        ensure_debug_logger();
        let svc = make_service(300).await;
        // localhost 解析（缓存未命中 → DNS 解析 → set 成功 → line 76 debug! 宏）
        let result = svc.lookup_host("localhost", 80).await;
        assert!(result.is_ok(), "localhost should resolve");
        let ips = result.unwrap();
        assert!(!ips.is_empty());
        let has_loopback = ips.iter().any(|ip| ip.is_loopback());
        assert!(has_loopback, "localhost should resolve to loopback");
    }

    // =========================================================================
    // resolve_single_flight：per-key single-flight 行为
    // hickory Resolver 为具体类型不可注入 fake，故按设计把 single-flight 逻辑
    // 抽成独立函数，用计数 fake 缓存/解析器直接测试。
    // =========================================================================

    use std::sync::atomic::{AtomicUsize, Ordering};

    /// single-flight 独立测试用的 fake 缓存：Option<ips>
    type FakeCache = Arc<tokio::sync::Mutex<Option<Vec<IpAddr>>>>;

    fn new_in_flight() -> Arc<DashMap<String, Arc<tokio::sync::Mutex<()>>>> {
        Arc::new(DashMap::new())
    }

    #[tokio::test]
    async fn test_single_flight_ten_concurrent_callers_resolve_once() {
        let in_flight = new_in_flight();
        let cache: FakeCache = Arc::new(tokio::sync::Mutex::new(None));
        let resolve_count = Arc::new(AtomicUsize::new(0));
        let ips = vec![IpAddr::from([203, 0, 113, 77])];

        let mut handles = Vec::new();
        for _ in 0..10 {
            let in_flight = Arc::clone(&in_flight);
            let cache_get = Arc::clone(&cache);
            let cache_store = Arc::clone(&cache);
            let resolve_count = Arc::clone(&resolve_count);
            let expected = ips.clone();
            handles.push(tokio::spawn(async move {
                resolve_single_flight(
                    &in_flight,
                    "dns:fake.example:443",
                    move || async move { cache_get.lock().await.clone() },
                    move || async move {
                        resolve_count.fetch_add(1, Ordering::SeqCst);
                        // 模拟真实解析耗时，制造并发窗口
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        Ok(expected.clone())
                    },
                    move |resolved| async move {
                        *cache_store.lock().await = Some(resolved);
                    },
                )
                .await
            }));
        }

        for handle in handles {
            let got = handle
                .await
                .expect("task join failed")
                .expect("single-flight call should succeed");
            assert_eq!(got, ips, "所有调用者应拿到相同的解析结果");
        }
        assert_eq!(
            resolve_count.load(Ordering::SeqCst),
            1,
            "同 key 并发 10 次查询只应实际解析 1 次"
        );
        assert!(in_flight.is_empty(), "完成后在飞记录应被清理");
    }

    #[tokio::test]
    async fn test_single_flight_waiter_waits_for_lock_then_hits_cache_without_resolve() {
        let in_flight = new_in_flight();
        let cache: FakeCache = Arc::new(tokio::sync::Mutex::new(None));
        let resolve_count = Arc::new(AtomicUsize::new(0));
        let cached_ips = vec![IpAddr::from([198, 51, 100, 42])];

        // 模拟"第一个调用者正在解析"：外部持有该 key 的在飞锁，结果已写入缓存
        let key_lock = {
            let entry = in_flight
                .entry("dns:held.example:80".to_string())
                .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())));
            Arc::clone(entry.value())
        };
        let guard = key_lock.lock().await;
        *cache.lock().await = Some(cached_ips.clone());

        // 第二个调用者：应阻塞在锁上，锁释放后 double-check 命中缓存，不再解析
        let in_flight2 = Arc::clone(&in_flight);
        let cache_get = Arc::clone(&cache);
        let cache_store = Arc::clone(&cache);
        let resolve_count2 = Arc::clone(&resolve_count);
        let waiter = tokio::spawn(async move {
            resolve_single_flight(
                &in_flight2,
                "dns:held.example:80",
                move || async move { cache_get.lock().await.clone() },
                move || async move {
                    resolve_count2.fetch_add(1, Ordering::SeqCst);
                    Ok(vec![IpAddr::from([203, 0, 113, 1])])
                },
                move |resolved| async move {
                    *cache_store.lock().await = Some(resolved);
                },
            )
            .await
        });

        // 锁仍被持有时，第二个调用者必须处于等待状态（未解析）
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            resolve_count.load(Ordering::SeqCst),
            0,
            "锁被持有时第二个调用者必须等待，不得自行解析"
        );

        drop(guard);
        let got = waiter
            .await
            .expect("task join failed")
            .expect("waiter should succeed via cache hit");
        assert_eq!(got, cached_ips, "等待者应拿到第一个调用者写入缓存的结果");
        assert_eq!(
            resolve_count.load(Ordering::SeqCst),
            0,
            "缓存命中后不应再解析"
        );
        assert!(in_flight.is_empty(), "等待者完成后应清理在飞记录");
    }

    #[tokio::test]
    async fn test_single_flight_resolve_failure_cleans_in_flight_and_allows_retry() {
        let in_flight = new_in_flight();
        let cache: FakeCache = Arc::new(tokio::sync::Mutex::new(None));
        let attempts = Arc::new(AtomicUsize::new(0));

        // 第一次调用：解析失败
        let attempts1 = Arc::clone(&attempts);
        let cache_get1 = Arc::clone(&cache);
        let first = resolve_single_flight(
            &in_flight,
            "dns:flaky.example:80",
            move || async move { cache_get1.lock().await.clone() },
            move || async move {
                attempts1.fetch_add(1, Ordering::SeqCst);
                Err(std::io::Error::other("simulated DNS failure"))
            },
            move |_resolved| async move {},
        )
        .await;
        assert!(first.is_err(), "解析失败应显性返回错误");
        assert!(
            in_flight.is_empty(),
            "失败后在飞记录必须清理，否则该 host 被永久串行化"
        );

        // 第二次调用：不被残留在飞记录挡住，可重试并成功
        let attempts2 = Arc::clone(&attempts);
        let cache_get2 = Arc::clone(&cache);
        let cache_store2 = Arc::clone(&cache);
        let second = resolve_single_flight(
            &in_flight,
            "dns:flaky.example:80",
            move || async move { cache_get2.lock().await.clone() },
            move || async move {
                attempts2.fetch_add(1, Ordering::SeqCst);
                Ok(vec![IpAddr::from([192, 0, 2, 9])])
            },
            move |resolved| async move {
                *cache_store2.lock().await = Some(resolved);
            },
        )
        .await;
        assert!(second.is_ok(), "失败清理后应允许重试");
        assert_eq!(attempts.load(Ordering::SeqCst), 2, "重试应实际再次解析");
    }

    #[tokio::test]
    async fn test_single_flight_cache_hit_returns_without_resolving() {
        let in_flight = new_in_flight();
        let cache: FakeCache = Arc::new(tokio::sync::Mutex::new(None));
        let cached_ips = vec![IpAddr::from([192, 0, 2, 55])];
        *cache.lock().await = Some(cached_ips.clone());

        let cache_get = Arc::clone(&cache);
        let got = resolve_single_flight(
            &in_flight,
            "dns:hot.example:80",
            move || async move { cache_get.lock().await.clone() },
            || async move { panic!("缓存命中时绝不应执行解析") },
            move |_resolved| async move {},
        )
        .await
        .expect("cache hit should succeed");
        assert_eq!(got, cached_ips, "应返回缓存内容而非新解析结果");
    }

    #[tokio::test]
    async fn test_lookup_host_concurrent_misses_share_single_resolution() {
        // 公开入口冒烟：hickory Resolver 不可注入，解析次数无法直接计数，
        // 退而验证并发 miss 下全部成功、结果一致、缓存已写入。
        // 解析次数 == 1 的断言见上方 resolve_single_flight 独立测试。
        let svc = Arc::new(make_service(300).await);

        let mut handles = Vec::new();
        for _ in 0..10 {
            let svc = Arc::clone(&svc);
            handles.push(tokio::spawn(async move {
                svc.lookup_host("localhost", 80).await
            }));
        }

        let mut expected: Option<Vec<IpAddr>> = None;
        for handle in handles {
            let ips = handle
                .await
                .expect("task join failed")
                .expect("concurrent lookup should succeed");
            match &expected {
                None => expected = Some(ips),
                Some(prev) => assert_eq!(&ips, prev, "single-flight 下并发结果应一致"),
            }
        }
        assert!(expected.is_some());
        let key = generate_dns_key("localhost", 80);
        assert!(
            svc.cache.get(&key).await.unwrap().is_some(),
            "解析结果应已写入缓存"
        );
    }
}
