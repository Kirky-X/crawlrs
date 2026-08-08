// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

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

    /// Fallback 搜索引擎配置
    pub fallback: SearchFallbackConfig,
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
    }

    #[test]
    fn test_search_clone_preserves_all_fields() {
        let settings = SearchSettings {
            timeout_seconds: 45,
            rate_limiting_enabled: false,
            test_data_enabled: true,
            max_retries: 5,
            retry_delay_ms: 500,
            fallback: SearchFallbackConfig::default(),
        };
        let cloned = settings.clone();
        assert_eq!(cloned.timeout_seconds, settings.timeout_seconds);
        assert_eq!(cloned.rate_limiting_enabled, settings.rate_limiting_enabled);
        assert_eq!(cloned.test_data_enabled, settings.test_data_enabled);
        assert_eq!(cloned.max_retries, settings.max_retries);
        assert_eq!(cloned.retry_delay_ms, settings.retry_delay_ms);
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
}
