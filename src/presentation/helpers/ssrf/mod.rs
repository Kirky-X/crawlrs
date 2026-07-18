// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Unified SSRF Protection Module
//!
//! This module provides comprehensive SSRF (Server-Side Request Forgery) protection
//! with multiple defense layers:
//!
//! 1. **Static URL Validation**: Fast pre-check for obviously malicious URLs
//! 2. **DNS Resolution Validation**: Prevents DNS rebinding attacks
//! 3. **Redirect Validation**: Validates redirect targets during HTTP requests
//! 4. **TOCTOU Protection**: Time-of-check to time-of-use attack prevention
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    SSRF Protection Flow                     │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                             │
//! │  1. Static Validation (is_internal_url)                    │
//! │     - Check scheme (http/https only)                       │
//! │     - Check hostname patterns (localhost, etc.)            │
//! │     - Check IP ranges (private, loopback, etc.)            │
//! │                                                             │
//! │  2. DNS Resolution (validate_url_with_dns)                 │
//! │     - Resolve DNS with caching                             │
//! │     - Validate all resolved IPs                            │
//! │     - Detect DNS rebinding (mixed private/public)          │
//! │                                                             │
//! │  3. Redirect Validation (validate_redirect)                │
//! │     - Validate redirect target URL                         │
//! │     - Prevent redirect to internal addresses               │
//! │     - Track redirect chain depth                           │
//! │                                                             │
//! │  4. Connection Validation (validate_connection_ip)         │
//! │     - Verify actual connection IP matches validated IP     │
//! │     - TOCTOU protection                                     │
//! │                                                             │
//! └─────────────────────────────────────────────────────────────┘
//! ```

mod error;
mod redirect;
mod static_validator;
mod types;

pub use error::SsrfError;
pub use redirect::{RedirectPolicy, RedirectValidator};
pub use static_validator::is_internal_url;
pub use types::{SsrfConfig, SsrfValidationResult, ValidatedUrl};

use crate::engines::shared::{is_blocked_hostname, is_private_ip};
use std::net::IpAddr;
use tokio::net::lookup_host;
use url::Url;

use crate::infrastructure::dns::DnsCacheService;
use std::sync::Arc;

/// Maximum URL length to prevent resource exhaustion
#[allow(dead_code)]
const MAX_URL_LENGTH: usize = 2048;

/// Maximum number of redirects to follow
#[allow(dead_code)]
const MAX_REDIRECTS: u8 = 10;

/// Direct DNS resolution helper (no cache). Used when oxcache-cache feature is disabled
/// or when SsrfValidator is constructed without a DNS cache.
async fn resolve_dns_direct(hostname: &str, port: u16) -> Result<Vec<IpAddr>, SsrfError> {
    let addr_str = format!("{}:{}", hostname, port);
    let addrs: Vec<IpAddr> = lookup_host(&addr_str)
        .await
        .map_err(|e| SsrfError::DnsResolutionFailed {
            hostname: hostname.to_string(),
            reason: e.to_string(),
        })?
        .map(|socket_addr| socket_addr.ip())
        .collect();
    Ok(addrs)
}

/// Default DNS cache TTL in seconds
#[allow(dead_code)]
const DEFAULT_DNS_CACHE_TTL: u64 = 300;

/// Unified SSRF protection validator with DNS caching support.
///
/// This struct provides the main entry point for SSRF protection.
/// It combines static validation, DNS resolution, and redirect validation.
#[derive(Clone, Default)]
pub struct SsrfValidator {
    /// DNS cache for resolution results
    dns_cache: Option<Arc<DnsCacheService>>,
    /// Configuration options
    config: SsrfConfig,
}

impl SsrfValidator {
    /// Create a new SSRF validator without DNS caching.
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new SSRF validator with DNS caching support.
    pub fn with_dns_cache(dns_cache: Arc<DnsCacheService>) -> Self {
        Self {
            dns_cache: Some(dns_cache),
            config: SsrfConfig::default(),
        }
    }

