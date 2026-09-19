// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! SSRF Error Types
//!
//! Defines all error types for SSRF protection operations.

use std::net::IpAddr;
use thiserror::Error;

/// SSRF protection error types.
///
/// These errors indicate various SSRF attack attempts or validation failures.
#[derive(Debug, Error)]
pub enum SsrfError {
    /// URL exceeds maximum allowed length
    #[error("URL exceeds maximum length of {max} characters (actual: {actual})")]
    UrlTooLong {
        /// Maximum allowed length
        max: usize,
        /// Actual URL length
        actual: usize,
    },

    /// Invalid URL format
    #[error("Invalid URL '{url}': {reason}")]
    InvalidUrl {
        /// The invalid URL
        url: String,
        /// Reason for invalidity
        reason: String,
    },

    /// Invalid or blocked URL scheme
    #[error("Invalid URL scheme '{scheme}'. Only http and https are allowed")]
    InvalidScheme {
        /// The invalid scheme
        scheme: String,
    },

    /// URL is missing host component
    #[error("URL is missing host component: {url}")]
    MissingHost {
        /// The URL without host
        url: String,
    },

    /// Hostname is in the blocked list
    #[error("Hostname '{hostname}' is blocked")]
    BlockedHostname {
        /// The blocked hostname
        hostname: String,
    },

    /// DNS resolution failed
    #[error("DNS resolution failed for '{hostname}': {reason}")]
    DnsResolutionFailed {
        /// The hostname that failed to resolve
        hostname: String,
        /// Reason for failure
        reason: String,
    },

    /// No IP addresses resolved for hostname
    #[error("No IP addresses resolved for hostname '{hostname}'")]
    NoIpResolved {
        /// The hostname with no IPs
        hostname: String,
    },

    /// DNS rebinding attack detected
    #[error("DNS rebinding attack detected: hostname '{hostname}' resolved to mixed private/public IPs: {ips:?}")]
    DnsRebindingDetected {
        /// The suspicious hostname
        hostname: String,
        /// List of resolved IPs
        ips: Vec<String>,
    },

    /// Attempt to access private IP address
    #[error("Private IP access is not allowed: {ip}")]
    PrivateIpAccess {
        /// The private IP address
        ip: String,
    },

    /// Redirect to internal URL detected
    #[error("Redirect to internal URL is not allowed: {url}")]
    RedirectToInternal {
        /// The redirect target URL
        url: String,
    },

    /// Maximum redirect limit exceeded
    #[error("Maximum redirect limit ({limit}) exceeded")]
    MaxRedirectsExceeded {
        /// The redirect limit
        limit: u8,
    },

    /// TOCTOU attack detected
    #[error(
        "TOCTOU attack detected: connection IP {actual} does not match validated IP {expected}"
    )]
    ToctouAttack {
        /// Expected (validated) IP
        expected: IpAddr,
        /// Actual connection IP
        actual: IpAddr,
    },

    /// Domain is in blacklist
    #[error("Domain '{domain}' is in blacklist")]
    DomainBlacklisted {
        /// The blacklisted domain
        domain: String,
    },

    /// Port is not allowed
    #[error("Port {port} is not allowed")]
    PortNotAllowed {
        /// The disallowed port
        port: u16,
    },

    /// General validation error
    #[error("SSRF validation error: {0}")]
    ValidationFailed(String),
}

