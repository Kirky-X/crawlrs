// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Enhanced URL validator with DNS caching and SSRF protection
//!
//! This module provides an enhanced validator that uses DNS caching
//! for improved performance while maintaining full SSRF protection.
//!
//! ## Architecture
//!
//! This module wraps the unified SSRF protection module with DNS caching
//! capabilities for scenarios where repeated validation of the same
//! hostnames is expected.

use crate::infrastructure::dns::DnsCacheService;
use crate::presentation::helpers::ssrf::{
    is_internal_url, validate_domain_blacklist, SsrfConfig, SsrfError, SsrfValidator, ValidatedUrl,
};
use std::sync::Arc;

/// Enhanced URL validator with DNS caching support.
///
/// This validator provides the same SSRF protection as `SsrfValidator`
/// but with DNS caching for improved performance.
#[derive(Clone)]
pub struct ValidatedUrlValidator {
    inner: SsrfValidator,
}

impl ValidatedUrlValidator {
    /// Create a new validator with DNS caching.
    pub fn new(dns_cache: Arc<DnsCacheService>) -> Self {
        Self {
            inner: SsrfValidator::with_dns_cache(dns_cache),
        }
    }

    /// Create a validator with DNS cache and custom configuration.
    pub fn with_config(dns_cache: Arc<DnsCacheService>, config: SsrfConfig) -> Self {
        Self {
            inner: SsrfValidator::with_dns_cache_and_config(dns_cache, config),
        }
    }

    /// Validate URL for SSRF protection using DNS cache.
    ///
    /// This method performs:
    /// 1. Static URL validation (scheme, hostname patterns)
    /// 2. DNS resolution with caching
    /// 3. IP address validation
    /// 4. DNS rebinding detection
    ///
    /// # Arguments
    ///
    /// * `url_str` - The URL string to validate
    ///
    /// # Returns
    ///
    /// * `Ok(ValidatedUrl)` - The validated URL with resolved IPs
    /// * `Err(SsrfError)` - Validation failed
    pub async fn validate_url(&self, url_str: &str) -> Result<ValidatedUrl, SsrfError> {
        self.inner.validate(url_str).await
    }

    /// Quick synchronous check if URL is internal.
    pub fn is_internal_url(&self, url_str: &str) -> bool {
        is_internal_url(url_str)
    }

    /// Validate domain against blacklist.
    pub fn validate_domain_blacklist(
        &self,
        url_str: &str,
        blacklist: &[String],
    ) -> Result<(), SsrfError> {
        validate_domain_blacklist(url_str, blacklist)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    async fn create_test_dns_cache() -> Arc<DnsCacheService> {
        let cache = Arc::new(
            oxcache::Cache::builder()
                .capacity(100)
                .ttl(Duration::from_secs(300))
                .build()
                .await
                .expect("failed to build oxcache for test"),
        );
        Arc::new(DnsCacheService::new(cache, 300))
    }

    #[tokio::test]
    async fn test_validated_url_validator() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);

        // Localhost variants should be blocked
        assert!(validator.validate_url("http://localhost").await.is_err());
        assert!(validator.validate_url("http://127.0.0.1").await.is_err());

        // Private IPs should be blocked
        assert!(validator.validate_url("http://10.0.0.1").await.is_err());
        assert!(validator.validate_url("http://192.168.1.1").await.is_err());

        // Cloud metadata endpoints should be blocked
        assert!(validator
            .validate_url("http://169.254.169.254")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_is_internal_url() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);

