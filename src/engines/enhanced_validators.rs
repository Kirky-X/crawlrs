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

use crate::infrastructure::dns::dns_cache::DnsCache;
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
    pub fn new(dns_cache: Arc<DnsCache>) -> Self {
        Self {
            inner: SsrfValidator::with_dns_cache(dns_cache),
        }
    }

    /// Create a validator with DNS cache and custom configuration.
    pub fn with_config(dns_cache: Arc<DnsCache>, config: SsrfConfig) -> Self {
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
    use crate::infrastructure::oxcache;
    use std::time::Duration;

    async fn create_test_dns_cache() -> Arc<DnsCache> {
        let cache = oxcache::Cache::builder()
            .capacity(100)
            .ttl(Duration::from_secs(300))
            .build()
            .await
            .unwrap();
        Arc::new(DnsCache::new(cache, 300))
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
}
