// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! API Key 哈希验证模块
//!
//! 提供安全的 API Key 哈希存储和验证功能
//! 使用 bcrypt 算法进行密码学安全的哈希处理

use anyhow::{anyhow, Context, Result};
use bcrypt::{hash, verify, DEFAULT_COST};
use hex::encode;
use sha2::{Digest, Sha256};

// 重新导出环境变量安全模块
pub mod env_var_security;

// 安全 IP 提取模块
pub mod secure_ip;

// 重新导出常用类型
pub use secure_ip::{get_secure_client_ip, SecureIpExtractor, TrustedProxyConfig};

/// 安全模块错误类型
#[derive(Debug, thiserror::Error)]
pub enum SecurityError {
    #[error("API key hashing failed: {0}")]
    HashingFailed(String),
}

/// 计算 API Key 的哈希值
///
/// 使用 bcrypt 对 API Key 进行哈希处理，提供强大的暴力破解防护
///
/// # 参数
///
/// * `api_key` - 原始 API Key 字符串
///
/// # 返回值
///
/// * Result 包含 bcrypt 哈希字符串
///
/// # 示例
///
/// ```ignore
/// use crawlrs::infrastructure::security::hash_api_key;
///
/// let key = "my_secret_api_key";
/// let hash = hash_api_key(key);
/// assert!(!hash.is_empty());
/// ```
pub fn hash_api_key(api_key: &str) -> Result<String> {
    hash(api_key, DEFAULT_COST)
        .context("Failed to hash API key")
        .map_err(|e| anyhow!("Hashing failed: {}", e))
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
    verify(api_key, key_hash).unwrap_or(false)
}

/// 检测 API Key 哈希是否为旧版 SHA-256 格式
///
/// 旧版格式为纯十六进制字符串（64字符），新版 bcrypt 哈希以 `$2b$` 开头
///
/// # 参数
///
/// * `key_hash` - 要检测的哈希值
///
/// # 返回值
///
/// * `true` - 是旧版 SHA-256 格式
/// * `false` - 是新版 bcrypt 格式
pub fn is_legacy_sha256_hash(key_hash: &str) -> bool {
    // bcrypt 哈希以 "$2b$", "$2y$", or "$2a$" 开头
    // 旧版 SHA-256 是纯十六进制，64字符
    !key_hash.starts_with('$') && key_hash.len() == 64
}

/// 计算 API Key 的 SHA-256 哈希值（用于缓存键）
///
/// 使用 SHA-256 生成一致的哈希值，适用于缓存键和数据库查询。
/// 注意：这不是bcrypt，不能用于密码存储。
///
/// # 参数
///
/// * `api_key` - 原始 API Key 字符串
///
/// # 返回值
///
/// * 十六进制编码的 SHA-256 哈希字符串（64字符）
pub fn hash_api_key_sha256(api_key: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(api_key.as_bytes());
    encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_api_key_sha256() {
        let key = "test_api_key_12345";
        let hash1 = hash_api_key_sha256(key);
        let hash2 = hash_api_key_sha256(key);

        // SHA-256 是确定性的，相同的输入应该产生相同的输出
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.len(), 64); // SHA-256 十六进制是 64 字符

        // 不同的输入应该产生不同的哈希
        let hash3 = hash_api_key_sha256("different_key");
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_sha256_hash_format() {
        let key = "my_secret_key";
        let hash = hash_api_key_sha256(key);

        // 应该是纯十六进制字符串
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(hash.len(), 64);
    }

    #[test]
    fn test_verify_api_key() {
        let key = "test_api_key_12345";
        let hash = hash_api_key(key).unwrap();

        // 正确的密钥应该验证成功
        assert!(verify_api_key(key, &hash));

        // 错误的密钥应该验证失败
        assert!(!verify_api_key("wrong_key", &hash));
    }

    #[test]
    fn test_hash_is_deterministic() {
        let key = "consistent_key";
        let hash1 = hash_api_key(key).unwrap();
        let hash2 = hash_api_key(key).unwrap();

        // bcrypt 每次生成不同的哈希（随机盐），这是预期行为
        // 正确的测试：验证两者都能通过同一个 key
        assert_ne!(hash1, hash2); // 不同的哈希值
        assert!(verify_api_key(key, &hash1)); // 都能验证通过
        assert!(verify_api_key(key, &hash2));
    }

    #[test]
    fn test_legacy_hash_detection() {
        // 旧版 SHA-256 格式
        let legacy_hash = "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2";
        assert!(is_legacy_sha256_hash(legacy_hash));

        // 新版 bcrypt 格式
        let new_hash = "$2b$12$LQv3c1yqBWVHxkd0LHAkCOYz6TtxMQJqhN8/X4aFTTGq9dKjfSJOy";
        assert!(!is_legacy_sha256_hash(new_hash));
    }

    #[test]
    fn test_verify_rejects_wrong_key() {
        let key = "secure_api_key_xyz";
        let hash = hash_api_key(key).unwrap();

        assert!(!verify_api_key("wrong_key_123", &hash));
        assert!(!verify_api_key("", &hash));
        assert!(!verify_api_key(key, "invalid_hash"));
    }
}
