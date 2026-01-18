// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! DNS缓存模块
//!
//! 提供线程安全的DNS缓存，支持TTL过期机制
//! 防止DNS查询泄露访问模式

use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::lookup_host;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// DNS解析结果缓存条目
#[derive(Debug, Clone)]
struct DnsCacheEntry {
    /// 解析出的IP地址列表
    ips: Vec<IpAddr>,
    /// 缓存创建时间
    created_at: Instant,
    /// TTL（生存时间）
    ttl: Duration,
}

impl DnsCacheEntry {
    /// 检查缓存是否过期
    fn is_expired(&self) -> bool {
        Instant::now().duration_since(self.created_at) >= self.ttl
    }

    /// 获取剩余有效时间
    fn remaining_ttl(&self) -> Duration {
        let elapsed = Instant::now().duration_since(self.created_at);
        self.ttl.saturating_sub(elapsed)
    }
}

/// DNS缓存配置
#[derive(Debug, Clone)]
pub struct DnsCacheConfig {
    /// 缓存条目最大数量
    pub max_entries: usize,
    /// 默认TTL（秒）
    pub default_ttl_seconds: u64,
    /// 清理间隔（秒）
    pub cleanup_interval_seconds: u64,
}

impl Default for DnsCacheConfig {
    fn default() -> Self {
        Self {
            max_entries: 1000,
            default_ttl_seconds: 300, // 5分钟
            cleanup_interval_seconds: 60,
        }
    }
}

/// 线程安全的DNS缓存
#[derive(Debug, Clone)]
pub struct DnsCache {
    inner: Arc<RwLock<DnsCacheInner>>,
}

#[derive(Debug)]
struct DnsCacheInner {
    /// 缓存存储：hostname -> cache entry
    cache: std::collections::HashMap<String, DnsCacheEntry>,
    /// 配置
    config: DnsCacheConfig,
    /// 缓存命中次数
    hit_count: u64,
    /// 缓存未命中次数
    miss_count: u64,
}

impl DnsCache {
    /// 创建新的DNS缓存
    pub fn new(config: DnsCacheConfig) -> Self {
        Self {
            inner: Arc::new(RwLock::new(DnsCacheInner {
                cache: std::collections::HashMap::with_capacity(config.max_entries),
                config,
                hit_count: 0,
                miss_count: 0,
            })),
        }
    }

    /// 创建默认配置的DNS缓存
    pub fn default() -> Self {
        Self::new(DnsCacheConfig::default())
    }