    /// Create a new SSRF validator with custom configuration.
    pub fn with_config(config: SsrfConfig) -> Self {
        Self {
            dns_cache: None,
            config,
        }
    }

    /// Create a new SSRF validator with DNS cache and custom configuration.
    pub fn with_dns_cache_and_config(dns_cache: Arc<DnsCacheService>, config: SsrfConfig) -> Self {
        Self {
            dns_cache: Some(dns_cache),
            config,
        }
    }

    /// Perform full SSRF validation on a URL.
    ///
    /// This method performs:
    /// 1. Static URL validation (scheme, hostname patterns)
    /// 2. DNS resolution with IP validation
    /// 3. DNS rebinding detection
    ///
    /// # Arguments
    ///
    /// * `url_str` - The URL string to validate
    ///
    /// # Returns
    ///
    /// * `Ok(ValidatedUrl)` - The validated URL with resolved IPs
    /// * `Err(SsrfError)` - Validation failed
    ///
    /// # Example
    ///
    /// ```ignore
    /// use crate::presentation::helpers::ssrf::SsrfValidator;
    ///
    /// let validator = SsrfValidator::new();
    /// let result = validator.validate("https://example.com").await;
    /// assert!(result.is_ok());
    /// ```
    pub async fn validate(&self, url_str: &str) -> Result<ValidatedUrl, SsrfError> {
        // Step 1: URL length check
        if url_str.len() > self.config.max_url_length {
            return Err(SsrfError::UrlTooLong {
                max: self.config.max_url_length,
                actual: url_str.len(),
            });
        }

        // Step 2: Parse URL
        let url = Url::parse(url_str).map_err(|e| SsrfError::InvalidUrl {
            url: url_str.to_string(),
            reason: e.to_string(),
        })?;

        // Step 3: Scheme validation
        match url.scheme() {
            "http" | "https" => {}
            scheme => {
                return Err(SsrfError::InvalidScheme {
                    scheme: scheme.to_string(),
                });
            }
        }

        // Step 4: Extract host
        let host = url.host_str().ok_or_else(|| SsrfError::MissingHost {
            url: url_str.to_string(),
        })?;

        // Step 5: Static hostname check (before DNS resolution)
        if is_blocked_hostname(host) {
            return Err(SsrfError::BlockedHostname {
                hostname: host.to_string(),
            });
        }

        // Step 6: DNS resolution and IP validation
        let port = url.port_or_known_default().unwrap_or(80);
        let resolved_ips = self.resolve_and_validate_ips(host, port).await?;

        // Step 7: Create validated URL
        Ok(ValidatedUrl {
            url: url_str.to_string(),
            parsed_url: url,
            resolved_ips,
            port,
        })
    }

    /// Resolve DNS and validate all returned IP addresses.
    ///
    /// This method implements DNS rebinding protection by:
    /// 1. Resolving all IP addresses for the hostname
    /// 2. Checking for mixed private/public IPs (DNS rebinding signature)
    /// 3. Rejecting if any private IP is found
    async fn resolve_and_validate_ips(
        &self,
        hostname: &str,
        port: u16,
    ) -> Result<Vec<IpAddr>, SsrfError> {
        // Use DNS cache if available (requires oxcache-cache feature)
        let ips = {
            if let Some(cache) = &self.dns_cache {
                cache.lookup_host(hostname, port).await.map_err(|e| {
                    SsrfError::DnsResolutionFailed {
                        hostname: hostname.to_string(),
                        reason: e.to_string(),
                    }
                })?
            } else {
                resolve_dns_direct(hostname, port).await?
            }
        };

        // Check if any IPs were resolved
        if ips.is_empty() {
            return Err(SsrfError::NoIpResolved {
                hostname: hostname.to_string(),
            });
        }

        // DNS rebinding detection: check for mixed private/public IPs
        let has_private = ips.iter().any(|ip| is_private_ip(*ip));
        let has_public = ips.iter().any(|ip| !is_private_ip(*ip));

        if has_private && has_public {
            log::warn!(
                "DNS rebinding attack detected: hostname {} resolved to mixed private/public IPs",
                hostname
            );
            return Err(SsrfError::DnsRebindingDetected {
                hostname: hostname.to_string(),
                ips: ips.iter().map(|ip| ip.to_string()).collect(),
            });
        }

        // Check all IPs are not private
        for ip in &ips {
            if is_private_ip(*ip) {
                return Err(SsrfError::PrivateIpAccess { ip: ip.to_string() });
            }
        }

        // Log if multiple IPs resolved (potential DNS round-robin)
        if ips.len() > 1 {
            log::debug!(
                "DNS resolution returned {} IPs for hostname {}",
                ips.len(),
                hostname
            );
        }

        Ok(ips)
    }