        assert!(validator.is_internal_url("http://localhost"));
        assert!(validator.is_internal_url("http://192.168.1.1"));
        assert!(!validator.is_internal_url("http://google.com"));
    }

    #[tokio::test]
    async fn test_validate_domain_blacklist() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);

        let blacklist = vec!["example.com".to_string()];
        assert!(validator
            .validate_domain_blacklist("http://example.com", &blacklist)
            .is_err());
        assert!(validator
            .validate_domain_blacklist("http://google.com", &blacklist)
            .is_ok());
    }

    // ========== 构造测试 ==========

    #[tokio::test]
    async fn test_new_creates_validator_with_dns_cache() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        // 内部状态不可直接访问，通过行为验证：能正常处理 URL（即使是错的也能返回错误）
        let result = validator.validate_url("http://localhost").await;
        assert!(result.is_err(), "localhost should be blocked");
    }

    #[tokio::test]
    async fn test_with_config_creates_validator_with_custom_max_url_length() {
        let cache = create_test_dns_cache().await;
        let config = SsrfConfig::new().with_max_url_length(32);
        let validator = ValidatedUrlValidator::with_config(cache, config);
        // 短 URL 不超长（但因 host 被阻断）
        assert!(validator.validate_url("http://localhost").await.is_err());
        // 长 URL 应被 UrlTooLong 拦截
        let long_url = format!("http://example.com/{}", "a".repeat(64));
        let result = validator.validate_url(&long_url).await;
        assert!(matches!(result, Err(SsrfError::UrlTooLong { .. })));
    }

    #[test]
    fn test_validator_is_clone() {
        // ValidatedUrlValidator 派生 Clone；此处仅验证类型可克隆（编译时检查）。
        // 由于 SsrfValidator 也 Clone，且 DnsCache 是 Arc，Clone 是廉价的引用计数递增。
        // 该测试若类型未派生 Clone 会编译失败。
        fn assert_clone<T: Clone>() {}
        assert_clone::<ValidatedUrlValidator>();
    }

    // ========== validate_url: 静态拦截路径（无需 DNS） ==========

    #[tokio::test]
    async fn test_validate_url_blocks_invalid_scheme() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        // 非 http/https scheme 应在 scheme 校验阶段被拦截
        assert!(validator.validate_url("file:///etc/passwd").await.is_err());
        assert!(validator.validate_url("ftp://example.com").await.is_err());
        assert!(validator.validate_url("gopher://localhost").await.is_err());
    }

    #[tokio::test]
    async fn test_validate_url_blocks_invalid_url_format() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        // 无法解析的 URL 应返回 InvalidUrl
        let result = validator.validate_url("http://[invalid").await;
        assert!(result.is_err());
        assert!(matches!(result, Err(SsrfError::InvalidUrl { .. })));
    }

    #[tokio::test]
    async fn test_validate_url_blocks_url_too_long() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        let long_url = format!("http://example.com/{}", "a".repeat(3000));
        let result = validator.validate_url(&long_url).await;
        assert!(matches!(result, Err(SsrfError::UrlTooLong { .. })));
    }

    #[tokio::test]
    async fn test_validate_url_blocks_ipv6_loopback() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        // IPv6 loopback 应被静态拦截
        assert!(validator.validate_url("http://[::1]").await.is_err());
        assert!(validator.validate_url("http://[::1]:8080").await.is_err());
    }

    #[tokio::test]
    async fn test_validate_url_blocks_ipv6_unique_local() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        // IPv6 ULA 应被静态拦截
        assert!(validator.validate_url("http://[fc00::1]").await.is_err());
        assert!(validator.validate_url("http://[fd00::1]").await.is_err());
    }

    #[tokio::test]
    async fn test_validate_url_blocks_carrier_grade_nat() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        // CGNAT 范围 100.64.0.0/10 应被静态拦截
        assert!(validator.validate_url("http://100.64.0.1").await.is_err());
        assert!(validator
            .validate_url("http://100.127.255.255")
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_validate_url_blocks_broadcast_and_unspecified() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        assert!(validator.validate_url("http://0.0.0.0").await.is_err());
        assert!(validator
            .validate_url("http://255.255.255.255")
            .await
            .is_err());
    }

    // ========== is_internal_url: 同步静态检查 ==========
    // 注意：is_internal_url 和 validate_domain_blacklist 不依赖 self.inner，
    // 但仍需通过 ValidatedUrlValidator 实例调用，因此使用 tokio 测试创建实例。

    #[tokio::test]
    async fn test_is_internal_url_blocks_non_http_schemes() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        // 非 http/https scheme 应返回 true（视为内部）
        assert!(validator.is_internal_url("file:///etc/passwd"));
        assert!(validator.is_internal_url("ftp://example.com"));
        assert!(validator.is_internal_url("gopher://localhost"));
        assert!(validator.is_internal_url("javascript:alert(1)"));
    }

    #[tokio::test]
    async fn test_is_internal_url_returns_false_for_external_hosts() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        assert!(!validator.is_internal_url("http://google.com"));
        assert!(!validator.is_internal_url("https://www.example.com"));
        assert!(!validator.is_internal_url("http://8.8.8.8"));
        assert!(!validator.is_internal_url("http://1.1.1.1"));
    }

    #[tokio::test]
    async fn test_is_internal_url_blocks_private_ipv4_ranges() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        // 10.0.0.0/8
        assert!(validator.is_internal_url("http://10.0.0.1"));
        assert!(validator.is_internal_url("http://10.255.255.255"));
        // 172.16.0.0/12
        assert!(validator.is_internal_url("http://172.16.0.1"));
        assert!(validator.is_internal_url("http://172.31.255.255"));
        // 192.168.0.0/16
        assert!(validator.is_internal_url("http://192.168.0.1"));
        assert!(validator.is_internal_url("http://192.168.100.200"));
        // 边界外不应匹配
        assert!(!validator.is_internal_url("http://172.15.0.1"));
        assert!(!validator.is_internal_url("http://172.32.0.1"));
    }

    #[tokio::test]
    async fn test_is_internal_url_blocks_ipv6_ranges() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        // IPv6 loopback
        assert!(validator.is_internal_url("http://[::1]"));
        assert!(validator.is_internal_url("http://[::1]:8080"));
        // IPv6 link-local
        assert!(validator.is_internal_url("http://[fe80::1]"));
        assert!(validator.is_internal_url("http://[febf::1]"));
        // IPv6 ULA
        assert!(validator.is_internal_url("http://[fc00::1]"));
        assert!(validator.is_internal_url("http://[fd00::1]"));
    }

    #[tokio::test]
    async fn test_is_internal_url_handles_invalid_urls() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        // 无效 URL 解析失败时返回 false
        assert!(!validator.is_internal_url("not-a-url"));
        assert!(!validator.is_internal_url(""));
        assert!(!validator.is_internal_url("http://"));
    }

    #[tokio::test]
    async fn test_is_internal_url_blocks_ipv4_mapped_ipv6() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        assert!(validator.is_internal_url("http://[::ffff:192.168.1.1]"));
        assert!(validator.is_internal_url("http://[::FFFF:10.0.0.1]"));
    }

    // ========== validate_domain_blacklist: 边界场景 ==========

    #[tokio::test]
    async fn test_validate_domain_blacklist_empty_blacklist_allows_all() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        let blacklist: Vec<String> = vec![];
        assert!(validator
            .validate_domain_blacklist("http://example.com", &blacklist)
            .is_ok());
        assert!(validator
            .validate_domain_blacklist("http://google.com", &blacklist)
            .is_ok());
    }

    #[tokio::test]
    async fn test_validate_domain_blacklist_subdomain_match() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        let blacklist = vec!["example.com".to_string()];
        // 子域名应被阻断
        assert!(validator
            .validate_domain_blacklist("http://sub.example.com", &blacklist)
            .is_err());
        assert!(validator
            .validate_domain_blacklist("http://api.sub.example.com", &blacklist)
            .is_err());
    }

    #[tokio::test]
    async fn test_validate_domain_blacklist_no_partial_match() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        let blacklist = vec!["example.com".to_string()];
        // 部分匹配不应阻断
        assert!(validator
            .validate_domain_blacklist("http://myexample.com", &blacklist)
            .is_ok());
        assert!(validator
            .validate_domain_blacklist("http://example.org", &blacklist)
            .is_ok());
        assert!(validator
            .validate_domain_blacklist("http://notexample.com", &blacklist)
            .is_ok());
    }

    #[tokio::test]
    async fn test_validate_domain_blacklist_invalid_url() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        let result = validator.validate_domain_blacklist("not-a-valid-url", &[]);
        assert!(result.is_err());
        assert!(matches!(result, Err(SsrfError::InvalidUrl { .. })));
    }

    #[tokio::test]
    async fn test_validate_domain_blacklist_missing_host() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        // mailto: URL 可解析但无 host
        let result = validator.validate_domain_blacklist("mailto:foo@bar.com", &[]);
        assert!(result.is_err());
        assert!(matches!(result, Err(SsrfError::MissingHost { .. })));
    }

    #[tokio::test]
    async fn test_validate_domain_blacklist_multiple_domains() {
        let cache = create_test_dns_cache().await;
        let validator = ValidatedUrlValidator::new(cache);
        let blacklist = vec![
            "example.com".to_string(),
            "malicious.net".to_string(),
            "evil.io".to_string(),
        ];
        // 精确匹配
        assert!(validator
            .validate_domain_blacklist("http://example.com", &blacklist)
            .is_err());
        assert!(validator
            .validate_domain_blacklist("http://malicious.net", &blacklist)
            .is_err());
        assert!(validator
            .validate_domain_blacklist("http://evil.io", &blacklist)
            .is_err());
        // 子域名
        assert!(validator
            .validate_domain_blacklist("http://sub.evil.io", &blacklist)
            .is_err());
        // 未列入
        assert!(validator
            .validate_domain_blacklist("http://safe.org", &blacklist)
            .is_ok());
    }
}
