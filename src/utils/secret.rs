// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 安全敏感数据处理模块
//!
//! 提供敏感数据的安全存储和清理功能，防止敏感信息在内存中长期驻留

use zeroize::Zeroize;

/// 可清理 trait - 用于安全清理敏感数据
///
/// 任何实现此 trait 的类型都可以通过 `clear()` 方法安全地清理其内部数据
pub trait Clearable {
    /// 清理内部敏感数据，将其清零
    fn clear(&mut self);
}

/// 秘密字符串包装类型
///
/// 自动在析构时清零内部数据，防止敏感信息在内存中长期驻留
///
/// # 示例
///
/// ```ignore
/// use utils::SecretString;
///
/// fn process_api_key() {
///     let api_key = SecretString::new("sk-1234567890abcdef");
///
///     // 使用 API key
///     let key_ref = api_key.as_ref();
///     // ... 处理逻辑
///
///     // 当 api_key 离开作用域时，内存自动清零
/// }
/// ```
#[derive(Clone)]
pub struct SecretString {
    data: String,
}

impl SecretString {
    /// 创建新的 SecretString 实例
    ///
    /// # 参数
    ///
    /// * `data` - 要保护的敏感数据
    ///
    /// # 示例
    ///
    /// ```ignore
    /// use utils::SecretString;
    ///
    /// let secret = SecretString::new("my-secret-api-key");
    /// ```
    pub fn new(data: &str) -> Self {
        Self {
            data: data.to_string(),
        }
    }

    /// 获取内部数据的不可变引用
    pub fn as_str(&self) -> &str {
        &self.data
    }
}

impl AsRef<str> for SecretString {
    fn as_ref(&self) -> &str {
        &self.data
    }
}

impl std::fmt::Debug for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[REDACTED]")
    }
}

impl std::fmt::Display for SecretString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[REDACTED]")
    }
}

impl Drop for SecretString {
    fn drop(&mut self) {
        // 清零内部字符串数据
        self.data.zeroize();
    }
}

impl Clearable for SecretString {
    fn clear(&mut self) {
        self.data.zeroize();
    }
}

/// 为所有实现 Zeroize 的类型提供 blanket implementation
impl<T: Zeroize> Clearable for T {
    fn clear(&mut self) {
        self.zeroize();
    }
}

#[cfg(test)]
mod tests {
    use super::{Clearable, SecretString};

    #[test]
    fn test_secret_string_debug_output() {
        let secret = SecretString::new("sensitive-data");
        let debug_output = format!("{:?}", secret);
        assert_eq!(debug_output, "[REDACTED]");
        assert!(!debug_output.contains("sensitive"));
    }

    #[test]
    fn test_secret_string_display_output() {
        let secret = SecretString::new("sensitive-data");
        let display_output = secret.to_string();
        assert_eq!(display_output, "[REDACTED]");
    }

    #[test]
    fn test_secret_string_as_ref() {
        let secret = SecretString::new("test-data");
        assert_eq!(secret.as_ref(), "test-data");
    }

    #[test]
    fn test_clearable_trait() {
        let mut secret = SecretString::new("sensitive");
        secret.clear();
        assert_eq!(secret.as_ref(), "");
    }

    #[test]
    fn test_clone() {
        let original = SecretString::new("clone-me");
        let cloned = original.clone();
        assert_eq!(cloned.as_ref(), "clone-me");
    }

    // ========== as_str() method tests ==========

    #[test]
    fn test_secret_string_as_str_returns_data() {
        let secret = SecretString::new("my-api-key-12345");
        assert_eq!(secret.as_str(), "my-api-key-12345");
    }

    #[test]
    fn test_secret_string_as_str_empty() {
        let secret = SecretString::new("");
        assert_eq!(secret.as_str(), "");
    }

    #[test]
    fn test_secret_string_as_str_unicode() {
        let secret = SecretString::new("密码-🔑");
        assert_eq!(secret.as_str(), "密码-🔑");
    }

    #[test]
    fn test_secret_string_as_str_consistent_with_as_ref() {
        let secret = SecretString::new("consistent-value");
        assert_eq!(secret.as_str(), secret.as_ref());
    }

    // ========== Clearable blanket impl tests ==========

    #[test]
    fn test_clearable_blanket_impl_for_vec_u8() {
        let mut data: Vec<u8> = vec![1, 2, 3, 4, 5];
        Clearable::clear(&mut data);
        // zeroize on Vec<T> zeros elements then clears the vector
        assert!(data.is_empty(), "Vec<u8> should be cleared after zeroize");
    }

    #[test]
    fn test_clearable_blanket_impl_for_string() {
        let mut data: String = String::from("secret-value-to-clear");
        Clearable::clear(&mut data);
        assert!(data.is_empty(), "String should be cleared after zeroize");
    }

    #[test]
    fn test_clearable_blanket_impl_preserves_type() {
        // The blanket impl should work for any T: Zeroize, returning the same type
        let mut data: Vec<u8> = vec![0xFF; 32];
        Clearable::clear(&mut data);
        assert!(data.is_empty());
        // Verify we can still use the cleared value as a Vec<u8>
        data.push(1);
        assert_eq!(data.len(), 1);
    }

    #[test]
    fn test_clearable_blanket_impl_empty_vec_is_noop() {
        let mut data: Vec<u8> = vec![];
        Clearable::clear(&mut data);
        assert!(data.is_empty());
    }
}
