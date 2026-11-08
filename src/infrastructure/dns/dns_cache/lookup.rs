// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! `DnsCacheService` 缓存读取与解析写回（自 `lookup_host` 提取）。

use super::{oxcache_error_to_io, DnsCacheService};
use crate::common::metrics_shim::counter;
use crate::infrastructure::oxcache::DnsCacheEntry;
use log::{debug, warn};
use std::net::IpAddr;
use std::time::Instant;

impl DnsCacheService {
    /// 缓存读取：命中返回 IP 列表；未命中或缓存错误返回 `None`（均记原指标口径）。
    ///
    /// 命中/未命中指标口径不变：按调用计数，cache_type=dns。
    pub(super) async fn cache_lookup(&self, cache_key: &String) -> Option<Vec<IpAddr>> {
        match self.cache.get(cache_key).await {
            Ok(Some(entry)) => {
                debug!("DNS cache hit for {}", cache_key);
                // DNS 缓存命中指标
                counter!(
                    "crawlrs_cache_hit_total",
                    "cache_type" => "dns",
                    "result" => "hit"
                )
                .increment(1);
                Some(entry.ips)
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
                None
            }
            Err(e) => {
                warn!("DNS cache error for {}: {}", cache_key, e);
                counter!(
                    "crawlrs_cache_hit_total",
                    "cache_type" => "dns",
                    "result" => "miss"
                )
                .increment(1);
                None
            }
        }
    }

    /// 未命中路径：single-flight 由 oxcache `get_or_with_ttl` 内建接管——同 key 并发
    /// 查询只放行一个真实解析，follower 等 leader 写回后读缓存；解析失败不写
    /// 缓存且错误透传（leader 原始错误经 IoError 变体回转）。
    pub(super) async fn resolve_and_cache(
        &self,
        cache_key: &String,
        hostname: &str,
    ) -> Result<DnsCacheEntry, std::io::Error> {
        let resolver = &self.resolver;
        let default_ttl = self.default_ttl;
        self.cache
            .get_or_with_ttl(cache_key, Some(default_ttl), || async {
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
            .map_err(oxcache_error_to_io)
    }
}
