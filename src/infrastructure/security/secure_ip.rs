// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 安全的客户端 IP 提取模块
//!
//! 提供安全的客户端真实 IP 地址提取功能，防止 X-Forwarded-For 头伪造攻击。
//!
//! # 安全说明
//!
//! 在反向代理环境中，HTTP 请求头（如 X-Forwarded-For、X-Real-IP）可以被客户端伪造。
//! 如果服务器直接信任这些头，攻击者可以：
//! - 绕过基于 IP 的速率限制
//! - 绕过 IP 访问控制
//! - 伪造审计日志中的 IP 地址
//!
//! # 解决方案
//!
//! 本模块实现以下安全策略：
//! 1. 仅当请求来自可信代理时才信任转发头
//! 2. 可信代理列表通过配置文件管理
//! 3. 支持单个 IP 和 CIDR 格式的可信代理配置

use axum::extract::Request;
use log::{debug, warn};
use std::net::{IpAddr, SocketAddr};

/// 安全的客户端 IP 提取器
///
/// 根据可信代理配置安全地提取客户端真实 IP 地址。
#[derive(Debug, Clone)]
pub struct SecureIpExtractor {
    /// 可信代理配置
    trusted_proxies: TrustedProxyConfig,
}

/// 可信代理配置
#[derive(Debug, Clone)]
pub struct TrustedProxyConfig {
    /// 是否启用可信代理验证
    pub enabled: bool,
    /// 可信代理 IP 列表（支持 CIDR 格式）
    pub proxies: Vec<String>,
}

impl Default for TrustedProxyConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            proxies: vec![
                "10.0.0.0/8".to_string(),
                "172.16.0.0/12".to_string(),
                "192.168.0.0/16".to_string(),
                "127.0.0.1".to_string(),
                "::1".to_string(),
            ],
        }
    }
}

impl TrustedProxyConfig {
    /// 从配置设置创建可信代理配置
    pub fn from_settings(enabled: bool, proxies: Vec<String>) -> Self {
        Self { enabled, proxies }
    }

    /// 检查 IP 地址是否在可信代理列表中
    ///
    /// # 参数
    ///
    /// * `ip` - 要检查的 IP 地址
    ///
    /// # 返回值
    ///
    /// 如果 IP 在可信代理列表中返回 true，否则返回 false
    pub fn is_trusted(&self, ip: &IpAddr) -> bool {
        use std::str::FromStr;

        for proxy in &self.proxies {
            // 尝试解析为 CIDR
            if let Ok(network) = ipnetwork::IpNetwork::from_str(proxy) {
                if network.contains(*ip) {
                    return true;
                }
            } else {
                // 尝试解析为单个 IP
                if let Ok(trusted_ip) = proxy.parse::<IpAddr>() {
                    if &trusted_ip == ip {
                        return true;
                    }
                }
            }
        }
        false
    }
}

impl SecureIpExtractor {
    /// 创建新的安全 IP 提取器
    pub fn new(trusted_proxies: TrustedProxyConfig) -> Self {
        Self { trusted_proxies }
    }

