// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 引擎配置
//!
//! 包含 FlareSolverr 各模式（Full/CDP/TLS）抓取引擎的配置设置

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
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__ENGINES__FLARESOLVERR__")]
pub struct FlareSolverrSettings {
    /// 是否启用 FlareSolverr 引擎
    #[config(default = false)]
    pub enabled: bool,

    /// FlareSolverr 服务器 URL
    #[config(default = "http://localhost:8191/v1".to_string())]
    pub url: String,

    /// 请求超时时间（秒）
    #[config(default = 30)]
    pub timeout_seconds: u64,

    /// 最大重试次数
    #[config(default = 3)]
    pub max_retries: u32,
}

/// FlareSolverr CDP 配置设置
///
/// 配置 FlareSolverr CDP（Chrome DevTools Protocol）模式的参数
///
/// # 字段说明
///
/// * `enabled` - 是否启用 FlareSolverr CDP 模式
/// * `url` - FlareSolverr CDP 服务器 URL
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__ENGINES__FLARESOLVERR_CDP__")]
pub struct FlareSolverrCdpSettings {
    /// 是否启用 FlareSolverr CDP 模式
    #[config(default = false)]
    pub enabled: bool,

    /// FlareSolverr CDP 服务器 URL
    #[config(default = "http://localhost:8191/v1".to_string())]
    pub url: String,
}

/// FlareSolverr TLS 配置设置
///
/// 配置 FlareSolverr TLS 模式的参数，专注于 TLS 指纹对抗
///
/// # 字段说明
///
/// * `enabled` - 是否启用 FlareSolverr TLS 模式
/// * `url` - FlareSolverr TLS 服务器 URL
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__ENGINES__FLARESOLVERR_TLS__")]
pub struct FlareSolverrTlsSettings {
    /// 是否启用 FlareSolverr TLS 模式
    #[config(default = false)]
    pub enabled: bool,

    /// FlareSolverr TLS 服务器 URL
    #[config(default = "http://localhost:8191/v1".to_string())]
    pub url: String,
}

/// Wreq TLS 指纹引擎配置设置（Phase 1 / D4）
///
/// 配置 `wreq`（BoringSSL 后端）TLS 指纹引擎的参数，用于 `needs_tls_fingerprint`
/// 请求的真实 JA3/JA4 伪装。
///
/// # 字段说明
///
/// * `enabled` - 是否启用 Wreq TLS 指纹引擎（默认关闭，需显式开启）
/// * `timeout_seconds` - 引擎级请求超时（秒），默认 15（与 `EngineTimeoutSettings::tls_seconds` 一致）
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__ENGINES__TLS_FINGERPRINT__")]
pub struct TlsFingerprintEngineSettings {
    /// 是否启用 Wreq TLS 指纹引擎
    #[config(default = false)]
    pub enabled: bool,

    /// 引擎级请求超时（秒）
    #[config(default = 15)]
    pub timeout_seconds: u32,
}

/// MLLM 自主导航爬取引擎配置（Phase 3）
///
/// 配置视觉大模型驱动的浏览器自主导航引擎。
/// 依赖 `engine-playwright`（浏览器）+ `genai-llm`（视觉模型）。
///
/// # 字段说明
///
/// * `enabled` - 是否启用 MLLM 引擎
/// * `vision_model` - 视觉模型标识（如 "gemini:gemini-2.0-flash"）
/// * `max_iterations` - 最大导航迭代次数（防止无限循环）
/// * `screenshot_quality` - 截图质量 (0-100)
/// * `max_token_budget` - 单次请求最大 token 预算
/// * `mrt_seconds` - 引擎级最大响应时间（秒）
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__ENGINES__MLLM__")]
pub struct MllmEngineSettings {
    /// 是否启用 MLLM 引擎
    #[config(default = false)]
    pub enabled: bool,

    /// 视觉模型标识
    #[config(default = "gemini:gemini-2.0-flash".to_string())]
    pub vision_model: String,

    /// 最大导航迭代次数
    #[config(default = 10)]
    pub max_iterations: u8,

    /// 截图质量 (0-100)
    #[config(default = 70)]
    pub screenshot_quality: u8,

    /// 单次请求最大 token 预算
    #[config(default = 4096)]
    pub max_token_budget: u32,

    /// 引擎级最大响应时间（秒）
    #[config(default = 60)]
    pub mrt_seconds: u64,
}

/// 引擎配置集合
///
/// 包含所有抓取引擎的配置
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__ENGINES__")]
pub struct EngineSettings {
    /// FlareSolverr 引擎配置
    pub flaresolverr: FlareSolverrSettings,

    /// FlareSolverr CDP 模式配置
    pub flaresolverr_cdp: FlareSolverrCdpSettings,

    /// FlareSolverr TLS 模式配置
    pub flaresolverr_tls: FlareSolverrTlsSettings,

    /// Wreq TLS 指纹引擎配置
    pub tls_fingerprint: TlsFingerprintEngineSettings,

    /// MLLM 自主导航引擎配置（Phase 3）
    pub mllm: MllmEngineSettings,
}