impl SsrfError {
    /// Sanitized machine-readable category for this error.
    ///
    /// Unlike `Display`, this never embeds hostnames, IPs, or URLs,
    /// so it is safe to surface in API responses, webhook event records,
    /// and metrics labels without leaking internal network topology.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::UrlTooLong { .. } => "url_too_long",
            Self::InvalidUrl { .. } => "invalid_url",
            Self::InvalidScheme { .. } => "invalid_scheme",
            Self::MissingHost { .. } => "missing_host",
            Self::BlockedHostname { .. } => "blocked_hostname",
            Self::DnsResolutionFailed { .. } => "dns_resolution_failed",
            Self::NoIpResolved { .. } => "no_ip_resolved",
            Self::DnsRebindingDetected { .. } => "dns_rebinding",
            Self::PrivateIpAccess { .. } => "private_ip_access",
            Self::RedirectToInternal { .. } => "redirect_to_internal",
            Self::MaxRedirectsExceeded { .. } => "max_redirects_exceeded",
            Self::ToctouAttack { .. } => "toctou_attack",
            Self::DomainBlacklisted { .. } => "domain_blacklisted",
            Self::PortNotAllowed { .. } => "port_not_allowed",
            Self::ValidationFailed(_) => "validation_failed",
        }
    }

    /// Check if this error indicates a potential attack.
    ///
    /// Returns true for errors that suggest malicious intent.
    pub fn is_attack(&self) -> bool {
        matches!(
            self,
            Self::DnsRebindingDetected { .. }
                | Self::ToctouAttack { .. }
                | Self::RedirectToInternal { .. }
        )
    }

    /// Check if this error is retryable.
    ///
    /// Returns true for transient errors that might succeed on retry.
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Self::DnsResolutionFailed { .. } | Self::NoIpResolved { .. }
        )
    }

    /// Get a security severity level for this error.
    ///
    /// Returns 'critical', 'high', 'medium', or 'low'.
    pub fn severity(&self) -> &'static str {
        match self {
            Self::DnsRebindingDetected { .. }
            | Self::ToctouAttack { .. }
            | Self::RedirectToInternal { .. } => "critical",
            Self::BlockedHostname { .. }
            | Self::PrivateIpAccess { .. }
            | Self::InvalidScheme { .. } => "high",
            Self::InvalidUrl { .. } | Self::MissingHost { .. } | Self::DomainBlacklisted { .. } => {
                "medium"
            }
            _ => "low",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_attack() {
        let error = SsrfError::DnsRebindingDetected {
            hostname: "evil.com".to_string(),
            ips: vec!["10.0.0.1".to_string(), "8.8.8.8".to_string()],
        };
        assert!(error.is_attack());

        let error = SsrfError::PrivateIpAccess {
            ip: "10.0.0.1".to_string(),
        };
        assert!(!error.is_attack());
    }

    #[test]
    fn test_kind_is_sanitized() {
        // kind() 只返回固定类别名，不得携带任何主机名/IP/URL 细节
        let cases: Vec<(SsrfError, &'static str)> = vec![
            (
                SsrfError::PrivateIpAccess {
                    ip: "10.0.0.1".to_string(),
                },
                "private_ip_access",
            ),
            (
                SsrfError::DnsRebindingDetected {
                    hostname: "internal.corp".to_string(),
                    ips: vec!["10.0.0.1".to_string()],
                },
                "dns_rebinding",
            ),
            (
                SsrfError::BlockedHostname {
                    hostname: "localhost".to_string(),
                },
                "blocked_hostname",
            ),
            (SsrfError::PortNotAllowed { port: 5432 }, "port_not_allowed"),
            (
                SsrfError::ValidationFailed("http://10.0.0.1 bad".to_string()),
                "validation_failed",
            ),
        ];
        for (error, expected_kind) in cases {
            assert_eq!(error.kind(), expected_kind);
            assert!(!error.kind().contains("10.0.0.1"));
            assert!(!error.kind().contains("localhost"));
            assert!(!error.kind().contains("internal.corp"));
        }
    }

    #[test]
    fn test_is_retryable() {
        let error = SsrfError::DnsResolutionFailed {
            hostname: "example.com".to_string(),
            reason: "timeout".to_string(),
        };
        assert!(error.is_retryable());

        let error = SsrfError::PrivateIpAccess {
            ip: "10.0.0.1".to_string(),
        };
        assert!(!error.is_retryable());
    }

    #[test]
    fn test_severity() {
        let error = SsrfError::DnsRebindingDetected {
            hostname: "evil.com".to_string(),
            ips: vec![],
        };
        assert_eq!(error.severity(), "critical");

        let error = SsrfError::BlockedHostname {
            hostname: "localhost".to_string(),
        };
        assert_eq!(error.severity(), "high");

        let error = SsrfError::InvalidUrl {
            url: "invalid".to_string(),
            reason: "parse error".to_string(),
        };
        assert_eq!(error.severity(), "medium");

        let error = SsrfError::UrlTooLong {
            max: 2048,
            actual: 3000,
        };
        assert_eq!(error.severity(), "low");
    }
}
