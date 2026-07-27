// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 配置模块
//!
//! 使用 confers 库进行配置管理，支持：
//! - TOML 配置文件
//! - 环境变量覆盖 (CRAWLRS__ 前缀)
//! - 类型安全的配置解析
//! - 内置验证

use serde::{Deserialize, Serialize};
use validator::Validate;

// 重新导出子模块中的类型
pub use super::app::{ConcurrencySettings, DatabaseSettings, RateLimitingSettings, ServerSettings};
pub use super::engines::{
    EngineSettings, FlareSolverrCdpSettings, FlareSolverrSettings, FlareSolverrTlsSettings,
};
pub use super::llm::LLMSettings;
pub use super::logging::{ConsoleLoggingSettings, FileLoggingSettings, LoggingSettings};
pub use super::search::{BingSearchSettings, SearchSettings};

// =============================================================================
// 主配置结构
// =============================================================================

/// 应用程序配置设置
///
/// 包含数据库、服务器、速率限制和并发控制等所有配置项
///
/// # 使用示例
///
/// ```ignore
/// use crawlrs::config::Settings;
///
/// fn main() -> Result<(), Box<dyn std::error::Error>> {
///     let settings = Settings::load()?;
///     println!("Server will run on {}:{}", settings.server.host, settings.server.port);
///     Ok(())
/// }
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, Validate, confers::Config)]
#[config(env_prefix = "CRAWLRS__", validate)]
pub struct Settings {
    /// 服务器配置
    pub server: ServerSettings,

    /// 数据库配置
    pub database: DatabaseSettings,

    /// CORS 配置
    pub cors: CorsSettings,

    /// 速率限制配置
    pub rate_limiting: RateLimitingSettings,

    /// 并发控制配置
    pub concurrency: ConcurrencySettings,

    /// Webhook 配置
    pub webhook: WebhookSettings,

    /// Bing Search API 配置
    pub bing_search: BingSearchSettings,

    /// 搜索配置 (包含 A/B 测试)
    pub search: SearchSettings,

    /// LLM 配置
    pub llm: LLMSettings,

    /// HTTP 代理配置
    pub proxy: ProxySettings,

    /// 引擎配置
    pub engines: EngineSettings,

    /// 日志配置
    pub logging: LoggingSettings,

    /// Worker 配置
    pub workers: WorkerSettings,

    /// 超时配置
    #[validate(nested)]
    pub timeouts: TimeoutSettings,

    /// 缓存配置
    pub cache: CacheSettings,

    /// 可信代理配置
    pub trusted_proxies: TrustedProxySettings,

    /// 认证配置（R-auth-engine-002 / T011：garrison JWT 密钥等）
    pub auth: AuthSettings,
}

// =============================================================================
// CORS 配置
// =============================================================================

/// CORS 配置设置
///
/// 配置跨域资源共享（CORS）策略
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__CORS__")]
pub struct CorsSettings {
    /// 允许的跨域来源列表（逗号分隔）
    #[config(default = "*".to_string())]
    pub allowed_origins: String,
}

// =============================================================================
// Webhook 配置
// =============================================================================

/// Webhook配置设置
///
/// 配置 Webhook 功能的参数
///
/// # 字段说明
///
/// * `secret` - Webhook 签名密钥，用于验证请求真实性（敏感信息，仅 crate 可见）
///
/// # 安全提示
///
/// `secret` 字段包含 Webhook 签名密钥，泄露可能导致伪造请求。
/// 该字段仅对 crate 可见，外部模块应使用 `secret()` 方法访问。
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__WEBHOOK__")]
pub struct WebhookSettings {
    /// Webhook签名密钥 (敏感信息)
    /// 注意：此字段包含敏感信息，仅 crate 内部可访问
    pub(crate) secret: String,

    /// 最大重试次数
    #[config(default = 5)]
    pub max_retries: u32,

    /// 批处理大小
    #[config(default = 1000)]
    pub batch_size: usize,
}

impl WebhookSettings {
    /// 获取 Webhook 签名密钥
    ///
    /// # 安全提示
    ///
    /// 此方法返回 Webhook 签名密钥，调用者应谨慎处理，
    /// 不要记录到日志或暴露给用户。
    pub fn secret(&self) -> &str {
        &self.secret
    }
}

// =============================================================================
// 认证配置（R-auth-engine-002 / T011）
// =============================================================================

/// 认证配置设置。
///
/// 配置 garrison 认证鉴权框架的 JWT 密钥等参数。
///
/// # 字段说明
///
/// * `jwt_secret` - JWT 签名密钥（HS256），敏感信息，仅 crate 可见
///
/// # 安全提示
///
/// `jwt_secret` 字段包含 JWT 签名密钥，泄露可能导致 token 伪造。
/// 该字段仅对 crate 可见，外部模块应使用 `jwt_secret()` 方法访问。
/// 密钥长度须 ≥32 字节（HS256 最小要求），弱密钥会在 `build_garrison_config` 中被拒绝。
///
/// # Debug 脱敏
///
/// 手动实现 `Debug`（不派生 `#[derive(Debug)]`），对 `jwt_secret` 字段输出 `"***REDACTED***"`，
/// 防止日志打印 Settings 时泄露密钥（CWE-532 防护）。对齐 [`ProxySettings`] 的 redaction 模式。
#[derive(Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__AUTH__")]
pub struct AuthSettings {
    /// JWT 签名密钥（HS256，敏感信息）。
    ///
    /// 默认空字符串——`auth` feature 启用时，`build_garrison_config("")` 会返回 `Err(EmptySecret)`，
    /// 导致启动 panic（fail-fast），强制运维通过环境变量或配置文件提供强密钥。
    #[config(default = "".to_string())]
    pub(crate) jwt_secret: String,
}

impl std::fmt::Debug for AuthSettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthSettings")
            .field("jwt_secret", &"***REDACTED***")
            .finish()
    }
}

impl AuthSettings {
    /// 获取 JWT 签名密钥。
    ///
    /// # 安全提示
    ///
    /// 此方法返回 JWT 签名密钥，调用者应谨慎处理，
    /// 不要记录到日志或暴露给用户。
    pub fn jwt_secret(&self) -> &str {
        &self.jwt_secret
    }
}

// =============================================================================
// 代理配置
// =============================================================================

/// 代理轮换策略（design.md §12，T055/R-identity-003）
///
/// 决定调用方在 ProxyPool 上的默认行为：
/// - `RoundRobin`：每次请求调用 `ProxyPool::next(category)` 取下一个
/// - `Sticky`：按 `session_id` 调用 `ProxyPool::sticky(session_id)` 锁定同一代理
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProxyStrategy {
    #[default]
    RoundRobin,
    Sticky,
}

