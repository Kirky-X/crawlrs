// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! IPv4-only DNS resolver for reqwest.
//!
//! 部署环境通常无 IPv6 连通性。reqwest 默认使用 getaddrinfo 解析 DNS，
//! 返回 IPv4 + IPv6 地址后优先尝试 IPv6，导致 "Connection refused" 后不
//! 自动 fallback IPv4（例如 people.com.cn 的 AAAA 记录解析到不可达的 IPv6
//! 地址）。本 resolver 用 tokio::net::lookup_host 解析后过滤 IPv6，只返回 IPv4。
//!
//! 性能优化：集成 DnsCacheService，优先查 oxcache 缓存，未命中再走系统 DNS。
//! 避免每次 HTTP 请求都触发系统调用（搜索引擎场景频繁抓取相同域名）。

use std::net::SocketAddr;
use std::sync::Arc;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};

use super::dns_cache::DnsCacheService;

/// IPv4-only DNS resolver.
///
/// 用 tokio::net::lookup_host 解析 DNS，过滤掉所有 IPv6 地址，只返回 IPv4
/// 地址。用于部署环境无 IPv6 连通性时强制 IPv4 连接。
///
/// 集成 DnsCacheService 后，优先查缓存，未命中再走系统 DNS。
/// 缓存不可用时（cache disabled）fallback 到无缓存模式。
#[derive(Debug, Clone)]
pub struct Ipv4OnlyResolver {
    dns_cache: Option<Arc<DnsCacheService>>,
}

impl Default for Ipv4OnlyResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl Ipv4OnlyResolver {
    /// 创建无缓存的 IPv4-only resolver（向后兼容，用于测试或 cache disabled 场景）.
    pub fn new() -> Self {
        Self { dns_cache: None }
    }

    /// 创建带 DNS 缓存的 IPv4-only resolver（用于生产环境）.
    pub fn with_cache(dns_cache: Arc<DnsCacheService>) -> Self {
        Self {
            dns_cache: Some(dns_cache),
        }
    }
}

impl Resolve for Ipv4OnlyResolver {
    fn resolve(&self, name: Name) -> Resolving {
        let host = name.as_str().to_string();
        let cache = self.dns_cache.clone();
        Box::pin(async move {
            // 如果有缓存，优先查缓存（避免每次请求都走系统 DNS 调用）
            if let Some(cache) = cache {
                match cache.lookup_host(&host, 0).await {
                    Ok(ips) => {
                        // 过滤 IPv6，只保留 IPv4
                        let ipv4_addrs: Vec<SocketAddr> = ips
                            .into_iter()
                            .filter(|ip| ip.is_ipv4())
                            .map(|ip| SocketAddr::new(ip, 0))
                            .collect();
                        log::debug!(
                            "Ipv4OnlyResolver resolved {} -> {} IPv4 addrs (cached)",
                            host,
                            ipv4_addrs.len()
                        );
                        return Ok(Box::new(ipv4_addrs.into_iter()) as Addrs);
                    }
                    Err(e) => {
                        // 缓存查询失败（如 DNS 解析失败）时 fallback 到系统 DNS
                        log::warn!(
                            "DNS cache lookup failed for {}: {}, falling back to system DNS",
                            host,
                            e
                        );
                    }
                }
            }

            // 无缓存或缓存失败，fallback 到系统 DNS
            // 用 port 0 解析，reqwest 会根据 scheme 替换为正确端口
            let lookup = format!("{}:0", host);
            let resolved = tokio::net::lookup_host(lookup)
                .await
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;
            // 过滤掉 IPv6 地址，只保留 IPv4
            let ipv4_addrs: Vec<SocketAddr> = resolved.filter(|a| a.is_ipv4()).collect();
            // 安全：不输出 IP 地址本身，只输出数量（防止 debug 日志泄露内网 DNS 解析结果）
            log::debug!(
                "Ipv4OnlyResolver resolved {} -> {} IPv4 addrs",
                host,
                ipv4_addrs.len()
            );
            Ok(Box::new(ipv4_addrs.into_iter()) as Addrs)
        })
    }
}

/// 创建 Arc 包装的无缓存 IPv4-only resolver（向后兼容，用于测试）.
pub fn create_ipv4_only_resolver() -> Arc<dyn Resolve> {
    Arc::new(Ipv4OnlyResolver::new())
}

