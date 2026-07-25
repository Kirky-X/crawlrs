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
//! - R-auth-engine-002：`Auth(String)` 变体包装 garrison 初始化失败原因。

use thiserror::Error;

/// Bootstrap 阶段错误。
///
/// 目前仅含 `Auth` 变体（R-auth-engine-002），后续可按需扩展（如 `Database`、`Cache` 等）。
#[derive(Debug, Error)]
pub enum BootstrapError {
    /// 认证/鉴权初始化失败（如 garrison `GarrisonManager::init` 失败、弱 JWT 密钥）。
    ///
    /// 内部 `String` 携带具体原因（如 `"garrison init failed: ..."`）。
    #[error("auth initialization failed: {0}")]
    Auth(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    /// R-auth-engine-002：Auth 变体 Display 输出含 "auth initialization failed" 前缀。
    #[test]
    fn test_auth_variant_display_contains_prefix() {
        let err = BootstrapError::Auth("garrison init failed: db connection refused".to_string());
        let msg = format!("{err}");
        assert!(
            msg.contains("auth initialization failed"),
            "Auth variant Display should contain 'auth initialization failed' prefix, got: {msg}"
        );
        assert!(
            msg.contains("garrison init failed"),
            "Auth variant Display should contain the inner message, got: {msg}"
        );
    }

    /// R-auth-engine-002：Auth 变体可经 source() 获取内部错误链（thiserror 自动实现）。
    #[test]
    fn test_auth_variant_is_std_error() {
        let err = BootstrapError::Auth("test".to_string());
        // 确认实现了 std::error::Error trait（thiserror 自动派生）
        let _: &dyn std::error::Error = &err;
    }
}
