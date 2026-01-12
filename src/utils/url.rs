// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! URL处理工具模块
//!
//! 提供URL验证、解析和安全检查功能

use std::net::IpAddr;
use thiserror::Error;
use url::{ParseError, Url};

/// 验证错误类型
#[derive(Error, Debug)]
pub enum ValidationError {
    #[error("Invalid URL")]
    InvalidUrl,
    #[error("SSRF detected")]
    SsrfDetected,
}

/// 检查IP地址是否安全
pub fn is_safe_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ipv4) => {
            !ipv4.is_loopback()
                && !ipv4.is_private()
                && !ipv4.is_link_local()
                && !ipv4.is_broadcast()
                && !ipv4.is_documentation()
        }
        IpAddr::V6(ipv6) => !ipv6.is_loopback() && !ipv6.is_unspecified(),
    }
}

/// 验证URL
pub async fn validate_url(url: &str) -> Result<(), ValidationError> {
    let parsed = Url::parse(url).map_err(|_| ValidationError::InvalidUrl)?;
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return Err(ValidationError::InvalidUrl);
    }
    let host = parsed.host_str().ok_or(ValidationError::InvalidUrl)?;
    let addrs = tokio::net::lookup_host((host, 0))
        .await
        .map_err(|_| ValidationError::InvalidUrl)?
        .collect::<Vec<_>>();
    for addr in addrs {
        if !is_safe_ip(addr.ip()) {
            return Err(ValidationError::SsrfDetected);
        }
    }
    Ok(())
}

/// 将可能为相对路径的URL转换为绝对路径URL
pub fn resolve_url(base_url: &Url, path: &str) -> Result<Url, ParseError> {
    base_url.join(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_absolute_url() {
        let base = Url::parse("http://example.com/a/b").unwrap();
        let path = "http://t.co/c";
        assert_eq!(resolve_url(&base, path).unwrap().as_str(), "http://t.co/c");
    }

    #[test]
    fn test_resolve_protocol_relative_url() {
        let base = Url::parse("https://example.com/a/b").unwrap();
        let path = "//t.co/c";
        assert_eq!(resolve_url(&base, path).unwrap().as_str(), "https://t.co/c");
    }

    #[test]
    fn test_resolve_root_relative_url() {
        let base = Url::parse("http://example.com/a/b").unwrap();
        let path = "/c";
        assert_eq!(
            resolve_url(&base, path).unwrap().as_str(),
            "http://example.com/c"
        );
    }

    #[test]
    fn test_resolve_relative_url() {
        let base = Url::parse("http://example.com/a/b").unwrap();
        let path = "c";
        assert_eq!(
            resolve_url(&base, path).unwrap().as_str(),
            "http://example.com/a/c"
        );
    }
}
