// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! API Key 哈希验证模块
//!
//! 提供安全的 API Key 哈希存储和验证功能

use hex::encode;
use sha2::{Digest, Sha256};

/// 计算 API Key 的哈希值
///
/// 使用 SHA-256 对 API Key 进行哈希处理
///
/// # 参数
///
/// * `api_key` - 原始 API Key 字符串
///
/// # 返回值
///
/// * 十六进制编码的哈希字符串
pub fn hash_api_key(api_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    let result = hasher.finalize();
    encode(result)
}

/// 验证 API Key 是否匹配哈希值
///
/// # 参数
///
/// * `api_key` - 原始 API Key 字符串
/// * `key_hash` - 存储的哈希值
///
/// # 返回值
///
/// * `true` - 验证成功
/// * `false` - 验证失败
pub fn verify_api_key(api_key: &str, key_hash: &str) -> bool {
    let computed_hash = hash_api_key(api_key);
    computed_hash == key_hash
}

/// 从明文密钥计算哈希
/// 用于在创建新密钥时生成哈希值
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_api_key() {
        let key = "test_api_key_12345";
        let hash = hash_api_key(key);

        // 哈希应该是 64 个字符的十六进制字符串
        assert_eq!(hash.len(), 64);

        // 相同的输入应该产生相同的哈希
        assert_eq!(hash_api_key(key), hash);

        // 不同的输入应该产生不同的哈希
        assert_ne!(hash_api_key("different_key"), hash);
    }

    #[test]
    fn test_verify_api_key() {
        let key = "test_api_key_12345";
        let hash = hash_api_key(key);

        // 正确的密钥应该验证成功
        assert!(verify_api_key(key, &hash));

        // 错误的密钥应该验证失败
        assert!(!verify_api_key("wrong_key", &hash));
    }

    #[test]
    fn test_hash_is_deterministic() {
        let key = "consistent_key";
        let hash1 = hash_api_key(key);
        let hash2 = hash_api_key(key);
        assert_eq!(hash1, hash2);
    }
}