/// HTTP代理配置设置
///
/// 配置HTTP代理参数，用于转发爬虫请求。
///
/// 支持多代理轮换池（design.md §12 / R-identity-003）：
/// - `urls`: 代理 URL 列表，可包含 userinfo（日志输出会脱敏）
/// - `strategy`: 轮换策略（RoundRobin / Sticky）
/// - `enabled`: 是否启用代理
/// - `sticky_ttl_seconds`: 粘性会话 TTL（秒），TTL 内同一 session_id 返回同一代理
/// - `cooldown_seconds`: 失败代理默认冷却时长（秒）
#[derive(Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__PROXY__")]
pub struct ProxySettings {
    /// 代理 URL 列表（支持多个代理轮换，按 strategy 决定调度方式）
    #[config(default = Vec::<String>::new())]
    pub urls: Vec<String>,

    /// 代理轮换策略
    #[serde(default)]
    pub strategy: ProxyStrategy,

    /// 是否启用代理
    #[config(default = false)]
    pub enabled: bool,

    /// 粘性会话 TTL（秒）
    ///
    /// `ProxyStrategy::Sticky` 时，TTL 内同一 `session_id` 返回同一代理；
    /// TTL 过期或代理冷却中时重选。
    /// 默认 60 秒（MEDIUM-2 修复：从 di/modules.rs 硬编码移入配置）。
    #[config(default = 60)]
    #[serde(default = "default_sticky_ttl_seconds")]
    pub sticky_ttl_seconds: u64,

    /// 失败代理默认冷却时长（秒）
    ///
    /// `mark_failure` 后代理进入冷却，在此期间不被 `next` / `sticky` 选中。
    /// 默认 30 秒（MEDIUM-2 修复：从 di/modules.rs 硬编码移入配置）。
    #[config(default = 30)]
    #[serde(default = "default_cooldown_seconds")]
    pub cooldown_seconds: u64,
}

/// serde 默认值：粘性会话 TTL（与 `#[config(default = 60)]` 保持一致）
fn default_sticky_ttl_seconds() -> u64 {
    60
}

/// serde 默认值：失败代理冷却时长（与 `#[config(default = 30)]` 保持一致）
fn default_cooldown_seconds() -> u64 {
    30
}

impl std::fmt::Debug for ProxySettings {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // 安全：urls 可能含 user:pass@host 凭证，Debug 输出统一脱敏
        f.debug_struct("ProxySettings")
            .field("urls", &"***REDACTED***")
            .field("strategy", &self.strategy)
            .field("enabled", &self.enabled)
            .field("sticky_ttl_seconds", &self.sticky_ttl_seconds)
            .field("cooldown_seconds", &self.cooldown_seconds)
            .finish()
    }
}

impl ProxySettings {
    /// 获取代理 URL 列表
    pub fn urls(&self) -> &[String] {
        &self.urls
    }

    /// 获取代理轮换策略
    pub fn strategy(&self) -> ProxyStrategy {
        self.strategy
    }
}

// =============================================================================
// Worker 配置
// =============================================================================

/// Worker配置设置
///
/// 配置后台Worker进程的数量和类型
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__WORKERS__")]
pub struct WorkerSettings {
    /// Worker数量配置
    pub count: WorkerCount,
}

/// Worker数量配置
///
/// 支持固定数量或自动检测CPU核心数
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(untagged)]
pub enum WorkerCount {
    Auto(String),
    Fixed(usize),
}

impl Default for WorkerCount {
    fn default() -> Self {
        WorkerCount::Auto("auto".to_string())
    }
}

impl WorkerCount {
    /// 解析为实际的worker数量
    pub fn resolve(&self) -> usize {
        match self {
            WorkerCount::Auto(s) if s.eq_ignore_ascii_case("auto") => {
                let logical_cores = std::thread::available_parallelism()
                    .map(|n| n.get())
                    .unwrap_or(4);
                logical_cores * 2
            }
            WorkerCount::Fixed(n) => *n,
            _ => 5,
        }
    }
}

// =============================================================================
// 超时配置
// =============================================================================

/// 超时配置设置
///
/// 配置各种操作的超时时间
///
/// # 安全验证（T062 安全审查 HIGH-1 修复）
///
/// `engines` 字段添加 `#[validate(nested)]`，使 `Settings::validate()` 递归
/// 调用 `EngineTimeoutSettings::validate()`，覆盖所有 `#[validate(range(min=1, max=600))]`
/// 约束。其余子结构（workers/retry/cache）无 range 约束，不需要 nested。
#[derive(Debug, Clone, Deserialize, Serialize, Validate, confers::Config)]
#[config(env_prefix = "CRAWLRS__TIMEOUTS__")]
pub struct TimeoutSettings {
    /// Worker相关超时
    pub workers: WorkerTimeoutSettings,

    /// 引擎相关超时
    #[validate(nested)]
    pub engines: EngineTimeoutSettings,

    /// 重试策略超时
    pub retry: RetryTimeoutSettings,

    /// 缓存TTL设置
    pub cache: CacheTimeoutSettings,
}

/// Worker超时设置
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__TIMEOUTS__WORKERS__")]
pub struct WorkerTimeoutSettings {
    /// Webhook worker处理间隔（秒）
    #[config(default = 5)]
    pub webhook_interval_seconds: u64,

    /// Backlog worker处理间隔（秒）
    #[config(default = 30)]
    pub backlog_interval_seconds: u64,
}

/// 引擎超时设置
///
/// # 安全验证（T062 安全审查 MEDIUM-1 修复）
///
/// 所有超时字段均添加 `#[validate(range(min = 1, max = 600))]` 约束：
/// - `min = 1`：防止配置为 0 秒导致 `tokio::time::timeout(Duration::ZERO, ...)`
///   立即超时，触发瀑布式 fallback 直到所有引擎失败，造成 DoS
/// - `max = 600`：防止配置过大值（10 分钟已足够覆盖最慢的浏览器引擎）
#[derive(Debug, Clone, Deserialize, Serialize, Validate, confers::Config)]
#[config(env_prefix = "CRAWLRS__TIMEOUTS__ENGINES__")]
pub struct EngineTimeoutSettings {
    /// 默认请求超时（秒）
    #[config(default = 30)]
    #[validate(range(min = 1, max = 600))]
    pub default_timeout_seconds: u64,

    /// Playwright引擎超时（秒）
    #[config(default = 30)]
    #[validate(range(min = 1, max = 600))]
    pub playwright_timeout_seconds: u64,

    /// FlareSolverr超时（秒）
    #[config(default = 30)]
    #[validate(range(min = 1, max = 600))]
    pub flaresolverr_timeout_seconds: u64,