    /// 从请求中安全地提取客户端 IP 地址
    ///
    /// # 安全策略
    ///
    /// 1. 如果可信代理验证已禁用（开发模式），直接信任转发头
    /// 2. 如果可信代理验证已启用：
    ///    - 获取直接连接的 IP 地址（socket address）
    ///    - 检查该 IP 是否在可信代理列表中
    ///    - 如果是可信代理，则从转发头中提取客户端 IP
    ///    - 如果不是可信代理，则使用直接连接的 IP
    ///
    /// # 参数
    ///
    /// * `req` - HTTP 请求
    /// * `direct_ip_override` - 可选的直接连接 IP 地址覆盖（用于从 ConnectInfo 提取）
    ///
    /// # 返回值
    ///
    /// 返回客户端的真实 IP 地址，如果无法确定则返回 None
    pub fn extract_client_ip_with_override(
        &self,
        req: &Request,
        direct_ip_override: Option<std::net::IpAddr>,
    ) -> Option<String> {
        // 如果提供了覆盖 IP，直接使用它
        if let Some(direct_ip) = direct_ip_override {
            // 如果可信代理验证已禁用，直接信任转发头
            if !self.trusted_proxies.enabled {
                debug!("Trusted proxy validation disabled, using forwarded headers");
                return self
                    .extract_from_forwarded_headers(req)
                    .or_else(|| Some(direct_ip.to_string()));
            }

            // 检查直接连接的 IP 是否来自可信代理
            if self.trusted_proxies.is_trusted(&direct_ip) {
                debug!(
                    "Request from trusted proxy {}, extracting IP from forwarded headers",
                    direct_ip
                );
                return self
                    .extract_from_forwarded_headers(req)
                    .or_else(|| Some(direct_ip.to_string()));
            } else {
                // 不是可信代理，使用直接连接的 IP
                debug!(
                    "Request from non-trusted source {}, using direct IP",
                    direct_ip
                );
                return Some(direct_ip.to_string());
            }
        }

        // 否则从请求扩展中获取
        // Axum 的 into_make_service_with_connect_info 存入的是 ConnectInfo<SocketAddr>，
        // 不是裸 SocketAddr，需用包装类型提取
        let direct_ip = req
            .extensions()
            .get::<axum::extract::ConnectInfo<SocketAddr>>()
            .map(|ci| ci.0.ip());

        // 如果可信代理验证已禁用，直接信任转发头（不安全，仅用于开发）
        if !self.trusted_proxies.enabled {
            debug!("Trusted proxy validation disabled, using forwarded headers");
            return self
                .extract_from_forwarded_headers(req)
                .or_else(|| direct_ip.map(|ip| ip.to_string()));
        }

        // 获取直接连接的 IP
        let direct_ip = match direct_ip {
            Some(ip) => ip,
            None => {
                warn!("No direct connection IP found in request");
                return None;
            }
        };

        // 检查直接连接的 IP 是否来自可信代理
        if self.trusted_proxies.is_trusted(&direct_ip) {
            debug!(
                "Request from trusted proxy {}, extracting IP from forwarded headers",
                direct_ip
            );
            // 从转发头中提取客户端 IP
            self.extract_from_forwarded_headers(req)
                .or_else(|| Some(direct_ip.to_string()))
        } else {
            // 不是可信代理，使用直接连接的 IP
            debug!(
                "Request from non-trusted source {}, using direct IP",
                direct_ip
            );
            Some(direct_ip.to_string())
        }
    }

    /// 从请求中安全地提取客户端 IP 地址（便捷函数）
    ///
    /// # 参数
    ///
    /// * `req` - HTTP 请求
    ///
    /// # 返回值
    ///
    /// 返回客户端的真实 IP 地址，如果无法确定则返回 None
    pub fn extract_client_ip(&self, req: &Request) -> Option<String> {
        self.extract_client_ip_with_override(req, None)
    }

    /// 从转发头中提取 IP 地址
    ///
    /// 按照以下优先级提取：
    /// 1. X-Forwarded-For 头的第一个 IP（原始客户端）
    /// 2. X-Real-IP 头
    fn extract_from_forwarded_headers(&self, req: &Request) -> Option<String> {
        // 检查 X-Forwarded-For 头
        if let Some(forwarded) = req.headers().get("x-forwarded-for") {
            if let Ok(ip_str) = forwarded.to_str() {
                // X-Forwarded-For 格式: client, proxy1, proxy2, ...
                // 取第一个 IP（原始客户端）
                if let Some(client_ip) = ip_str.split(',').next() {
                    let trimmed = client_ip.trim();
                    if self.is_valid_ip(trimmed) {
                        return Some(trimmed.to_string());
                    }
                }
            }
        }

        // 检查 X-Real-IP 头
        if let Some(real_ip) = req.headers().get("x-real-ip") {
            if let Ok(ip_str) = real_ip.to_str() {
                let trimmed = ip_str.trim();
                if self.is_valid_ip(trimmed) {
                    return Some(trimmed.to_string());
                }
            }
        }

        None
    }

