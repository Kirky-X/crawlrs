// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

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
