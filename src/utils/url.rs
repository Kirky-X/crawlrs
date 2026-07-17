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

    // ========== SafeUrl construction tests ==========

    #[test]
    fn test_safe_url_new_https() {
        let url = SafeUrl::new("https://example.com/path").unwrap();
        assert_eq!(url.as_str(), "https://example.com/path");
    }

    #[test]
    fn test_safe_url_new_http() {
        let url = SafeUrl::new("http://example.com").unwrap();
        assert_eq!(url.as_str(), "http://example.com/");
    }

    #[test]
    fn test_safe_url_new_invalid_returns_error() {
        let result = SafeUrl::new("not a valid url");
        assert!(result.is_err());
    }

    #[test]
    fn test_safe_url_new_with_port() {
        let url = SafeUrl::new("http://example.com:8080/path").unwrap();
        assert_eq!(url.port(), Some(8080));
    }

    #[test]
    fn test_safe_url_new_without_port() {
        let url = SafeUrl::new("http://example.com/path").unwrap();
        assert_eq!(url.port(), None);
    }

    #[test]
    fn test_safe_url_as_str() {
        let url = SafeUrl::new("https://example.com/page?q=1").unwrap();
        assert_eq!(url.as_str(), "https://example.com/page?q=1");
    }

    #[test]
    fn test_safe_url_inner_returns_url_ref() {
        let url = SafeUrl::new("https://example.com").unwrap();
        let inner = url.inner();
        assert_eq!(inner.scheme(), "https");
        assert_eq!(inner.host_str(), Some("example.com"));
    }

    #[test]
    fn test_safe_url_host_with_hostname() {
        let url = SafeUrl::new("https://example.com/path").unwrap();
        assert_eq!(url.host(), Some("example.com"));
    }

    #[test]
    fn test_safe_url_host_for_ip() {
        let url = SafeUrl::new("http://127.0.0.1:8080").unwrap();
        assert_eq!(url.host(), Some("127.0.0.1"));
    }

    #[test]
    fn test_safe_url_path_simple() {
        let url = SafeUrl::new("https://example.com/a/b/c").unwrap();
        assert_eq!(url.path(), "/a/b/c");
    }

    #[test]
    fn test_safe_url_path_root() {
        let url = SafeUrl::new("https://example.com").unwrap();
        assert_eq!(url.path(), "/");
    }

    #[test]
    fn test_safe_url_is_https_true() {
        let url = SafeUrl::new("https://example.com").unwrap();
        assert!(url.is_https());
    }

    #[test]
    fn test_safe_url_is_https_false() {
        let url = SafeUrl::new("http://example.com").unwrap();
        assert!(!url.is_https());
    }

    #[test]
    fn test_safe_url_port_default_http() {
        let url = SafeUrl::new("http://example.com").unwrap();
        // Default port 80 is not returned by url crate
        assert_eq!(url.port(), None);
    }

    #[test]
    fn test_safe_url_port_default_https() {
        let url = SafeUrl::new("https://example.com").unwrap();
        assert_eq!(url.port(), None);
    }

    // ========== SafeUrl Display tests ==========

    #[test]
    fn test_safe_url_display() {
        let url = SafeUrl::new("https://example.com/path?q=1#frag").unwrap();
        assert_eq!(format!("{}", url), "https://example.com/path?q=1#frag");
    }

    #[test]
    fn test_safe_url_display_matches_as_str() {
        let url = SafeUrl::new("http://test.org:9090/x").unwrap();
        assert_eq!(format!("{}", url), url.as_str());
    }

    // ========== SafeUrl FromStr tests ==========

    #[test]
    fn test_safe_url_from_str_valid() {
        let url: SafeUrl = "https://example.com".parse().unwrap();
        assert_eq!(url.as_str(), "https://example.com/");
    }

    #[test]
    fn test_safe_url_from_str_invalid() {
        let result: Result<SafeUrl, _> = "invalid url".parse();
        assert!(result.is_err());
    }

    // ========== SafeUrl equality/hash tests ==========

    #[test]
    fn test_safe_url_equality() {
        let url1 = SafeUrl::new("https://example.com/path").unwrap();
        let url2 = SafeUrl::new("https://example.com/path").unwrap();
        assert_eq!(url1, url2);
    }

    #[test]
    fn test_safe_url_inequality() {
        let url1 = SafeUrl::new("https://example.com/a").unwrap();
        let url2 = SafeUrl::new("https://example.com/b").unwrap();
        assert_ne!(url1, url2);
    }

    #[test]
    fn test_safe_url_clone() {
        let url = SafeUrl::new("https://example.com/path").unwrap();
        let cloned = url.clone();
        assert_eq!(url, cloned);
    }

    #[test]
    fn test_safe_url_hash_in_hashmap() {
        use std::collections::HashMap;
        let url1 = SafeUrl::new("https://example.com").unwrap();
        let url2 = SafeUrl::new("https://example.com").unwrap();
        let mut map: HashMap<SafeUrl, i32> = HashMap::new();
        map.insert(url1, 42);
        assert_eq!(map.get(&url2), Some(&42));
    }

    // ========== SafeUrl serde tests ==========

    #[test]
    fn test_safe_url_serialize() {
        let url = SafeUrl::new("https://example.com/path").unwrap();
        let json = serde_json::to_string(&url).unwrap();
        assert_eq!(json, "\"https://example.com/path\"");
    }

    #[test]
    fn test_safe_url_deserialize_valid() {
        let json = "\"https://example.com/path\"";
        let url: SafeUrl = serde_json::from_str(json).unwrap();
        assert_eq!(url.as_str(), "https://example.com/path");
    }

    #[test]
    fn test_safe_url_deserialize_invalid() {
        let json = "\"not a url\"";
        let result: Result<SafeUrl, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_safe_url_serde_roundtrip() {
        let url = SafeUrl::new("http://test.org:8080/path?q=1#f").unwrap();
        let json = serde_json::to_string(&url).unwrap();
        let back: SafeUrl = serde_json::from_str(&json).unwrap();
        assert_eq!(url, back);
    }

    // ========== SafeUrl Debug test ==========

    #[test]
    fn test_safe_url_debug() {
        let url = SafeUrl::new("https://example.com").unwrap();
        let dbg = format!("{:?}", url);
        assert!(dbg.contains("SafeUrl"));
    }

    // ========== resolve_url edge cases ==========

    #[test]
    fn test_resolve_url_with_query_string() {
        let base = Url::parse("http://example.com/page").unwrap();
        let path = "?q=test";
        assert_eq!(
            resolve_url(&base, path).unwrap().as_str(),
            "http://example.com/page?q=test"
        );
    }

    #[test]
    fn test_resolve_url_with_fragment() {
        let base = Url::parse("http://example.com/page").unwrap();
        let path = "#section";
        assert_eq!(
            resolve_url(&base, path).unwrap().as_str(),
            "http://example.com/page#section"
        );
    }

    #[test]
    fn test_resolve_url_empty_path() {
        let base = Url::parse("http://example.com/page").unwrap();
        let path = "";
        assert_eq!(
            resolve_url(&base, path).unwrap().as_str(),
            "http://example.com/page"
        );
    }

    #[test]
    fn test_resolve_url_dot_segment() {
        let base = Url::parse("http://example.com/a/b").unwrap();
        let path = "./c";
        assert_eq!(
            resolve_url(&base, path).unwrap().as_str(),
            "http://example.com/a/c"
        );
    }

    #[test]
    fn test_resolve_url_parent_segment() {
        let base = Url::parse("http://example.com/a/b/c").unwrap();
        let path = "../d";
        assert_eq!(
            resolve_url(&base, path).unwrap().as_str(),
            "http://example.com/a/d"
        );
    }

    #[test]
    fn test_resolve_url_https_base() {
        let base = Url::parse("https://secure.example.com/base").unwrap();
        let path = "/path";
        assert_eq!(
            resolve_url(&base, path).unwrap().as_str(),
            "https://secure.example.com/path"
        );
    }

    // ========== UrlError tests ==========

    #[test]
    fn test_url_error_display() {
        let err = UrlError::InvalidUrl("bad url".to_string());
        assert!(err.to_string().contains("Invalid URL"));
        assert!(err.to_string().contains("bad url"));
    }

    // ========== Visitor::expecting 方法覆盖测试 ==========

    #[test]
    fn test_safe_url_deserialize_number_triggers_expecting() {
        // 反序列化数字类型触发 Visitor::expecting 方法（行 111, 114-115）。
        // serde_json 看到 number，调用 Visitor::visit_u64（未定义），
        // 默认实现返回 invalid_type 错误，错误消息使用 expecting() 输出。
        let json = "123";
        let result: Result<SafeUrl, _> = serde_json::from_str(json);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        // expecting 输出 "a valid URL string"，应出现在错误消息中
        assert!(
            err_msg.contains("a valid URL string"),
            "error should contain expecting message, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_safe_url_deserialize_object_triggers_expecting() {
        // 反序列化对象类型触发 Visitor::expecting 方法。
        let json = "{}";
        let result: Result<SafeUrl, _> = serde_json::from_str(json);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("a valid URL string"),
            "error should contain expecting message, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_safe_url_deserialize_array_triggers_expecting() {
        // 反序列化数组类型触发 Visitor::expecting 方法。
        let json = "[]";
        let result: Result<SafeUrl, _> = serde_json::from_str(json);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("a valid URL string"),
            "error should contain expecting message, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_safe_url_deserialize_bool_triggers_expecting() {
        // 反序列化布尔类型触发 Visitor::expecting 方法。
        let json = "true";
        let result: Result<SafeUrl, _> = serde_json::from_str(json);
        assert!(result.is_err());
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("a valid URL string"),
            "error should contain expecting message, got: {}",
            err_msg
        );
    }

    #[test]
    fn test_safe_url_deserialize_null_triggers_expecting() {
        // 反序列化 null 类型触发 Visitor::expecting 方法。
        let json = "null";
        let result: Result<SafeUrl, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }
}