/// 创建 Arc 包装的带缓存 IPv4-only resolver（用于生产环境）.
pub fn create_ipv4_only_resolver_with_cache(dns_cache: Arc<DnsCacheService>) -> Arc<dyn Resolve> {
    Arc::new(Ipv4OnlyResolver::with_cache(dns_cache))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr, SocketAddrV4, SocketAddrV6};
    use std::str::FromStr;

    #[test]
    fn test_ipv4_only_resolver_creation() {
        let _resolver = Ipv4OnlyResolver::new();
        let _arc = create_ipv4_only_resolver();
    }

    #[test]
    fn test_ipv4_only_resolver_with_cache_creation() {
        // 验证 Ipv4OnlyResolver::with_cache 和 create_ipv4_only_resolver_with_cache
        // 都接受 Arc<DnsCacheService>（生产环境的标准签名）。
        let rt = tokio::runtime::Runtime::new().unwrap();
        let dns_cache: Arc<DnsCacheService> = rt.block_on(async {
            let cache = Arc::new(
                oxcache::Cache::builder()
                    .capacity(100)
                    .ttl(std::time::Duration::from_secs(300))
                    .build()
                    .await
                    .unwrap(),
            );
            Arc::new(DnsCacheService::new(cache, 300))
        });
        // 验证 Ipv4OnlyResolver::with_cache 接受 Arc<DnsCacheService>
        let _resolver = Ipv4OnlyResolver::with_cache(dns_cache.clone());
        // 验证 create_ipv4_only_resolver_with_cache 函数签名正确
        let _arc = create_ipv4_only_resolver_with_cache(dns_cache);
    }

    #[test]
    fn test_filter_ipv6_addresses() {
        let addrs: Vec<SocketAddr> = vec![
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 80)),
            SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, 80, 0, 0)),
            SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(1, 2, 3, 4), 443)),
        ];
        let ipv4_only: Vec<SocketAddr> = addrs.into_iter().filter(|a| a.is_ipv4()).collect();
        assert_eq!(ipv4_only.len(), 2);
        assert!(ipv4_only.iter().all(|a| a.is_ipv4()));
    }

    #[tokio::test]
    async fn test_resolve_ipv4_only_without_cache() {
        let resolver = Ipv4OnlyResolver::new();
        let name = Name::from_str("example.com").unwrap();
        let result = resolver.resolve(name).await;
        if let Ok(addrs) = result {
            let collected: Vec<_> = addrs.collect();
            if !collected.is_empty() {
                assert!(
                    collected.iter().all(|a| a.is_ipv4()),
                    "All addresses should be IPv4, got: {:?}",
                    collected
                );
            }
        }
    }

    #[tokio::test]
    async fn test_resolve_ipv4_only_with_cache() {
        // 创建带缓存的 resolver，缓存预置一个 IPv4 地址
        let cache = Arc::new(
            oxcache::Cache::builder()
                .capacity(100)
                .ttl(std::time::Duration::from_secs(300))
                .build()
                .await
                .unwrap(),
        );
        let dns_cache = Arc::new(DnsCacheService::new(cache, 300));

        // 预置缓存：example.com -> 203.0.113.5
        let test_ip: std::net::IpAddr = "203.0.113.5".parse().unwrap();
        dns_cache.set("example.com", 0, vec![test_ip], 300).await;

        let resolver = Ipv4OnlyResolver::with_cache(dns_cache);
        let name = Name::from_str("example.com").unwrap();
        let result = resolver.resolve(name).await;
        assert!(result.is_ok(), "resolve should succeed via cache");
        let addrs: Vec<_> = result.unwrap().collect();
        assert_eq!(addrs.len(), 1);
        assert!(addrs[0].is_ipv4());
        assert_eq!(addrs[0].ip(), test_ip);
    }

    #[tokio::test]
    async fn test_resolve_falls_back_to_system_dns_on_cache_miss() {
        // 缓存未命中时应该 fallback 到系统 DNS
        let cache = Arc::new(
            oxcache::Cache::builder()
                .capacity(100)
                .ttl(std::time::Duration::from_secs(300))
                .build()
                .await
                .unwrap(),
        );
        let dns_cache = Arc::new(DnsCacheService::new(cache, 300));

        let resolver = Ipv4OnlyResolver::with_cache(dns_cache);
        // localhost 不在缓存中，应该 fallback 到系统 DNS
        let name = Name::from_str("localhost").unwrap();
        let result = resolver.resolve(name).await;
        if let Ok(addrs) = result {
            let collected: Vec<_> = addrs.collect();
            if !collected.is_empty() {
                assert!(
                    collected.iter().all(|a| a.is_ipv4()),
                    "All addresses should be IPv4, got: {:?}",
                    collected
                );
            }
        }
    }
}
