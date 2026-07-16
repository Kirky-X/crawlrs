// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! ssrf/mod.rs 单元测试
//!
//! 补齐 src/presentation/helpers/ssrf/mod.rs 中未覆盖的行：
//! - SsrfValidator::with_dns_cache() 构造器
//! - SsrfValidator::with_dns_cache_and_config() 构造器
//! - resolve_and_validate_ips 的 DNS cache 分支
//! - DNS rebinding 检测（混合私有/公网 IP）
//! - 私有 IP 通过 DNS cache 拦截
//! - 多 IP 解析的 debug 日志路径
//! - DNS 解析失败路径（cache miss + 真实 DNS 失败）

#![cfg(test)]

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use crawlrs::infrastructure::dns::DnsCacheService;
use crawlrs::presentation::helpers::ssrf::{SsrfConfig, SsrfError, SsrfValidator};

// ============================================================
// 辅助函数
// ============================================================

/// 创建一个 DnsCacheService 用于测试。
///
/// 使用 oxcache::Cache::builder 构建真实缓存，
/// 可以通过 `svc.set(hostname, port, ips, ttl)` 预填充条目。
async fn make_dns_cache_service(ttl_seconds: u64) -> DnsCacheService {
    let cache = Arc::new(
        oxcache::Cache::builder()
            .capacity(100)
            .ttl(Duration::from_secs(ttl_seconds))
            .build()
            .await
            .expect("failed to build oxcache for test"),
    );
    DnsCacheService::new(cache, ttl_seconds)
}

/// 解析 IP 地址字符串为 IpAddr
fn ip(s: &str) -> IpAddr {
    s.parse().expect("invalid IP string in test")
}

// ============================================================
// SsrfValidator::with_dns_cache 构造器测试
// ============================================================

#[tokio::test]
async fn tc_ssrf_validator_with_dns_cache_constructor() {
    let dns_cache = make_dns_cache_service(300).await;
    let validator = SsrfValidator::with_dns_cache(Arc::new(dns_cache));
    // 验证 validator 可以正常使用（不 panic）
    // 使用一个会被静态检查拦截的 URL 来验证 validator 工作正常
    let result = validator.validate("http://localhost").await;
    assert!(result.is_err(), "localhost should be blocked");
}

