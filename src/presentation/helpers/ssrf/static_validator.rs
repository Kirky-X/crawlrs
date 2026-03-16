// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Static URL Validation for SSRF Protection
//!
//! Provides fast synchronous URL validation without DNS resolution.
//! This module performs static checks for:
//! - URL scheme validation
//! - Hostname pattern matching
//! - IP address range checking
//!
//! ## Usage
//!
//! Use this for quick pre-filtering before performing full async validation
//! with DNS resolution.

use url::Url;

/// Check if a URL points to an internal/private network address.
///
/// This function performs **static validation only** - it does NOT perform
/// DNS resolution. For complete SSRF protection, use `SsrfValidator::validate()`
/// which includes DNS resolution and rebinding protection.
///
/// # Security Note
///
/// This function alone is **NOT sufficient** for SSRF protection because:
/// 1. It cannot detect DNS rebinding attacks
/// 2. It cannot validate domain names that resolve to private IPs
/// 3. It only checks the URL string, not the actual resolved address
///
/// Always combine with DNS-based validation for complete protection.
///
/// # Arguments
///
/// * `url_str` - The URL string to check
///
/// # Returns
///
/// * `true` if the URL points to an internal address (should be blocked)
/// * `false` if the URL appears to be external (needs further validation)
///
/// # Examples
///
/// ```
/// use crate::presentation::helpers::ssrf::is_internal_url;
///
/// // Internal URLs - returns true
/// assert!(is_internal_url("http://localhost:8080"));
/// assert!(is_internal_url("http://127.0.0.1:8080"));
/// assert!(is_internal_url("http://10.0.0.1"));
/// assert!(is_internal_url("http://192.168.1.1"));
/// assert!(is_internal_url("http://172.16.0.1"));
/// assert!(is_internal_url("http://[::1]:8080"));
/// assert!(is_internal_url("http://[fe80::1]"));
///
/// // External URLs - returns false
/// assert!(!is_internal_url("http://8.8.8.8"));
/// assert!(!is_internal_url("http://google.com"));
/// assert!(!is_internal_url("http://[2001:db8::1]"));
/// ```
pub fn is_internal_url(url_str: &str) -> bool {
    let Ok(url) = Url::parse(url_str) else {
        return false;
    };

    // Check URL scheme - only allow http and https
    if !matches!(url.scheme(), "http" | "https") {
        return true; // Block non-HTTP/HTTPS URLs
    }

    let Some(host) = url.host_str() else {
        return false;
    };

    // Normalize IPv6 host (remove brackets)
    let host = if host.starts_with('[') && host.ends_with(']') {
        &host[1..host.len() - 1]
    } else {
        host
    };

    // Check for IPv4-mapped IPv6 addresses (::ffff:192.168.1.1)
    if host.contains("::ffff:") || host.contains("::FFFF:") {
        return true;
    }

    // === IPv4 Checks ===

    // Localhost variants
    if is_localhost(host) {
        return true;
    }

    // Private IPv4 ranges (RFC 1918)
    if is_private_ipv4(host) {
        return true;
    }

    // IPv4 link-local (169.254.0.0/16)
    if is_link_local_ipv4(host) {
        return true;
    }

    // Carrier-grade NAT (100.64.0.0/10)
    if is_carrier_grade_nat(host) {
        return true;
    }

    // Broadcast and unspecified addresses
    if is_broadcast_or_unspecified(host) {
        return true;
    }

    // === IPv6 Checks ===

    // IPv6 localhost
    if host == "::1" {
        return true;
    }

    // IPv6 link-local (fe80::/10)
    if is_link_local_ipv6(host) {
        return true;
    }

    // IPv6 unique local addresses (ULA) - fc00::/7
    if is_unique_local_ipv6(host) {
        return true;
    }

    // IPv6 site-local (deprecated but still check)
    if is_site_local_ipv6(host) {
        return true;
    }

    false
}