    /// HTTP fetch 引擎 MRT（Maximum Response Time，秒）—— design.md §14 / T061。
    ///
    /// 用于 `ReqwestEngine` 单引擎最大响应时间。router 顺序 fallback 路径
    /// 用 `min(remaining, fetch_seconds)` 包裹单引擎调用，超时即切下一引擎。
    /// 默认 5 秒（HTTP fetch 引擎比浏览器引擎快）。
    #[config(default = 5)]
    #[serde(default = "default_fetch_seconds")]
    #[validate(range(min = 1, max = 600))]
    pub fetch_seconds: u64,

    /// TLS 指纹引擎 MRT（秒）—— design.md §14 / T061。
    ///
    /// 用于 `FlareSolverrEngine::Tls` 模式单引擎最大响应时间。
    /// 默认 15 秒（TLS 指纹对抗比完整浏览器快）。
    #[config(default = 15)]
    #[serde(default = "default_tls_seconds")]
    #[validate(range(min = 1, max = 600))]
    pub tls_seconds: u64,

    /// CDP/浏览器引擎 MRT（秒）—— design.md §14 / T061。
    ///
    /// 用于 `PlaywrightEngine` 和 `FlareSolverrEngine::{Cdp, Full}` 模式
    /// 单引擎最大响应时间。默认 30 秒（覆盖完整浏览器启动 + JS 渲染）。
    #[config(default = 30)]
    #[serde(default = "default_cdp_seconds")]
    #[validate(range(min = 1, max = 600))]
    pub cdp_seconds: u64,
}

/// serde 默认值：HTTP fetch 引擎 MRT（与 `#[config(default = 5)]` 保持一致）
fn default_fetch_seconds() -> u64 {
    5
}

/// serde 默认值：TLS 指纹引擎 MRT（与 `#[config(default = 15)]` 保持一致）
fn default_tls_seconds() -> u64 {
    15
}

/// serde 默认值：CDP/浏览器引擎 MRT（与 `#[config(default = 30)]` 保持一致）
fn default_cdp_seconds() -> u64 {
    30
}

/// 重试超时设置
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__TIMEOUTS__RETRY__")]
pub struct RetryTimeoutSettings {
    /// 初始退避时间（秒）
    #[config(default = 1)]
    pub initial_backoff_seconds: u64,

    /// 最大退避时间（秒）
    #[config(default = 60)]
    pub max_backoff_seconds: u64,
}

/// 缓存超时设置
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__TIMEOUTS__CACHE__")]
pub struct CacheTimeoutSettings {
    /// 默认TTL（秒）
    #[config(default = 600)]
    pub default_ttl_seconds: u64,

    /// 内存缓存TTL（秒）
    #[config(default = 600)]
    pub memory_ttl_seconds: u64,
}

// =============================================================================
// Cache Configuration
// =============================================================================

/// 缓存类型配置
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__CACHE__TYPES__")]
pub struct CacheTypeSettings {
    #[config(default = 300)]
    pub ttl_seconds: u64,
    #[config(default = 10000)]
    pub max_size: u64,
}

/// 统一缓存配置（oxcache）
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__CACHE__")]
pub struct CacheSettings {
    /// 是否启用缓存
    #[config(default = true)]
    pub enabled: bool,

    /// L1 内存缓存配置
    pub memory: MemoryCacheSettings,

    /// 各缓存类型特定配置
    pub types: CacheTypeSpecificSettings,
}

/// L1 内存缓存配置（Moka）
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__CACHE__MEMORY__")]
pub struct MemoryCacheSettings {
    /// 最大容量
    #[config(default = 10000)]
    pub capacity: u64,
    /// TTL（秒）
    #[config(default = 300)]
    pub ttl_seconds: u64,
}

/// 各缓存类型特定配置
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__CACHE__TYPES__")]
pub struct CacheTypeSpecificSettings {
    /// 搜索结果缓存配置
    pub search: CacheTypeSettings,

    /// DNS 缓存配置
    pub dns: CacheTypeSettings,

    /// 正则缓存配置
    pub regex: CacheTypeSettings,
}

// =============================================================================
// 可信代理配置
// =============================================================================

/// 可信代理配置设置
///
/// 用于安全地提取客户端真实 IP 地址。
/// 仅当请求来自可信代理时才信任 X-Forwarded-For 等请求头。
///
/// # 安全说明
///
/// 如果不配置可信代理，攻击者可以伪造 X-Forwarded-For 头来绕过
/// 基于 IP 的安全控制（如速率限制、访问控制等）。
///
/// # 配置示例
///
/// ```toml
/// [trusted_proxies]
/// enabled = true
/// proxies = ["10.0.0.0/8", "172.16.0.0/12", "192.168.0.0/16", "127.0.0.1"]
/// ```
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__TRUSTED_PROXIES__")]
pub struct TrustedProxySettings {
    /// 是否启用可信代理验证
    ///
    /// - true: 仅当请求来自可信代理时才信任转发头
    /// - false: 总是信任转发头（不安全，仅用于开发环境）
    #[config(default = true)]
    pub enabled: bool,

    /// 可信代理 IP 地址列表
    ///
    /// 支持 CIDR 格式（如 "10.0.0.0/8"）和单个 IP（如 "127.0.0.1"）
    ///
    /// 默认包含常见的私有 IP 地址范围：
    /// - 10.0.0.0/8 (Class A 私有网络)
    /// - 172.16.0.0/12 (Class B 私有网络)
    /// - 192.168.0.0/16 (Class C 私有网络)
    /// - 127.0.0.1 (本地回环)
    /// - ::1 (IPv6 本地回环)
    #[config(default = vec![
        "10.0.0.0/8".to_string(),
        "172.16.0.0/12".to_string(),
        "192.168.0.0/16".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ])]
    pub proxies: Vec<String>,
}

impl TrustedProxySettings {
    /// 检查 IP 地址是否在可信代理列表中
    ///
    /// # 参数
    ///
    /// * `ip` - 要检查的 IP 地址
    ///
    /// # 返回值
    ///
    /// 如果 IP 在可信代理列表中返回 true，否则返回 false
    pub fn is_trusted(&self, ip: &std::net::IpAddr) -> bool {
        use std::net::IpAddr;
        use std::str::FromStr;

        for proxy in &self.proxies {
            // 尝试解析为 CIDR
            if let Ok(network) = ipnetwork::IpNetwork::from_str(proxy) {
                if network.contains(*ip) {
                    return true;
                }
            } else {
                // 尝试解析为单个 IP
                if let Ok(trusted_ip) = proxy.parse::<IpAddr>() {
                    if trusted_ip == *ip {
                        return true;
                    }
                }
            }
        }
        false
    }
}

