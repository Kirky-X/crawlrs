// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 搜索相关配置
//!
//! 包含 Bing Search 和搜索功能配置

use serde::{Deserialize, Serialize};

/// Bing Search API 配置设置
///
/// # 安全提示
///
/// `api_key` 字段包含 Bing Search API 密钥，泄露可能导致未经授权的访问。
/// 该字段仅对 crate 可见，外部模块应使用 `api_key()` 方法访问。
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__BING_SEARCH__")]
pub struct BingSearchSettings {
    /// Bing Search API 密钥 (敏感信息)
    /// 注意：此字段包含敏感信息，仅 crate 内部可访问
    pub(crate) api_key: Option<String>,
}

impl BingSearchSettings {
    /// 获取 Bing Search API 密钥
    ///
    /// # 安全提示
    ///
    /// 此方法返回 Bing Search API 密钥，调用者应谨慎处理，
    /// 不要记录到日志或暴露给用户。
    pub fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }
}

/// 搜索配置设置
///
/// 配置搜索相关功能参数
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__SEARCH__")]
pub struct SearchSettings {
    /// 搜索超时时间（秒）
    #[config(default = 30)]
    pub timeout_seconds: u64,

    /// 是否启用速率限制
    #[config(default = true)]
    pub rate_limiting_enabled: bool,

    /// 是否启用测试数据
    #[config(default = false)]
    pub test_data_enabled: bool,

    /// 最大重试次数
    #[config(default = 3)]
    pub max_retries: u32,

    /// 重试延迟（毫秒）
    #[config(default = 1000)]
    pub retry_delay_ms: u64,

    /// 单次搜索最大返回结果数（API 请求的 limit 不得超过此值）
    #[config(default = 50)]
    pub max_results: u32,

    /// 默认搜索引擎（单引擎兜底路径：`SearchCommand` 未显式指定 engine 时使用）。
    ///
    /// 合法值：bing / baidu / sogou / exa / tavily（须已注册；API 引擎需配置凭据）。
    /// 缺省或非法值回退 `bing` 并输出 warn 日志。
    /// `serde(default)` 兼容旧 config/default.toml 缺键（同 robots.fail_closed 先例）。
    #[serde(default)]
    pub default_engine: Option<String>,

    /// 内置引擎注册开关（控制 smart 聚合与单引擎路径的注册集）。
    ///
    /// `serde(default)` 兼容旧 config/default.toml 缺键（同 robots.fail_closed 先例）。
    #[serde(default)]
    pub engines: SearchEnginesSettings,

    /// Exa 引擎凭据配置（SearchClient 单引擎路径装配）
    ///
    /// `serde(default)` 兼容旧 config/default.toml 缺键（同 robots.fail_closed 先例）。
    #[serde(default)]
    pub exa: ExaEngineSettings,

    /// Tavily 引擎凭据配置（SearchClient 单引擎路径装配）
    ///
    /// `serde(default)` 兼容旧 config/default.toml 缺键（同 robots.fail_closed 先例）。
    #[serde(default)]
    pub tavily: TavilyEngineSettings,

    /// SearXNG 元搜索配置（实例生命周期 + 引擎注册）
    ///
    /// `serde(default)` 兼容旧 config/default.toml 缺段：缺段等价于 `enabled = false`
    /// （不构造实例服务、不注册引擎、不拉起周期 worker）。
    #[serde(default)]
    pub searxng: SearxngSearchSettings,

    /// Fallback 搜索引擎配置
    pub fallback: SearchFallbackConfig,
}

/// 内置搜索引擎注册开关（此前为死配置，现已由 `SearchClient::with_configured_engines` 消费）。
///
/// 关闭的引擎不注册：smart 聚合（多引擎 fan-out + RRF）与显式单引擎路径都不可用该引擎。
/// 全部缺省 `true`（与历史行为一致——引擎无条件注册）。
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__SEARCH__ENGINES__")]
pub struct SearchEnginesSettings {
    /// 注册 Bing 内置引擎
    #[config(default = true)]
    pub bing_enabled: bool,

