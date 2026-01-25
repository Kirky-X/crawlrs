// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 引擎配置
//!
//! 包含 FlareSolverr、Fire Engine 等抓取引擎的配置设置

use confers::Config;
use serde::{Deserialize, Serialize};

/// FlareSolverr 引擎配置设置
///
/// 配置 FlareSolverr 引擎的参数，用于绕过 Cloudflare 和其他反爬虫保护
///
/// # 字段说明
///
/// * `enabled` - 是否启用 FlareSolverr 引擎
/// * `url` - FlareSolverr 服务器 URL
/// * `timeout_seconds` - 请求超时时间（秒）
/// * `max_retries` - 最大重试次数
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__ENGINES__FLARESOLVERR__")]
pub struct FlareSolverrSettings {
    /// 是否启用 FlareSolverr 引擎
    #[config(default = false)]
    pub enabled: bool,

    /// FlareSolverr 服务器 URL
    #[config(default = "http://localhost:8191/v1")]
    pub url: String,

    /// 请求超时时间（秒）
    #[config(default = 30)]
    pub timeout_seconds: u64,

    /// 最大重试次数
    #[config(default = 3)]
    pub max_retries: u32,
}

/// Fire Engine CDP 配置设置
///
/// 配置 Fire Engine CDP（Chrome DevTools Protocol）的参数
///
/// # 字段说明
///
/// * `enabled` - 是否启用 Fire Engine CDP
/// * `url` - Fire Engine CDP 服务器 URL
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__ENGINES__FIRE_CDP__")]
pub struct FireCdpSettings {
    /// 是否启用 Fire Engine CDP
    #[config(default = false)]
    pub enabled: bool,

    /// Fire Engine CDP 服务器 URL
    #[config(default = "http://localhost:8191/v1")]
    pub url: String,
}

/// Fire Engine TLS 配置设置
///
/// 配置 Fire Engine TLS 的参数，专注于 TLS 指纹对抗
///
/// # 字段说明
///
/// * `enabled` - 是否启用 Fire Engine TLS
/// * `url` - Fire Engine TLS 服务器 URL
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__ENGINES__FIRE_TLS__")]
pub struct FireTlsSettings {
    /// 是否启用 Fire Engine TLS
    #[config(default = false)]
    pub enabled: bool,

    /// Fire Engine TLS 服务器 URL
    #[config(default = "http://localhost:8191/v1")]
    pub url: String,
}

/// 引擎配置集合
///
/// 包含所有抓取引擎的配置
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__ENGINES__")]
pub struct EngineSettings {
    /// FlareSolverr 引擎配置
    #[config(default)]
    pub flaresolverr: FlareSolverrSettings,

    /// Fire Engine CDP 配置
    #[config(default)]
    pub fire_cdp: FireCdpSettings,

    /// Fire Engine TLS 配置
    #[config(default)]
    pub fire_tls: FireTlsSettings,
}
