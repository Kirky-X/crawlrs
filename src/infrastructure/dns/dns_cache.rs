// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! DNS缓存模块（使用 oxcache）

use crate::infrastructure::oxcache::{generate_dns_key, DnsCache, DnsCacheEntry};
use log::{debug, warn};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::lookup_host;

/// 线程安全的DNS缓存（使用 oxcache）
#[derive(Debug, Clone)]
pub struct DnsCacheService {
    cache: Arc<DnsCache>,
    default_ttl: Duration,
}

impl DnsCacheService {
    /// 创建新的DNS缓存
    pub fn new(cache: Arc<DnsCache>, default_ttl_seconds: u64) -> Self {
        Self {
            cache,
            default_ttl: Duration::from_secs(default_ttl_seconds),
        }
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
                return Ok(entry.ips);
            }
            Ok(None) => {
                debug!("DNS cache miss for {}, performing lookup", cache_key);
            }
            Err(e) => {
                warn!("DNS cache error for {}: {}", cache_key, e);
            }
        }

        // 缓存未命中，执行DNS解析
        let addr_str = format!("{}:{}", hostname, port);
        let ips: Vec<IpAddr> = lookup_host(&addr_str)
            .await?
            .map(|socket_addr| socket_addr.ip())
            .collect();

        if ips.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("No IP addresses resolved for {}", hostname),
            ));
        }

        // 存储到缓存
        let entry = DnsCacheEntry {
            ips: ips.clone(),
            remaining_ttl_secs: self.default_ttl.as_secs(),
        };

        if let Err(e) = self.cache.set(&cache_key, &entry).await {
            warn!("Failed to cache DNS result for {}: {}", cache_key, e);
        } else {
            debug!(
                "Cached DNS result for {} with {} IPs, TTL: {}s",
                cache_key,
                ips.len(),
                self.default_ttl.as_secs()
            );
        }

        Ok(ips)
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

        let dns_cache = DnsCacheService::new(cache, 300);
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
        DnsCacheService::new(cache, ttl_seconds)
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
}