// =============================================================================
// 自定义验证函数
// =============================================================================

/// 安全验证函数
///
/// 验证配置的安全性要求，包括 webhook secret、数据库密码、S3 凭据等
pub fn validate_security(settings: &Settings) -> Result<(), validator::ValidationError> {
    // 检查 webhook secret 是否为空
    if settings.webhook.secret().is_empty() {
        return Err(validator::ValidationError::new("webhook_secret_empty"));
    }

    // 检查 webhook secret 是否使用默认值
    let weak_secrets = [
        "your-webhook-secret",
        "your-secret-key",
        "secret",
        "webhook-secret",
        "change-me",
        "password",
    ];
    if weak_secrets.contains(&settings.webhook.secret()) {
        return Err(validator::ValidationError::new("webhook_secret_weak"));
    }

    // 检查 webhook secret 长度
    if settings.webhook.secret().len() < 32 {
        return Err(validator::ValidationError::new("webhook_secret_short"));
    }

    // 检查速率限制是否禁用
    if !settings.rate_limiting.enabled {
        return Err(validator::ValidationError::new("rate_limiting_disabled"));
    }

    // 检查数据库密码
    let weak_patterns = ["password=password", "password=postgres", "password=admin"];
    if weak_patterns
        .iter()
        .any(|p| settings.database.url().contains(p))
    {
        return Err(validator::ValidationError::new("database_password_weak"));
    }

    // 生产环境密码长度验证
    let env = std::env::var("APP_ENVIRONMENT")
        .or_else(|_| std::env::var("CRAWLRS_ENV"))
        .unwrap_or_else(|_| "development".to_string());
    let is_production = env.eq_ignore_ascii_case("production") || env.eq_ignore_ascii_case("prod");

    if is_production {
        let password_length = extract_password_length(settings.database.url());
        if password_length > 0 && password_length < 16 {
            return Err(validator::ValidationError::new(
                "database_password_short_production",
            ));
        }
    }

    // JWT secret 校验（auth-on 时强制，MEDIUM-3 修复：早期失败反馈）
    //
    // 仅在 `auth` feature 启用时检查——auth-off 走 default_identity_middleware，
    // 不读取 jwt_secret。空 / 弱密钥（< 32 字节）会被 `build_garrison_config` 二次拒绝，
    // 此处提前检查让运维在启动时即收到反馈而非等到 garrison 初始化。
    #[cfg(feature = "auth")]
    {
        let jwt_secret = settings.auth.jwt_secret();
        if jwt_secret.is_empty() {
            return Err(validator::ValidationError::new("auth_jwt_secret_empty"));
        }
        if jwt_secret.len() < 32 {
            return Err(validator::ValidationError::new("auth_jwt_secret_weak"));
        }
    }

    Ok(())
}

/// 值验证函数
///
/// 验证配置值的有效性
pub fn validate_values(settings: &Settings) -> Result<(), validator::ValidationError> {
    // 验证端口范围
    if settings.server.port == 0 {
        return Err(validator::ValidationError::new("invalid_port"));
    }

    // 验证 A/B 测试权重范围
    if settings.search.variant_b_weight < 0.0 || settings.search.variant_b_weight > 1.0 {
        return Err(validator::ValidationError::new("invalid_variant_b_weight"));
    }

    Ok(())
}

