// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use std::net::IpAddr;
use tokio::net::lookup_host;
use url::Url;

/// 验证 URL 是否安全 (防止 SSRF)
///
/// 检查解析后的 IP 是否为私有地址或环回地址
/// 包含 DNS Rebinding 攻击防护
///
/// # 安全警告
///
/// SSRF 保护是关键的安全功能，不应该在生产环境中禁用。
/// 如果确实需要在测试环境中禁用，请确保：
/// 1. 仅在受控的测试环境中使用
/// 2. 永远不要在生产环境或 CI/CD 流程中设置此环境变量
pub async fn validate_url(url_str: &str) -> anyhow::Result<()> {
    // 检查是否禁用 SSRF 保护（仅用于测试环境）
    // 安全警告：生产环境必须启用 SSRF 保护
    let ssrf_disabled = std::env::var("CRAWLRS_DISABLE_SSRF_PROTECTION").unwrap_or_default();
    if ssrf_disabled.eq_ignore_ascii_case("true") {
        // 添加安全警告日志
        tracing::warn!(
            "SECURITY WARNING: SSRF protection is DISABLED for URL: {}. This should NEVER be enabled in production!",
            url_str
        );
        // 检查是否在生产环境
        let env = std::env::var("CRAWLRS_ENV").unwrap_or_default();
        if env.eq_ignore_ascii_case("production") || env.eq_ignore_ascii_case("prod") {
            return Err(anyhow::anyhow!(
                "SECURITY ERROR: SSRF protection cannot be disabled in production environment"
            ));
        }
        // 额外检查：如果不是明确的测试环境，也拒绝禁用
        if !env.eq_ignore_ascii_case("test")
            && !env.eq_ignore_ascii_case("development")
            && !env.eq_ignore_ascii_case("dev")
        {
            tracing::error!(
                "SECURITY ERROR: Attempted to disable SSRF protection in unknown environment: {}. This is not allowed.",
                env
            );
            return Err(anyhow::anyhow!(
                "SECURITY ERROR: SSRF protection can only be disabled in test/development environments"
            ));
        }
        return Ok(());
    }

    let url = Url::parse(url_str)?;

    // 检查协议：只允许 http 和 https
    match url.scheme() {
        "http" | "https" => {}
        scheme => {
            return Err(anyhow::anyhow!(
                "SSRF protection: Only http and https protocols are allowed, got: {}",
                scheme
            ));
        }
    }

    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Missing host"))?;

    // 基于域名的预检查（在 DNS 解析之前）
    if is_blocked_hostname(host) {
        return Err(anyhow::anyhow!(
            "SSRF protection: Hostname '{}' is not allowed",
            host
        ));
    }

    // 解析 DNS
    let port = url.port_or_known_default().unwrap_or(80);
    let addr_str = format!("{}:{}", host, port);

    let addrs: Vec<_> = lookup_host(&addr_str).await?.collect();

    // DNS Rebinding 防护：检查所有解析出的 IP
    if addrs.is_empty() {
        return Err(anyhow::anyhow!(
            "SSRF protection: No IP addresses resolved for host"
        ));
    }

    // 检查是否存在混合的公网/私网 IP（DNS rebinding 特征）
    let has_private = addrs.iter().any(|addr| is_private_ip(addr.ip()));
    let has_public = addrs.iter().any(|addr| !is_private_ip(addr.ip()));

    if has_private && has_public {
        return Err(anyhow::anyhow!(
            "SSRF protection: Mixed private and public IP addresses detected (possible DNS rebinding attack)"
        ));
    }

    // 检查所有解析出的 IP 是否为私有地址
    for addr in &addrs {
        if is_private_ip(addr.ip()) {
            return Err(anyhow::anyhow!(
                "SSRF protection: Private IP access is not allowed: {}",
                addr.ip()
            ));
        }
    }

    // DNS Rebinding 防护：验证所有 IP 是否一致
    // 如果有多个 IP，确保它们都属于同一类别（公网）
    if addrs.len() > 1 {
        // 记录警告：多个 IP 解析可能表示 DNS 不稳定
        tracing::warn!(
            "DNS warning: Host {} resolved to {} IP addresses",
            host,
            addrs.len()
        );
    }

    Ok(())
}