    /// 验证字符串是否为有效的 IP 地址
    fn is_valid_ip(&self, ip_str: &str) -> bool {
        ip_str.parse::<IpAddr>().is_ok()
    }
}

/// 从请求中安全地提取客户端 IP 地址（便捷函数）
///
/// # 参数
///
/// * `req` - HTTP 请求
/// * `trusted_proxies` - 可信代理配置
///
/// # 返回值
///
/// 返回客户端的真实 IP 地址，如果无法确定则返回 "unknown"
pub fn get_secure_client_ip(req: &Request, trusted_proxies: &TrustedProxyConfig) -> String {
    let extractor = SecureIpExtractor::new(trusted_proxies.clone());
    extractor
        .extract_client_ip(req)
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request as HttpRequest;

    #[test]
    fn test_trusted_proxy_config_default() {
        let config = TrustedProxyConfig::default();
        assert!(config.enabled);

        // 测试私有 IP 地址
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(config.is_trusted(&ip));

        let ip: IpAddr = "172.16.0.1".parse().unwrap();
        assert!(config.is_trusted(&ip));

        let ip: IpAddr = "192.168.1.1".parse().unwrap();
        assert!(config.is_trusted(&ip));

        let ip: IpAddr = "127.0.0.1".parse().unwrap();
        assert!(config.is_trusted(&ip));

        // 测试公网 IP
        let ip: IpAddr = "8.8.8.8".parse().unwrap();
        assert!(!config.is_trusted(&ip));
    }

    #[test]
    fn test_trusted_proxy_config_ipv6() {
        let config = TrustedProxyConfig::default();

        // 测试 IPv6 本地回环
        let ip: IpAddr = "::1".parse().unwrap();
        assert!(config.is_trusted(&ip));

        // 测试 IPv6 公网地址
        let ip: IpAddr = "2001:db8::1".parse().unwrap();
        assert!(!config.is_trusted(&ip));
    }

    #[test]
    fn test_trusted_proxy_config_custom_cidr() {
        let config = TrustedProxyConfig::from_settings(
            true,
            vec!["203.0.113.0/24".to_string(), "198.51.100.1".to_string()],
        );

        // 测试 CIDR 范围内的 IP
        let ip: IpAddr = "203.0.113.100".parse().unwrap();
        assert!(config.is_trusted(&ip));

        // 测试单个 IP
        let ip: IpAddr = "198.51.100.1".parse().unwrap();
        assert!(config.is_trusted(&ip));

        // 测试不在范围内的 IP
        let ip: IpAddr = "203.0.114.1".parse().unwrap();
        assert!(!config.is_trusted(&ip));
    }

    #[test]
    fn test_is_valid_ip() {
        let extractor = SecureIpExtractor::new(TrustedProxyConfig::default());

        // 有效的 IPv4
        assert!(extractor.is_valid_ip("192.168.1.1"));
        assert!(extractor.is_valid_ip("0.0.0.0"));
        assert!(extractor.is_valid_ip("255.255.255.255"));

        // 有效的 IPv6
        assert!(extractor.is_valid_ip("::1"));
        assert!(extractor.is_valid_ip("2001:db8::1"));

        // 无效的 IP
        assert!(!extractor.is_valid_ip(""));
        assert!(!extractor.is_valid_ip("invalid"));
        assert!(!extractor.is_valid_ip("256.256.256.256"));
        assert!(!extractor.is_valid_ip("192.168.1"));
    }

    #[test]
    fn test_extract_from_forwarded_headers() {
        let extractor = SecureIpExtractor::new(TrustedProxyConfig::default());

        // 测试 X-Forwarded-For 头
        let req = HttpRequest::builder()
            .header("x-forwarded-for", "203.0.113.1, 198.51.100.1, 192.0.2.1")
            .body(axum::body::Body::empty())
            .unwrap();

        let ip = extractor.extract_from_forwarded_headers(&req);
        assert_eq!(ip, Some("203.0.113.1".to_string()));

        // 测试 X-Real-IP 头
        let req = HttpRequest::builder()
            .header("x-real-ip", "203.0.113.2")
            .body(axum::body::Body::empty())
            .unwrap();

        let ip = extractor.extract_from_forwarded_headers(&req);
        assert_eq!(ip, Some("203.0.113.2".to_string()));

        // 测试无头的情况
        let req = HttpRequest::builder()
            .body(axum::body::Body::empty())
            .unwrap();

        let ip = extractor.extract_from_forwarded_headers(&req);
        assert!(ip.is_none());
    }

    #[test]
    fn test_extract_from_forwarded_headers_with_spaces() {
        let extractor = SecureIpExtractor::new(TrustedProxyConfig::default());

        // 测试带空格的 X-Forwarded-For 头
        let req = HttpRequest::builder()
            .header("x-forwarded-for", "  203.0.113.1  ,  198.51.100.1  ")
            .body(axum::body::Body::empty())
            .unwrap();

        let ip = extractor.extract_from_forwarded_headers(&req);
        assert_eq!(ip, Some("203.0.113.1".to_string()));
    }

    #[test]
    fn test_extract_from_forwarded_headers_invalid_ip() {
        let extractor = SecureIpExtractor::new(TrustedProxyConfig::default());

        // 测试无效 IP
        let req = HttpRequest::builder()
            .header("x-forwarded-for", "invalid-ip, 198.51.100.1")
            .body(axum::body::Body::empty())
            .unwrap();

        let ip = extractor.extract_from_forwarded_headers(&req);
        assert!(ip.is_none());
    }

    #[test]
    fn test_secure_ip_extractor_new_constructor() {
        // 验证 SecureIpExtractor::new 构造器
        let config = TrustedProxyConfig::from_settings(false, vec![]);
        let extractor = SecureIpExtractor::new(config);
        // 无转发头、无 ConnectInfo → None
        let req = HttpRequest::builder()
            .body(axum::body::Body::empty())
            .unwrap();
        let ip = extractor.extract_client_ip(&req);
        assert!(ip.is_none());
    }

    #[test]
    fn test_trusted_proxy_config_from_settings_enabled() {
        // 验证 from_settings enabled=true + 自定义 proxies
        let config = TrustedProxyConfig::from_settings(
            true,
            vec!["10.0.0.1".to_string(), "192.168.1.0/24".to_string()],
        );
        assert!(config.enabled);
        assert_eq!(config.proxies.len(), 2);

        // 单 IP 匹配
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(config.is_trusted(&ip));

        // CIDR 匹配
        let ip: IpAddr = "192.168.1.100".parse().unwrap();
        assert!(config.is_trusted(&ip));

        // 不匹配
        let ip: IpAddr = "8.8.8.8".parse().unwrap();
        assert!(!config.is_trusted(&ip));
    }

    #[test]
    fn test_trusted_proxy_config_from_settings_disabled() {
        // 验证 from_settings enabled=false + 空 proxies
        let config = TrustedProxyConfig::from_settings(false, vec![]);
        assert!(!config.enabled);
        assert!(config.proxies.is_empty());

        // 即使 enabled=false，is_trusted 仍按 proxies 列表检查
        let ip: IpAddr = "8.8.8.8".parse().unwrap();
        assert!(!config.is_trusted(&ip));
    }

    #[test]
    fn test_extract_client_ip_with_override_disabled_uses_forwarded() {
        // direct_ip_override=Some + enabled=false → 信任转发头，回退到 direct_ip
        let config = TrustedProxyConfig::from_settings(false, vec![]);
        let extractor = SecureIpExtractor::new(config);

        let direct_ip: IpAddr = "8.8.8.8".parse().unwrap();
        let req = HttpRequest::builder()
            .header("x-forwarded-for", "1.2.3.4")
            .body(axum::body::Body::empty())
            .unwrap();

        let ip = extractor.extract_client_ip_with_override(&req, Some(direct_ip));
        assert_eq!(ip, Some("1.2.3.4".to_string()));
    }

    #[test]
    fn test_extract_client_ip_with_override_disabled_fallback_to_direct() {
        // direct_ip_override=Some + enabled=false + 无转发头 → 回退到 direct_ip
        let config = TrustedProxyConfig::from_settings(false, vec![]);
        let extractor = SecureIpExtractor::new(config);

        let direct_ip: IpAddr = "8.8.8.8".parse().unwrap();
        let req = HttpRequest::builder()
            .body(axum::body::Body::empty())
            .unwrap();

        let ip = extractor.extract_client_ip_with_override(&req, Some(direct_ip));
        assert_eq!(ip, Some("8.8.8.8".to_string()));
    }

    #[test]
    fn test_extract_client_ip_with_override_enabled_trusted_proxy() {
        // direct_ip_override=Some + enabled=true + 可信代理 → 提取转发头
        let config = TrustedProxyConfig::from_settings(true, vec!["10.0.0.1".to_string()]);
        let extractor = SecureIpExtractor::new(config);

        let direct_ip: IpAddr = "10.0.0.1".parse().unwrap();
        let req = HttpRequest::builder()
            .header("x-forwarded-for", "203.0.113.1")
            .body(axum::body::Body::empty())
            .unwrap();

        let ip = extractor.extract_client_ip_with_override(&req, Some(direct_ip));
        assert_eq!(ip, Some("203.0.113.1".to_string()));
    }

    #[test]
    fn test_extract_client_ip_with_override_enabled_trusted_proxy_fallback() {
        // direct_ip_override=Some + enabled=true + 可信代理 + 无转发头 → 回退到 direct_ip
        let config = TrustedProxyConfig::from_settings(true, vec!["10.0.0.1".to_string()]);
        let extractor = SecureIpExtractor::new(config);

        let direct_ip: IpAddr = "10.0.0.1".parse().unwrap();
        let req = HttpRequest::builder()
            .body(axum::body::Body::empty())
            .unwrap();

        let ip = extractor.extract_client_ip_with_override(&req, Some(direct_ip));
        assert_eq!(ip, Some("10.0.0.1".to_string()));
    }

    #[test]
    fn test_extract_client_ip_with_override_enabled_non_trusted() {
        // direct_ip_override=Some + enabled=true + 非可信 → 使用 direct_ip（忽略转发头）
        let config = TrustedProxyConfig::from_settings(true, vec!["10.0.0.1".to_string()]);
        let extractor = SecureIpExtractor::new(config);

        let direct_ip: IpAddr = "8.8.8.8".parse().unwrap();
        let req = HttpRequest::builder()
            .header("x-forwarded-for", "1.2.3.4")
            .body(axum::body::Body::empty())
            .unwrap();

        let ip = extractor.extract_client_ip_with_override(&req, Some(direct_ip));
        assert_eq!(ip, Some("8.8.8.8".to_string()));
    }

    #[test]
    fn test_extract_client_ip_with_override_none_with_connect_info_trusted() {
        // direct_ip_override=None + ConnectInfo + 可信代理 → 提取转发头
        let config = TrustedProxyConfig::from_settings(true, vec!["127.0.0.1".to_string()]);
        let extractor = SecureIpExtractor::new(config);

        let socket_addr: SocketAddr = "127.0.0.1:8080".parse().unwrap();
        let mut req = HttpRequest::builder()
            .header("x-forwarded-for", "203.0.113.5")
            .body(axum::body::Body::empty())
            .unwrap();
        req.extensions_mut()
            .insert(axum::extract::ConnectInfo(socket_addr));

        let ip = extractor.extract_client_ip_with_override(&req, None);
        assert_eq!(ip, Some("203.0.113.5".to_string()));
    }

    #[test]
    fn test_extract_client_ip_with_override_none_with_connect_info_non_trusted() {
        // direct_ip_override=None + ConnectInfo + 非可信 → 使用 direct_ip
        let config = TrustedProxyConfig::from_settings(true, vec!["127.0.0.1".to_string()]);
        let extractor = SecureIpExtractor::new(config);

        let socket_addr: SocketAddr = "8.8.8.8:8080".parse().unwrap();
        let mut req = HttpRequest::builder()
            .header("x-forwarded-for", "1.2.3.4")
            .body(axum::body::Body::empty())
            .unwrap();
        req.extensions_mut()
            .insert(axum::extract::ConnectInfo(socket_addr));

        let ip = extractor.extract_client_ip_with_override(&req, None);
        assert_eq!(ip, Some("8.8.8.8".to_string()));
    }

    #[test]
    fn test_extract_client_ip_with_override_none_no_connect_info_enabled() {
        // direct_ip_override=None + 无 ConnectInfo + enabled=true → None
        let config = TrustedProxyConfig::from_settings(true, vec!["127.0.0.1".to_string()]);
        let extractor = SecureIpExtractor::new(config);

        let req = HttpRequest::builder()
            .header("x-forwarded-for", "1.2.3.4")
            .body(axum::body::Body::empty())
            .unwrap();

        let ip = extractor.extract_client_ip_with_override(&req, None);
        assert!(ip.is_none());
    }

    #[test]
    fn test_extract_client_ip_with_override_none_no_connect_info_disabled() {
        // direct_ip_override=None + 无 ConnectInfo + enabled=false → 转发头或 None
        let config = TrustedProxyConfig::from_settings(false, vec![]);
        let extractor = SecureIpExtractor::new(config);

        // 有转发头 → 返回转发头 IP
        let req = HttpRequest::builder()
            .header("x-forwarded-for", "1.2.3.4")
            .body(axum::body::Body::empty())
            .unwrap();
        let ip = extractor.extract_client_ip_with_override(&req, None);
        assert_eq!(ip, Some("1.2.3.4".to_string()));

        // 无转发头 → None
        let req = HttpRequest::builder()
            .body(axum::body::Body::empty())
            .unwrap();
        let ip = extractor.extract_client_ip_with_override(&req, None);
        assert!(ip.is_none());
    }

    #[test]
    fn test_extract_client_ip_delegates_to_with_override_none() {
        // extract_client_ip 应委托给 with_override(None)
        let config = TrustedProxyConfig::from_settings(false, vec![]);
        let extractor = SecureIpExtractor::new(config);

        let req = HttpRequest::builder()
            .header("x-real-ip", "203.0.113.99")
            .body(axum::body::Body::empty())
            .unwrap();

        let ip = extractor.extract_client_ip(&req);
        assert_eq!(ip, Some("203.0.113.99".to_string()));
    }

    #[test]
    fn test_get_secure_client_ip_returns_ip() {
        // get_secure_client_ip 返回 IP 字符串
        let config = TrustedProxyConfig::from_settings(false, vec![]);
        let req = HttpRequest::builder()
            .header("x-forwarded-for", "203.0.113.10")
            .body(axum::body::Body::empty())
            .unwrap();

        let ip = get_secure_client_ip(&req, &config);
        assert_eq!(ip, "203.0.113.10");
    }

    #[test]
    fn test_get_secure_client_ip_returns_unknown() {
        // get_secure_client_ip 在无法提取时返回 "unknown"
        let config = TrustedProxyConfig::from_settings(true, vec!["127.0.0.1".to_string()]);
        let req = HttpRequest::builder()
            .body(axum::body::Body::empty())
            .unwrap();

        let ip = get_secure_client_ip(&req, &config);
        assert_eq!(ip, "unknown");
    }

    #[test]
    fn test_extract_from_forwarded_headers_invalid_ip_falls_through_to_x_real_ip() {
        // X-Forwarded-For 第一个 IP 无效 → 应回退到 X-Real-IP
        let extractor = SecureIpExtractor::new(TrustedProxyConfig::default());

        let req = HttpRequest::builder()
            .header("x-forwarded-for", "invalid-ip")
            .header("x-real-ip", "203.0.113.20")
            .body(axum::body::Body::empty())
            .unwrap();

        let ip = extractor.extract_from_forwarded_headers(&req);
        assert_eq!(ip, Some("203.0.113.20".to_string()));
    }

    #[test]
    fn test_extract_from_forwarded_headers_empty_x_forwarded_for() {
        // 空 X-Forwarded-For → 应回退到 X-Real-IP 或返回 None
        let extractor = SecureIpExtractor::new(TrustedProxyConfig::default());

        let req = HttpRequest::builder()
            .header("x-forwarded-for", "")
            .header("x-real-ip", "203.0.113.21")
            .body(axum::body::Body::empty())
            .unwrap();

        let ip = extractor.extract_from_forwarded_headers(&req);
        assert_eq!(ip, Some("203.0.113.21".to_string()));

        // 空 X-Forwarded-For 且无 X-Real-IP → None
        let req = HttpRequest::builder()
            .header("x-forwarded-for", "")
            .body(axum::body::Body::empty())
            .unwrap();

        let ip = extractor.extract_from_forwarded_headers(&req);
        assert!(ip.is_none());
    }

    #[test]
    fn test_extract_from_forwarded_headers_empty_x_real_ip() {
        // 空 X-Real-IP → 返回 None
        let extractor = SecureIpExtractor::new(TrustedProxyConfig::default());

        let req = HttpRequest::builder()
            .header("x-real-ip", "")
            .body(axum::body::Body::empty())
            .unwrap();

        let ip = extractor.extract_from_forwarded_headers(&req);
        assert!(ip.is_none());
    }

    #[test]
    fn test_is_trusted_with_invalid_proxy_entry_skips_and_continues() {
        // 包含无效 CIDR/IP 的条目应被跳过，继续检查后续条目
        let config = TrustedProxyConfig::from_settings(
            true,
            vec!["not-a-valid-cidr-or-ip".to_string(), "10.0.0.1".to_string()],
        );

        // 10.0.0.1 匹配第二个有效条目
        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(config.is_trusted(&ip));

        // 8.8.8.8 不匹配任何条目
        let ip: IpAddr = "8.8.8.8".parse().unwrap();
        assert!(!config.is_trusted(&ip));
    }

    #[test]
    fn test_is_trusted_all_invalid_entries_returns_false() {
        // 所有条目都无效时返回 false
        let config = TrustedProxyConfig::from_settings(
            true,
            vec!["invalid1".to_string(), "invalid2".to_string()],
        );

        let ip: IpAddr = "10.0.0.1".parse().unwrap();
        assert!(!config.is_trusted(&ip));
    }

    #[test]
    fn test_extract_from_forwarded_headers_non_utf8_x_forwarded_for_falls_to_x_real_ip() {
        // X-Forwarded-For 头包含非 ASCII 字节 → to_str() 失败 → 回退到 X-Real-IP
        let extractor = SecureIpExtractor::new(TrustedProxyConfig::default());

        let mut req = HttpRequest::builder()
            .header("x-real-ip", "203.0.113.1")
            .body(axum::body::Body::empty())
            .unwrap();
        req.headers_mut().insert(
            "x-forwarded-for",
            axum::http::HeaderValue::from_bytes(&[0x80, 0x81, 0x82]).unwrap(),
        );

        let ip = extractor.extract_from_forwarded_headers(&req);
        assert_eq!(ip, Some("203.0.113.1".to_string()));
    }

    #[test]
    fn test_extract_from_forwarded_headers_non_utf8_both_headers_returns_none() {
        // 两个头都包含非 ASCII 字节 → 返回 None
        let extractor = SecureIpExtractor::new(TrustedProxyConfig::default());

        let mut req = HttpRequest::builder()
            .body(axum::body::Body::empty())
            .unwrap();
        req.headers_mut().insert(
            "x-forwarded-for",
            axum::http::HeaderValue::from_bytes(&[0x80, 0x81]).unwrap(),
        );
        req.headers_mut().insert(
            "x-real-ip",
            axum::http::HeaderValue::from_bytes(&[0x82, 0x83]).unwrap(),
        );

        let ip = extractor.extract_from_forwarded_headers(&req);
        assert!(ip.is_none());
    }

    #[test]
    fn test_is_trusted_ipv6_cidr() {
        // 测试 IPv6 CIDR 格式
        let config = TrustedProxyConfig::from_settings(true, vec!["2001:db8::/32".to_string()]);

        let ip: IpAddr = "2001:db8::1".parse().unwrap();
        assert!(config.is_trusted(&ip));

        let ip: IpAddr = "2001:db9::1".parse().unwrap();
        assert!(!config.is_trusted(&ip));
    }
}
