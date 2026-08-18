// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! SSRF Protection Types
//!
//! Defines configuration and result types for SSRF protection.

use std::net::IpAddr;
use url::Url;

/// Maximum URL length to prevent resource exhaustion
pub const MAX_URL_LENGTH: usize = 2048;

/// Maximum number of redirects to follow
pub const MAX_REDIRECTS: u8 = 10;

/// Default DNS cache TTL in seconds
pub const DEFAULT_DNS_CACHE_TTL: u64 = 300;

/// Configuration for SSRF protection.
#[derive(Debug, Clone)]
pub struct SsrfConfig {
    /// Maximum URL length (default: 2048)
    pub max_url_length: usize,
    /// Maximum number of redirects to follow (default: 10)
    pub max_redirects: u8,
    /// DNS cache TTL in seconds (default: 300)
    pub dns_cache_ttl: u64,
    /// Whether to allow redirects to same host only
    pub same_host_redirects_only: bool,
    /// List of additional blocked hostnames
    pub blocked_hostnames: Vec<String>,
    /// List of allowed ports (empty = all allowed except blocked)
    pub allowed_ports: Vec<u16>,
    /// List of blocked ports
    pub blocked_ports: Vec<u16>,
}

impl Default for SsrfConfig {
    fn default() -> Self {
        Self {
            max_url_length: MAX_URL_LENGTH,
            max_redirects: MAX_REDIRECTS,
            dns_cache_ttl: DEFAULT_DNS_CACHE_TTL,
            same_host_redirects_only: false,
            blocked_hostnames: Vec::new(),
            allowed_ports: Vec::new(),
            blocked_ports: vec![
                25,    // SMTP
                465,   // SMTPS
                587,   // SMTP submission
                3306,  // MySQL
                5432,  // PostgreSQL
                6379,  // Redis
                27017, // MongoDB
            ],
        }
    }
}

impl SsrfConfig {
    /// Create a new configuration with default values.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set maximum URL length.
    pub fn with_max_url_length(mut self, max: usize) -> Self {
        self.max_url_length = max;
        self
    }

    /// Set maximum number of redirects.
    pub fn with_max_redirects(mut self, max: u8) -> Self {
        self.max_redirects = max;
        self
    }

    /// Set DNS cache TTL.
    pub fn with_dns_cache_ttl(mut self, ttl: u64) -> Self {
        self.dns_cache_ttl = ttl;
        self
    }

    /// Enable same-host redirects only.
    pub fn with_same_host_redirects_only(mut self, enabled: bool) -> Self {
        self.same_host_redirects_only = enabled;
        self
    }

    /// Add blocked hostname.
    pub fn with_blocked_hostname(mut self, hostname: impl Into<String>) -> Self {
        self.blocked_hostnames.push(hostname.into());
        self
    }

    /// Add blocked port.
    pub fn with_blocked_port(mut self, port: u16) -> Self {
        self.blocked_ports.push(port);
        self
    }

    /// Check if a port is allowed.
    pub fn is_port_allowed(&self, port: u16) -> bool {
        // If allowed_ports is not empty, check against it
        if !self.allowed_ports.is_empty() {
            return self.allowed_ports.contains(&port);
        }

        // Otherwise, check against blocked_ports
        !self.blocked_ports.contains(&port)
    }
}

/// A validated URL with resolved IP addresses.
///
/// This struct contains the result of SSRF validation,
/// including the parsed URL and resolved IP addresses.
#[derive(Debug, Clone)]
pub struct ValidatedUrl {
    /// The original URL string
    pub url: String,
    /// The parsed URL
    pub parsed_url: Url,
    /// Resolved IP addresses
    pub resolved_ips: Vec<IpAddr>,
    /// The port number
    pub port: u16,
}

impl ValidatedUrl {
    /// Get the hostname from the URL.
    pub fn hostname(&self) -> Option<&str> {
        self.parsed_url.host_str()
    }

    /// Get the scheme from the URL.
    pub fn scheme(&self) -> &str {
        self.parsed_url.scheme()
    }

    /// Check if the URL uses HTTPS.
    pub fn is_https(&self) -> bool {
        self.parsed_url.scheme() == "https"
    }

    /// Get the first resolved IP address.
    pub fn primary_ip(&self) -> Option<&IpAddr> {
        self.resolved_ips.first()
    }

    /// Get all resolved IP addresses.
    pub fn ips(&self) -> &[IpAddr] {
        &self.resolved_ips
    }
}

