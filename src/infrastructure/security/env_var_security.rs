// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 环境变量安全模块（向后兼容 re-export shim）
//!
//! SEC-005：实际实现已拆分至：
//! - [`super::env_injection`] — 注入检测、白名单、脱敏、报告
//! - [`super::env_validation`] — 值验证、过滤、安全审计

// 重新导出所有公共 API，保持现有 `use env_var_security::*` 调用路径不变
pub use super::env_injection::{
    EnvVarCheckResult, EnvVarSecurityMonitor, EnvVarSecurityReport, EnvVarWhitelist,
};
pub use super::env_validation::{
    EnvVarValidator, LoggingSecurityWarning, LoggingWarningType, SecurityValidationResult,
    SensitiveVarWarning, SensitiveVarWarningType, WarningSeverity,
};
