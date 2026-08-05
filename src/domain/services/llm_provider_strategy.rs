// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! LLM Provider 策略模式
//!
//! 替代 `llm_service.rs` 中 `url.contains(":11434")`/`url.contains("172.24.160.1")`
//! 硬编码判定，将 provider 解析逻辑封装为可独立测试的策略对象。
//!
//! # 背景
//!
//! crawlrs 通过 OpenAI 兼容 API 调用本地 Ollama（端口 11434 或 WSL2 网关 172.24.160.1）。
//! genai 库会对 localhost 强制注入 Ollama 适配器，导致请求格式不兼容。
//! 解决方案：检测到 Ollama URL 时，将 provider 标记为 "openai"，让 genai 走 OpenAI 路径。

/// Provider 解析策略 trait
///
/// 根据可选的 `api_base_url` 决定使用哪个 LLM provider 标识符。
/// 实现方封装具体的 URL 模式匹配规则。
pub trait ProviderStrategy: Send + Sync {
    /// 解析 provider 标识符
    ///
    /// # 参数
    ///
    /// * `api_base_url` - 配置的 LLM API 基础 URL，可能为 None
    ///
    /// # 返回值
    ///
    /// 返回 provider 字符串（如 "openai"、"ollama"）
    fn resolve_provider(&self, api_base_url: Option<&str>) -> String;
}

/// Ollama URL 检测策略
///
/// 检测 URL 是否指向本地 Ollama 实例（端口 11434 或 WSL2 网关 172.24.160.1），
/// 命中时返回 "openai"（Ollama 提供 OpenAI 兼容 API），否则返回 fallback provider。
pub struct OllamaStrategy {
    /// URL 未命中 Ollama 模式时使用的回退 provider
    fallback_provider: String,
}

impl OllamaStrategy {
    /// 创建策略实例
    ///
    /// # 参数
    ///
    /// * `fallback_provider` - URL 未命中时的回退 provider（通常为显式配置或默认 "openai"）
    pub fn new(fallback_provider: String) -> Self {
        Self { fallback_provider }
    }

    /// 判断 URL 是否指向 Ollama 实例
    ///
    /// 匹配规则（与原硬编码一致，保持行为兼容）：
    /// - 包含 `:11434`（Ollama 默认端口）
    /// - 包含 `172.24.160.1`（WSL2 网关回环地址）
    fn is_ollama_url(url: &str) -> bool {
        url.contains(":11434") || url.contains("172.24.160.1")
    }
}

impl ProviderStrategy for OllamaStrategy {
    fn resolve_provider(&self, api_base_url: Option<&str>) -> String {
        match api_base_url {
            Some(url) if Self::is_ollama_url(url) => "openai".to_string(),
            _ => self.fallback_provider.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== OllamaStrategy::is_ollama_url ==========

    #[test]
    fn test_ollama_url_with_port_11434() {
        assert!(OllamaStrategy::is_ollama_url("http://localhost:11434"));
        assert!(OllamaStrategy::is_ollama_url("http://192.168.1.5:11434"));
        assert!(OllamaStrategy::is_ollama_url("http://127.0.0.1:11434/v1"));
    }

    #[test]
    fn test_ollama_url_with_wsl2_gateway() {
        assert!(OllamaStrategy::is_ollama_url("http://172.24.160.1:8080"));
        assert!(OllamaStrategy::is_ollama_url("http://172.24.160.1:11434"));
    }

    #[test]
    fn test_ollama_url_with_standard_openai_url() {
        assert!(!OllamaStrategy::is_ollama_url("https://api.openai.com/v1"));
        assert!(!OllamaStrategy::is_ollama_url("https://api.openai.com"));
    }

    #[test]
    fn test_ollama_url_with_empty_or_non_matching() {
        assert!(!OllamaStrategy::is_ollama_url(""));
        assert!(!OllamaStrategy::is_ollama_url("https://example.com:8080"));
    }

    // ========== OllamaStrategy::resolve_provider ==========

    #[test]
    fn test_ollama_strategy_returns_openai_for_11434_url() {
        let strategy = OllamaStrategy::new("ollama".to_string());
        assert_eq!(
            strategy.resolve_provider(Some("http://192.168.1.5:11434")),
            "openai"
        );
    }

    #[test]
    fn test_ollama_strategy_returns_openai_for_172_24_160_1_url() {
        let strategy = OllamaStrategy::new("ollama".to_string());
        assert_eq!(
            strategy.resolve_provider(Some("http://172.24.160.1:8080")),
            "openai"
        );
    }

    #[test]
    fn test_ollama_strategy_returns_fallback_for_standard_openai_url() {
        let strategy = OllamaStrategy::new("openai".to_string());
        assert_eq!(
            strategy.resolve_provider(Some("https://api.openai.com/v1")),
            "openai"
        );
    }

    #[test]
    fn test_ollama_strategy_returns_fallback_when_url_is_none() {
        let strategy = OllamaStrategy::new("ollama".to_string());
        assert_eq!(strategy.resolve_provider(None), "ollama");
    }

    #[test]
    fn test_ollama_strategy_returns_default_fallback_for_non_matching_url() {
        let strategy = OllamaStrategy::new("anthropic".to_string());
        assert_eq!(
            strategy.resolve_provider(Some("https://api.anthropic.com")),
            "anthropic"
        );
    }

    // ========== 集成场景：模拟 llm_service.rs 的解析流程 ==========

    #[test]
    fn test_integration_explicit_ollama_with_11434_url_forces_openai() {
        // 模拟 settings.llm.provider = Some("ollama"), api_base_url = Some(":11434")
        let explicit_provider = "ollama".to_string();
        let strategy = OllamaStrategy::new(explicit_provider);
        assert_eq!(
            strategy.resolve_provider(Some("http://192.168.1.5:11434")),
            "openai"
        );
    }

    #[test]
    fn test_integration_explicit_ollama_with_no_url_respects_explicit() {
        // 模拟 settings.llm.provider = Some("ollama"), api_base_url = None
        let explicit_provider = "ollama".to_string();
        let strategy = OllamaStrategy::new(explicit_provider);
        assert_eq!(strategy.resolve_provider(None), "ollama");
    }

    #[test]
    fn test_integration_default_provider_with_11434_url() {
        // 模拟 settings.llm.provider = None (→ "openai"), api_base_url = Some(":11434")
        let default_provider = "openai".to_string();
        let strategy = OllamaStrategy::new(default_provider);
        assert_eq!(
            strategy.resolve_provider(Some("http://localhost:11434")),
            "openai"
        );
    }
}
