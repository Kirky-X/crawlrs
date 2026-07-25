// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Shared validation utilities for SSRF protection
//!
//! Contains common validation functions used across multiple validator implementations
//! to ensure consistent security checks and avoid code duplication.

use std::net::IpAddr;

/// Check if an IP address is a private/reserved address that should be blocked for SSRF protection.
///
/// This function covers:
/// - IPv4 private addresses (RFC 1918): 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
/// - IPv4 loopback: 127.0.0.0/8
/// - IPv4 link-local: 169.254.0.0/16
/// - IPv4 multicast: 224.0.0.0/4
/// - IPv4 carrier-grade NAT: 100.64.0.0/10
/// - IPv4 broadcast: 0.0.0.0/8, 255.255.255.255
/// - IPv6 loopback: ::1/128
/// - IPv6 unique local (ULA): fc00::/7
/// - IPv6 link-local: fe80::/10
/// - IPv6 multicast: ff00::/8
/// - IPv6 unspecified: ::/128
/// - IPv6 documentation: 2001:db8::/32
pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            let octets = ipv4.octets();
            // 10.0.0.0/8
            octets[0] == 10
            // 172.16.0.0/12
            || (octets[0] == 172 && (16..=31).contains(&octets[1]))
            // 192.168.0.0/16
            || (octets[0] == 192 && octets[1] == 168)
            // Loopback 127.0.0.0/8
            || ipv4.is_loopback()
            // Link-local 169.254.0.0/16
            || ipv4.is_link_local()
            // Multicast 224.0.0.0/4
            || (224..=239).contains(&octets[0])
            // Carrier-Grade NAT 100.64.0.0/10
            || (octets[0] == 100 && (64..=127).contains(&octets[1]))
            // Broadcast 0.0.0.0/8
            || octets[0] == 0
            // 255.255.255.255
            || (octets[0] == 255 && octets[1] == 255 && octets[2] == 255 && octets[3] == 255)
        }
        IpAddr::V6(ipv6) => {
            let segs = ipv6.segments();
            ipv6.is_loopback()
                || (segs[0] & 0xfe00) == 0xfc00 // fc00::/7 ULA
                || (segs[0] & 0xffc0) == 0xfe80 // fe80::/10 link-local
                || (segs[0] & 0xff00) == 0xff00 // ff00::/8 multicast
                || segs == [0, 0, 0, 0, 0, 0, 0, 0] // ::/128 unspecified
                || (segs[0] == 0x2001 && segs[1] == 0x0db8) // 2001:db8::/32 documentation
                // IPv4-mapped IPv6 (::ffff:x.x.x.x) - use std lib (Rust 1.65+)
                || ipv6
                    .to_ipv4_mapped()
                    .map(|v4| is_private_ip(IpAddr::V4(v4)))
                    .unwrap_or(false)
                // IPv4-compatible IPv6 (::x.x.x.x, RFC 4291 deprecated) - bypass risk
                // to_ipv4_mapped() does NOT cover this form; must check explicitly.
                // Exclude ::/128 (unspecified) and ::1 (loopback) - already handled above.
                || (segs[0..6] == [0, 0, 0, 0, 0, 0]
                    && (segs[6] != 0 || segs[7] != 0)
                    && {
                        let ipv4 = std::net::Ipv4Addr::new(
                            (segs[6] >> 8) as u8,
                            (segs[6] & 0xff) as u8,
                            (segs[7] >> 8) as u8,
                            (segs[7] & 0xff) as u8,
                        );
                        is_private_ip(IpAddr::V4(ipv4))
                    })
        }
    }
}

/// List of blocked hostnames that should never be accessed via SSRF.
///
/// This includes:
/// - Localhost variants
/// - Link-local addresses as strings
/// - Cloud provider metadata endpoints
pub fn get_blocked_hostnames() -> &'static [&'static str] {
    &[
        // Localhost variants
        "localhost",
        "localhost.localdomain",
        "ip6-localhost",
        "ip6-loopback",
        // Link-local addresses
        "0.0.0.0",
        "::1",
        "::",
        // Cloud provider metadata endpoints
        "169.254.169.254",             // AWS, Azure, GCP
        "metadata.google.internal",    // GCP
        "metadata.azure.com",          // Azure
        "metadata.msftidentity.com",   // Azure
        "metadata.nova.canonical.com", // OpenStack
        "metadata.packet.csi.com",     // Packet
    ]
}