/// 检查主机名是否在阻止列表中
fn is_blocked_hostname(host: &str) -> bool {
    let host_lower = host.to_lowercase();

    // 使用 BTreeSet 避免重复并提供高效的查找
    use std::collections::BTreeSet;
    let blocked: BTreeSet<&str> = [
        // 本地主机变体
        "localhost",
        "localhost.localdomain",
        "ip6-localhost",
        "ip6-loopback",
        "0.0.0.0",
        "::1",
        "::",
        // AWS 元数据端点
        "169.254.169.254",
        "metadata.google.internal",
        // Azure 元数据端点
        "metadata.azure.com",
        "metadata.msftidentity.com",
        // GCP 元数据端点
        "metadata.google.internal",
        // 其他云服务元数据端点
        "metadata.nova.canonical.com",
        "metadata.packet.csi.com",
    ]
    .into_iter()
    .collect();

    if blocked.contains(host_lower.as_str()) {
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
            // 10.0.0.0/8 - RFC 1918 私有地址
            if ipv4.octets()[0] == 10 {
                return true;
            }
            // 172.16.0.0/12 - RFC 1918 私有地址
            if ipv4.octets()[0] == 172 && (ipv4.octets()[1] >= 16 && ipv4.octets()[1] <= 31) {
                return true;
            }
            // 192.168.0.0/16 - RFC 1918 私有地址
            if ipv4.octets()[0] == 192 && ipv4.octets()[1] == 168 {
                return true;
            }
            // 127.0.0.0/8 - 环回地址
            if ipv4.is_loopback() {
                return true;
            }
            // 169.254.0.0/16 -链路本地地址 (Link-local)
            if ipv4.is_link_local() {
                return true;
            }
            // 224.0.0.0/4 - 多播地址
            if ipv4.octets()[0] >= 224 && ipv4.octets()[0] <= 239 {
                return true;
            }
            // 100.64.0.0/10 - 共享地址空间 (Carrier-Grade NAT)
            if ipv4.octets()[0] == 100 && ipv4.octets()[1] >= 64 && ipv4.octets()[1] <= 127 {
                return true;
            }
            // 0.0.0.0/8 - 广播源地址
            if ipv4.octets()[0] == 0 {
                return true;
            }
            // 255.255.255.255 - 广播地址
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
            // ::1/128 - IPv6 环回地址
            if ipv6.is_loopback() {
                return true;
            }
            // fc00::/7 - 唯一本地地址 (ULA)
            if (ipv6.segments()[0] & 0xfe00) == 0xfc00 {
                return true;
            }
            // fe80::/10 - 链路本地地址
            if (ipv6.segments()[0] & 0xffc0) == 0xfe80 {
                return true;
            }
            // ff00::/8 - 多播地址
            if (ipv6.segments()[0] & 0xff00) == 0xff00 {
                return true;
            }
            // ::/128 - 未指定地址
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
            // 2001:db8::/32 - 文档示例地址
            if ipv6.segments()[0] == 0x2001 && ipv6.segments()[1] == 0x0db8 {
                return true;
            }
            false
        }
    }
}

/// 验证 URL 是否在黑名单域名中
pub fn validate_domain_blacklist(url_str: &str, blacklist: &[String]) -> anyhow::Result<()> {
    let url = Url::parse(url_str)?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Missing host"))?;

    for domain in blacklist {
        if host == domain || host.ends_with(&format!(".{}", domain)) {
            return Err(anyhow::anyhow!("Domain {} is in blacklist", host));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_validate_url_ssrf() {
        // Localhost 变体应该被阻止
        assert!(validate_url("http://localhost").await.is_err());
        assert!(validate_url("http://localhost.localdomain").await.is_err());
        assert!(validate_url("http://127.0.0.1").await.is_err());
        assert!(validate_url("http://0.0.0.0").await.is_err());

        // 私有 IP 应该被阻止
        assert!(validate_url("http://10.0.0.1").await.is_err());
        assert!(validate_url("http://192.168.1.1").await.is_err());
        assert!(validate_url("http://172.16.0.1").await.is_err());

        // 云元数据端点应该被阻止
        assert!(validate_url("http://169.254.169.254").await.is_err());
        assert!(validate_url("http://metadata.google.internal")
            .await
            .is_err());

        // 不允许的协议
        assert!(validate_url("file:///etc/passwd").await.is_err());
        assert!(validate_url("ftp://example.com").await.is_err());
        assert!(validate_url("gopher://localhost").await.is_err());

        // 有效的公共 URL - 只在显式启用网络测试时运行
        // 使用 CRAWLRS_ENABLE_NETWORK_TESTS 环境变量控制，而不是依赖 CI
        if std::env::var("CRAWLRS_ENABLE_NETWORK_TESTS").is_ok() {
            assert!(validate_url("http://example.com").await.is_ok());
            assert!(validate_url("https://google.com").await.is_ok());
        }
    }

    #[test]
    fn test_is_private_ip() {
        assert!(is_private_ip("127.0.0.1".parse().unwrap()));
        assert!(is_private_ip("10.0.0.1".parse().unwrap()));
        assert!(is_private_ip("192.168.1.1".parse().unwrap()));
        assert!(is_private_ip("172.16.0.1".parse().unwrap()));
        assert!(!is_private_ip("8.8.8.8".parse().unwrap()));
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

        // Partial match should not block (e.g. example.com.cn should not be blocked by example.com)
        // Current implementation: host.ends_with(".example.com")
        assert!(validate_domain_blacklist("http://myexample.com", &blacklist).is_ok());
    }
}