    /// Validate a redirect URL during HTTP request.
    ///
    /// This method should be called for each redirect to ensure
    /// the redirect target is not an internal address.
    pub fn validate_redirect(&self, redirect_url: &str) -> Result<(), SsrfError> {
        // Quick static check for redirect URL
        if is_internal_url(redirect_url) {
            return Err(SsrfError::RedirectToInternal {
                url: redirect_url.to_string(),
            });
        }
        Ok(())
    }

    /// Validate that the actual connection IP matches the validated IP.
    ///
    /// This provides TOCTOU (Time-of-check to time-of-use) protection
    /// by verifying the IP address at connection time.
    pub fn validate_connection_ip(&self, ip: IpAddr) -> Result<(), SsrfError> {
        if is_private_ip(ip) {
            return Err(SsrfError::PrivateIpAccess { ip: ip.to_string() });
        }
        Ok(())
    }

    /// Quick synchronous check if a URL is internal (without DNS resolution).
    ///
    /// Use this for fast pre-filtering before async validation.
    pub fn is_internal_url_sync(&self, url_str: &str) -> bool {
        is_internal_url(url_str)
    }
}

/// Convenience function for quick URL validation without DNS caching.
///
/// This function performs full SSRF validation including DNS resolution.
/// For repeated validations, use `SsrfValidator::with_dns_cache()` instead.
pub async fn validate_url(url_str: &str) -> Result<ValidatedUrl, SsrfError> {
    let validator = SsrfValidator::new();
    validator.validate(url_str).await
}