    /// 注册 Baidu 内置引擎
    #[config(default = true)]
    pub baidu_enabled: bool,

    /// 注册 Sogou 内置引擎
    #[config(default = true)]
    pub sogou_enabled: bool,
}

/// Exa 引擎凭据配置
///
/// SearchClient 单引擎路径（engine="exa" 显式调用）的装配凭据。
/// `api_key` 未配置时 SearchClient 跳过注册 Exa 引擎（fallback 路径不受影响）。
///
/// # 安全提示
///
/// `api_key` 字段包含 Exa API 密钥，泄露可能导致未经授权的访问。
/// 该字段仅对 crate 可见，外部模块应使用 `api_key()` 方法访问。
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__SEARCH__EXA__")]
pub struct ExaEngineSettings {
    /// Exa API Key (敏感信息)
    /// 注意：此字段包含敏感信息，仅 crate 内部可访问
    pub(crate) api_key: Option<String>,

    /// Exa MCP 端点 URL 覆盖（留空使用引擎内置默认端点）
    pub(crate) base_url: Option<String>,
}

impl ExaEngineSettings {
    /// 获取 Exa API 密钥
    ///
    /// # 安全提示
    ///
    /// 此方法返回 Exa API 密钥，调用者应谨慎处理，
    /// 不要记录到日志或暴露给用户。
    pub fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }

    /// 获取 Exa MCP 端点 URL 覆盖
    pub fn base_url(&self) -> Option<&str> {
        self.base_url.as_deref()
    }
}

/// Tavily 引擎凭据配置
///
/// SearchClient 单引擎路径（engine="tavily" 显式调用）的装配凭据。
/// `api_key` 未配置时 SearchClient 跳过注册 Tavily 引擎（fallback 路径不受影响）。
///
/// # 安全提示
///
/// `api_key` 字段包含 Tavily API 密钥，泄露可能导致未经授权的访问。
/// 该字段仅对 crate 可见，外部模块应使用 `api_key()` 方法访问。
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__SEARCH__TAVILY__")]
pub struct TavilyEngineSettings {
    /// Tavily API Key (敏感信息)
    /// 注意：此字段包含敏感信息，仅 crate 内部可访问
    pub(crate) api_key: Option<String>,

    /// Tavily API 基础 URL 覆盖（留空使用引擎内置默认端点）
    pub(crate) base_url: Option<String>,
}

impl TavilyEngineSettings {
    /// 获取 Tavily API 密钥
    ///
    /// # 安全提示
    ///
    /// 此方法返回 Tavily API 密钥，调用者应谨慎处理，
    /// 不要记录到日志或暴露给用户。
    pub fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }

    /// 获取 Tavily API 基础 URL 覆盖
    pub fn base_url(&self) -> Option<&str> {
        self.base_url.as_deref()
    }
}

/// SearXNG 元搜索配置
///
/// 公共 SearXNG 实例生态的完整生命周期配置：采集（searx.space 数据集）、
/// 探活（JSON API 端到端探测）、热池（进程内可用实例缓存）、熔断与故障切换。
///
/// 默认 `enabled = false`（opt-in）：启用后会拉起两个周期 worker 并对外探测第三方实例。
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__SEARCH__SEARXNG__")]
// 容器级 serde(default)：缺字段回落到 `Default`（confers 按 `#[config(default)]` 生成），
// 保证旧配置里半截 `[search.searxng]` 段也能反序列化
#[serde(default)]
pub struct SearxngSearchSettings {
    /// 是否启用 SearXNG 元搜索（默认关闭；关闭时不构造实例服务、不注册引擎）
    #[config(default = false)]
    pub enabled: bool,

    /// 热池目标成员数（控制回路尽力目标，非对外部第三方的硬保证）
    #[config(default = 3)]
    pub pool_target: usize,