    /// 解析主机名（使用缓存）
    ///
    /// # Arguments
    ///
    /// * `hostname` - 要解析的主机名
    /// * `port` - 端口号
    ///
    /// # Returns
    ///
    /// 解析出的IP地址列表
    ///
    /// # Errors
    ///
    /// 返回DNS解析错误
    pub async fn lookup_host(
        &self,
        hostname: &str,
        port: u16,
    ) -> Result<Vec<IpAddr>, std::io::Error> {
        let cache_key = format!("{}:{}", hostname, port);

        // 尝试从缓存获取
        {
            let inner = self.inner.read().await;
            if let Some(entry) = inner.cache.get(&cache_key) {
                if !entry.is_expired() {
                    debug!("DNS cache hit for {}", cache_key);
                    inner.hit_count += 1;
                    return Ok(entry.ips.clone());
                }
            }
        }

        // 缓存未命中，执行DNS解析
        debug!("DNS cache miss for {}, performing lookup", cache_key);
        {
            let mut inner = self.inner.write().await;
            inner.miss_count += 1;
        }

        let addr_str = format!("{}:{}", hostname, port);
        let ips: Vec<IpAddr> = lookup_host(&addr_str)
            .await?
            .map(|result| result.map(|socket_addr| socket_addr.ip()))
            .collect();

        if ips.is_empty() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("No IP addresses resolved for {}", hostname),
            ));
        }

        // 存储到缓存
        {
            let mut inner = self.inner.write().await;
            let ttl = Duration::from_secs(inner.config.default_ttl_seconds);

            // 如果缓存已满，清理过期的条目
            if inner.cache.len() >= inner.config.max_entries {
                Self::cleanup_expired(&mut inner);
            }

            inner.cache.insert(
                cache_key.clone(),
                DnsCacheEntry {
                    ips: ips.clone(),
                    created_at: Instant::now(),
                    ttl,
                },
            );
            debug!(
                "Cached DNS result for {} with {} IPs, TTL: {}s",
                cache_key,
                ips.len(),
                ttl.as_secs()
            );
        }

        Ok(ips)
    }

    /// 清理过期的缓存条目
    fn cleanup_expired(inner: &mut DnsCacheInner) {
        let before_count = inner.cache.len();
        inner.cache.retain(|_, entry| !entry.is_expired());
        let removed_count = before_count.saturating_sub(inner.cache.len());

        if removed_count > 0 {
            warn!("Cleaned up {} expired DNS cache entries", removed_count);
        }
    }

    /// 清理过期的缓存条目（异步）
    pub async fn cleanup(&self) {
        let mut inner = self.inner.write().await;
        Self::cleanup_expired(&mut inner);
    }

    /// 获取缓存统计信息
    pub async fn get_stats(&self) -> DnsCacheStats {
        let inner = self.inner.read().await;
        let total_requests = inner.hit_count + inner.miss_count;

        DnsCacheStats {
            total_entries: inner.cache.len(),
            hit_count: inner.hit_count,
            miss_count: inner.miss_count,
            hit_rate: if total_requests > 0 {
                inner.hit_count as f64 / total_requests as f64
            } else {
                0.0
            },
            max_entries: inner.config.max_entries,
            default_ttl_seconds: inner.config.default_ttl_seconds,
        }
    }

    /// 清除所有缓存
    pub async fn clear(&self) {
        let mut inner = self.inner.write().await;
        inner.cache.clear();
        inner.hit_count = 0;
        inner.miss_count = 0;
        debug!("DNS cache cleared");
    }

    /// 手动设置缓存条目（用于预热）
    pub async fn set(&self, hostname: &str, port: u16, ips: Vec<IpAddr>, ttl_seconds: u64) {
        let cache_key = format!("{}:{}", hostname, port);
        let mut inner = self.inner.write().await;

        // 如果缓存已满，清理过期的条目
        if inner.cache.len() >= inner.config.max_entries {
            Self::cleanup_expired(&mut inner);
        }

        inner.cache.insert(
            cache_key,
            DnsCacheEntry {
                ips,
                created_at: Instant::now(),
                ttl: Duration::from_secs(ttl_seconds),
            },
        );
        debug!("Manually set DNS cache for {}", cache_key);
    }

    /// 移除特定主机的缓存
    pub async fn remove(&self, hostname: &str, port: u16) {
        let cache_key = format!("{}:{}", hostname, port);
        let mut inner = self.inner.write().await;
        inner.cache.remove(&cache_key);
        debug!("Removed DNS cache for {}", cache_key);
    }
}

/// DNS缓存统计信息
#[derive(Debug, Clone)]
pub struct DnsCacheStats {
    /// 当前缓存条目数
    pub total_entries: u64,
    /// 缓存命中次数
    pub hit_count: u64,
    /// 缓存未命中次数
    pub miss_count: u64,
    /// 缓存命中率
    pub hit_rate: f64,
    /// 最大条目数
    pub max_entries: usize,
    /// 默认TTL（秒）
    pub default_ttl_seconds: u64,
}

impl std::fmt::Display for DnsCacheStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "DNS Cache Stats: entries={}, hits={}, misses={}, hit_rate={:.2}%, max_entries={}, ttl={}s",
            self.total_entries,
            self.hit_count,
            self.miss_count,
            self.hit_rate * 100.0,
            self.max_entries,
            self.default_ttl_seconds
        )
    }
}

/// DNS缓存管理器
#[derive(Debug)]
pub struct DnsCacheManager {
    cache: DnsCache,
    cleanup_task: Option<tokio::task::JoinHandle<()>>,
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl DnsCacheManager {
    /// 创建新的DNS缓存管理器
    pub fn new(config: DnsCacheConfig) -> Self {
        Self {
            cache: DnsCache::new(config),
            cleanup_task: None,
            shutdown_tx: None,
        }
    }

    /// 启动缓存管理器（后台清理任务）
    pub fn start(&mut self) {
        if self.cleanup_task.is_some() {
            return;
        }

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);

        let cache = self.cache.clone();
        let interval = self
            .cache
            .inner
            .read()
            .await
            .config
            .cleanup_interval_seconds;

        self.cleanup_task = Some(tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval));

            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        cache.cleanup().await;
                    }
                    _ = shutdown_rx => {
                        break;
                    }
                }
            }
        }));

        tracing::info!(
            "DNS cache manager started with cleanup interval: {}s",
            interval
        );
    }

    /// 停止缓存管理器
    pub fn stop(&mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }

        if let Some(task) = self.cleanup_task.take() {
            tokio::spawn(async move {
                task.await.ok();
            });
        }

        tracing::info!("DNS cache manager stopped");
    }

    /// 获取缓存实例
    pub fn cache(&self) -> &DnsCache {
        &self.cache
    }
}

