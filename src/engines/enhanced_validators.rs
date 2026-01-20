// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Enhanced URL validator with DNS caching and SSRF protection
//!
//! Provides URL validation, DNS cache support, and SSRF protection.
//! Uses shared validation functions from [`crate::engines::shared`] for consistency.

use crate::engines::shared::{is_blocked_hostname, is_private_ip};
use std::net::IpAddr;
use std::sync::Arc;
use tokio::net::lookup_host;
use url::Url;

use crate::infrastructure::dns::dns_cache::DnsCache;

/// Validation error types
#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error("URL exceeds maximum length of {} characters", MAX_URL_LENGTH)]
    UrlTooLong,

    #[error("SSRF protection: Only http and https protocols are allowed, got: {}", .0)]
    InvalidProtocol(String),

    #[error("SSRF protection: Hostname '{}' is not allowed", .0)]
    BlockedHostname(String),

    #[error("SSRF protection: No IP addresses resolved for host")]
    NoIpResolved,

    #[error("SSRF protection: Mixed private and public IP addresses detected (possible DNS rebinding attack)")]
    MixedIpAddresses,

    #[error("SSRF protection: Private IP access is not allowed: {}", .0)]
    PrivateIpAccess(String),

    #[error("SSRF protection: DNS rebinding detected - inconsistent IP addresses")]
    DnsRebinding,
}

const MAX_URL_LENGTH: usize = 2048;

/// Enhanced URL validator with DNS caching
#[derive(Clone)]
pub struct ValidatedUrlValidator {
    dns_cache: Arc<DnsCache>,
}

impl ValidatedUrlValidator {
    /// Create a new validator
    pub fn new(dns_cache: Arc<DnsCache>) -> Self {
        Self { dns_cache }
    }

    /// Validate URL for SSRF protection using DNS cache
    ///
    /// Uses DNS cache to reduce duplicate queries and prevent leaking access patterns
    pub async fn validate_url(&self, url_str: &str) -> Result<(), ValidationError> {
        // URL length limit for SSRF and resource exhaustion prevention
        if url_str.len() > MAX_URL_LENGTH {
            return Err(ValidationError::UrlTooLong);
        }

        let url = Url::parse(url_str)
            .map_err(|_| ValidationError::InvalidProtocol(url_str.to_string()))?;

        // Protocol check: only http and https allowed
        match url.scheme() {
            "http" | "https" => {}
            scheme => {
                return Err(ValidationError::InvalidProtocol(scheme.to_string()));
            }
        }

        let host = url
            .host_str()
            .ok_or_else(|| ValidationError::BlockedHostname("missing".to_string()))?;

        // Pre-check hostname before DNS resolution
        if is_blocked_hostname(host) {
            return Err(ValidationError::BlockedHostname(host.to_string()));
        }

        // Use DNS cache for resolution
        let port = url.port_or_known_default().unwrap_or(80);
        let addrs: Vec<IpAddr> = self
            .dns_cache
            .lookup_host(host, port)
            .await
            .map_err(|_| ValidationError::NoIpResolved)?;

        // DNS Rebinding protection: check all resolved IPs
        if addrs.is_empty() {
            return Err(ValidationError::NoIpResolved);
        }

        // Check for mixed private/public IPs (DNS rebinding signature)
        let has_private = addrs.iter().any(|addr| is_private_ip(addr.ip()));
        let has_public = addrs.iter().any(|addr| !is_private_ip(addr.ip()));

        if has_private && has_public {
            return Err(ValidationError::MixedIpAddresses);
        }

        // Check all resolved IPs are not private
        for addr in &addrs {
            if is_private_ip(addr.ip()) {
                return Err(ValidationError::PrivateIpAccess(addr.ip().to_string()));
            }
        }

        // DNS Rebinding protection: log multiple IPs
        if addrs.len() > 1 {
            tracing::warn!(
                "DNS warning: Host {} resolved to {} IP addresses",
                host,
                addrs.len()
            );
        }

        Ok(())
    }
}

/// Validate URL against domain blacklist
pub fn validate_domain_blacklist(
    url_str: &str,
    blacklist: &[String],
) -> Result<(), ValidationError> {
    let url =
        Url::parse(url_str).map_err(|_| ValidationError::InvalidProtocol(url_str.to_string()))?;
    let host = url
        .host_str()
        .ok_or_else(|| ValidationError::BlockedHostname("missing".to_string()))?;

    for domain in blacklist {
        if host == domain || host.ends_with(&format!(".{}", domain)) {
            return Err(ValidationError::BlockedHostname(host.to_string()));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::dns::dns_cache::DnsCache;

    #[tokio::test]
    async fn test_validated_url_validator() {
        let cache = Arc::new(DnsCache::new(Default::default()));
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

    #[test]
    fn test_is_blocked_hostname() {
        assert!(is_blocked_hostname("localhost"));
        assert!(is_blocked_hostname("169.254.169.254"));
        assert!(!is_blocked_hostname("example.com"));
    }
}
