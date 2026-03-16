// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! SSRF protection and URL validation utilities
//!
//! This module provides security-focused validation for URLs to prevent
//! Server-Side Request Forgery (SSRF) attacks.
//!
//! ## Architecture
//!
//! This module now delegates to the unified SSRF protection module at
//! `crate::presentation::helpers::ssrf` which provides:
//! - Static URL validation (fast pre-check)
//! - DNS resolution validation (DNS rebinding protection)
//! - Redirect validation
//! - TOCTOU protection
//!
//! ## Usage
//!
//! For quick synchronous checks:
//! ```ignore
//! use crate::engines::validators::is_internal_url;
//!
//! if is_internal_url(&url) {
//!     return Err("Internal URLs not allowed");
//! }
//! ```
//!
//! For full async validation with DNS resolution:
//! ```ignore
//! use crate::engines::validators::validate_url;
//!
//! validate_url(&url).await?;
//! ```

// Re-export from unified SSRF module
pub use crate::presentation::helpers::ssrf::{
    is_internal_url, validate_domain_blacklist, validate_url, RedirectPolicy, RedirectValidator,
    SsrfConfig, SsrfError, SsrfValidationResult, SsrfValidator, ValidatedUrl,
};

// Re-export shared utilities for backward compatibility
pub use crate::engines::shared::{is_blocked_hostname, is_private_ip};

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_validate_url_ssrf() {
        // Localhost variants should be blocked
        assert!(validate_url("http://localhost").await.is_err());
        assert!(validate_url("http://localhost.localdomain").await.is_err());
        assert!(validate_url("http://127.0.0.1").await.is_err());
        assert!(validate_url("http://0.0.0.0").await.is_err());

        // Private IPs should be blocked
        assert!(validate_url("http://10.0.0.1").await.is_err());
        assert!(validate_url("http://192.168.1.1").await.is_err());
        assert!(validate_url("http://172.16.0.1").await.is_err());

        // Cloud metadata endpoints should be blocked
        assert!(validate_url("http://169.254.169.254").await.is_err());
        assert!(validate_url("http://metadata.google.internal")
            .await
            .is_err());

        // Invalid schemes should be blocked
        assert!(validate_url("file:///etc/passwd").await.is_err());
        assert!(validate_url("ftp://example.com").await.is_err());
        assert!(validate_url("gopher://localhost").await.is_err());

        // Valid public URLs - only run with network tests enabled
        if std::env::var("CRAWLRS_ENABLE_NETWORK_TESTS").is_ok() {
            assert!(validate_url("http://example.com").await.is_ok());
            assert!(validate_url("https://google.com").await.is_ok());
        }
    }

    #[test]
    fn test_is_internal_url() {
        // Localhost
        assert!(is_internal_url("http://localhost"));
        assert!(is_internal_url("http://127.0.0.1"));

        // Private IPs
        assert!(is_internal_url("http://10.0.0.1"));
        assert!(is_internal_url("http://192.168.1.1"));
        assert!(is_internal_url("http://172.16.0.1"));

        // Link-local
        assert!(is_internal_url("http://169.254.0.1"));

        // IPv6
        assert!(is_internal_url("http://[::1]"));
        assert!(is_internal_url("http://[fe80::1]"));
        assert!(is_internal_url("http://[fc00::1]"));

        // External
        assert!(!is_internal_url("http://8.8.8.8"));
        assert!(!is_internal_url("http://google.com"));
    }

    #[test]
    fn test_validate_domain_blacklist() {
        let blacklist = vec!["example.com".to_string(), "malicious.net".to_string()];

        // Blocked exact match
        assert!(validate_domain_blacklist("http://example.com", &blacklist).is_err());
        assert!(validate_domain_blacklist("http://malicious.net/path", &blacklist).is_err());

        // Blocked subdomain
        assert!(validate_domain_blacklist("http://sub.example.com", &blacklist).is_err());
        assert!(validate_domain_blacklist("http://api.malicious.net", &blacklist).is_err());

        // Allowed
        assert!(validate_domain_blacklist("http://google.com", &blacklist).is_ok());
        assert!(validate_domain_blacklist("http://example.org", &blacklist).is_ok());

        // Partial match should not block
        assert!(validate_domain_blacklist("http://myexample.com", &blacklist).is_ok());
    }

    #[test]
    fn test_is_private_ip() {
        // IPv4 private
        assert!(is_private_ip("10.0.0.1".parse().unwrap()));
        assert!(is_private_ip("172.16.0.1".parse().unwrap()));
        assert!(is_private_ip("192.168.1.1".parse().unwrap()));

        // IPv4 loopback
        assert!(is_private_ip("127.0.0.1".parse().unwrap()));

        // IPv4 link-local
        assert!(is_private_ip("169.254.0.1".parse().unwrap()));

        // IPv6
        assert!(is_private_ip("::1".parse().unwrap()));
        assert!(is_private_ip("fe80::1".parse().unwrap()));
        assert!(is_private_ip("fc00::1".parse().unwrap()));

        // Public IPs
        assert!(!is_private_ip("8.8.8.8".parse().unwrap()));
        assert!(!is_private_ip("1.1.1.1".parse().unwrap()));
    }

    #[test]
    fn test_is_blocked_hostname() {
        assert!(is_blocked_hostname("localhost"));
        assert!(is_blocked_hostname("127.0.0.1"));
        assert!(is_blocked_hostname("169.254.169.254"));
        assert!(is_blocked_hostname("metadata.google.internal"));

        // Case insensitive
        assert!(is_blocked_hostname("LOCALHOST"));
        assert!(is_blocked_hostname("LocalHost"));

        // Not blocked
        assert!(!is_blocked_hostname("google.com"));
        assert!(!is_blocked_hostname("example.com"));
        assert!(!is_blocked_hostname("8.8.8.8"));
    }
}