/// Result of SSRF validation.
#[derive(Debug, Clone)]
pub enum SsrfValidationResult {
    /// URL is safe to access
    Safe(ValidatedUrl),
    /// URL is blocked
    Blocked {
        /// The blocked URL
        url: String,
        /// Reason for blocking
        reason: String,
    },
    /// Validation requires DNS resolution
    RequiresDnsResolution {
        /// The URL to resolve
        url: String,
        /// The hostname to resolve
        hostname: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ssrf_config_default() {
        let config = SsrfConfig::default();
        assert_eq!(config.max_url_length, MAX_URL_LENGTH);
        assert_eq!(config.max_redirects, MAX_REDIRECTS);
        assert_eq!(config.dns_cache_ttl, DEFAULT_DNS_CACHE_TTL);
        assert!(!config.same_host_redirects_only);
        assert!(config.blocked_hostnames.is_empty());
        assert!(config.allowed_ports.is_empty());
        assert!(!config.blocked_ports.is_empty());
    }

    #[test]
    fn test_ssrf_config_builder() {
        let config = SsrfConfig::new()
            .with_max_url_length(1024)
            .with_max_redirects(5)
            .with_dns_cache_ttl(600)
            .with_same_host_redirects_only(true)
            .with_blocked_hostname("evil.com")
            .with_blocked_port(8080);

        assert_eq!(config.max_url_length, 1024);
        assert_eq!(config.max_redirects, 5);
        assert_eq!(config.dns_cache_ttl, 600);
        assert!(config.same_host_redirects_only);
        assert!(config.blocked_hostnames.contains(&"evil.com".to_string()));
        assert!(config.blocked_ports.contains(&8080));
    }

    #[test]
    fn test_ssrf_config_port_allowed() {
        let config = SsrfConfig::default();

        // Default blocked ports
        assert!(!config.is_port_allowed(25)); // SMTP
        assert!(!config.is_port_allowed(3306)); // MySQL
        assert!(!config.is_port_allowed(5432)); // PostgreSQL

        // Allowed ports
        assert!(config.is_port_allowed(80));
        assert!(config.is_port_allowed(443));
        assert!(config.is_port_allowed(8080));
    }

    #[test]
    fn test_ssrf_config_allowed_ports_only() {
        let config = SsrfConfig {
            allowed_ports: vec![80, 443],
            ..Default::default()
        };

        assert!(config.is_port_allowed(80));
        assert!(config.is_port_allowed(443));
        assert!(!config.is_port_allowed(8080));
        assert!(!config.is_port_allowed(3306));
    }

    #[test]
    fn test_ssrf_config_new_equals_default() {
        let new_config = SsrfConfig::new();
        let default_config = SsrfConfig::default();
        assert_eq!(new_config.max_url_length, default_config.max_url_length);
        assert_eq!(new_config.max_redirects, default_config.max_redirects);
        assert_eq!(new_config.dns_cache_ttl, default_config.dns_cache_ttl);
    }

    fn make_validated_url(scheme: &str, host: &str, ips: Vec<IpAddr>) -> ValidatedUrl {
        let url_str = format!("{}://{}/path", scheme, host);
        ValidatedUrl {
            url: url_str.clone(),
            parsed_url: Url::parse(&url_str).unwrap(),
            resolved_ips: ips,
            port: if scheme == "https" { 443 } else { 80 },
        }
    }

    #[test]
    fn test_validated_url_hostname() {
        let vu = make_validated_url("https", "example.com", vec!["8.8.8.8".parse().unwrap()]);
        assert_eq!(vu.hostname(), Some("example.com"));
    }

    #[test]
    fn test_validated_url_scheme() {
        let https_vu = make_validated_url("https", "example.com", vec![]);
        assert_eq!(https_vu.scheme(), "https");

        let http_vu = make_validated_url("http", "example.com", vec![]);
        assert_eq!(http_vu.scheme(), "http");
    }

    #[test]
    fn test_validated_url_is_https() {
        let https_vu = make_validated_url("https", "example.com", vec![]);
        assert!(https_vu.is_https());

        let http_vu = make_validated_url("http", "example.com", vec![]);
        assert!(!http_vu.is_https());
    }

    #[test]
    fn test_validated_url_primary_ip() {
        let ips = vec![
            "8.8.8.8".parse::<IpAddr>().unwrap(),
            "1.1.1.1".parse().unwrap(),
        ];
        let vu = make_validated_url("https", "example.com", ips.clone());
        assert_eq!(vu.primary_ip(), Some(&ips[0]));
    }

    #[test]
    fn test_validated_url_primary_ip_empty() {
        let vu = make_validated_url("https", "example.com", vec![]);
        assert!(vu.primary_ip().is_none());
    }

    #[test]
    fn test_validated_url_ips() {
        let ips = vec![
            "8.8.8.8".parse::<IpAddr>().unwrap(),
            "1.1.1.1".parse().unwrap(),
        ];
        let vu = make_validated_url("https", "example.com", ips.clone());
        assert_eq!(vu.ips().len(), 2);
        assert_eq!(vu.ips()[0], ips[0]);
        assert_eq!(vu.ips()[1], ips[1]);
    }

    #[test]
    fn test_ssrf_validation_result_variants() {
        // Safe variant
        let safe = SsrfValidationResult::Safe(make_validated_url("https", "example.com", vec![]));
        assert!(matches!(safe, SsrfValidationResult::Safe(_)));

        // Blocked variant
        let blocked = SsrfValidationResult::Blocked {
            url: "http://localhost".to_string(),
            reason: "internal address".to_string(),
        };
        assert!(matches!(blocked, SsrfValidationResult::Blocked { .. }));

        // RequiresDnsResolution variant
        let requires_dns = SsrfValidationResult::RequiresDnsResolution {
            url: "http://example.com".to_string(),
            hostname: "example.com".to_string(),
        };
        assert!(matches!(
            requires_dns,
            SsrfValidationResult::RequiresDnsResolution { .. }
        ));
    }
}
