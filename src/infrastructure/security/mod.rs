// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 安全模块入口
//!
//! 本模块聚合 `infrastructure::security` 下的所有子模块并通过 `pub use`
//! 重新导出公共 API，遵循规则 25（mod/crate 接口隔离）：
//! - `mod.rs` 只放模块声明、类型别名、re-export
//! - 具体实现拆到独立文件
//!
//! # 子模块
//!
//! | 模块 | 职责 |
//! |------|------|
//! | [`env_var_security`] | 环境变量安全读取与验证 |
//! | [`secure_ip`] | 受信代理下的客户端 IP 提取 |
//! | [`constant_time_compare`] | 常量时间字符串比较（防时序侧信道） |
//! | [`api_key_hash`] | API Key 的 bcrypt 哈希与验证 |

// 子模块声明
pub mod api_key_hash;
pub mod constant_time_compare;
pub mod env_var_security;
pub mod secure_ip;

// 重新导出常用类型与函数（保持现有调用路径 `security::hash_api_key` 等不变）
pub use api_key_hash::{
    hash_api_key, hash_api_key_sha256, is_legacy_sha256_hash, verify_api_key,
};
pub use constant_time_compare::constant_time_eq_str;
pub use secure_ip::{get_secure_client_ip, SecureIpExtractor, TrustedProxyConfig};
