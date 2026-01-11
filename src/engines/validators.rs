// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use std::net::IpAddr;
use tokio::net::lookup_host;
use url::Url;

/// 验证 URL 是否安全 (防止 SSRF)
///
/// 检查解析后的 IP 是否为私有地址或环回地址
pub async fn validate_url(url_str: &str) -> anyhow::Result<()> {
    // 允许通过环境变量禁用 SSRF 保护（用于测试）
    if std::env::var("CRAWLRS_DISABLE_SSRF_PROTECTION").unwrap_or_default() == "true" {
        tracing::warn!("SSRF protection is DISABLED via environment variable");
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
    // 注意：这里的端口处理可能需要根据 scheme 默认值调整，lookup_host 需要 host:port
    let port = url.port_or_known_default().unwrap_or(80);
    let addr_str = format!("{}:{}", host, port);

    let addrs = lookup_host(&addr_str).await?;

    // 检查所有解析出的 IP
    for addr in addrs {
        if is_private_ip(addr.ip()) {
            return Err(anyhow::anyhow!(
                "SSRF protection: Private IP access is not allowed: {}",
                addr.ip()
            ));
        }
    }

    Ok(())
}

/// 检查主机名是否在阻止列表中
fn is_blocked_hostname(host: &str) -> bool {
    let host_lower = host.to_lowercase();

    // 阻止的主机名列表
    let blocked = [
        "localhost",
        "localhost.localdomain",
        "ip6-localhost",
        "ip6-loopback",
        "0.0.0.0",
        "::",
        // AWS 元数据端点
        "169.254.169.254",
        "metadata.google.internal",
        // Azure 元数据端点
        "169.254.169.254",
        // GCP 元数据端点
        "metadata.google.internal",
    ];

    if blocked.contains(&host_lower.as_str()) {
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
            // 10.0.0.0/8
            if ipv4.octets()[0] == 10 {
                return true;
            }
            // 172.16.0.0/12
            if ipv4.octets()[0] == 172 && (ipv4.octets()[1] >= 16 && ipv4.octets()[1] <= 31) {
                return true;
            }
            // 192.168.0.0/16
            if ipv4.octets()[0] == 192 && ipv4.octets()[1] == 168 {
                return true;
            }
            // 127.0.0.0/8 (Loopback)
            if ipv4.is_loopback() {
                return true;
            }
            // 169.254.0.0/16 (Link-local)
            if ipv4.is_link_local() {
                return true;
            }
            // 224.0.0.0/4 (Multicast)
            if ipv4.octets()[0] >= 224 && ipv4.octets()[0] <= 239 {
                return true;
            }
            false
        }
        IpAddr::V6(ipv6) => {
            // Loopback
            if ipv6.is_loopback() {
                return true;
            }
            // Unique Local Address (fc00::/7)
            if (ipv6.segments()[0] & 0xfe00) == 0xfc00 {
                return true;
            }
            // Link-local (fe80::/10)
            if (ipv6.segments()[0] & 0xffc0) == 0xfe80 {
                return true;
            }
            // Multicast (ff00::/8)
            if (ipv6.segments()[0] & 0xff00) == 0xff00 {
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
