// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! URL处理工具模块
//!
//! 提供URL解析、强类型封装和路径解析功能

use thiserror::Error;
use url::{ParseError, Url};

/// URL解析错误类型
#[derive(Error, Debug)]
pub enum UrlError {
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),
}

/// 将可能为相对路径的URL转换为绝对路径URL
pub fn resolve_url(base_url: &Url, path: &str) -> Result<Url, ParseError> {
    base_url.join(path)
}

/// 强类型URL封装
///
/// 替代String类型，提供类型安全的URL处理
/// 只能通过验证的URL字符串创建
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SafeUrl {
    /// 内部URL解析
    url: Url,
}

impl SafeUrl {
    /// 从已验证的URL字符串创建SafeUrl
    ///
    /// # Arguments
    ///
    /// * `url_str` - 已验证的URL字符串
    ///
    /// # Returns
    ///
    /// * `Ok(SafeUrl)` - 成功创建的SafeUrl
    /// * `Err(ParseError)` - URL解析失败
    pub fn new(url_str: &str) -> Result<Self, ParseError> {
        Url::parse(url_str).map(|url| SafeUrl { url })
    }

    /// 获取URL字符串引用
    pub fn as_str(&self) -> &str {
        self.url.as_str()
    }

    /// 获取内部Url引用
    pub fn inner(&self) -> &Url {
        &self.url
    }

    /// 获取主机部分
    pub fn host(&self) -> Option<&str> {
        self.url.host_str()
    }

    /// 获取路径部分
    pub fn path(&self) -> &str {
        self.url.path()
    }

    /// 检查是否为HTTPS
    pub fn is_https(&self) -> bool {
        self.url.scheme() == "https"
    }

    /// 获取端口（如果有）
    pub fn port(&self) -> Option<u16> {
        self.url.port()
    }
}

impl std::fmt::Display for SafeUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.url)
    }
}

impl std::str::FromStr for SafeUrl {
    type Err = ParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        SafeUrl::new(s)
    }
}

impl serde::Serialize for SafeUrl {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> serde::Deserialize<'de> for SafeUrl {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = SafeUrl;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a valid URL string")
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                SafeUrl::new(value).map_err(|_| E::custom("invalid URL format"))
            }
        }

        deserializer.deserialize_str(Visitor)
    }
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
