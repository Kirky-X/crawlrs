// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! LLM 配置
//!
//! 包含大语言模型服务配置

use confers::Config;
use serde::{Deserialize, Serialize};

/// LLM 配置设置
///
/// 配置 LLM（大语言模型）服务的参数
///
/// # 字段说明
///
/// * `api_key` - LLM API 密钥（敏感信息，仅 crate 可见）
/// * `model` - 使用的模型名称，默认 "gpt-3.5-turbo"
/// * `api_base_url` - LLM API 基础 URL，默认 "https://api.openai.com/v1"
///
/// # 安全提示
///
/// `api_key` 字段包含 LLM API 密钥，泄露可能导致未经授权的访问。
/// 该字段仅对 crate 可见，外部模块应使用 `api_key()` 方法访问。
#[derive(Debug, Clone, Deserialize, Serialize, Config)]
#[config(env_prefix = "CRAWLRS__LLM__")]
pub struct LLMSettings {
    /// LLM 提供商 (openai, ollama, anthropic, etc)
    pub provider: Option<String>,

    /// LLM API 密钥 (敏感信息)
    pub(crate) api_key: Option<String>,

    /// 使用的模型名称
    #[config(default = Some("gpt-3.5-turbo".to_string()))]
    pub model: Option<String>,

    /// LLM API 基础 URL
    #[config(default = Some("https://api.openai.com/v1".to_string()))]
    pub api_base_url: Option<String>,
}

impl LLMSettings {
    /// 获取 LLM API 密钥
    ///
    /// # 安全提示
    ///
    /// 此方法返回 LLM API 密钥，调用者应谨慎处理，
    /// 不要记录到日志或暴露给用户。
    pub fn api_key(&self) -> Option<&str> {
        self.api_key.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_llm_default_api_key_is_none() {
        let settings = LLMSettings::default();
        assert!(
            settings.api_key().is_none(),
            "default api_key should be None"
        );
    }

    #[test]
    fn test_llm_default_provider_is_none() {
        let settings = LLMSettings::default();
        assert!(
            settings.provider.is_none(),
            "default provider should be None"
        );
    }

    #[test]
    fn test_llm_default_model() {
        let settings = LLMSettings::default();
        assert_eq!(
            settings.model.as_deref(),
            Some("gpt-3.5-turbo"),
            "default model should be gpt-3.5-turbo"
        );
    }

    #[test]
    fn test_llm_default_api_base_url() {
        let settings = LLMSettings::default();
        assert_eq!(
            settings.api_base_url.as_deref(),
            Some("https://api.openai.com/v1"),
            "default api_base_url should be OpenAI endpoint"
        );
    }

    #[test]
    fn test_llm_api_key_returns_some_value() {
        let settings = LLMSettings {
            provider: Some("openai".to_string()),
            api_key: Some("sk-secret-key".to_string()),
            model: Some("gpt-4".to_string()),
            api_base_url: Some("https://api.openai.com/v1".to_string()),
        };
        assert_eq!(settings.api_key(), Some("sk-secret-key"));
    }

    #[test]
    fn test_llm_api_key_returns_none_when_not_set() {
        let settings = LLMSettings {
            provider: None,
            api_key: None,
            model: None,
            api_base_url: None,
        };
        assert!(settings.api_key().is_none());
    }

    #[test]
    fn test_llm_serde_roundtrip_with_key() {
        let settings = LLMSettings {
            provider: Some("anthropic".to_string()),
            api_key: Some("key-abc".to_string()),
            model: Some("claude-3".to_string()),
            api_base_url: Some("https://api.anthropic.com".to_string()),
        };
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: LLMSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.provider.as_deref(), Some("anthropic"));
        assert_eq!(back.api_key(), Some("key-abc"));
        assert_eq!(back.model.as_deref(), Some("claude-3"));
        assert_eq!(
            back.api_base_url.as_deref(),
            Some("https://api.anthropic.com")
        );
    }

    #[test]
    fn test_llm_serde_roundtrip_default() {
        let settings = LLMSettings::default();
        let json = serde_json::to_string(&settings).expect("serialize");
        let back: LLMSettings = serde_json::from_str(&json).expect("deserialize");
        assert!(back.api_key().is_none());
        assert!(back.provider.is_none());
        assert_eq!(back.model.as_deref(), settings.model.as_deref());
        assert_eq!(
            back.api_base_url.as_deref(),
            settings.api_base_url.as_deref()
        );
    }

    #[test]
    fn test_llm_clone_preserves_api_key() {
        let settings = LLMSettings {
            provider: Some("ollama".to_string()),
            api_key: Some("cloned-key".to_string()),
            model: Some("llama2".to_string()),
            api_base_url: Some("http://localhost:11434".to_string()),
        };
        let cloned = settings.clone();
        assert_eq!(cloned.provider, settings.provider);
        assert_eq!(cloned.api_key(), Some("cloned-key"));
        assert_eq!(cloned.model, settings.model);
        assert_eq!(cloned.api_base_url, settings.api_base_url);
    }

    #[test]
    fn test_llm_debug_does_not_panic() {
        let settings = LLMSettings {
            provider: Some("openai".to_string()),
            api_key: Some("debug-key".to_string()),
            model: Some("gpt-4".to_string()),
            api_base_url: Some("https://api.openai.com/v1".to_string()),
        };
        let debug = format!("{:?}", settings);
        assert!(
            debug.contains("LLMSettings"),
            "Debug should contain struct name"
        );
    }
}
