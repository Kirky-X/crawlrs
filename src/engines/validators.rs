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
        return Ok(());
    }

    let url = Url::parse(url_str)?;
    let host = url
        .host_str()
        .ok_or_else(|| anyhow::anyhow!("Missing host"))?;

    // 如果是 localhost 或 127.0.0.1 等，直接拒绝
    if host == "localhost" {
        return Err(anyhow::anyhow!("SSRF protection: localhost is not allowed"));
    }

    // 解析 DNS
    // 注意：这里的端口处理可能需要根据 scheme 默认值调整，lookup_host 需要 host:port
    let port = url.port_or_known_default().unwrap_or(80);
    let addr_str = format!("{}:{}", host, port);

    let addrs = lookup_host(addr_str).await?;

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
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_validate_url_public() {
        assert!(validate_url("https://www.google.com").await.is_ok());
        assert!(validate_url("http://example.com").await.is_ok());
    }

    #[tokio::test]
    async fn test_validate_url_private() {
        // Ensure SSRF protection is ENABLED for this test
        // However, environment variables are process-global.
        // We need to be careful not to interfere with other tests running in parallel if they rely on this env var.
        // For unit tests running via cargo test, they might run in threads.
        // Let's explicitly unset the variable to be sure, assuming other tests don't need it set to true.
        // Or better, we only set it to true where needed (integration tests usually).

        // Save current value
        let old_val = std::env::var("CRAWLRS_DISABLE_SSRF_PROTECTION").ok();
        std::env::remove_var("CRAWLRS_DISABLE_SSRF_PROTECTION");

        assert!(validate_url("http://127.0.0.1").await.is_err());
        assert!(validate_url("http://localhost").await.is_err());
        assert!(validate_url("http://192.168.1.1").await.is_err());
        assert!(validate_url("http://10.0.0.1").await.is_err());

        // Restore value
        if let Some(v) = old_val {
            std::env::set_var("CRAWLRS_DISABLE_SSRF_PROTECTION", v);
        }
    }
}
