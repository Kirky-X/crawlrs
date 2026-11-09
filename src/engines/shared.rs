// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Shared validation utilities for SSRF protection
//!
//! 实现已下沉到 `infrastructure::security::ssrf`（消除 utils → engines
//! 反向依赖）；本模块仅保留 re-export 兼容既有 `engines::shared::*` 路径。

pub use crate::infrastructure::security::ssrf::{
    get_blocked_hostnames, is_blocked_hostname, is_private_ip,
};

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

        // IPv4-mapped IPv6 (ffff:x.x.x.x) - must check embedded IPv4
        assert!(is_private_ip("::ffff:10.0.0.1".parse().unwrap()));
        assert!(is_private_ip("::ffff:127.0.0.1".parse().unwrap()));
        assert!(is_private_ip("::ffff:169.254.169.254".parse().unwrap()));
        assert!(is_private_ip("::ffff:192.168.1.1".parse().unwrap()));
        assert!(!is_private_ip("::ffff:8.8.8.8".parse().unwrap()));

        // IPv4-compatible IPv6 (x.x.x.x, RFC 4291 deprecated) - bypass risk
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

        assert!(is_blocked_hostname("localhost"));
        assert!(is_blocked_hostname("127.0.0.1"));
        assert!(is_blocked_hostname("169.254.169.254"));
        assert!(is_blocked_hostname("metadata.google.internal"));

        // Case insensitive
        assert!(is_blocked_hostname("LOCALHOST"));
        assert!(is_blocked_hostname("LocalHost"));

        // IPv6 hosts from url::Url::host_str come bracketed; must still be detected
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