    /// 单次搜索内最多切换的实例数（故障切换上限）
    #[config(default = 2)]
    pub max_failover_attempts: u32,

    /// 手工引导实例（数据源不可达时的兜底；采集器永不因数据集缺席退役它们）
    #[config(default = Vec::<String>::new())]
    pub seed_urls: Vec<String>,

    /// 探测与搜索请求统一 User-Agent
    #[config(default = "crawlrs-searxng/0.1 (+https://github.com/Kirky-X/crawlrs)".to_string())]
    pub user_agent: String,

    /// 采集器配置
    #[serde(default)]
    pub collector: SearxngCollectorSettings,

    /// 健康探活配置
    #[serde(default)]
    pub health: SearxngHealthSettings,

    /// 熔断器配置
    #[serde(default)]
    pub breaker: SearxngBreakerSettings,
}

/// SearXNG 实例采集器配置
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__SEARCH__SEARXNG__COLLECTOR__")]
#[serde(default)]
pub struct SearxngCollectorSettings {
    /// 采集周期（秒），默认 6 小时
    #[config(default = 21600)]
    pub interval_seconds: u64,

    /// searx.space 数据集 URL（可指向镜像/缓存副本）
    #[config(default = "https://searx.space/data/instances.json".to_string())]
    pub data_url: String,

    /// 数据集拉取超时（秒）
    #[config(default = 30)]
    pub request_timeout_seconds: u64,

    /// 准入的最低日在线率（百分比）
    #[config(default = 95.0)]
    pub min_uptime_day: f64,

    /// 连续缺席数据集多少轮后退役（仅 `searx_space` 来源）
    #[config(default = 4)]
    pub retire_miss_cycles: i32,
}

/// SearXNG 实例健康探活配置
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__SEARCH__SEARXNG__HEALTH__")]
#[serde(default)]
pub struct SearxngHealthSettings {
    /// 探活周期（秒）
    #[config(default = 300)]
    pub interval_seconds: u64,

    /// 单实例探测超时（秒）
    #[config(default = 8)]
    pub probe_timeout_seconds: u64,

    /// 探测查询词（走真实 JSON API 路径）
    #[config(default = "crawlrs".to_string())]
    pub probe_query: String,

    /// 每轮探测的候选实例上限（礼貌性约束）
    #[config(default = 10)]
    pub candidate_batch: u64,

    /// 连续探活失败多少轮后退役（cooldown → retired）
    #[config(default = 5)]
    pub failure_retire_threshold: i32,
}

/// SearXNG 专属熔断器配置
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__SEARCH__SEARXNG__BREAKER__")]
#[serde(default)]
pub struct SearxngBreakerSettings {
    /// 连续失败阈值（跳闸）
    #[config(default = 3)]
    pub failure_threshold: u32,

    /// 熔断打开后的冷却时长（秒）
    #[config(default = 120)]
    pub recovery_timeout_seconds: u64,
}

/// Fallback 搜索引擎配置
///
/// 控制主搜索引擎全部失败时的 fallback 行为。
/// 三个 API 引擎（Exa/Parallel/Tavily）按 `engines` 数组声明顺序依次尝试。
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__SEARCH__FALLBACK__")]
pub struct SearchFallbackConfig {
    /// 是否启用 fallback 搜索引擎
    #[config(default = false)]
    pub enabled: bool,

    /// Fallback 引擎名称列表（按声明顺序尝试）
    /// 有效值: "exa", "parallel", "tavily"
    #[config(default = vec!["exa".to_string(), "parallel".to_string(), "tavily".to_string()])]
    pub engines: Vec<String>,

    /// Exa 引擎配置
    pub exa: FallbackEngineConfig,

    /// Parallel 引擎配置
    pub parallel: FallbackEngineConfig,

    /// Tavily 引擎配置
    pub tavily: FallbackEngineConfig,
}