#[tokio::test]
async fn tc_ssrf_validator_with_dns_cache_and_config_constructor() {
    let dns_cache = make_dns_cache_service(300).await;
    let config = SsrfConfig::new().with_max_url_length(512);
    let validator = SsrfValidator::with_dns_cache_and_config(Arc::new(dns_cache), config);
    // 验证 config 生效：超过 512 字符的 URL 应返回 UrlTooLong
    let long_url = format!("http://example.com/{}", "a".repeat(600));
    let result = validator.validate(&long_url).await;
    assert!(result.is_err(), "URL exceeding config max_url_length should fail");
    match result {
        Err(SsrfError::UrlTooLong { max, .. }) => assert_eq!(max, 512),
        Err(e) => panic!("expected UrlTooLong, got: {:?}", e),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

// ============================================================
// DNS cache 分支：成功路径（公网 IP）
// ============================================================

#[tokio::test]
async fn tc_validate_with_dns_cache_public_ip_succeeds() {
    let dns_cache = make_dns_cache_service(300).await;
    // 预填充缓存：公网 IP
    let test_host = "ssrf-test-public.invalid";
    dns_cache
        .set(test_host, 80, vec![ip("203.0.113.5")], 300)
        .await;

    let validator = SsrfValidator::with_dns_cache(Arc::new(dns_cache));
    let result = validator.validate(&format!("http://{}", test_host)).await;
    assert!(
        result.is_ok(),
        "validate with cached public IP should succeed, got error: {:?}",
        result.err()
    );
    let validated = result.unwrap();
    assert_eq!(validated.resolved_ips.len(), 1);
    assert_eq!(validated.resolved_ips[0], ip("203.0.113.5"));
}

// ============================================================
// DNS cache 分支：私有 IP 拦截
// ============================================================

#[tokio::test]
async fn tc_validate_with_dns_cache_private_ip_blocked() {
    let dns_cache = make_dns_cache_service(300).await;
    // 预填充缓存：私有 IP
    let test_host = "ssrf-test-private.invalid";
    dns_cache
        .set(test_host, 80, vec![ip("10.0.0.1")], 300)
        .await;

    let validator = SsrfValidator::with_dns_cache(Arc::new(dns_cache));
    let result = validator.validate(&format!("http://{}", test_host)).await;
    match result {
        Err(SsrfError::PrivateIpAccess { ip: ip_str }) => {
            assert_eq!(ip_str, "10.0.0.1");
        }
        Err(e) => panic!("expected PrivateIpAccess, got: {:?}", e),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

// ============================================================
// DNS rebinding 检测：混合私有/公网 IP
// ============================================================

#[tokio::test]
async fn tc_validate_dns_rebinding_detected() {
    let dns_cache = make_dns_cache_service(300).await;
    // 预填充缓存：混合私有和公网 IP（DNS rebinding 签名）
    let test_host = "ssrf-test-rebinding.invalid";
    let mixed_ips = vec![ip("10.0.0.1"), ip("8.8.8.8")];
    dns_cache.set(test_host, 80, mixed_ips.clone(), 300).await;

    let validator = SsrfValidator::with_dns_cache(Arc::new(dns_cache));
    let result = validator.validate(&format!("http://{}", test_host)).await;
    match result {
        Err(SsrfError::DnsRebindingDetected { hostname, ips }) => {
            assert_eq!(hostname, test_host);
            assert_eq!(ips.len(), 2);
            assert!(ips.contains(&"10.0.0.1".to_string()));
            assert!(ips.contains(&"8.8.8.8".to_string()));
        }
        Err(e) => panic!("expected DnsRebindingDetected, got: {:?}", e),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

// ============================================================
// DNS cache 分支：多 IP 解析（debug 日志路径）
// ============================================================

#[tokio::test]
async fn tc_validate_with_dns_cache_multiple_public_ips() {
    let dns_cache = make_dns_cache_service(300).await;
    // 预填充缓存：多个公网 IP（触发 debug 日志路径）
    let test_host = "ssrf-test-multi.invalid";
    let multi_ips = vec![ip("203.0.113.5"), ip("203.0.113.6")];
    dns_cache.set(test_host, 80, multi_ips.clone(), 300).await;

    let validator = SsrfValidator::with_dns_cache(Arc::new(dns_cache));
    let result = validator.validate(&format!("http://{}", test_host)).await;
    assert!(
        result.is_ok(),
        "validate with multiple public IPs should succeed, got error: {:?}",
        result.err()
    );
    let validated = result.unwrap();
    assert_eq!(
        validated.resolved_ips.len(),
        2,
        "should resolve to 2 IPs"
    );
}

// ============================================================
// DNS cache 分支：DNS 解析失败（cache miss + 真实 DNS 失败）
// ============================================================

#[tokio::test]
async fn tc_validate_with_dns_cache_unresolvable_host_fails() {
    let dns_cache = make_dns_cache_service(300).await;
    // 不预填充缓存，使用一个不可解析的主机名
    let test_host = "nonexistent-ssrf-test-host-12345.invalid";

    let validator = SsrfValidator::with_dns_cache(Arc::new(dns_cache));
    let result = validator.validate(&format!("http://{}", test_host)).await;
    match result {
        Err(SsrfError::DnsResolutionFailed { hostname, .. }) => {
            assert_eq!(hostname, test_host);
        }
        Err(e) => panic!("expected DnsResolutionFailed, got: {:?}", e),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

// ============================================================
// DNS cache + 自定义配置组合测试
// ============================================================

#[tokio::test]
async fn tc_validate_with_dns_cache_and_config_applies_config() {
    let dns_cache = make_dns_cache_service(300).await;
    // 预填充缓存：公网 IP
    let test_host = "ssrf-test-config.invalid";
    dns_cache
        .set(test_host, 443, vec![ip("203.0.113.10")], 300)
        .await;

    let config = SsrfConfig::new().with_max_url_length(2048);
    let validator = SsrfValidator::with_dns_cache_and_config(Arc::new(dns_cache), config);
    let result = validator
        .validate(&format!("https://{}", test_host))
        .await;
    assert!(
        result.is_ok(),
        "validate with DNS cache and config should succeed for public IP, got error: {:?}",
        result.err()
    );
    let validated = result.unwrap();
    assert_eq!(validated.port, 443);
    assert_eq!(validated.resolved_ips[0], ip("203.0.113.10"));
}

// ============================================================
// 无 DNS cache 的直接 DNS 解析路径（补充覆盖）
// ============================================================

#[tokio::test]
async fn tc_validate_without_dns_cache_unresolvable_host_fails() {
    // 不使用 DNS cache，直接 DNS 解析路径
    let validator = SsrfValidator::new();
    let test_host = "nonexistent-ssrf-direct-test-67890.invalid";
    let result = validator.validate(&format!("http://{}", test_host)).await;
    match result {
        Err(SsrfError::DnsResolutionFailed { hostname, .. }) => {
            assert_eq!(hostname, test_host);
        }
        Err(e) => panic!("expected DnsResolutionFailed, got: {:?}", e),
        Ok(_) => panic!("expected error, got Ok"),
    }
}
