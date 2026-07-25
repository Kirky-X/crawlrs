// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Bootstrap 阶段错误类型。
//!
//! 为 `bootstrap` 模块中可能失败的操作提供类型化错误（替代 `anyhow::Error` 的字符串上下文），
//! 使调用方可 `match` 判断原因而非字符串匹配（规则8 惯例优先于新颖）。
//!
//! ## Spec
//!
//! - R-auth-engine-002：garrison 初始化失败的类型化变体（`GarrisonConfig` / `GarrisonDao` / `GarrisonManager`）。
//!   Stage 3 重构：原 `Auth(String)` 兜底变体已删除（规则5 简洁优先 + 用户规则"禁止向后兼容"），
//!   三类故障层级已完整覆盖所有 garrison 初始化失败场景。
//!
//! ## Feature 门控
//!
//! `GarrisonConfig` 变体及 `GarrisonConfigError` 类型别名仅在 `auth` feature 启用时编译，
//! 因为其依赖的 `crate::infrastructure::auth::garrison_config::GarrisonConfigError` 本身是 auth-gated。
//! 其他变体（`GarrisonDao` / `GarrisonManager`）始终编译，承载非 auth 场景的字符串上下文。

use thiserror::Error;

/// Garrison 配置错误（弱密钥 / 空密钥 / 格式非法）。
///
/// 由 [`crate::infrastructure::auth::garrison_config::GarrisonConfigError`]
/// 转换而来，保留原始错误链（`#[from]` 自动实现）。
///
/// ## Feature 门控
///
/// 仅在 `auth` feature 启用时可用——`GarrisonConfigError` 源类型本身是 auth-gated。
#[cfg(feature = "auth")]
pub type GarrisonConfigError = crate::infrastructure::auth::garrison_config::GarrisonConfigError;

/// Bootstrap 阶段错误。
///
/// ## 变体分层（按故障层级）
///
/// | 变体 | 故障层级 | 典型原因 | Feature |
/// |------|---------|---------|---------|
/// | `GarrisonConfig` | 配置层 | 弱密钥 / 空密钥 / 格式非法 | `auth` |
/// | `GarrisonDao` | DAO 层 | oxcache 初始化失败 / 内存资源不足 | always |
/// | `GarrisonManager` | Manager 层 | `GarrisonManager::init` 失败 / 配置非法 | always |
#[derive(Debug, Error)]
pub enum BootstrapError {
    /// garrison 配置构造失败（弱密钥 / 空密钥 / 格式非法）。
    ///
    /// 由 `build_garrison_config` 返回 `GarrisonConfigError` 转换而来。
    /// 调用方可 `match` 判断是 `EmptySecret` 还是 `WeakSecret`。
    ///
    /// ## Feature 门控
    ///
    /// 仅在 `auth` feature 启用时编译——`GarrisonConfigError` 源类型本身是 auth-gated。
    #[cfg(feature = "auth")]
    #[error("garrison config error: {0}")]
    GarrisonConfig(#[from] GarrisonConfigError),

    /// garrison DAO 初始化失败（oxcache 实例创建失败 / 内存资源不足）。
    ///
    /// 内部 `String` 携带 dbnexus/oxcache 错误详情。
    #[error("garrison dao init failed: {0}")]
    GarrisonDao(String),

    /// garrison Manager 初始化失败（`GarrisonManager::init` 返回 `Err`）。
    ///
    /// 内部 `String` 携带 garrison manager 错误详情。
    #[error("garrison manager init failed: {0}")]
    GarrisonManager(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// GarrisonConfig 变体 Display 输出含 "garrison config error" 前缀。
    #[cfg(feature = "auth")]
    #[test]
    fn test_garrison_config_variant_display() {
        let err = BootstrapError::GarrisonConfig(GarrisonConfigError::EmptySecret);
        let msg = format!("{err}");
        assert!(
            msg.contains("garrison config error"),
            "GarrisonConfig variant Display should contain 'garrison config error' prefix, got: {msg}"
        );
        assert!(
            msg.contains("jwt_secret must not be empty or missing"),
            "GarrisonConfig variant Display should contain the inner error, got: {msg}"
        );
    }

    /// GarrisonConfig 变体的 WeakSecret 子类型 Display 输出含长度信息。
    #[cfg(feature = "auth")]
    #[test]
    fn test_garrison_config_weak_secret_display() {
        let err =
            BootstrapError::GarrisonConfig(GarrisonConfigError::WeakSecret { len: 9, min: 32 });
        let msg = format!("{err}");
        assert!(msg.contains("9"));
        assert!(msg.contains("32"));
    }

    /// GarrisonDao 变体 Display 输出含 "garrison dao init failed" 前缀。
    #[test]
    fn test_garrison_dao_variant_display() {
        let err = BootstrapError::GarrisonDao("oxcache capacity exceeded".to_string());
        let msg = format!("{err}");
        assert!(
            msg.contains("garrison dao init failed"),
            "GarrisonDao variant Display should contain 'garrison dao init failed' prefix, got: {msg}"
        );
        assert!(
            msg.contains("oxcache capacity exceeded"),
            "GarrisonDao variant Display should contain the inner message, got: {msg}"
        );
    }

    /// GarrisonManager 变体 Display 输出含 "garrison manager init failed" 前缀。
    #[test]
    fn test_garrison_manager_variant_display() {
        let err = BootstrapError::GarrisonManager("invalid config format".to_string());
        let msg = format!("{err}");
        assert!(
            msg.contains("garrison manager init failed"),
            "GarrisonManager variant Display should contain 'garrison manager init failed' prefix, got: {msg}"
        );
        assert!(
            msg.contains("invalid config format"),
            "GarrisonManager variant Display should contain the inner message, got: {msg}"
        );
    }

    /// 所有变体均实现 std::error::Error trait（thiserror 自动派生）。
    #[test]
    fn test_all_variants_are_std_error() {
        let dao_err = BootstrapError::GarrisonDao("test".to_string());
        let _: &dyn std::error::Error = &dao_err;

        let manager_err = BootstrapError::GarrisonManager("test".to_string());
        let _: &dyn std::error::Error = &manager_err;

        // GarrisonConfig 变体仅在 auth feature 启用时编译
        #[cfg(feature = "auth")]
        {
            let config_err = BootstrapError::GarrisonConfig(GarrisonConfigError::EmptySecret);
            let _: &dyn std::error::Error = &config_err;
        }
    }

    /// GarrisonConfigError 可经 `?` 透传到 BootstrapError（#[from] 自动转换）。
    #[cfg(feature = "auth")]
    #[test]
    fn test_garrison_config_error_from_conversion() {
        fn fallible() -> Result<(), BootstrapError> {
            Err(GarrisonConfigError::EmptySecret)?;
            Ok(())
        }
        let err = fallible().unwrap_err();
        assert!(matches!(
            err,
            BootstrapError::GarrisonConfig(GarrisonConfigError::EmptySecret)
        ));
    }
}
