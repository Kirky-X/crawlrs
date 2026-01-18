// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! SSRF Protection Helper
//!
//! Provides unified URL validation to prevent Server-Side Request Forgery attacks.
//! This module consolidates SSRF protection logic from crawl_handler and scrape_handler.
//!
//! ## Features
//! - IPv4 private address detection (10.x, 172.16-172.31, 192.168.x)
//! - IPv6 localhost and link-local address detection
//! - Comprehensive coverage of RFC 1918 and RFC 4291 private addresses

use url::Url;

/// Check if a URL points to an internal/private network address.
///
/// This function prevents SSRF attacks by blocking requests to:
/// - IPv4 private addresses (RFC 1918: 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16)
/// - IPv6 localhost (::1)
/// - IPv6 link-local addresses (fe80::/10, fe00::/9)
///
/// # Arguments
///
/// * `url_str` - The URL string to check
///
/// # Returns
///
/// * `true` if the URL points to an internal address
/// * `false` if the URL is external
///
/// # Examples
///
/// ```
/// use crate::presentation::helpers::ssrf_helper::is_internal_url;
///
/// assert!(is_internal_url("http://localhost:8080"));
/// assert!(is_internal_url("http://127.0.0.1:8080"));
/// assert!(is_internal_url("http://10.0.0.1"));
/// assert!(is_internal_url("http://192.168.1.1"));
/// assert!(is_internal_url("http://172.16.0.1"));
/// assert!(is_internal_url("http://[::1]:8080"));
/// assert!(is_internal_url("http://[fe80::1]"));
///
/// assert!(!is_internal_url("http://8.8.8.8"));
/// assert!(!is_internal_url("http://google.com"));
/// assert!(!is_internal_url("http://[2001:db8::1]"));
/// ```
pub fn is_internal_url(url_str: &str) -> bool {
    let Ok(url) = Url::parse(url_str) else {
        return false;
    };

    let Some(host) = url.host_str() else {
        return false;
    };

    let host = if host.starts_with('[') && host.ends_with(']') {
        &host[1..host.len() - 1]
    } else {
        host
    };

    // IPv4 localhost
    if host == "localhost" || host == "127.0.0.1" {
        return true;
    }

    // IPv6 localhost
    if host == "::1" {
        return true;
    }

    // IPv4 private ranges (RFC 1918)
    if host.starts_with("10.") {
        return true;
    }

    if host.starts_with("192.168.") {
        return true;
    }

    // IPv4 private range 172.16.0.0 - 172.31.255.255
    if let Some(rest) = host.strip_prefix("172.") {
        if let Some(second_octet) = rest.split('.').next() {
            if let Ok(num) = second_octet.parse::<u8>() {
                if (16..=31).contains(&num) {
                    return true;
                }
            }
        }
    }

    // IPv4 link-local (169.254.0.0/16) - used for auto-configuration
    if host.starts_with("169.254.") {
        return true;
    }

    // IPv6 link-local (fe80::/10 = fe80: to febf:ffff:...)
    // Check if second hex digit is between 8 and B (inclusive)
    if host.starts_with("fe") {
        if let Some(second_octet) = host.get(2..3) {
            let second_octet_upper = second_octet.to_ascii_uppercase();
            if let Some(c) = second_octet_upper.chars().next() {
                if ('8'..='B').contains(&c) {
                    return true;
                }
            }
        }
    }

    // IPv6 unique local addresses (ULA) - fc00::/7
    // fc00::/8 is reserved, fd00::/8 is used for private networks (RFC 4193)
    if host.starts_with("fc") || host.starts_with("fd") {
        return true;
    }

    // IPv6 site-local (fec0::/10 - deprecated but some networks may still use it)
    if host.starts_with("fec0:")
        || host.starts_with("fed0:")
        || host.starts_with("fee0:")
        || host.starts_with("fef0:")
    {
        return true;
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ipv4_localhost() {
        assert!(is_internal_url("http://localhost"));
        assert!(is_internal_url("http://localhost:8080"));
        assert!(is_internal_url("http://127.0.0.1"));
        assert!(is_internal_url("http://127.0.0.1:3000"));
    }

    #[test]
    fn test_ipv4_private_10() {
        assert!(is_internal_url("http://10.0.0.1"));
        assert!(is_internal_url("http://10.10.10.10"));
        assert!(is_internal_url("http://10.255.255.255"));
    }

    #[test]
    fn test_ipv4_private_172() {
        assert!(is_internal_url("http://172.16.0.1"));
        assert!(is_internal_url("http://172.20.0.1"));
        assert!(is_internal_url("http://172.31.0.1"));
        // 172.0-172.15 should not match
        assert!(!is_internal_url("http://172.15.0.1"));
        assert!(!is_internal_url("http://172.32.0.1"));
    }

    #[test]
    fn test_ipv4_private_192() {
        assert!(is_internal_url("http://192.168.0.1"));
        assert!(is_internal_url("http://192.168.100.200"));
        assert!(is_internal_url("http://192.168.255.255"));
    }

    #[test]
    fn test_ipv4_link_local() {
        assert!(is_internal_url("http://169.254.0.1"));
        assert!(is_internal_url("http://169.254.255.255"));
    }

    #[test]
    fn test_ipv6_localhost() {
        assert!(is_internal_url("http://[::1]"));
        assert!(is_internal_url("http://[::1]:8080"));
    }

    #[test]
    fn test_ipv6_link_local() {
        assert!(is_internal_url("http://[fe80::1]"));
        assert!(is_internal_url("http://[fe80:abcd::1]"));
        assert!(is_internal_url("http://[febf::1]"));
    }

    #[test]
    fn test_ipv6_site_local() {
        // Deprecated but included for completeness
        assert!(is_internal_url("http://[fec0::1]"));
        assert!(is_internal_url("http://[fef0::1]"));
    }

    #[test]
    fn test_ipv6_unique_local() {
        // RFC 4193 - Unique Local Addresses (ULA) fc00::/7
        assert!(is_internal_url("http://[fd00::1]"));
        assert!(is_internal_url("http://[fd12:3456:789a::1]"));
        assert!(is_internal_url("http://[fc00::1]")); // Reserved but block it
    }

    #[test]
    fn test_external_ipv4() {
        assert!(!is_internal_url("http://8.8.8.8"));
        assert!(!is_internal_url("http://1.1.1.1"));
        assert!(!is_internal_url("http://9.9.9.9"));
    }

    #[test]
    fn test_external_ipv6() {
        assert!(!is_internal_url("http://[2001:db8::1]"));
        assert!(!is_internal_url("http://[2606:4700::1]")); // cloudflare
    }

    #[test]
    fn test_external_hostnames() {
        assert!(!is_internal_url("http://google.com"));
        assert!(!is_internal_url("https://www.example.com"));
        assert!(!is_internal_url("http://api.github.com"));
    }

    #[test]
    fn test_invalid_urls() {
        assert!(!is_internal_url("not-a-url"));
        assert!(!is_internal_url(""));
        assert!(!is_internal_url("http://"));
    }

    #[test]
    fn test_urls_with_paths() {
        assert!(is_internal_url("http://192.168.1.1/admin"));
        assert!(is_internal_url("http://10.0.0.1/api/v1/users"));
        assert!(!is_internal_url("http://google.com/admin"));
    }
}
