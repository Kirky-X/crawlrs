// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Garrison 配置构造器。
//!
//! 从 confers 读取 `CRAWLRS__AUTH__JWT_SECRET` 与超时配置，
//! 在 [`GarrisonConfig::default_config()`] 基础上覆盖业务字段，
//! 弱密钥（<32 字节或空）拒绝。
//!
//! ## Spec
//!
//! - R-session-jwt-001：`build_garrison_config` 在 `jwt_secret` 为空/弱（<32 字节）时返回 `Err`；
//!   `token_style` 为 `jwt`。

use garrison::prelude::GarrisonConfig;
use thiserror::Error;

/// JWT 密钥最小长度（CWE-326 弱密钥防护）。
///
/// 32 字节 = 256 bit，匹配 HS256 算法的安全强度。
/// 短于该长度的密钥会被 [`build_garrison_config`] 拒绝。
pub const MIN_JWT_SECRET_BYTES: usize = 32;

/// [`build_garrison_config`] 的类型化错误。
///
/// 用类型化错误替代 `String`（规则8 惯例优先于新颖），使调用方可 `match` 判断原因
/// 而非字符串匹配；可经 `?` 透传到上层 `GarrisonResult` 或 crawlrs 自有 Error 枚举。
#[derive(Debug, Error)]
pub enum GarrisonConfigError {
    /// `jwt_secret` 为空字符串或未配置。
    #[error("jwt_secret must not be empty or missing")]
    EmptySecret,
    /// `jwt_secret` 长度 < [`MIN_JWT_SECRET_BYTES`]（HS256 要求 ≥32 字节）。
    #[error("weak jwt_secret: length {len} < {min} bytes (HS256 minimum)")]
    WeakSecret {
        /// 实际长度。
        len: usize,
        /// 最小要求长度（[`MIN_JWT_SECRET_BYTES`]）。
        min: usize,
    },
}

/// 构造 crawlrs 业务的 [`GarrisonConfig`]。
///
/// # 字段覆盖策略
///
/// 基于 `GarrisonConfig::default_config()` 默认值，覆盖以下业务字段：
/// - `token_style = "jwt"`（启用 JWT 协议，对应 garrison `protocol-jwt` feature）
/// - `jwt_secret` = 从 confers `CRAWLRS__AUTH__JWT_SECRET` 读取
/// - `jwt_algorithm = "HS256"`
/// - `is_read_cookie = false`（前后端分离，仅从 Header 读 token）
/// - `is_write_cookie = false`
/// - `frontend_separation = true`
///
/// # 弱密钥拒绝
///
/// 若 `jwt_secret` 为空或长度 < [`MIN_JWT_SECRET_BYTES`]，返回 `Err(GarrisonConfigError)`。
/// HS256 要求 ≥32 字节，由 garrison::config::validate 在 `GarrisonManager::init` 时再校验一次；
/// 此处提前拒绝以便给出业务清晰的错误。
///
/// # 返回
///
/// - `Ok(GarrisonConfig)` — 校验通过的业务配置
/// - `Err(GarrisonConfigError)` — 弱密钥错误（类型化，可 match 判断原因）
///
/// # Spec
///
/// - R-session-jwt-001
pub fn build_garrison_config(jwt_secret: &str) -> Result<GarrisonConfig, GarrisonConfigError> {
    if jwt_secret.is_empty() {
        return Err(GarrisonConfigError::EmptySecret);
    }
    if jwt_secret.len() < MIN_JWT_SECRET_BYTES {
        return Err(GarrisonConfigError::WeakSecret {
            len: jwt_secret.len(),
            min: MIN_JWT_SECRET_BYTES,
        });
    }

    // 基于 garrison 默认配置覆盖业务字段（规则5：复用而非重写）。
    let mut config = GarrisonConfig::default_config();
    config.token_style = "jwt".to_string();
    config.jwt_algorithm = "HS256".to_string();
    // protocol-zeroize feature 启用时 jwt_secret 字段为 Zeroizing<String>，
    // 用 .into() 兼容 String 和 Zeroizing<String> 两种类型（CWE-316 防护）。
    config.jwt_secret = jwt_secret.to_string().into();
    // 前后端分离：仅从 Authorization Header 读取 token，不读写 Cookie
    config.is_read_cookie = false;
    config.is_write_cookie = false;
    config.frontend_separation = true;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R-session-jwt-001：jwt_secret 为空时返回 `Err(EmptySecret)`
    #[test]
    fn test_build_garrison_config_rejects_empty_secret() {
        let result = build_garrison_config("");
        assert!(
            matches!(result, Err(GarrisonConfigError::EmptySecret)),
            "empty jwt_secret must be rejected with EmptySecret, got: {:?}",
            result
        );
    }

    /// R-session-jwt-001：jwt_secret 弱（<32 字节）时返回 `Err(WeakSecret { len, min })`
    #[test]
    fn test_build_garrison_config_rejects_weak_secret() {
        let weak = "too_short"; // 9 字节
        let result = build_garrison_config(weak);
        assert!(
            matches!(result, Err(GarrisonConfigError::WeakSecret { len: 9, min: 32 })),
            "weak jwt_secret (<32 bytes) must be rejected with WeakSecret {{ len: 9, min: 32 }}, got: {:?}",
            result
        );
    }

    /// R-session-jwt-001：强密钥（>=32 字节）返回 Ok 且 token_style=jwt
    #[test]
    fn test_build_garrison_config_strong_secret_returns_jwt_config() {
        let strong = "a-very-strong-secret-key-32-bytes-or-more!!"; // 44 字节
        let result = build_garrison_config(strong);
        assert!(result.is_ok(), "strong jwt_secret must be accepted");
        let config = result.unwrap();
        assert_eq!(
            config.token_style, "jwt",
            "token_style must be 'jwt' for crawlrs garrison auth"
        );
    }

    /// R-session-jwt-001：恰好 32 字节边界返回 Ok
    #[test]
    fn test_build_garrison_config_boundary_32_bytes_accepted() {
        let boundary = "0123456789abcdef0123456789abcdef"; // 32 字节
        let result = build_garrison_config(boundary);
        assert!(
            result.is_ok(),
            "32-byte jwt_secret (boundary) must be accepted"
        );
    }

    /// R-session-jwt-001：业务字段覆盖正确（is_read_cookie=false / frontend_separation=true）
    #[test]
    fn test_build_garrison_config_business_fields_overridden() {
        let strong = "a-very-strong-secret-key-32-bytes-or-more!!";
        let config = build_garrison_config(strong).unwrap();
        assert!(
            !config.is_read_cookie,
            "is_read_cookie must be false (frontend separation)"
        );
        assert!(
            !config.is_write_cookie,
            "is_write_cookie must be false (frontend separation)"
        );
        assert!(
            config.frontend_separation,
            "frontend_separation must be true"
        );
        assert_eq!(config.jwt_algorithm, "HS256", "jwt_algorithm must be HS256");
    }
}