/// Check if a hostname is in the blocked list.
///
/// This function checks exact hostname matches only. For IP address checks,
/// use `is_private_ip()` instead.
pub fn is_blocked_hostname(host: &str) -> bool {
    let host_lower = host.to_lowercase();
    let blocked = get_blocked_hostnames();

    if blocked.iter().any(|&b| host_lower == b) {
        return true;
    }

    // IPv6 host from url::Url::host_str() comes wrapped in brackets (e.g. "[::1]").
    // Strip brackets before parsing to IpAddr, otherwise parse fails and IPv6
    // IP checks are silently skipped (SSRF bypass vector).
    let host_for_parse = host_lower
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(&host_lower);

    if let Ok(ip) = host_for_parse.parse::<std::net::IpAddr>() {
        return is_private_ip(ip);
    }

    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_private_ip_ipv4() {
        // Private ranges
        assert!(is_private_ip("10.0.0.1".parse().unwrap()));
        assert!(is_private_ip("10.255.255.255".parse().unwrap()));
        assert!(is_private_ip("172.16.0.1".parse().unwrap()));
        assert!(is_private_ip("172.31.255.255".parse().unwrap()));
        assert!(is_private_ip("192.168.0.1".parse().unwrap()));
        assert!(is_private_ip("192.168.255.255".parse().unwrap()));

        // Loopback
        assert!(is_private_ip("127.0.0.1".parse().unwrap()));
        assert!(is_private_ip("127.255.255.255".parse().unwrap()));

        // Link-local
        assert!(is_private_ip("169.254.0.1".parse().unwrap()));

        // Multicast
        assert!(is_private_ip("224.0.0.1".parse().unwrap()));
        assert!(is_private_ip("239.255.255.255".parse().unwrap()));

        // Carrier-grade NAT
        assert!(is_private_ip("100.64.0.1".parse().unwrap()));
        assert!(is_private_ip("100.127.255.255".parse().unwrap()));

        // Broadcast
        assert!(is_private_ip("0.0.0.0".parse().unwrap()));
        assert!(is_private_ip("255.255.255.255".parse().unwrap()));

        // Public IPs should not be private
        assert!(!is_private_ip("8.8.8.8".parse().unwrap()));
        assert!(!is_private_ip("1.1.1.1".parse().unwrap()));
        assert!(!is_private_ip("9.9.9.9".parse().unwrap()));
    }

    #[test]
    fn test_is_private_ip_ipv6() {
        // Loopback
        assert!(is_private_ip("::1".parse().unwrap()));

        // ULA
        assert!(is_private_ip("fc00::1".parse().unwrap()));
        assert!(is_private_ip("fd00::1".parse().unwrap()));

        // Link-local
        assert!(is_private_ip("fe80::1".parse().unwrap()));

        // Multicast
        assert!(is_private_ip("ff00::1".parse().unwrap()));

        // Unspecified
        assert!(is_private_ip("::".parse().unwrap()));

        // Documentation
        assert!(is_private_ip("2001:db8::1".parse().unwrap()));

        // IPv4-mapped IPv6 (::ffff:x.x.x.x) - must check embedded IPv4
        assert!(is_private_ip("::ffff:10.0.0.1".parse().unwrap()));
        assert!(is_private_ip("::ffff:127.0.0.1".parse().unwrap()));
        assert!(is_private_ip("::ffff:169.254.169.254".parse().unwrap()));
        assert!(is_private_ip("::ffff:192.168.1.1".parse().unwrap()));
        assert!(!is_private_ip("::ffff:8.8.8.8".parse().unwrap()));

        // IPv4-compatible IPv6 (::x.x.x.x, RFC 4291 deprecated) - bypass risk
        assert!(is_private_ip("::127.0.0.1".parse().unwrap()));
        assert!(is_private_ip("::169.254.169.254".parse().unwrap()));
        assert!(is_private_ip("::10.0.0.1".parse().unwrap()));
        assert!(is_private_ip("::192.168.1.1".parse().unwrap()));
        assert!(!is_private_ip("::8.8.8.8".parse().unwrap()));

        // Public IPv6 should not be private
        assert!(!is_private_ip("2001:4860:4860::8888".parse().unwrap())); // Google DNS
    }

    #[test]
    fn test_is_blocked_hostname() {
        let _blocked = get_blocked_hostnames();

        // Check blocked hostnames
        assert!(is_blocked_hostname("localhost"));
        assert!(is_blocked_hostname("127.0.0.1"));
        assert!(is_blocked_hostname("169.254.169.254"));
        assert!(is_blocked_hostname("metadata.google.internal"));

        // Case insensitive
        assert!(is_blocked_hostname("LOCALHOST"));
        assert!(is_blocked_hostname("LocalHost"));

        // IPv6 hosts from url::Url::host_str() come bracketed; must still be detected
        assert!(is_blocked_hostname("[::1]"));
        assert!(is_blocked_hostname("[::ffff:127.0.0.1]"));
        assert!(is_blocked_hostname("[::127.0.0.1]"));
        assert!(is_blocked_hostname("[fe80::1]"));

        // Not blocked
        assert!(!is_blocked_hostname("google.com"));
        assert!(!is_blocked_hostname("example.com"));
        assert!(!is_blocked_hostname("8.8.8.8"));
        assert!(!is_blocked_hostname("[2001:4860:4860::8888]"));
    }
}