/// 单个 fallback 引擎的配置
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__SEARCH__FALLBACK__")]
pub struct FallbackEngineConfig {
    /// API Key（可选，部分引擎支持匿名访问）
    #[config(default = String::new())]
    pub api_key: String,

    /// API 端点 URL
    #[config(default = String::new())]
    pub endpoint: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== BingSearchSettings tests ==========

    #[test]
    fn test_bing_default_api_key_is_none() {
        let settings = BingSearchSettings::default();
        assert!(
            settings.api_key().is_none(),
            "default api_key should be None"
        );
    }

    #[test]
    fn test_bing_api_key_returns_some_value() {
        let settings = BingSearchSettings {
            api_key: Some("secret-bing-key".to_string()),
        };
        assert_eq!(
            settings.api_key(),
            Some("secret-bing-key"),
            "api_key() should return the stored key"
        );
    }

    #[test]
    fn test_bing_api_key_returns_none_when_empty() {
        let settings = BingSearchSettings { api_key: None };
        assert!(
            settings.api_key().is_none(),
            "api_key() should return None when not set"
        );
    }

    #[test]
    fn test_bing_serde_roundtrip_with_key() {
        let settings = BingSearchSettings {
            api_key: Some("key-123".to_string()),
        };
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: BingSearchSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(
            back.api_key(),
            Some("key-123"),
            "serde roundtrip should preserve api_key"
        );
    }

    #[test]
    fn test_bing_serde_roundtrip_without_key() {
        let settings = BingSearchSettings::default();
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: BingSearchSettings = serde_json::from_str(&json).expect("deserialize");
        assert!(
            back.api_key().is_none(),
            "serde roundtrip should preserve None api_key"
        );
    }

    #[test]
    fn test_bing_clone_preserves_api_key() {
        let settings = BingSearchSettings {
            api_key: Some("cloned-key".to_string()),
        };
        let cloned = settings.clone();
        assert_eq!(
            cloned.api_key(),
            Some("cloned-key"),
            "clone should preserve api_key"
        );
    }

    #[test]
    fn test_bing_debug_does_not_panic() {
        let settings = BingSearchSettings {
            api_key: Some("debug-key".to_string()),
        };
        let debug = format!("{:?}", settings);
        assert!(
            debug.contains("BingSearchSettings"),
            "Debug output should contain struct name"
        );
    }

    // ========== SearchSettings tests ==========

    #[test]
    fn test_search_default_timeout_seconds() {
        let settings = SearchSettings::default();
        assert_eq!(
            settings.timeout_seconds, 30,
            "default timeout_seconds should be 30"
        );
    }

    #[test]
    fn test_search_default_rate_limiting_enabled() {
        let settings = SearchSettings::default();
        assert!(
            settings.rate_limiting_enabled,
            "default rate_limiting_enabled should be true"
        );
    }

    #[test]
    fn test_search_default_test_data_disabled() {
        let settings = SearchSettings::default();
        assert!(
            !settings.test_data_enabled,
            "default test_data_enabled should be false"
        );
    }

    #[test]
    fn test_search_default_max_retries() {
        let settings = SearchSettings::default();
        assert_eq!(settings.max_retries, 3, "default max_retries should be 3");
    }

    #[test]
    fn test_search_default_retry_delay_ms() {
        let settings = SearchSettings::default();
        assert_eq!(
            settings.retry_delay_ms, 1000,
            "default retry_delay_ms should be 1000"
        );
    }

    #[test]
    fn test_search_default_max_results() {
        let settings = SearchSettings::default();
        assert_eq!(settings.max_results, 50, "default max_results should be 50");
    }

    #[test]
    fn test_search_serde_roundtrip_default() {
        let settings = SearchSettings::default();
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: SearchSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(
            back.timeout_seconds, settings.timeout_seconds,
            "serde roundtrip should preserve timeout_seconds"
        );
        assert_eq!(
            back.max_retries, settings.max_retries,
            "serde roundtrip should preserve max_retries"
        );
    }