impl Drop for DnsCacheManager {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 提供全局DNS缓存实例
///
/// 使用OnceCell确保线程安全的单例模式
pub fn global_dns_cache() -> &'static DnsCache {
    use once_cell::sync::Lazy;
    static DNS_CACHE: Lazy<DnsCache> = Lazy::new(|| DnsCache::default());
    &DNS_CACHE
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_dns_cache_lookup() {
        let cache = DnsCache::new(DnsCacheConfig {
            max_entries: 100,
            default_ttl_seconds: 60,
            cleanup_interval_seconds: 10,
        });

        // 首次查询（缓存未命中）
        let result1 = cache.lookup_host("example.com", 80).await;
        assert!(result1.is_ok());
        let ips1 = result1.unwrap();
        assert!(!ips1.is_empty());

        // 第二次查询（缓存命中）
        let result2 = cache.lookup_host("example.com", 80).await;
        assert!(result2.is_ok());
        let ips2 = result2.unwrap();
        assert_eq!(ips1, ips2);

        // 验证统计信息
        let stats = cache.get_stats().await;
        assert_eq!(stats.hit_count, 1);
        assert_eq!(stats.miss_count, 1);
        assert_eq!(stats.hit_rate, 0.5);
    }

    #[tokio::test]
    async fn test_dns_cache_expiration() {
        let cache = DnsCache::new(DnsCacheConfig {
            max_entries: 100,
            default_ttl_seconds: 1, // 1秒TTL
            cleanup_interval_seconds: 10,
        });

        // 首次查询
        let result1 = cache.lookup_host("example.com", 80).await;
        assert!(result1.is_ok());

        // 等待过期
        tokio::time::sleep(Duration::from_secs(2)).await;

        // 再次查询（应该重新解析）
        let result2 = cache.lookup_host("example.com", 80).await;
        assert!(result2.is_ok());

        let stats = cache.get_stats().await;
        assert_eq!(stats.hit_count, 0); // 第一次查询是miss（因为是新建的cache）
        assert_eq!(stats.miss_count, 2);
    }

    #[tokio::test]
    async fn test_dns_cache_clear() {
        let cache = DnsCache::new(DnsCacheConfig::default());

        // 填充缓存
        let _ = cache.lookup_host("example.com", 80).await;

        // 清除缓存
        cache.clear().await;

        // 验证缓存已清空
        let stats = cache.get_stats().await;
        assert_eq!(stats.total_entries, 0);
        assert_eq!(stats.hit_count, 0);
        assert_eq!(stats.miss_count, 1);
    }

    #[tokio::test]
    async fn test_dns_cache_manual_set() {
        let cache = DnsCache::new(DnsCacheConfig::default());

        let ips = vec!["127.0.0.1".parse().unwrap()];

        // 手动设置缓存
        cache.set("test.local", 80, ips.clone(), 60).await;

        // 验证缓存命中
        let result = cache.lookup_host("test.local", 80).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ips);

        let stats = cache.get_stats().await;
        assert_eq!(stats.hit_count, 1);
        assert_eq!(stats.miss_count, 0);
    }

    #[tokio::test]
    async fn test_dns_cache_remove() {
        let cache = DnsCache::new(DnsCacheConfig::default());

        // 填充缓存
        let _ = cache.lookup_host("example.com", 80).await;

        // 移除缓存
        cache.remove("example.com", 80).await;

        // 验证缓存未命中
        let result = cache.lookup_host("example.com", 80).await;
        assert!(result.is_ok());

        let stats = cache.get_stats().await;
        assert_eq!(stats.hit_count, 0);
        assert_eq!(stats.miss_count, 2);
    }
}