/// Validate a domain against a blacklist.
///
/// # Arguments
///
/// * `url_str` - The URL string to validate
/// * `blacklist` - List of blocked domains
///
/// # Returns
///
/// * `Ok(())` - Domain is not in blacklist
/// * `Err(SsrfError)` - Domain is in blacklist
pub fn validate_domain_blacklist(url_str: &str, blacklist: &[String]) -> Result<(), SsrfError> {
    let url = Url::parse(url_str).map_err(|e| SsrfError::InvalidUrl {
        url: url_str.to_string(),
        reason: e.to_string(),
    })?;

    let host = url.host_str().ok_or_else(|| SsrfError::MissingHost {
        url: url_str.to_string(),
    })?;

    for domain in blacklist {
        if host == domain || host.ends_with(&format!(".{}", domain)) {
            return Err(SsrfError::BlockedHostname {
                hostname: host.to_string(),
            });
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_internal_url_localhost() {
        assert!(is_internal_url("http://localhost"));
        assert!(is_internal_url("http://localhost:8080"));
        assert!(is_internal_url("http://127.0.0.1"));
        assert!(is_internal_url("http://127.0.0.1:3000"));
    }

    #[test]
    fn test_is_internal_url_private_ipv4() {
        // 10.0.0.0/8
        assert!(is_internal_url("http://10.0.0.1"));
        assert!(is_internal_url("http://10.255.255.255"));

        // 172.16.0.0/12
        assert!(is_internal_url("http://172.16.0.1"));
        assert!(is_internal_url("http://172.31.255.255"));

        // 192.168.0.0/16
        assert!(is_internal_url("http://192.168.0.1"));
        assert!(is_internal_url("http://192.168.255.255"));

        // Not in private range
        assert!(!is_internal_url("http://172.15.0.1"));
        assert!(!is_internal_url("http://172.32.0.1"));
    }

    #[test]
    fn test_is_internal_url_link_local() {
        assert!(is_internal_url("http://169.254.0.1"));
        assert!(is_internal_url("http://169.254.255.255"));
    }

    #[test]
    fn test_is_internal_url_ipv6() {
        // Loopback
        assert!(is_internal_url("http://[::1]"));
        assert!(is_internal_url("http://[::1]:8080"));

        // Link-local
        assert!(is_internal_url("http://[fe80::1]"));
        assert!(is_internal_url("http://[febf::1]"));

        // Unique local
        assert!(is_internal_url("http://[fc00::1]"));
        assert!(is_internal_url("http://[fd00::1]"));
    }

    #[test]
    fn test_is_internal_url_external() {
        assert!(!is_internal_url("http://8.8.8.8"));
        assert!(!is_internal_url("http://1.1.1.1"));
        assert!(!is_internal_url("http://google.com"));
        assert!(!is_internal_url("https://www.example.com"));
    }

    #[test]
    fn test_is_internal_url_invalid_scheme() {
        assert!(is_internal_url("file:///etc/passwd"));
        assert!(is_internal_url("ftp://example.com"));
        assert!(is_internal_url("gopher://localhost"));
    }

    #[test]
    fn test_validate_domain_blacklist() {
        let blacklist = vec!["example.com".to_string(), "malicious.net".to_string()];

        // Exact match
        assert!(validate_domain_blacklist("http://example.com", &blacklist).is_err());
        assert!(validate_domain_blacklist("http://malicious.net/path", &blacklist).is_err());

        // Subdomain match
        assert!(validate_domain_blacklist("http://sub.example.com", &blacklist).is_err());
        assert!(validate_domain_blacklist("http://api.malicious.net", &blacklist).is_err());

        // Not in blacklist
        assert!(validate_domain_blacklist("http://google.com", &blacklist).is_ok());
        assert!(validate_domain_blacklist("http://example.org", &blacklist).is_ok());

        // Partial match should not block
        assert!(validate_domain_blacklist("http://myexample.com", &blacklist).is_ok());
    }

    #[tokio::test]
    async fn test_validate_url_ssrf() {
        let validator = SsrfValidator::new();

        // Localhost variants should be blocked
        assert!(validator.validate("http://localhost").await.is_err());
        assert!(validator
            .validate("http://localhost.localdomain")
            .await
            .is_err());
        assert!(validator.validate("http://127.0.0.1").await.is_err());
        assert!(validator.validate("http://0.0.0.0").await.is_err());

        // Private IPs should be blocked
        assert!(validator.validate("http://10.0.0.1").await.is_err());
        assert!(validator.validate("http://192.168.1.1").await.is_err());
        assert!(validator.validate("http://172.16.0.1").await.is_err());

        // Cloud metadata endpoints should be blocked
        assert!(validator.validate("http://169.254.169.254").await.is_err());
        assert!(validator
            .validate("http://metadata.google.internal")
            .await
            .is_err());

        // Invalid schemes should be blocked
        assert!(validator.validate("file:///etc/passwd").await.is_err());
        assert!(validator.validate("ftp://example.com").await.is_err());
        assert!(validator.validate("gopher://localhost").await.is_err());
    }

    #[tokio::test]
    async fn test_validate_url_too_long() {
        let validator = SsrfValidator::new();
        let long_url = format!("http://example.com/{}", "a".repeat(3000));
        let result = validator.validate(&long_url).await;
        assert!(matches!(result, Err(SsrfError::UrlTooLong { .. })));
    }

    #[test]
    fn test_ssrf_validator_default() {
        let validator = SsrfValidator::default();
        assert!(validator.dns_cache.is_none());
        assert_eq!(validator.config.max_url_length, MAX_URL_LENGTH);
    }

    #[test]
    fn test_ssrf_validator_with_config() {
        let config = SsrfConfig::new().with_max_url_length(512);
        let validator = SsrfValidator::with_config(config);
        assert!(validator.dns_cache.is_none());
        assert_eq!(validator.config.max_url_length, 512);
    }

    #[test]
    fn test_validate_connection_ip_private() {
        let validator = SsrfValidator::new();
        // Private IPs should be blocked
        assert!(validator
            .validate_connection_ip("10.0.0.1".parse().unwrap())
            .is_err());
        assert!(validator
            .validate_connection_ip("192.168.1.1".parse().unwrap())
            .is_err());
        assert!(validator
            .validate_connection_ip("127.0.0.1".parse().unwrap())
            .is_err());
        assert!(validator
            .validate_connection_ip("169.254.169.254".parse().unwrap())
            .is_err());
        assert!(validator
            .validate_connection_ip("::1".parse().unwrap())
            .is_err());
    }

    #[test]
    fn test_validate_connection_ip_public() {
        let validator = SsrfValidator::new();
        // Public IPs should be allowed
        assert!(validator
            .validate_connection_ip("8.8.8.8".parse().unwrap())
            .is_ok());
        assert!(validator
            .validate_connection_ip("1.1.1.1".parse().unwrap())
            .is_ok());
    }

    #[test]
    fn test_is_internal_url_sync_method() {
        let validator = SsrfValidator::new();
        assert!(validator.is_internal_url_sync("http://localhost"));
        assert!(validator.is_internal_url_sync("http://10.0.0.1"));
        assert!(validator.is_internal_url_sync("http://192.168.1.1"));
        assert!(!validator.is_internal_url_sync("http://example.com"));
        assert!(!validator.is_internal_url_sync("https://8.8.8.8"));
    }

    #[test]
    fn test_validate_redirect_method_blocks_internal() {
        let validator = SsrfValidator::new();
        assert!(validator.validate_redirect("http://localhost").is_err());
        assert!(validator.validate_redirect("http://192.168.1.1").is_err());
        assert!(validator.validate_redirect("http://10.0.0.1").is_err());
    }

    #[test]
    fn test_validate_redirect_method_allows_external() {
        let validator = SsrfValidator::new();
        assert!(validator.validate_redirect("http://example.com").is_ok());
        assert!(validator.validate_redirect("https://google.com").is_ok());
    }

    #[tokio::test]
    async fn test_validate_url_convenience_function() {
        // Internal URLs should be blocked
        assert!(validate_url("http://localhost").await.is_err());
        assert!(validate_url("http://10.0.0.1").await.is_err());
        assert!(validate_url("file:///etc/passwd").await.is_err());
        // URL too long
        let long_url = format!("http://example.com/{}", "a".repeat(3000));
        assert!(validate_url(&long_url).await.is_err());
    }

    #[tokio::test]
    async fn test_validate_invalid_url_format() {
        let validator = SsrfValidator::new();
        // Unparseable URL should fail with InvalidUrl
        let result = validator.validate("http://[invalid").await;
        assert!(result.is_err());
        assert!(matches!(result, Err(SsrfError::InvalidUrl { .. })));
    }

    #[test]
    fn test_validate_domain_blacklist_invalid_url() {
        let result = validate_domain_blacklist("not-a-valid-url", &[]);
        assert!(result.is_err());
        assert!(matches!(result, Err(SsrfError::InvalidUrl { .. })));
    }

    #[test]
    fn test_validate_domain_blacklist_missing_host() {
        // mailto: URLs parse but have no host component
        let result = validate_domain_blacklist("mailto:foo@bar.com", &[]);
        assert!(result.is_err());
        assert!(matches!(result, Err(SsrfError::MissingHost { .. })));
    }

    // ========== Constructor tests with DNS cache ==========

    async fn make_dns_cache() -> Arc<DnsCacheService> {
        let cache: Arc<crate::infrastructure::oxcache::DnsCache> = Arc::new(
            oxcache::Cache::builder()
                .capacity(100)
                .ttl(std::time::Duration::from_secs(300))
                .build()
                .await
                .expect("Failed to build oxcache for test"),
        );
        Arc::new(DnsCacheService::new(cache, 300))
    }

    #[tokio::test]
    async fn test_with_dns_cache_constructor() {
        let dns_cache = make_dns_cache().await;
        let validator = SsrfValidator::with_dns_cache(dns_cache);
        assert!(validator.dns_cache.is_some());
        assert_eq!(validator.config.max_url_length, MAX_URL_LENGTH);
    }

    #[tokio::test]
    async fn test_with_dns_cache_and_config_constructor() {
        let dns_cache = make_dns_cache().await;
        let config = SsrfConfig::new().with_max_url_length(512);
        let validator = SsrfValidator::with_dns_cache_and_config(dns_cache, config);
        assert!(validator.dns_cache.is_some());
        assert_eq!(validator.config.max_url_length, 512);
    }

    #[tokio::test]
    async fn test_validate_with_dns_cache_blocks_internal() {
        let dns_cache = make_dns_cache().await;
        let validator = SsrfValidator::with_dns_cache(dns_cache);

        // Internal URLs should still be blocked even with DNS cache
        assert!(validator.validate("http://localhost").await.is_err());
        assert!(validator.validate("http://127.0.0.1").await.is_err());
        assert!(validator.validate("http://10.0.0.1").await.is_err());
        assert!(validator.validate("file:///etc/passwd").await.is_err());
    }

    #[tokio::test]
    async fn test_validate_with_dns_cache_url_too_long() {
        let dns_cache = make_dns_cache().await;
        let validator = SsrfValidator::with_dns_cache(dns_cache);
        let long_url = format!("http://example.com/{}", "a".repeat(3000));
        let result = validator.validate(&long_url).await;
        assert!(matches!(result, Err(SsrfError::UrlTooLong { .. })));
    }

    #[tokio::test]
    async fn test_validate_with_dns_cache_invalid_url() {
        let dns_cache = make_dns_cache().await;
        let validator = SsrfValidator::with_dns_cache(dns_cache);
        let result = validator.validate("http://[invalid").await;
        assert!(result.is_err());
        assert!(matches!(result, Err(SsrfError::InvalidUrl { .. })));
    }

    #[tokio::test]
    async fn test_validate_with_dns_cache_invalid_scheme() {
        let dns_cache = make_dns_cache().await;
        let validator = SsrfValidator::with_dns_cache(dns_cache);
        let result = validator.validate("ftp://example.com").await;
        assert!(result.is_err());
        assert!(matches!(result, Err(SsrfError::InvalidScheme { .. })));
    }

    #[tokio::test]
    async fn test_validate_with_dns_cache_blocked_hostname() {
        let dns_cache = make_dns_cache().await;
        let validator = SsrfValidator::with_dns_cache(dns_cache);
        // metadata.google.internal is in the blocked hostname list
        let result = validator.validate("http://metadata.google.internal").await;
        assert!(result.is_err());
        assert!(matches!(result, Err(SsrfError::BlockedHostname { .. })));
    }

    #[tokio::test]
    async fn test_validate_with_dns_cache_and_config_blocks_internal() {
        let dns_cache = make_dns_cache().await;
        let config = SsrfConfig::new().with_max_url_length(1024);
        let validator = SsrfValidator::with_dns_cache_and_config(dns_cache, config);
        assert!(validator.validate("http://10.0.0.1").await.is_err());
        assert!(validator.validate("http://192.168.1.1").await.is_err());
    }

    #[tokio::test]
    async fn test_validate_missing_host_with_dns_cache() {
        let dns_cache = make_dns_cache().await;
        let validator = SsrfValidator::with_dns_cache(dns_cache);
        // "http://" has no host
        let result = validator.validate("http://").await;
        assert!(result.is_err());
        // Could be InvalidUrl or MissingHost depending on Url::parse behavior
    }

    #[test]
    fn test_ssrf_validator_clone() {
        let validator = SsrfValidator::new();
        let cloned = validator.clone();
        assert_eq!(
            validator.config.max_url_length,
            cloned.config.max_url_length
        );
    }
}