    #[test]
    fn test_search_serde_roundtrip_custom_values() {
        let settings = SearchSettings {
            timeout_seconds: 60,
            rate_limiting_enabled: false,
            test_data_enabled: true,
            max_retries: 10,
            retry_delay_ms: 2000,
            max_results: 100,
            default_engine: None,
            engines: SearchEnginesSettings::default(),
            exa: ExaEngineSettings::default(),
            tavily: TavilyEngineSettings::default(),
            searxng: SearxngSearchSettings::default(),
            fallback: SearchFallbackConfig::default(),
        };
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: SearchSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.timeout_seconds, 60);
        assert!(
            !back.rate_limiting_enabled,
            "rate_limiting_enabled should survive roundtrip"
        );
        assert!(back.test_data_enabled);
        assert_eq!(back.max_retries, 10);
        assert_eq!(back.retry_delay_ms, 2000);
        assert_eq!(back.max_results, 100);
    }

    #[test]
    fn test_search_clone_preserves_all_fields() {
        let settings = SearchSettings {
            timeout_seconds: 45,
            rate_limiting_enabled: false,
            test_data_enabled: true,
            max_retries: 5,
            retry_delay_ms: 500,
            max_results: 75,
            default_engine: None,
            engines: SearchEnginesSettings::default(),
            exa: ExaEngineSettings::default(),
            tavily: TavilyEngineSettings::default(),
            searxng: SearxngSearchSettings::default(),
            fallback: SearchFallbackConfig::default(),
        };
        let cloned = settings.clone();
        assert_eq!(cloned.timeout_seconds, settings.timeout_seconds);
        assert_eq!(cloned.rate_limiting_enabled, settings.rate_limiting_enabled);
        assert_eq!(cloned.test_data_enabled, settings.test_data_enabled);
        assert_eq!(cloned.max_retries, settings.max_retries);
        assert_eq!(cloned.retry_delay_ms, settings.retry_delay_ms);
        assert_eq!(cloned.max_results, settings.max_results);
    }

    // ========== SearchFallbackConfig tests ==========

    #[test]
    fn test_fallback_default_disabled() {
        let settings = SearchFallbackConfig::default();
        assert!(!settings.enabled, "fallback should be disabled by default");
    }

    #[test]
    fn test_fallback_default_engines_order() {
        let settings = SearchFallbackConfig::default();
        assert_eq!(settings.engines, vec!["exa", "parallel", "tavily"]);
    }

    #[test]
    fn test_fallback_engine_config_default_empty() {
        let config = FallbackEngineConfig::default();
        assert_eq!(config.api_key, "");
        assert_eq!(config.endpoint, "");
    }

    #[test]
    fn test_fallback_serde_roundtrip() {
        let settings = SearchFallbackConfig {
            enabled: true,
            engines: vec!["tavily".to_string(), "exa".to_string()],
            exa: FallbackEngineConfig {
                api_key: "exa-key".to_string(),
                endpoint: "https://mcp.exa.ai/mcp".to_string(),
            },
            parallel: FallbackEngineConfig {
                api_key: String::new(),
                endpoint: "https://search.parallel.ai/mcp".to_string(),
            },
            tavily: FallbackEngineConfig {
                api_key: "tavily-key".to_string(),
                endpoint: "https://api.tavily.com".to_string(),
            },
        };
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: SearchFallbackConfig = serde_json::from_str(&json).expect("deserialize");
        assert!(back.enabled);
        assert_eq!(back.engines, vec!["tavily", "exa"]);
        assert_eq!(back.exa.api_key, "exa-key");
        assert_eq!(back.tavily.endpoint, "https://api.tavily.com");
    }

    #[test]
    fn test_fallback_clone_preserves_fields() {
        let settings = SearchFallbackConfig {
            enabled: true,
            engines: vec!["exa".to_string()],
            exa: FallbackEngineConfig {
                api_key: "key".to_string(),
                endpoint: "https://example.com".to_string(),
            },
            parallel: FallbackEngineConfig::default(),
            tavily: FallbackEngineConfig::default(),
        };
        let cloned = settings.clone();
        assert_eq!(cloned.enabled, settings.enabled);
        assert_eq!(cloned.engines, settings.engines);
        assert_eq!(cloned.exa.api_key, settings.exa.api_key);
    }

    #[test]
    fn test_search_settings_has_fallback_field() {
        let settings = SearchSettings::default();
        assert!(!settings.fallback.enabled);
        assert_eq!(settings.fallback.engines.len(), 3);
    }

    // ========== ExaEngineSettings tests ==========

    #[test]
    fn test_exa_engine_settings_default_is_none() {
        let settings = ExaEngineSettings::default();
        assert!(
            settings.api_key().is_none(),
            "default exa api_key should be None"
        );
        assert!(
            settings.base_url().is_none(),
            "default exa base_url should be None"
        );
    }

    #[test]
    fn test_exa_engine_settings_accessors_return_values() {
        let settings = ExaEngineSettings {
            api_key: Some("secret-exa-key".to_string()),
            base_url: Some("https://mcp.example.com/mcp".to_string()),
        };
        assert_eq!(settings.api_key(), Some("secret-exa-key"));
        assert_eq!(settings.base_url(), Some("https://mcp.example.com/mcp"));
    }

    #[test]
    fn test_exa_engine_settings_serde_roundtrip() {
        let settings = ExaEngineSettings {
            api_key: Some("key-456".to_string()),
            base_url: None,
        };
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: ExaEngineSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.api_key(), Some("key-456"));
        assert!(back.base_url().is_none());
    }

    #[test]
    fn test_exa_engine_settings_missing_env_key_deserializes_to_none() {
        // 模拟旧配置/环境未注入 api_key 的场景：空对象应反序列化为 None
        let back: ExaEngineSettings = serde_json::from_str("{}").expect("deserialize");
        assert!(back.api_key().is_none());
        assert!(back.base_url().is_none());
    }

    // ========== TavilyEngineSettings tests ==========

    #[test]
    fn test_tavily_engine_settings_default_is_none() {
        let settings = TavilyEngineSettings::default();
        assert!(
            settings.api_key().is_none(),
            "default tavily api_key should be None"
        );
        assert!(
            settings.base_url().is_none(),
            "default tavily base_url should be None"
        );
    }

    #[test]
    fn test_tavily_engine_settings_accessors_return_values() {
        let settings = TavilyEngineSettings {
            api_key: Some("secret-tavily-key".to_string()),
            base_url: Some("https://api.example.com".to_string()),
        };
        assert_eq!(settings.api_key(), Some("secret-tavily-key"));
        assert_eq!(settings.base_url(), Some("https://api.example.com"));
    }

    #[test]
    fn test_tavily_engine_settings_serde_roundtrip() {
        let settings = TavilyEngineSettings {
            api_key: Some("key-789".to_string()),
            base_url: Some("https://api.tavily.com".to_string()),
        };
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: TavilyEngineSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.api_key(), Some("key-789"));
        assert_eq!(back.base_url(), Some("https://api.tavily.com"));
    }

    // ========== SearchSettings exa/tavily integration tests ==========

    #[test]
    fn test_search_settings_default_engine_credentials_are_none() {
        let settings = SearchSettings::default();
        assert!(
            settings.exa.api_key().is_none(),
            "default SearchSettings should not configure exa api_key"
        );
        assert!(
            settings.tavily.api_key().is_none(),
            "default SearchSettings should not configure tavily api_key"
        );
    }

    #[test]
    fn test_search_settings_serde_roundtrip_preserves_none_credentials() {
        let settings = SearchSettings::default();
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: SearchSettings = serde_json::from_str(&json).expect("deserialize");
        assert!(back.exa.api_key().is_none());
        assert!(back.tavily.api_key().is_none());
    }

    // ========== SearxngSearchSettings tests ==========

    #[test]
    fn test_searxng_settings_defaults() {
        let settings = SearxngSearchSettings::default();
        assert!(!settings.enabled, "searxng must be opt-in (enabled=false)");
        assert_eq!(settings.pool_target, 3);
        assert_eq!(settings.max_failover_attempts, 2);
        assert!(settings.seed_urls.is_empty());
        assert!(!settings.user_agent.trim().is_empty());
    }

    #[test]
    fn test_searxng_sub_settings_defaults() {
        let settings = SearxngSearchSettings::default();
        assert_eq!(settings.collector.interval_seconds, 21600);
        assert_eq!(
            settings.collector.data_url,
            "https://searx.space/data/instances.json"
        );
        assert_eq!(settings.collector.request_timeout_seconds, 30);
        assert_eq!(settings.collector.min_uptime_day, 95.0);
        assert_eq!(settings.collector.retire_miss_cycles, 4);

        assert_eq!(settings.health.interval_seconds, 300);
        assert_eq!(settings.health.probe_timeout_seconds, 8);
        assert_eq!(settings.health.probe_query, "crawlrs");
        assert_eq!(settings.health.candidate_batch, 10);
        assert_eq!(settings.health.failure_retire_threshold, 5);

        assert_eq!(settings.breaker.failure_threshold, 3);
        assert_eq!(settings.breaker.recovery_timeout_seconds, 120);
    }

    #[test]
    fn test_searxng_settings_serde_roundtrip() {
        let settings = SearxngSearchSettings {
            enabled: true,
            pool_target: 5,
            max_failover_attempts: 4,
            seed_urls: vec!["https://searx.example.org/".to_string()],
            collector: SearxngCollectorSettings {
                min_uptime_day: 90.0,
                ..SearxngCollectorSettings::default()
            },
            health: SearxngHealthSettings {
                failure_retire_threshold: 7,
                ..SearxngHealthSettings::default()
            },
            breaker: SearxngBreakerSettings {
                failure_threshold: 2,
                ..SearxngBreakerSettings::default()
            },
            ..SearxngSearchSettings::default()
        };

        let json = serde_json::to_string(&settings).expect("serialize");
        let back: SearxngSearchSettings = serde_json::from_str(&json).expect("deserialize");
        assert!(back.enabled);
        assert_eq!(back.pool_target, 5);
        assert_eq!(back.max_failover_attempts, 4);
        assert_eq!(back.seed_urls, vec!["https://searx.example.org/"]);
        assert_eq!(back.collector.min_uptime_day, 90.0);
        assert_eq!(back.health.failure_retire_threshold, 7);
        assert_eq!(back.breaker.failure_threshold, 2);
    }

    #[test]
    fn test_searxng_settings_missing_section_equals_default() {
        // 旧配置文件缺 `[search.searxng]` 段→ 反序列化为默认值（enabled=false）
        let back: SearxngSearchSettings = serde_json::from_str("{}").expect("deserialize");
        let default = SearxngSearchSettings::default();
        assert_eq!(back.enabled, default.enabled);
        assert_eq!(back.pool_target, default.pool_target);
        assert_eq!(back.max_failover_attempts, default.max_failover_attempts);
        assert_eq!(back.collector.data_url, default.collector.data_url);
        assert_eq!(
            back.health.interval_seconds,
            default.health.interval_seconds
        );
        assert_eq!(
            back.breaker.failure_threshold,
            default.breaker.failure_threshold
        );
    }

    #[test]
    fn test_search_settings_default_searxng_disabled() {
        let settings = SearchSettings::default();
        assert!(!settings.searxng.enabled);
        assert_eq!(settings.searxng.pool_target, 3);
    }
}
