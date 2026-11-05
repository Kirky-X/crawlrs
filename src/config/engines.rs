// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

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

/// Wreq TLS 指纹引擎配置设置
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

/// MLLM 自主导航爬取引擎配置
///
/// 配置视觉大模型驱动的浏览器自主导航引擎。
/// 依赖 `engine-playwright`（浏览器）+ `llm`（视觉模型）。
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

    /// 混合 DOM+AX 树快照开关（F12）：true 时每轮导航决策附 `[id] role: name`
    /// 文本大纲，与截图视觉通道并存；序列化长度受 `max_token_budget` 约束
    #[serde(default)]
    #[config(default = false)]
    pub ax_tree_enabled: bool,
}

/// Playwright 引擎配置
///
/// 配置 Playwright（CDP 浏览器）引擎的扩展行为。
///
/// # 字段说明
///
/// * `capture_xhr` - 是否捕获 XHR/Fetch 后台响应（默认关）
/// * `capture_xhr_url_patterns` - 捕获 URL glob 白名单（空 = 全部）
/// * `capture_xhr_max_bytes` - 单响应体上限（字节）
/// * `capture_xhr_max_responses` - 单页面最大捕获响应数
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__ENGINES__PLAYWRIGHT__")]
pub struct PlaywrightEngineSettings {
    /// 是否捕获 XHR/Fetch 后台响应（false 时无 Network 监听开销）
    #[config(default = false)]
    pub capture_xhr: bool,

    /// 捕获 URL 匹配模式（glob，支持 `*` 与 `?`；空列表 = 捕获全部）
    #[config(default = Vec::<String>::new())]
    pub capture_xhr_url_patterns: Vec<String>,

    /// 单响应体捕获上限（字节），超出截断丢弃
    #[config(default = 262144)]
    pub capture_xhr_max_bytes: u64,

    /// 单页面最大捕获响应数（防内存/带宽放大）
    #[config(default = 50)]
    pub capture_xhr_max_responses: u32,
}

/// 内存水位调度器配置
///
/// 系统可用内存三档水位（Critical/Tight/Abundant）驱动引擎调度：
/// Critical 冻结浏览器系引擎（仅 moli 可承担 JS）、Tight moli 优先、Abundant 原策略不变。
///
/// # 字段说明
///
/// * `enabled` - 是否启用内存水位调度
/// * `critical_available_mib` - Critical 档阈值（可用 MiB，`<` 语义）
/// * `tight_available_mib` - Tight 档阈值（可用 MiB，`<` 语义；必须 > critical）
/// * `refresh_interval_ms` - 采样刷新周期（毫秒）
/// * `fallback_to_static` - Critical 且无可用 JS 引擎时是否回退静态抓取（否则快速失败）
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__ENGINES__MEMORY_SCHEDULER__")]
pub struct MemorySchedulerSettings {
    /// 是否启用内存水位调度
    #[config(default = true)]
    pub enabled: bool,

    /// Critical 档阈值（可用 MiB）
    #[config(default = 512)]
    pub critical_available_mib: u64,

    /// Tight 档阈值（可用 MiB）
    #[config(default = 2048)]
    pub tight_available_mib: u64,

    /// 采样刷新周期（毫秒）
    #[config(default = 2000)]
    pub refresh_interval_ms: u64,

    /// Critical 且无可用 JS 引擎时回退静态抓取（false = 快速失败）
    #[config(default = true)]
    pub fallback_to_static: bool,
}

/// MoliEngine 配置（engine-moli 特性，PoC GO 后落地）
///
/// 端点模式：crawlrs 不拉起/不托管 moli 进程，`moli serve --layout` 由部署方负责
/// （截图能力前置，见 poc-report.md）。
///
/// # 字段说明
///
/// * `enabled` - 是否启用 moli 引擎（默认关）
/// * `endpoint` - moli serve CDP 端点（http URL，含端口）
/// * `connect_timeout_ms` - CDP 建连超时（毫秒）
/// * `scrape_timeout_ms` - 单次抓取（导航+等待+取 DOM）超时（毫秒），即引擎 MRT
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__ENGINES__MOLI__")]
pub struct MoliEngineSettings {
    /// 是否启用 moli 引擎
    #[config(default = false)]
    pub enabled: bool,

    /// moli serve CDP 端点
    #[config(default = "http://127.0.0.1:9222".to_string())]
    pub endpoint: String,

    /// CDP 建连超时（毫秒）
    #[config(default = 3000)]
    pub connect_timeout_ms: u64,

    /// 单次抓取超时（毫秒）
    #[config(default = 30000)]
    pub scrape_timeout_ms: u64,
}

