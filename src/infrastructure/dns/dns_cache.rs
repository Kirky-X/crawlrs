// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! DNS缓存模块（使用 oxcache）

use crate::infrastructure::oxcache::{generate_dns_key, DnsCache, DnsCacheEntry};
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;
use tokio::net::lookup_host;
use log::{debug, warn};

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
}
