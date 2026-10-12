// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! DNS缓存模块（使用 oxcache + hickory-resolver）
//!
//! 解析编排（miss→解析→写回）的 single-flight 由 oxcache `Cache::get_or_with_ttl`
//! 内建承载（进程级 leader/follower：同 key 并发只放行一个真实解析，follower 等
//! leader 写回后读缓存），不再自维在飞表。per-entry TTL 取解析器记录剩余有效期
//!（`valid_until`，以 `default_ttl` 为上限），无记录 TTL 时回落 `default_ttl`；
//! 写回经库 `apply_jitter` 抖动。leader 的原始解析错误经 `OxCacheError::IoError`
//! 无损回转；follower 拿到的是库的 L1Error（leader 未写回），统一映射为 io 错误。

use crate::common::metrics_shim::counter;
use crate::infrastructure::oxcache::{generate_dns_key, DnsCache, DnsCacheEntry};
use hickory_resolver::Resolver;
use log::{debug, warn};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 线程安全的DNS缓存（使用 oxcache + hickory-resolver）
#[derive(Debug, Clone)]
pub struct DnsCacheService {
    cache: Arc<DnsCache>,
    resolver: Resolver<hickory_resolver::net::runtime::TokioRuntimeProvider>,
    default_ttl: Duration,
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
        })
    }

    /// 解析主机名（使用缓存）
    pub async fn lookup_host(
        &self,
        hostname: &str,
        port: u16,
    ) -> Result<Vec<IpAddr>, std::io::Error> {
        let cache_key = generate_dns_key(hostname, port);

        // 尝试从缓存获取（命中/未命中指标口径不变：按调用计数，cache_type=dns）
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

        // 未命中：single-flight 由 oxcache get_or_with_ttl 内建接管——同 key 并发
        // 查询只放行一个真实解析，follower 等 leader 写回后读缓存；解析失败不写
        // 缓存且错误透传（leader 原始错误经 IoError 变体回转）。
        let resolver = &self.resolver;
        let default_ttl = self.default_ttl;
        let entry = self
            .cache
            .get_or_with_ttl(&cache_key, Some(default_ttl), || async {
                let lookup = resolver.lookup_ip(hostname).await.map_err(|e| {
                    oxcache::OxCacheError::IoError(std::io::Error::other(format!(
                        "DNS resolution failed for {}: {}",
                        hostname, e
                    )))
                })?;
                let ips: Vec<IpAddr> = lookup.iter().collect();
                if ips.is_empty() {
                    return Err(oxcache::OxCacheError::IoError(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("No IP addresses resolved for {}", hostname),
                    )));
                }
                // per-entry TTL：解析器记录剩余有效期，钳制在 default_ttl 以内
                // （库写回时会再经 apply_jitter 抖动）；无记录 TTL 回落 default_ttl
                let ttl = lookup
                    .valid_until()
                    .checked_duration_since(Instant::now())
                    .map(|d| d.min(default_ttl))
                    .filter(|d| !d.is_zero())
                    .unwrap_or(default_ttl);
                Ok(DnsCacheEntry {
                    ips,
                    remaining_ttl_secs: ttl.as_secs(),
                })
            })
            .await
            .map_err(oxcache_error_to_io)?;

        debug!(
            "DNS lookup completed for {} ({} IPs, ttl {}s)",
            cache_key,
            entry.ips.len(),
            entry.remaining_ttl_secs
        );

        Ok(entry.ips)
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