/// 引擎配置集合
///
/// 包含所有抓取引擎的配置
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__ENGINES__")]
pub struct EngineSettings {
    /// 响应体大小上限（字节）：超出中断抓取，防止超大/恶意页面 OOM
    #[config(default = 10485760)]
    pub max_response_body_bytes: u64,

    /// FlareSolverr 引擎配置
    pub flaresolverr: FlareSolverrSettings,

    /// FlareSolverr CDP 模式配置
    pub flaresolverr_cdp: FlareSolverrCdpSettings,

    /// FlareSolverr TLS 模式配置
    pub flaresolverr_tls: FlareSolverrTlsSettings,

    /// Wreq TLS 指纹引擎配置
    pub tls_fingerprint: TlsFingerprintEngineSettings,

    /// MLLM 自主导航引擎配置
    pub mllm: MllmEngineSettings,

    /// Playwright 引擎配置（XHR/Fetch 后台响应捕获等）
    pub playwright: PlaywrightEngineSettings,

    /// 内存水位调度器配置
    pub memory_scheduler: MemorySchedulerSettings,

    /// MoliEngine 配置（engine-moli 特性）
    pub moli: MoliEngineSettings,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// MllmEngineSettings 默认值与 config/default.toml `[engines.mllm]` 节一致（F12）
    #[test]
    fn test_mllm_engine_settings_defaults_match_spec() {
        let s = MllmEngineSettings::default();
        assert!(!s.enabled);
        assert_eq!(s.max_iterations, 10);
        assert_eq!(s.screenshot_quality, 70);
        assert_eq!(s.max_token_budget, 4096);
        assert_eq!(s.mrt_seconds, 60);
        assert!(!s.ax_tree_enabled, "AX 树快照默认关（F12）");
    }

    /// MllmEngineSettings 从 toml 全键反序列化（含 ax_tree_enabled，F12）
    #[test]
    fn test_mllm_engine_settings_toml_parses_ax_tree_enabled() {
        let toml = r#"
            enabled = true
            vision_model = "gemini:gemini-2.0-flash"
            max_iterations = 5
            screenshot_quality = 80
            max_token_budget = 2048
            mrt_seconds = 90
            ax_tree_enabled = true
        "#;
        let s: MllmEngineSettings = toml::from_str(toml).expect("deserialize");
        assert!(s.enabled);
        assert_eq!(s.max_iterations, 5);
        assert_eq!(s.max_token_budget, 2048);
        assert!(s.ax_tree_enabled);
    }

    /// 旧配置缺 `ax_tree_enabled` 键时 serde 默认兜底（其余键按既有惯例必填，F12）
    #[test]
    fn test_mllm_engine_settings_without_ax_tree_key_defaults() {
        let toml = r#"
            enabled = false
            vision_model = "gemini:gemini-2.0-flash"
            max_iterations = 10
            screenshot_quality = 70
            max_token_budget = 4096
            mrt_seconds = 60
        "#;
        let s: MllmEngineSettings = toml::from_str(toml).expect("deserialize");
        assert!(!s.ax_tree_enabled);
    }

    /// MemorySchedulerSettings 默认值与 config/default.toml `[engines.memory_scheduler]` 一致
    #[test]
    fn test_memory_scheduler_settings_defaults() {
        let s = MemorySchedulerSettings::default();
        assert!(s.enabled);
        assert_eq!(s.critical_available_mib, 512);
        assert_eq!(s.tight_available_mib, 2048);
        assert_eq!(s.refresh_interval_ms, 2000);
        assert!(s.fallback_to_static);
    }

    /// MoliEngineSettings 默认值与 config/default.toml `[engines.moli]` 一致
    #[test]
    fn test_moli_engine_settings_defaults() {
        let s = MoliEngineSettings::default();
        assert!(!s.enabled);
        assert_eq!(s.endpoint, "http://127.0.0.1:9222");
        assert_eq!(s.connect_timeout_ms, 3000);
        assert_eq!(s.scrape_timeout_ms, 30000);
    }

    /// MemorySchedulerSettings 从 toml 全键反序列化
    #[test]
    fn test_memory_scheduler_settings_toml_parses() {
        let toml = r#"
            enabled = false
            critical_available_mib = 256
            tight_available_mib = 1024
            refresh_interval_ms = 1000
            fallback_to_static = false
        "#;
        let s: MemorySchedulerSettings = toml::from_str(toml).expect("deserialize");
        assert!(!s.enabled);
        assert_eq!(s.critical_available_mib, 256);
        assert_eq!(s.tight_available_mib, 1024);
        assert_eq!(s.refresh_interval_ms, 1000);
        assert!(!s.fallback_to_static);
    }
}