/// Check if hostname is a localhost variant.
fn is_localhost(host: &str) -> bool {
    let host_lower = host.to_lowercase();
    host_lower == "localhost"
        || host_lower == "localhost.localdomain"
        || host_lower == "ip6-localhost"
        || host_lower == "ip6-loopback"
        || host_lower.starts_with("127.")
}

/// Check if host is in private IPv4 ranges (RFC 1918).
fn is_private_ipv4(host: &str) -> bool {
    // 10.0.0.0/8
    if host.starts_with("10.") {
        return true;
    }

    // 192.168.0.0/16
    if host.starts_with("192.168.") {
        return true;
    }

    // 172.16.0.0/12 (172.16.0.0 - 172.31.255.255)
    if let Some(rest) = host.strip_prefix("172.") {
        if let Some(second_octet) = rest.split('.').next() {
            if let Ok(num) = second_octet.parse::<u8>() {
                if (16..=31).contains(&num) {
                    return true;
                }
            }
        }
    }

    false
}

/// Check if host is in IPv4 link-local range (169.254.0.0/16).
fn is_link_local_ipv4(host: &str) -> bool {
    host.starts_with("169.254.")
}

/// Check if host is in carrier-grade NAT range (100.64.0.0/10).
fn is_carrier_grade_nat(host: &str) -> bool {
    if let Some(rest) = host.strip_prefix("100.") {
        if let Some(second_octet) = rest.split('.').next() {
            if let Ok(num) = second_octet.parse::<u8>() {
                if (64..=127).contains(&num) {
                    return true;
                }
            }
        }
    }
    false
}

/// Check if host is broadcast or unspecified address.
fn is_broadcast_or_unspecified(host: &str) -> bool {
    // 0.0.0.0/8 - unspecified and "this network"
    if host.starts_with("0.") {
        return true;
    }

    // 255.255.255.255 - broadcast
    if host == "255.255.255.255" {
        return true;
    }

    // 224.0.0.0/4 - multicast
    if let Some(first_octet) = host.split('.').next() {
        if let Ok(num) = first_octet.parse::<u8>() {
            if (224..=239).contains(&num) {
                return true;
            }
        }
    }

    false
}

/// Check if host is in IPv6 link-local range (fe80::/10).
///
/// Link-local addresses have the format fe80::/10, meaning the first 10 bits
/// are 1111111010 (0xfe8 in hex). This covers fe80: through febf:.
fn is_link_local_ipv6(host: &str) -> bool {
    let host_lower = host.to_lowercase();
    if host_lower.starts_with("fe") {
        if let Some(third_char) = host_lower.chars().nth(2) {
            // Check if third character is 8, 9, a, b (case insensitive)
            return matches!(third_char, '8' | '9' | 'a' | 'b');
        }
    }
    false
}

/// Check if host is in IPv6 unique local address range (fc00::/7).
///
/// ULA addresses have the first 7 bits as 1111110, covering fc00::/8 and fd00::/8.
fn is_unique_local_ipv6(host: &str) -> bool {
    let host_lower = host.to_lowercase();
    host_lower.starts_with("fc") || host_lower.starts_with("fd")
}