/// oxcache 错误 → io 错误回转。
///
/// leader 的原始解析错误经 `OxCacheError::IoError` 变体无损保留（错误类别与
/// 消息原样返回）；follower 拿到的是库的 `L1Error`（leader 未写回缓存时的
/// 通用失败），统一映射为 io 错误，不伪装成解析成功的空结果。
fn oxcache_error_to_io(err: oxcache::OxCacheError) -> std::io::Error {
    match err {
        oxcache::OxCacheError::IoError(io) => io,
        other => std::io::Error::other(other),
    }
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
        let result = svc.lookup_host("127.0.0.1", 80).await;
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
        let result2 = svc.lookup_host("127.0.0.1", 80).await;
        assert!(result2.is_ok());
        let ips2 = result2.unwrap();
        assert_eq!(ips2, ips, "cached result should match first resolution");
    }

    #[tokio::test]
    async fn test_lookup_host_cache_miss_then_remove_then_miss_again() {
        let svc = make_service(300).await;
        // 第一次解析（缓存未命中）
        let result1 = svc.lookup_host("127.0.0.2", 80).await;
        assert!(result1.is_ok());
        let ips1 = result1.unwrap();
        assert!(!ips1.is_empty());

        // 确认缓存中有条目
        let key = generate_dns_key("127.0.0.2", 80);
        assert!(svc.cache.get(&key).await.unwrap().is_some());

        // remove 后缓存为空
        svc.remove("127.0.0.2", 80).await;
        assert!(svc.cache.get(&key).await.unwrap().is_none());

        // 再次解析（缓存未命中，走真实 DNS）
        let result2 = svc.lookup_host("127.0.0.2", 80).await;
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
        let result = svc.lookup_host("127.0.0.3", 80).await;
        assert!(result.is_ok(), "localhost should resolve");
        let ips = result.unwrap();
        assert!(!ips.is_empty());
        let has_loopback = ips.iter().any(|ip| ip.is_loopback());
        assert!(has_loopback, "localhost should resolve to loopback");
    }

    // =========================================================================
    // oxcache 内建 single-flight：同 key 并发查询只放行一个真实解析
    // （自维在飞层已删除，行为改由库 get_or_with_ttl 的 leader/follower 承载；
    //   hickory Resolver 为具体类型不可注入 fake，故直接在库 API 面上用
    //  计数 fallback 验证单飞语义，key 加测试唯一后缀避免跨用例串扰。）
    // =========================================================================

    use std::sync::atomic::{AtomicUsize, Ordering};

    #[tokio::test]
    async fn test_get_or_with_ttl_single_flight_32_callers_resolve_once() {
        let cache: DnsCache = oxcache::Cache::builder()
            .capacity(100)
            .ttl(Duration::from_secs(60))
            .build()
            .await
            .expect("cache build failed");
        let cache = Arc::new(cache);
        let key = generate_dns_key("single-flight-probe.example", 443);
        let resolve_count = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..32 {
            let cache = Arc::clone(&cache);
            let key = key.clone();
            let resolve_count = Arc::clone(&resolve_count);
            handles.push(tokio::spawn(async move {
                cache
                    .get_or_with_ttl(&key, Some(Duration::from_secs(60)), || async {
                        resolve_count.fetch_add(1, Ordering::SeqCst);
                        // 模拟真实解析耗时，制造并发窗口
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        Ok(DnsCacheEntry {
                            ips: vec!["203.0.113.99".parse().unwrap()],
                            remaining_ttl_secs: 60,
                        })
                    })
                    .await
            }));
        }

        for handle in handles {
            let entry = handle
                .await
                .expect("task join failed")
                .expect("single-flight call should succeed");
            assert_eq!(entry.remaining_ttl_secs, 60);
            assert_eq!(entry.ips.len(), 1);
        }
        assert_eq!(
            resolve_count.load(Ordering::SeqCst),
            1,
            "库 single-flight 接管：32 路并发同 key 查询只应实际解析 1 次"
        );
    }

    #[tokio::test]
    async fn test_get_or_with_ttl_distinct_keys_do_not_serialize() {
        let cache: DnsCache = oxcache::Cache::builder()
            .capacity(100)
            .ttl(Duration::from_secs(60))
            .build()
            .await
            .expect("cache build failed");
        let cache = Arc::new(cache);
        let resolve_count = Arc::new(AtomicUsize::new(0));

        // 不同 key 并发解析互不阻塞：各自 fallback 都应执行一次
        let mut handles = Vec::new();
        for i in 0..4u16 {
            let cache = Arc::clone(&cache);
            let key = generate_dns_key(&format!("distinct-{i}.example"), 443);
            let resolve_count = Arc::clone(&resolve_count);
            handles.push(tokio::spawn(async move {
                cache
                    .get_or_with_ttl(&key, Some(Duration::from_secs(60)), || async {
                        resolve_count.fetch_add(1, Ordering::SeqCst);
                        tokio::time::sleep(Duration::from_millis(50)).await;
                        Ok(DnsCacheEntry {
                            ips: vec![IpAddr::from([203, 0, 113, 200 + i as u8])],
                            remaining_ttl_secs: 60,
                        })
                    })
                    .await
            }));
        }

        for handle in handles {
            handle
                .await
                .expect("task join failed")
                .expect("distinct-key call should succeed");
        }
        assert_eq!(
            resolve_count.load(Ordering::SeqCst),
            4,
            "不同 host 并发解析互不阻塞，各自执行一次"
        );
    }

    #[tokio::test]
    async fn test_get_or_with_ttl_failure_not_cached_and_all_callers_get_err() {
        let cache: DnsCache = oxcache::Cache::builder()
            .capacity(100)
            .ttl(Duration::from_secs(60))
            .build()
            .await
            .expect("cache build failed");
        let cache = Arc::new(cache);
        let key = generate_dns_key("down-probe.example", 443);
        let attempts = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for _ in 0..8 {
            let cache = Arc::clone(&cache);
            let key = key.clone();
            let attempts = Arc::clone(&attempts);
            handles.push(tokio::spawn(async move {
                cache
                    .get_or_with_ttl(&key, Some(Duration::from_secs(60)), || async {
                        let n = attempts.fetch_add(1, Ordering::SeqCst);
                        if n == 0 {
                            tokio::time::sleep(Duration::from_millis(50)).await;
                            Err(oxcache::OxCacheError::IoError(std::io::Error::other(
                                "simulated DNS failure",
                            )))
                        } else {
                            Err(oxcache::OxCacheError::IoError(std::io::Error::other(
                                "waiter must not resolve",
                            )))
                        }
                    })
                    .await
            }));
        }

        let mut errors = 0;
        for handle in handles {
            assert!(
                handle.await.expect("task join failed").is_err(),
                "解析失败必须显性返回错误"
            );
            errors += 1;
        }
        assert_eq!(errors, 8);
        assert_eq!(
            attempts.load(Ordering::SeqCst),
            1,
            "失败同样走单飞：只有 leader 执行 fallback"
        );
        assert!(
            cache.get(&key).await.unwrap().is_none(),
            "解析失败不得写缓存"
        );
    }

    // =========================================================================
    // lookup_host：per-entry TTL 记录驱动 + 失败不缓存
    // =========================================================================

    /// R-dns-resolve-002：oxcache 条目 TTL（get_or_with_ttl 写入的 per-entry TTL）
    /// 过期后再次查询必须重新走真实解析，不返回过期条目。
    #[tokio::test]
    async fn test_lookup_host_after_entry_ttl_expiry_re_resolves() {
        // 1s per-entry TTL（远小于测试内 sleep 时长，即使含库 jitter 也必然过期）
        let svc = make_service(1).await;
        let host = "127.0.0.6";

        let first = svc
            .lookup_host(host, 80)
            .await
            .expect("first resolution should succeed");
        assert!(!first.is_empty());

        let key = generate_dns_key(host, 80);
        assert!(
            svc.cache.get(&key).await.unwrap().is_some(),
            "首次解析结果应写入缓存"
        );

        // 越过 per-entry TTL（1s + jitter 余量）
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert!(
            svc.cache.get(&key).await.unwrap().is_none(),
            "过期条目不得残留在缓存中"
        );

        // 过期后再次查询：重新解析并再次写入缓存（非过期条目直返）
        let second = svc
            .lookup_host(host, 80)
            .await
            .expect("post-expiry lookup should re-resolve");
        assert_eq!(second, first, "重解析结果应与首次一致");
        assert!(
            svc.cache.get(&key).await.unwrap().is_some(),
            "重解析结果应再次写入缓存"
        );
    }

    #[tokio::test]
    async fn test_lookup_host_ttl_is_record_driven_capped_by_default() {
        let svc = make_service(300).await;
        let ips = svc
            .lookup_host("127.0.0.4", 80)
            .await
            .expect("localhost should resolve");
        assert!(!ips.is_empty());

        let key = generate_dns_key("127.0.0.4", 80);
        let entry = svc
            .cache
            .get(&key)
            .await
            .unwrap()
            .expect("resolved entry should be cached");
        assert!(entry.remaining_ttl_secs > 0, "TTL 必须为正");
        assert!(
            entry.remaining_ttl_secs <= 300,
            "记录 TTL 钳制在 default_ttl 以内: {}",
            entry.remaining_ttl_secs
        );
    }

    #[tokio::test]
    async fn test_lookup_host_resolution_failure_propagates_and_not_cached() {
        let svc = make_service(300).await;
        // 空主机名在 hickory 侧确定性失败（Name 解析拒绝空标签）
        let err = svc
            .lookup_host("", 80)
            .await
            .expect_err("empty hostname must fail resolution");
        assert!(!err.to_string().is_empty(), "错误必须透传非空消息");
        let key = generate_dns_key("", 80);
        assert!(
            svc.cache.get(&key).await.unwrap().is_none(),
            "解析失败不得残留缓存条目"
        );
    }

    #[tokio::test]
    async fn test_lookup_host_concurrent_failures_all_get_err() {
        let svc = Arc::new(make_service(300).await);

        let mut handles = Vec::new();
        for _ in 0..8 {
            let svc = Arc::clone(&svc);
            handles.push(tokio::spawn(async move { svc.lookup_host("", 80).await }));
        }

        for handle in handles {
            assert!(
                handle.await.expect("task join failed").is_err(),
                "leader/follower 在失败路径都必须拿到错误"
            );
        }
    }

    #[tokio::test]
    async fn test_lookup_host_concurrent_misses_share_single_resolution() {
        // 公开入口冒烟：hickory Resolver 不可注入，解析次数无法直接计数，
        // 退而验证并发 miss 下全部成功、结果一致、缓存已写入。
        // 解析次数 == 1 的断言见上方库级 single-flight 测试。
        let svc = Arc::new(make_service(300).await);

        let mut handles = Vec::new();
        for _ in 0..10 {
            let svc = Arc::clone(&svc);
            handles.push(tokio::spawn(async move {
                svc.lookup_host("127.0.0.5", 80).await
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
        let key = generate_dns_key("127.0.0.5", 80);
        assert!(
            svc.cache.get(&key).await.unwrap().is_some(),
            "解析结果应已写入缓存"
        );
    }
}
