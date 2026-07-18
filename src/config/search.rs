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
    /// 是否启用 A/B 测试
    #[config(default = false)]
    pub ab_test_enabled: bool,

    /// Variant B 的流量权重 (0.0 到 1.0)
    #[config(default = 0.1)]
    pub variant_b_weight: f64,

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
    fn test_search_default_ab_test_disabled() {
        let settings = SearchSettings::default();
        assert!(
            !settings.ab_test_enabled,
            "default ab_test_enabled should be false"
        );
    }

    #[test]
    fn test_search_default_variant_b_weight() {
        let settings = SearchSettings::default();
        assert!(
            (settings.variant_b_weight - 0.1).abs() < f64::EPSILON,
            "default variant_b_weight should be 0.1"
        );
    }

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
            back.ab_test_enabled, settings.ab_test_enabled,
            "serde roundtrip should preserve ab_test_enabled"
        );
        assert!(
            (back.variant_b_weight - settings.variant_b_weight).abs() < f64::EPSILON,
            "serde roundtrip should preserve variant_b_weight"
        );
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
            ab_test_enabled: true,
            variant_b_weight: 0.5,
            timeout_seconds: 60,
            rate_limiting_enabled: false,
            test_data_enabled: true,
            max_retries: 10,
            retry_delay_ms: 2000,
        };
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: SearchSettings = serde_json::from_str(&json).expect("deserialize");
        assert!(
            back.ab_test_enabled,
            "ab_test_enabled should survive roundtrip"
        );
        assert!(
            (back.variant_b_weight - 0.5).abs() < f64::EPSILON,
            "variant_b_weight should survive roundtrip"
        );
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
            ab_test_enabled: true,
            variant_b_weight: 0.3,
            timeout_seconds: 45,
            rate_limiting_enabled: false,
            test_data_enabled: true,
            max_retries: 5,
            retry_delay_ms: 500,
        };
        let cloned = settings.clone();
        assert_eq!(cloned.ab_test_enabled, settings.ab_test_enabled);
        assert!((cloned.variant_b_weight - settings.variant_b_weight).abs() < f64::EPSILON);
        assert_eq!(cloned.timeout_seconds, settings.timeout_seconds);
        assert_eq!(cloned.rate_limiting_enabled, settings.rate_limiting_enabled);
        assert_eq!(cloned.test_data_enabled, settings.test_data_enabled);
        assert_eq!(cloned.max_retries, settings.max_retries);
        assert_eq!(cloned.retry_delay_ms, settings.retry_delay_ms);
    }
}