/// Check if host is in IPv6 site-local range (deprecated).
///
/// Site-local addresses (fec0::/10) are deprecated by RFC 3879,
/// but we still block them for security.
fn is_site_local_ipv6(host: &str) -> bool {
    let host_lower = host.to_lowercase();
    host_lower.starts_with("fec0:")
        || host_lower.starts_with("fed0:")
        || host_lower.starts_with("fee0:")
        || host_lower.starts_with("fef0:")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_localhost() {
        assert!(is_internal_url("http://localhost"));
        assert!(is_internal_url("http://localhost:8080"));
        assert!(is_internal_url("http://LOCALHOST"));
        assert!(is_internal_url("http://localhost.localdomain"));
        assert!(is_internal_url("http://127.0.0.1"));
        assert!(is_internal_url("http://127.0.0.1:3000"));
        assert!(is_internal_url("http://127.255.255.255"));
    }

    #[test]
    fn test_private_ipv4_10() {
        assert!(is_internal_url("http://10.0.0.1"));
        assert!(is_internal_url("http://10.10.10.10"));
        assert!(is_internal_url("http://10.255.255.255"));
    }

    #[test]
    fn test_private_ipv4_172() {
        assert!(is_internal_url("http://172.16.0.1"));
        assert!(is_internal_url("http://172.20.0.1"));
        assert!(is_internal_url("http://172.31.0.1"));
        // 172.0-172.15 should NOT match
        assert!(!is_internal_url("http://172.15.0.1"));
        assert!(!is_internal_url("http://172.32.0.1"));
    }

    #[test]
    fn test_private_ipv4_192() {
        assert!(is_internal_url("http://192.168.0.1"));
        assert!(is_internal_url("http://192.168.100.200"));
        assert!(is_internal_url("http://192.168.255.255"));
    }

    #[test]
    fn test_link_local_ipv4() {
        assert!(is_internal_url("http://169.254.0.1"));
        assert!(is_internal_url("http://169.254.255.255"));
    }

    #[test]
    fn test_carrier_grade_nat() {
        assert!(is_internal_url("http://100.64.0.1"));
        assert!(is_internal_url("http://100.127.255.255"));
        // Outside CGN range
        assert!(!is_internal_url("http://100.63.255.255"));
        assert!(!is_internal_url("http://100.128.0.1"));
    }

    #[test]
    fn test_broadcast_and_unspecified() {
        assert!(is_internal_url("http://0.0.0.0"));
        assert!(is_internal_url("http://0.0.0.1"));
        assert!(is_internal_url("http://255.255.255.255"));
    }

    #[test]
    fn test_multicast() {
        assert!(is_internal_url("http://224.0.0.1"));
        assert!(is_internal_url("http://239.255.255.255"));
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
        // Not link-local
        assert!(!is_internal_url("http://[fec0::1]"));
    }

    #[test]
    fn test_ipv6_unique_local() {
        assert!(is_internal_url("http://[fc00::1]"));
        assert!(is_internal_url("http://[fd00::1]"));
        assert!(is_internal_url("http://[fd12:3456:789a::1]"));
    }

    #[test]
    fn test_ipv6_site_local() {
        assert!(is_internal_url("http://[fec0::1]"));
        assert!(is_internal_url("http://[fef0::1]"));
    }

    #[test]
    fn test_ipv4_mapped_ipv6() {
        assert!(is_internal_url("http://[::ffff:192.168.1.1]"));
        assert!(is_internal_url("http://[::FFFF:10.0.0.1]"));
    }

    #[test]
    fn test_external_ipv4() {
        assert!(!is_internal_url("http://8.8.8.8"));
        assert!(!is_internal_url("http://1.1.1.1"));
        assert!(!is_internal_url("http://9.9.9.9"));
        assert!(!is_internal_url("http://142.250.185.46")); // Google
    }

    #[test]
    fn test_external_ipv6() {
        assert!(!is_internal_url("http://[2001:4860:4860::8888]")); // Google DNS
        assert!(!is_internal_url("http://[2606:4700::1]")); // Cloudflare
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
    fn test_invalid_schemes() {
        assert!(is_internal_url("file:///etc/passwd"));
        assert!(is_internal_url("ftp://example.com"));
        assert!(is_internal_url("gopher://localhost"));
        assert!(is_internal_url("javascript:alert(1)"));
        assert!(is_internal_url("data:text/plain,hello"));
    }

    #[test]
    fn test_urls_with_paths() {
        assert!(is_internal_url("http://192.168.1.1/admin"));
        assert!(is_internal_url("http://10.0.0.1/api/v1/users"));
        assert!(!is_internal_url("http://google.com/admin"));
    }

    #[test]
    fn test_urls_with_query_and_fragment() {
        assert!(is_internal_url("http://localhost?query=test"));
        assert!(is_internal_url("http://127.0.0.1#fragment"));
        assert!(!is_internal_url("http://example.com?query=test"));
    }
}