fn extract_password_length(url: &str) -> usize {
    if let Some(at_pos) = url.find('@') {
        if let Some(colon_pos) = url[..at_pos].find(':') {
            return at_pos - colon_pos - 1;
        }
    }
    0
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_support::ENV_MUTEX;

    #[test]
    fn test_settings_structure() {
        let settings = Settings {
            server: ServerSettings::default(),
            database: DatabaseSettings::default(),
            cors: CorsSettings::default(),
            rate_limiting: RateLimitingSettings::default(),
            concurrency: ConcurrencySettings::default(),
            webhook: WebhookSettings::default(),
            bing_search: BingSearchSettings::default(),
            search: SearchSettings::default(),
            llm: LLMSettings::default(),
            proxy: ProxySettings::default(),
            engines: EngineSettings::default(),
            logging: LoggingSettings::default(),
            workers: WorkerSettings::default(),
            timeouts: TimeoutSettings::default(),
            cache: CacheSettings::default(),
            trusted_proxies: TrustedProxySettings::default(),
            auth: AuthSettings::default(),
        };

        assert_eq!(settings.server.port, 8899);
        assert!(settings.rate_limiting.enabled);
        assert!(settings.trusted_proxies.enabled);
    }

    /// R-auth-engine-002：AuthSettings Debug 输出对 jwt_secret 脱敏（CWE-532 防护）。
    ///
    /// 验证 `format!("{:?}", auth_settings)` 不含密钥明文，仅含 `"***REDACTED***"`。
    /// 防止日志打印 Settings 时泄露 JWT 签名密钥。
    #[test]
    fn test_auth_settings_debug_redacts_jwt_secret() {
        let auth = AuthSettings {
            jwt_secret: "super-secret-jwt-key-32-bytes-or-more!!".to_string(),
        };

        let debug_output = format!("{:?}", auth);
        assert!(
            !debug_output.contains("super-secret-jwt-key-32-bytes-or-more!!"),
            "Debug output must not contain plaintext jwt_secret, got: {debug_output}"
        );
        assert!(
            debug_output.contains("***REDACTED***"),
            "Debug output should contain '***REDACTED***' placeholder, got: {debug_output}"
        );
    }

    #[test]
    fn test_trusted_proxy_settings_default() {
        let settings = TrustedProxySettings::default();
        assert!(settings.enabled);
        assert!(!settings.proxies.is_empty());

        // 测试私有 IP 地址
        let ip: std::net::IpAddr = "10.0.0.1".parse().unwrap();
        assert!(settings.is_trusted(&ip));

        let ip: std::net::IpAddr = "172.16.0.1".parse().unwrap();
        assert!(settings.is_trusted(&ip));

        let ip: std::net::IpAddr = "192.168.1.1".parse().unwrap();
        assert!(settings.is_trusted(&ip));

        let ip: std::net::IpAddr = "127.0.0.1".parse().unwrap();
        assert!(settings.is_trusted(&ip));

        // 测试公网 IP
        let ip: std::net::IpAddr = "8.8.8.8".parse().unwrap();
        assert!(!settings.is_trusted(&ip));
    }

    #[test]
    fn test_trusted_proxy_settings_ipv6() {
        let settings = TrustedProxySettings::default();

        // 测试 IPv6 本地回环
        let ip: std::net::IpAddr = "::1".parse().unwrap();
        assert!(settings.is_trusted(&ip));

        // 测试 IPv6 公网地址
        let ip: std::net::IpAddr = "2001:db8::1".parse().unwrap();
        assert!(!settings.is_trusted(&ip));
    }

    #[test]
    fn test_trusted_proxy_settings_custom_cidr() {
        let settings = TrustedProxySettings {
            enabled: true,
            proxies: vec!["203.0.113.0/24".to_string(), "198.51.100.1".to_string()],
        };

        // 测试 CIDR 范围内的 IP
        let ip: std::net::IpAddr = "203.0.113.100".parse().unwrap();
        assert!(settings.is_trusted(&ip));

        // 测试单个 IP
        let ip: std::net::IpAddr = "198.51.100.1".parse().unwrap();
        assert!(settings.is_trusted(&ip));

        // 测试不在范围内的 IP
        let ip: std::net::IpAddr = "203.0.114.1".parse().unwrap();
        assert!(!settings.is_trusted(&ip));
    }

    // ========== WorkerCount tests ==========

    #[test]
    fn test_worker_count_fixed_resolve() {
        let count = WorkerCount::Fixed(8);
        assert_eq!(count.resolve(), 8);
    }

    #[test]
    fn test_worker_count_auto_resolve_positive() {
        let count = WorkerCount::Auto("auto".to_string());
        let resolved = count.resolve();
        assert!(
            resolved > 0,
            "Auto resolve should be positive, got {}",
            resolved
        );
    }

    #[test]
    fn test_worker_count_auto_case_insensitive() {
        let count = WorkerCount::Auto("AUTO".to_string());
        let resolved = count.resolve();
        assert!(resolved > 0, "AUTO should resolve to cpu-based value");
    }

    #[test]
    fn test_worker_count_auto_non_auto_string_falls_to_default() {
        let count = WorkerCount::Auto("not-auto".to_string());
        assert_eq!(count.resolve(), 5);
    }

    #[test]
    fn test_worker_count_default_is_auto() {
        let count = WorkerCount::default();
        assert!(matches!(count, WorkerCount::Auto(_)));
    }

    #[test]
    fn test_worker_count_clone() {
        let count = WorkerCount::Fixed(12);
        let cloned = count.clone();
        assert_eq!(cloned.resolve(), 12);
    }

    #[test]
    fn test_worker_count_serde_fixed() {
        let count = WorkerCount::Fixed(7);
        let json = serde_json::to_string(&count).expect("serialize");
        let back: WorkerCount = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.resolve(), 7);
    }

    #[test]
    fn test_worker_count_serde_auto() {
        let json = "\"auto\"";
        let count: WorkerCount = serde_json::from_str(json).expect("deserialize");
        assert!(matches!(count, WorkerCount::Auto(_)));
    }

    // ========== CorsSettings tests ==========

    #[test]
    fn test_cors_settings_default_allowed_origins() {
        let settings = CorsSettings::default();
        assert_eq!(settings.allowed_origins, "*");
    }

    #[test]
    fn test_cors_settings_construction() {
        let settings = CorsSettings {
            allowed_origins: "https://example.com".to_string(),
        };
        assert_eq!(settings.allowed_origins, "https://example.com");
    }

    #[test]
    fn test_cors_settings_clone() {
        let settings = CorsSettings {
            allowed_origins: "https://test.com,https://api.com".to_string(),
        };
        let cloned = settings.clone();
        assert_eq!(cloned.allowed_origins, settings.allowed_origins);
    }

    #[test]
    fn test_cors_settings_serde_roundtrip() {
        let settings = CorsSettings {
            allowed_origins: "https://roundtrip.com".to_string(),
        };
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: CorsSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.allowed_origins, "https://roundtrip.com");
    }

    // ========== ProxySettings tests ==========

    #[test]
    fn test_proxy_settings_default_empty_urls_and_round_robin_strategy() {
        let settings = ProxySettings::default();
        assert!(settings.urls.is_empty(), "default urls should be empty");
        assert_eq!(settings.strategy, ProxyStrategy::RoundRobin);
        assert!(!settings.enabled);
        // MEDIUM-2 修复：默认 sticky_ttl=60s, cooldown=30s
        assert_eq!(settings.sticky_ttl_seconds, 60);
        assert_eq!(settings.cooldown_seconds, 30);
    }

    #[test]
    fn test_proxy_settings_urls_and_strategy_accessor() {
        let settings = ProxySettings {
            urls: vec!["http://proxy.example.com:8080".to_string()],
            strategy: ProxyStrategy::Sticky,
            enabled: true,
            sticky_ttl_seconds: 120,
            cooldown_seconds: 45,
        };
        assert_eq!(settings.urls(), &["http://proxy.example.com:8080"]);
        assert_eq!(settings.strategy(), ProxyStrategy::Sticky);
        assert!(settings.enabled);
    }

    #[test]
    fn test_proxy_settings_debug_redacts_urls() {
        let settings = ProxySettings {
            urls: vec!["http://secret:password@proxy:8080".to_string()],
            strategy: ProxyStrategy::RoundRobin,
            enabled: true,
            sticky_ttl_seconds: 60,
            cooldown_seconds: 30,
        };
        let debug_str = format!("{:?}", settings);
        assert!(
            debug_str.contains("***REDACTED***"),
            "urls must be redacted in Debug"
        );
        assert!(
            !debug_str.contains("secret:password"),
            "credentials must not leak to Debug"
        );
        assert!(debug_str.contains("true"), "enabled should be visible");
        assert!(
            debug_str.contains("RoundRobin"),
            "strategy should be visible (non-sensitive)"
        );
    }

    #[test]
    fn test_proxy_settings_clone_preserves_fields() {
        let settings = ProxySettings {
            urls: vec!["http://clone-proxy:9090".to_string()],
            strategy: ProxyStrategy::Sticky,
            enabled: true,
            sticky_ttl_seconds: 90,
            cooldown_seconds: 20,
        };
        let cloned = settings.clone();
        assert_eq!(cloned.urls(), &["http://clone-proxy:9090"]);
        assert_eq!(cloned.strategy(), ProxyStrategy::Sticky);
        assert!(cloned.enabled);
        assert_eq!(cloned.sticky_ttl_seconds, 90);
        assert_eq!(cloned.cooldown_seconds, 20);
    }

    #[test]
    fn test_proxy_settings_serde_roundtrip() {
        let settings = ProxySettings {
            urls: vec!["http://serde-proxy:7070".to_string()],
            strategy: ProxyStrategy::Sticky,
            enabled: false,
            sticky_ttl_seconds: 100,
            cooldown_seconds: 50,
        };
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: ProxySettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.urls(), &["http://serde-proxy:7070"]);
        assert_eq!(back.strategy(), ProxyStrategy::Sticky);
        assert!(!back.enabled);
        assert_eq!(back.sticky_ttl_seconds, 100);
        assert_eq!(back.cooldown_seconds, 50);
    }

    /// strategy 字段 serde 用 snake_case：JSON/TOML 中 "round_robin" / "sticky"
    #[test]
    fn test_proxy_settings_strategy_serde_snake_case() {
        // RoundRobin ↔ "round_robin"
        let json = r#"{"urls":[],"strategy":"round_robin","enabled":false}"#;
        let s: ProxySettings = serde_json::from_str(json).expect("deserialize round_robin");
        assert_eq!(s.strategy, ProxyStrategy::RoundRobin);
        let back = serde_json::to_string(&s).expect("serialize");
        assert!(
            back.contains("\"round_robin\""),
            "serialized should be snake_case: {back}"
        );

        // Sticky ↔ "sticky"
        let json = r#"{"urls":[],"strategy":"sticky","enabled":false}"#;
        let s: ProxySettings = serde_json::from_str(json).expect("deserialize sticky");
        assert_eq!(s.strategy, ProxyStrategy::Sticky);
    }

    /// strategy 字段缺失时 serde(default) → RoundRobin
    #[test]
    fn test_proxy_settings_strategy_missing_falls_back_to_default() {
        let json = r#"{"urls":["http://x:8080"],"enabled":true}"#;
        let s: ProxySettings = serde_json::from_str(json).expect("deserialize missing strategy");
        assert_eq!(s.strategy, ProxyStrategy::RoundRobin);
        assert!(s.enabled);
    }

    // ========== WorkerSettings tests ==========

    #[test]
    fn test_worker_settings_default_uses_auto() {
        let settings = WorkerSettings::default();
        assert!(matches!(settings.count, WorkerCount::Auto(_)));
    }

    #[test]
    fn test_worker_settings_construction_fixed() {
        let settings = WorkerSettings {
            count: WorkerCount::Fixed(16),
        };
        assert_eq!(settings.count.resolve(), 16);
    }

    // ========== TimeoutSettings tests ==========

    #[test]
    fn test_timeout_settings_default() {
        let settings = TimeoutSettings::default();
        assert_eq!(settings.workers.webhook_interval_seconds, 5);
        assert_eq!(settings.workers.backlog_interval_seconds, 30);
        assert_eq!(settings.engines.default_timeout_seconds, 30);
        assert_eq!(settings.engines.playwright_timeout_seconds, 30);
        assert_eq!(settings.engines.flaresolverr_timeout_seconds, 30);
        assert_eq!(settings.retry.initial_backoff_seconds, 1);
        assert_eq!(settings.retry.max_backoff_seconds, 60);
        assert_eq!(settings.cache.default_ttl_seconds, 600);
        assert_eq!(settings.cache.memory_ttl_seconds, 600);
    }

    #[test]
    fn test_worker_timeout_settings_construction() {
        let settings = WorkerTimeoutSettings {
            webhook_interval_seconds: 10,
            backlog_interval_seconds: 60,
        };
        assert_eq!(settings.webhook_interval_seconds, 10);
        assert_eq!(settings.backlog_interval_seconds, 60);
    }

    #[test]
    fn test_engine_timeout_settings_construction() {
        let settings = EngineTimeoutSettings {
            default_timeout_seconds: 45,
            playwright_timeout_seconds: 50,
            flaresolverr_timeout_seconds: 55,
            fetch_seconds: 5,
            tls_seconds: 15,
            cdp_seconds: 30,
        };
        assert_eq!(settings.default_timeout_seconds, 45);
        assert_eq!(settings.playwright_timeout_seconds, 50);
        assert_eq!(settings.flaresolverr_timeout_seconds, 55);
        assert_eq!(settings.fetch_seconds, 5);
        assert_eq!(settings.tls_seconds, 15);
        assert_eq!(settings.cdp_seconds, 30);
    }

    /// T061：EngineTimeoutSettings 新增 MRT 字段使用 serde(default) 兼容旧 config 文件。
    ///
    /// 验证：旧 toml/env 中没有 `fetch_seconds` / `tls_seconds` / `cdp_seconds` 字段时，
    /// 反序列化应回退到默认值（fetch=5, tls=15, cdp=30），不报错。
    #[test]
    fn test_engine_timeout_settings_serde_default_for_mrt_fields() {
        // 模拟旧 config 文件：只有原有 3 个字段，无 MRT 字段
        let old_toml = r#"
            default_timeout_seconds = 45
            playwright_timeout_seconds = 50
            flaresolverr_timeout_seconds = 55
        "#;
        let settings: EngineTimeoutSettings = toml::from_str(old_toml).expect("deserialize");
        assert_eq!(settings.default_timeout_seconds, 45);
        assert_eq!(settings.playwright_timeout_seconds, 50);
        assert_eq!(settings.flaresolverr_timeout_seconds, 55);
        // 新字段应回退到 serde 默认值
        assert_eq!(settings.fetch_seconds, 5, "fetch_seconds default");
        assert_eq!(settings.tls_seconds, 15, "tls_seconds default");
        assert_eq!(settings.cdp_seconds, 30, "cdp_seconds default");
    }

    /// T061：EngineTimeoutSettings MRT 字段可被显式覆盖。
    #[test]
    fn test_engine_timeout_settings_mrt_fields_override() {
        let toml = r#"
            default_timeout_seconds = 60
            playwright_timeout_seconds = 45
            flaresolverr_timeout_seconds = 60
            fetch_seconds = 8
            tls_seconds = 20
            cdp_seconds = 45
        "#;
        let settings: EngineTimeoutSettings = toml::from_str(toml).expect("deserialize");
        assert_eq!(settings.fetch_seconds, 8);
        assert_eq!(settings.tls_seconds, 20);
        assert_eq!(settings.cdp_seconds, 45);
    }

    /// T062 安全审查 MEDIUM-1：EngineTimeoutSettings 的 Validate 派生应拒绝越界值。
    ///
    /// 验证：所有超时字段（含 MRT）必须 >=1 且 <=600 秒。
    /// - 0 秒 → `tokio::time::timeout(Duration::ZERO, ...)` 立即超时 → 瀑布式 fallback 全部失败 → DoS
    /// - 超过 600 秒 → 配置错误（10 分钟已足够覆盖最慢浏览器引擎）
    #[test]
    fn test_engine_timeout_settings_validate_rejects_zero() {
        let settings = EngineTimeoutSettings {
            default_timeout_seconds: 0,
            playwright_timeout_seconds: 30,
            flaresolverr_timeout_seconds: 30,
            fetch_seconds: 5,
            tls_seconds: 15,
            cdp_seconds: 30,
        };
        let result = settings.validate();
        assert!(
            result.is_err(),
            "default_timeout_seconds=0 should be rejected (DoS risk)"
        );
        let err = result.unwrap_err().to_string();
        assert!(
            err.contains("default_timeout_seconds"),
            "error should mention field name: {}",
            err
        );
    }

    #[test]
    fn test_engine_timeout_settings_validate_rejects_zero_mrt_fields() {
        // fetch_seconds=0 → tokio::time::timeout(Duration::ZERO) → 立即超时
        let settings = EngineTimeoutSettings {
            default_timeout_seconds: 30,
            playwright_timeout_seconds: 30,
            flaresolverr_timeout_seconds: 30,
            fetch_seconds: 0,
            tls_seconds: 15,
            cdp_seconds: 30,
        };
        assert!(
            settings.validate().is_err(),
            "fetch_seconds=0 should be rejected"
        );

        // tls_seconds=0
        let settings = EngineTimeoutSettings {
            default_timeout_seconds: 30,
            playwright_timeout_seconds: 30,
            flaresolverr_timeout_seconds: 30,
            fetch_seconds: 5,
            tls_seconds: 0,
            cdp_seconds: 30,
        };
        assert!(
            settings.validate().is_err(),
            "tls_seconds=0 should be rejected"
        );

        // cdp_seconds=0
        let settings = EngineTimeoutSettings {
            default_timeout_seconds: 30,
            playwright_timeout_seconds: 30,
            flaresolverr_timeout_seconds: 30,
            fetch_seconds: 5,
            tls_seconds: 15,
            cdp_seconds: 0,
        };
        assert!(
            settings.validate().is_err(),
            "cdp_seconds=0 should be rejected"
        );
    }

    #[test]
    fn test_engine_timeout_settings_validate_rejects_exceeding_max() {
        // 601 秒 → 超过 max=600（10 分钟）
        let settings = EngineTimeoutSettings {
            default_timeout_seconds: 30,
            playwright_timeout_seconds: 30,
            flaresolverr_timeout_seconds: 30,
            fetch_seconds: 601,
            tls_seconds: 15,
            cdp_seconds: 30,
        };
        assert!(
            settings.validate().is_err(),
            "fetch_seconds=601 should be rejected (exceeding max)"
        );
    }

    #[test]
    fn test_engine_timeout_settings_validate_accepts_boundary_values() {
        // 边界值：min=1 和 max=600 都应通过
        let settings = EngineTimeoutSettings {
            default_timeout_seconds: 1,
            playwright_timeout_seconds: 600,
            flaresolverr_timeout_seconds: 1,
            fetch_seconds: 1,
            tls_seconds: 600,
            cdp_seconds: 1,
        };
        assert!(
            settings.validate().is_ok(),
            "boundary values (1 and 600) should pass validation"
        );
    }

    #[test]
    fn test_engine_timeout_settings_validate_accepts_defaults() {
        // 默认值应通过验证
        let settings = EngineTimeoutSettings {
            default_timeout_seconds: 30,
            playwright_timeout_seconds: 30,
            flaresolverr_timeout_seconds: 30,
            fetch_seconds: 5,
            tls_seconds: 15,
            cdp_seconds: 30,
        };
        assert!(
            settings.validate().is_ok(),
            "default values should pass validation"
        );
    }

    #[test]
    fn test_retry_timeout_settings_construction() {
        let settings = RetryTimeoutSettings {
            initial_backoff_seconds: 2,
            max_backoff_seconds: 120,
        };
        assert_eq!(settings.initial_backoff_seconds, 2);
        assert_eq!(settings.max_backoff_seconds, 120);
    }

    #[test]
    fn test_cache_timeout_settings_construction() {
        let settings = CacheTimeoutSettings {
            default_ttl_seconds: 300,
            memory_ttl_seconds: 200,
        };
        assert_eq!(settings.default_ttl_seconds, 300);
        assert_eq!(settings.memory_ttl_seconds, 200);
    }

    // ========== CacheSettings tests ==========

    #[test]
    fn test_cache_settings_default() {
        let settings = CacheSettings::default();
        assert!(settings.enabled);
        assert_eq!(settings.memory.capacity, 10000);
        assert_eq!(settings.memory.ttl_seconds, 300);
    }

    #[test]
    fn test_memory_cache_settings_construction() {
        let settings = MemoryCacheSettings {
            capacity: 5000,
            ttl_seconds: 120,
        };
        assert_eq!(settings.capacity, 5000);
        assert_eq!(settings.ttl_seconds, 120);
    }

    #[test]
    fn test_cache_type_settings_construction() {
        let settings = CacheTypeSettings {
            ttl_seconds: 100,
            max_size: 500,
        };
        assert_eq!(settings.ttl_seconds, 100);
        assert_eq!(settings.max_size, 500);
    }

    #[test]
    fn test_cache_settings_serde_roundtrip() {
        let settings = CacheSettings {
            enabled: false,
            memory: MemoryCacheSettings {
                capacity: 100,
                ttl_seconds: 50,
            },
            types: CacheTypeSpecificSettings {
                search: CacheTypeSettings {
                    ttl_seconds: 10,
                    max_size: 20,
                },
                dns: CacheTypeSettings {
                    ttl_seconds: 30,
                    max_size: 40,
                },
                regex: CacheTypeSettings {
                    ttl_seconds: 60,
                    max_size: 70,
                },
            },
        };
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: CacheSettings = serde_json::from_str(&json).expect("deserialize");
        assert!(!back.enabled);
        assert_eq!(back.memory.capacity, 100);
        assert_eq!(back.types.search.ttl_seconds, 10);
        assert_eq!(back.types.dns.max_size, 40);
        assert_eq!(back.types.regex.ttl_seconds, 60);
    }

    // ========== extract_password_length tests ==========

    #[test]
    fn test_extract_password_length_with_password() {
        // 函数计算 @ 位置与第一个 : 位置（协议冒号）的差值
        let url = "postgresql://user:mypassword@localhost/db";
        let len = extract_password_length(url);
        // at_pos = 28, colon_pos = 10 (protocol colon), return = 28 - 10 - 1 = 17
        assert_eq!(len, 17);
    }

    #[test]
    fn test_extract_password_length_no_password() {
        let len = extract_password_length("postgresql://localhost/db");
        assert_eq!(len, 0);
    }

    #[test]
    fn test_extract_password_length_no_at_sign() {
        let len = extract_password_length("postgresql://user:pass");
        assert_eq!(len, 0);
    }

    #[test]
    fn test_extract_password_length_empty_password() {
        // user: 后立即跟 @，但函数仍从协议冒号开始计算
        let url = "postgresql://user:@localhost/db";
        let len = extract_password_length(url);
        // at_pos = 18, colon_pos = 10, return = 18 - 10 - 1 = 7
        assert_eq!(len, 7);
    }

    // ========== validate_values tests ==========

    fn build_test_settings() -> Settings {
        Settings {
            server: ServerSettings::default(),
            database: DatabaseSettings::default(),
            cors: CorsSettings::default(),
            rate_limiting: RateLimitingSettings::default(),
            concurrency: ConcurrencySettings::default(),
            webhook: WebhookSettings {
                secret: "a-very-strong-and-secure-webhook-secret-key-32+chars".to_string(),
                max_retries: 5,
                batch_size: 1000,
            },
            bing_search: BingSearchSettings::default(),
            search: SearchSettings::default(),
            llm: LLMSettings::default(),
            proxy: ProxySettings::default(),
            engines: EngineSettings::default(),
            logging: LoggingSettings::default(),
            workers: WorkerSettings::default(),
            timeouts: TimeoutSettings::default(),
            cache: CacheSettings::default(),
            trusted_proxies: TrustedProxySettings::default(),
            // MEDIUM-3 修复后 validate_security 在 auth-on 时校验 jwt_secret，
            // 测试用强密钥（32+ 字节）确保 validate_security 通过
            auth: AuthSettings {
                jwt_secret: "a-very-strong-and-secure-jwt-secret-key-32+chars".to_string(),
            },
        }
    }

    #[test]
    fn test_validate_values_valid_settings() {
        let settings = build_test_settings();
        assert!(validate_values(&settings).is_ok());
    }

    #[test]
    fn test_validate_values_invalid_port_zero() {
        let mut settings = build_test_settings();
        settings.server.port = 0;
        let result = validate_values(&settings);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code.to_string(), "invalid_port");
    }

    #[test]
    fn test_validate_values_invalid_variant_b_weight_negative() {
        let mut settings = build_test_settings();
        settings.search.variant_b_weight = -0.1;
        let result = validate_values(&settings);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code.to_string(),
            "invalid_variant_b_weight"
        );
    }

    #[test]
    fn test_validate_values_invalid_variant_b_weight_above_one() {
        let mut settings = build_test_settings();
        settings.search.variant_b_weight = 1.5;
        let result = validate_values(&settings);
        assert!(result.is_err());
    }

    // ========== validate_security tests (serialized via mutex due to env var) ==========

    // Use the shared global ENV_MUTEX to prevent cross-module env var race conditions
    // (config::settings and common::error all manipulate CRAWLRS_ENV)

    #[test]
    fn test_validate_security_valid() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("CRAWLRS_ENV");
        let settings = build_test_settings();
        assert!(validate_security(&settings).is_ok());
    }

    #[test]
    fn test_validate_security_empty_webhook_secret() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("CRAWLRS_ENV");
        let mut settings = build_test_settings();
        settings.webhook = WebhookSettings {
            secret: String::new(),
            max_retries: 5,
            batch_size: 1000,
        };
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code.to_string(), "webhook_secret_empty");
    }

    #[test]
    fn test_validate_security_weak_webhook_secret() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("CRAWLRS_ENV");
        let mut settings = build_test_settings();
        // 使用弱密钥列表中的精确值（validate_security 在长度检查之前检查弱密钥列表）
        settings.webhook = WebhookSettings {
            secret: "your-webhook-secret".to_string(),
            max_retries: 5,
            batch_size: 1000,
        };
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code.to_string(), "webhook_secret_weak");
    }

    #[test]
    fn test_validate_security_short_webhook_secret() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("CRAWLRS_ENV");
        let mut settings = build_test_settings();
        settings.webhook = WebhookSettings {
            secret: "short".to_string(),
            max_retries: 5,
            batch_size: 1000,
        };
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().code.to_string(), "webhook_secret_short");
    }

    #[test]
    fn test_validate_security_rate_limiting_disabled() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("CRAWLRS_ENV");
        let mut settings = build_test_settings();
        settings.rate_limiting.enabled = false;
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code.to_string(),
            "rate_limiting_disabled"
        );
    }

    #[test]
    fn test_validate_security_weak_database_password() {
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("APP_ENVIRONMENT");
        std::env::remove_var("CRAWLRS_ENV");
        let mut settings = build_test_settings();
        // validate_security 检查 URL 中是否包含 "password=password" 等弱密码模式
        settings.database = DatabaseSettings {
            url: "postgresql://user:name@localhost/db?password=password".to_string(),
            max_connections: Some(100),
            min_connections: Some(10),
            connect_timeout: Some(10),
            idle_timeout: Some(300),
            max_lifetime: Some(1800),
            connection_keepalive: Some(30),
            health_check_interval: Some(60),
        };
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code.to_string(),
            "database_password_weak"
        );
    }

    #[test]
    fn test_validate_security_production_short_password_returns_error() {
        // Cover lines 505-510: production environment + short password path.
        // In dev mode, `is_production` is false and the block is skipped; this
        // test forces production mode and supplies a URL whose password length
        // (as computed by extract_password_length) is < 16 to trigger the
        // `database_password_short_production` validation error.
        let _guard = ENV_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
        let saved_app = std::env::var("APP_ENVIRONMENT").ok();
        let saved_crawlrs = std::env::var("CRAWLRS_ENV").ok();
        std::env::set_var("APP_ENVIRONMENT", "production");
        std::env::remove_var("CRAWLRS_ENV");

        let mut settings = build_test_settings();
        // URL password "shortpwd" → extract_password_length returns 14
        // (distance from first ':' to '@'), which is < 16.
        settings.database = DatabaseSettings {
            url: "postgresql://user:shortpwd@localhost/db".to_string(),
            max_connections: Some(100),
            min_connections: Some(10),
            connect_timeout: Some(10),
            idle_timeout: Some(300),
            max_lifetime: Some(1800),
            connection_keepalive: Some(30),
            health_check_interval: Some(60),
        };
        let result = validate_security(&settings);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().code.to_string(),
            "database_password_short_production"
        );

        if let Some(v) = saved_app {
            std::env::set_var("APP_ENVIRONMENT", v);
        } else {
            std::env::remove_var("APP_ENVIRONMENT");
        }
        if let Some(v) = saved_crawlrs {
            std::env::set_var("CRAWLRS_ENV", v);
        } else {
            std::env::remove_var("CRAWLRS_ENV");
        }
    }
}
