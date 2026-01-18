// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 增强的URL验证器
//!
//! 提供URL验证、DNS缓存支持和SSRF防护

use std::net::IpAddr;
use std::sync::Arc;
use tokio::net::lookup_host;
use url::Url;

use crate::infrastructure::dns::dns_cache::DnsCache;

/// 验证错误类型
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

    #[error("SSRF protection disabled in production is not allowed")]
    SsrfDisabledInProduction,

    #[error("SSRF protection can only be disabled in test/development environments")]
    InvalidEnvironment,
}

const MAX_URL_LENGTH: usize = 2048;

/// 增强的URL验证器（使用DNS缓存）
#[derive(Clone)]
pub struct ValidatedUrlValidator {
    dns_cache: Arc<DnsCache>,
}

impl ValidatedUrlValidator {
    /// 创建新的验证器
    pub fn new(dns_cache: Arc<DnsCache>) -> Self {
        Self { dns_cache }
    }

    /// 验证 URL 是否安全 (防止 SSRF)
    ///
    /// 使用DNS缓存减少重复查询，防止泄露访问模式
    pub async fn validate_url(&self, url_str: &str) -> Result<(), ValidationError> {
        // URL 长度限制：防止 SSRF 和资源耗尽攻击
        if url_str.len() > MAX_URL_LENGTH {
            return Err(ValidationError::UrlTooLong);
        }

        // 检查是否禁用 SSRF 保护（仅用于测试环境）
        // 安全警告：生产环境必须启用 SSRF 保护
        let ssrf_disabled = std::env::var("CRAWLRS_DISABLE_SSRF_PROTECTION").unwrap_or_default();
        if ssrf_disabled.eq_ignore_ascii_case("true") {
            tracing::warn!(
                "SECURITY WARNING: SSRF protection is DISABLED for URL: {}. This should NEVER be enabled in production!",
                url_str
            );

            let env = std::env::var("CRAWLRS_ENV").unwrap_or_default();
            if env.eq_ignore_ascii_case("production") || env.eq_ignore_ascii_case("prod") {
                return Err(ValidationError::SsrfDisabledInProduction);
            }

            if !env.eq_ignore_ascii_case("test")
                && !env.eq_ignore_ascii_case("development")
                && !env.eq_ignore_ascii_case("dev")
            {
                tracing::error!(
                    "SECURITY ERROR: Attempted to disable SSRF protection in unknown environment: {}. This is not allowed.",
                    env
                );
                return Err(ValidationError::InvalidEnvironment);
            }
            return Ok(());
        }

        let url = Url::parse(url_str)
            .map_err(|_| ValidationError::InvalidProtocol(url_str.to_string()))?;

        // 检查协议：只允许 http 和 https
        match url.scheme() {
            "http" | "https" => {}
            scheme => {
                return Err(ValidationError::InvalidProtocol(scheme.to_string()));
            }
        }

        let host = url
            .host_str()
            .ok_or_else(|| ValidationError::BlockedHostname("missing".to_string()))?;

        // 基于域名的预检查（在 DNS 解析之前）
        if is_blocked_hostname(host) {
            return Err(ValidationError::BlockedHostname(host.to_string()));
        }

        // 使用DNS缓存进行解析
        let port = url.port_or_known_default().unwrap_or(80);
        let addrs: Vec<IpAddr> = self
            .dns_cache
            .lookup_host(host, port)
            .await
            .map_err(|_| ValidationError::NoIpResolved)?;

        // DNS Rebinding 防护：检查所有解析出的 IP
        if addrs.is_empty() {
            return Err(ValidationError::NoIpResolved);
        }

        // 检查是否存在混合的公网/私网 IP（DNS rebinding 特征）
        let has_private = addrs.iter().any(|addr| is_private_ip(addr.ip()));
        let has_public = addrs.iter().any(|addr| !is_private_ip(addr.ip()));

        if has_private && has_public {
            return Err(ValidationError::MixedIpAddresses);
        }

        // 检查所有解析出的 IP 是否为私有地址
        for addr in &addrs {
            if is_private_ip(addr.ip()) {
                return Err(ValidationError::PrivateIpAccess(addr.ip().to_string()));
            }
        }

        // DNS Rebinding 防护：验证所有 IP 是否一致
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

/// 检查主机名是否在阻止列表中
fn is_blocked_hostname(host: &str) -> bool {
    let host_lower = host.to_lowercase();

    let blocked = [
        "localhost",
        "localhost.localdomain",
        "ip6-localhost",
        "ip6-loopback",
        "0.0.0.0",
        "::1",
        "::",
        "169.254.169.254",
        "metadata.google.internal",
        "metadata.azure.com",
        "metadata.msftidentity.com",
        "metadata.google.internal",
        "metadata.nova.canonical.com",
        "metadata.packet.csi.com",
    ];

    if blocked.iter().any(|&b| host_lower == b) {
        return true;
    }

    // 检查是否为纯 IP 地址形式的字符串
    if let Ok(ip) = host_lower.parse::<std::net::IpAddr>() {
        return is_private_ip(ip);
    }

    false
}

fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            if ipv4.octets()[0] == 10 {
                return true;
            }
            if ipv4.octets()[0] == 172 && (ipv4.octets()[1] >= 16 && ipv4.octets()[1] <= 31) {
                return true;
            }
            if ipv4.octets()[0] == 192 && ipv4.octets()[1] == 168 {
                return true;
            }
            if ipv4.is_loopback() {
                return true;
            }
            if ipv4.is_link_local() {
                return true;
            }
            if ipv4.octets()[0] >= 224 && ipv4.octets()[0] <= 239 {
                return true;
            }
            if ipv4.octets()[0] == 100 && ipv4.octets()[1] >= 64 && ipv4.octets()[1] <= 127 {
                return true;
            }
            if ipv4.octets()[0] == 0 {
                return true;
            }
            if ipv4.octets()[0] == 255
                && ipv4.octets()[1] == 255
                && ipv4.octets()[2] == 255
                && ipv4.octets()[3] == 255
            {
                return true;
            }
            false
        }
        IpAddr::V6(ipv6) => {
            if ipv6.is_loopback() {
                return true;
            }
            if (ipv6.segments()[0] & 0xfe00) == 0xfc00 {
                return true;
            }
            if (ipv6.segments()[0] & 0xffc0) == 0xfe80 {
                return true;
            }
            if (ipv6.segments()[0] & 0xff00) == 0xff00 {
                return true;
            }
            if ipv6.segments()[0] == 0
                && ipv6.segments()[1] == 0
                && ipv6.segments()[2] == 0
                && ipv6.segments()[3] == 0
                && ipv6.segments()[4] == 0
                && ipv6.segments()[5] == 0
                && ipv6.segments()[6] == 0
                && ipv6.segments()[7] == 0
            {
                return true;
            }
            if ipv6.segments()[0] == 0x2001 && ipv6.segments()[1] == 0x0db8 {
                return true;
            }
            false
        }
    }
}

/// 验证 URL 是否在黑名单域名中
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

        // 本地主机应该被阻止
        assert!(validator.validate_url("http://localhost").await.is_err());
        assert!(validator.validate_url("http://127.0.0.1").await.is_err());

        // 私有IP应该被阻止
        assert!(validator.validate_url("http://10.0.0.1").await.is_err());
        assert!(validator.validate_url("http://192.168.1.1").await.is_err());

        // 云元数据端点应该被阻止
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
