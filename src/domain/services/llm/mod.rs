// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! LLMService - LLM provider interaction handling
//!
//! 核心调度层：组合 `prompt_builder`（模板 + prompt 构造）与
//! `provider_adapter`（HTTP 请求发送）完成 LLM 数据提取。

pub mod prompt_builder;
pub mod provider_adapter;
pub mod vision_adapter; // T049：视觉模型适配器（MLLM 引擎依赖）

#[cfg(test)]
use prompt_builder::TemplateLoaderTrait;
use prompt_builder::{
    build_prompt, parse_llm_response, FileTemplateLoader, InMemoryTemplateLoader, TemplateLoader,
};
use provider_adapter::{create_engine_client, resolve_provider};

use crate::config::settings::Settings;
use crate::engines::engine_client::EngineClient;
use anyhow::Result;
use async_trait::async_trait;
#[cfg(feature = "llm")]
use genai::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;

/// Token usage tracking for LLM calls
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[async_trait]
pub trait LLMServiceTrait: Send + Sync {
    async fn extract_data(
        &self,
        text: &str,
        schema: &Value,
        format: &str,
    ) -> Result<(Value, TokenUsage), anyhow::Error>;
}

/// LLM服务 - 处理与LLM提供商的交互
#[derive(Clone)]
pub struct LLMService {
    engine_client: Arc<EngineClient>,
    /// LLM 客户端
    #[cfg(feature = "llm")]
    _client: Client,
    /// 使用的模型
    model: String,
    /// 提供商
    provider: String,
    /// API 基础 URL
    api_base_url: Option<String>,
    /// API 密钥
    api_key: Option<String>,
    /// 提示模板（缓存）
    templates: HashMap<String, String>,
}

impl LLMService {
    /// Create LLMService with settings and custom template loader
    pub fn new_with_template_loader(
        settings: &Settings,
        http_client: Arc<reqwest::Client>,
        template_loader: TemplateLoader,
    ) -> Self {
        let templates = template_loader.load_templates().unwrap_or_default();
        let engine_client = create_engine_client(http_client);
        let provider = resolve_provider(settings);
        let model = settings
            .llm
            .model
            .clone()
            .unwrap_or_else(|| "gpt-3.5-turbo".to_string());
        let api_base_url = settings.llm.api_base_url.clone();
        let api_key = settings.llm.api_key.clone();

        Self {
            engine_client,
            #[cfg(feature = "llm")]
            _client: Client::default(),
            model,
            provider,
            api_base_url,
            api_key,
            templates,
        }
    }

    /// Create LLMService with settings (uses FileTemplateLoader)
    pub fn new(settings: &Settings, http_client: Arc<reqwest::Client>) -> Self {
        Self::new_with_template_loader(
            settings,
            http_client,
            Arc::new(FileTemplateLoader::default()),
        )
    }

    /// Create LLMService with explicit config and custom template loader
    pub fn new_with_config_and_loader(
        _api_key: String,
        model: String,
        api_base_url: String,
        http_client: Arc<reqwest::Client>,
        template_loader: TemplateLoader,
    ) -> Self {
        let templates = template_loader.load_templates().unwrap_or_default();
        let engine_client = create_engine_client(http_client);

        Self {
            engine_client,
            #[cfg(feature = "llm")]
            _client: Client::default(),
            model,
            provider: "openai".to_string(),
            api_base_url: Some(api_base_url),
            api_key: Some(_api_key),
            templates,
        }
    }

    /// Create LLMService with explicit config (uses InMemoryTemplateLoader with defaults)
    pub fn new_with_config(
        _api_key: String,
        model: String,
        api_base_url: String,
        http_client: Arc<reqwest::Client>,
    ) -> Self {
        let template_loader: TemplateLoader =
            Arc::new(InMemoryTemplateLoader::new().with_default_templates());
        let templates = template_loader.load_templates().unwrap_or_default();
        let engine_client = create_engine_client(http_client);

        Self {
            engine_client,
            #[cfg(feature = "llm")]
            _client: Client::default(),
            model,
            provider: "openai".to_string(),
            api_base_url: Some(api_base_url),
            api_key: Some(_api_key),
            templates,
        }
    }
}

#[async_trait]
impl LLMServiceTrait for LLMService {
    async fn extract_data(
        &self,
        text: &str,
        schema: &Value,
        format: &str,
    ) -> Result<(Value, TokenUsage)> {
        self.extract_data_internal(text, schema, format).await
    }
}

impl LLMService {
    pub async fn extract_data_internal(
        &self,
        text: &str,
        schema: &Value,
        format: &str,
    ) -> Result<(Value, TokenUsage)> {
        let template = self
            .templates
            .get(format)
            .ok_or_else(|| anyhow::anyhow!("Template not found for format: {}", format))?;

        // 使用 prompt_builder 构造 prompt
        let prompt = build_prompt(template, text, schema)?;

        // 使用 provider_adapter 发送请求
        let (content, usage) = if let Some(base_url) = &self.api_base_url {
            let url = provider_adapter::build_request_url(base_url);
            let response = provider_adapter::send_direct_request(
                &self.engine_client,
                &url,
                &self.model,
                &prompt,
                self.api_key.as_deref(),
            )
            .await?;
            (response.content, response.usage)
        } else {
            let response =
                provider_adapter::send_genai_request(&self.provider, &self.model, &prompt).await?;
            (response.content, response.usage)
        };

        // 使用 prompt_builder 解析响应
        let data = parse_llm_response(&content, format)?;
        Ok((data, usage))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    // ===== TokenUsage tests =====

    #[test]
    fn test_token_usage_default_is_zero() {
        let usage = TokenUsage::default();
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
    }

    #[test]
    fn test_token_usage_serde_roundtrip() {
        let usage = TokenUsage {
            prompt_tokens: 100,
            completion_tokens: 50,
            total_tokens: 150,
        };
        let serialized = serde_json::to_string(&usage).unwrap();
        let deserialized: TokenUsage = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.prompt_tokens, 100);
        assert_eq!(deserialized.completion_tokens, 50);
        assert_eq!(deserialized.total_tokens, 150);
    }

    #[test]
    fn test_token_usage_clone_preserves_values() {
        let usage = TokenUsage {
            prompt_tokens: 42,
            completion_tokens: 21,
            total_tokens: 63,
        };
        let cloned = usage.clone();
        assert_eq!(cloned.prompt_tokens, 42);
        assert_eq!(cloned.completion_tokens, 21);
        assert_eq!(cloned.total_tokens, 63);
    }

    // ===== FileTemplateLoader tests =====

    #[test]
    fn test_file_template_loader_new_valid_file_loads_templates() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("crawlrs_test_llm_loader");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("test_prompts.toml");
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(
            f,
            r#"
[extraction]
json = 'Extract JSON from {{{{text}}}} using schema {{{{schema}}}}'
markdown = 'Extract Markdown from {{{{text}}}}'
"#
        )
        .unwrap();

        let loader = FileTemplateLoader::new(file_path.to_str().unwrap());
        let templates = loader.load_templates().unwrap();
        assert!(templates.contains_key("json"));
        assert!(templates.contains_key("markdown"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_file_template_loader_read_templates_valid_toml() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("crawlrs_test_llm_loader2");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("test_prompts2.toml");
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(
            f,
            r#"
[extraction]
json = 'json template'
"#
        )
        .unwrap();

        let loader = FileTemplateLoader::new(file_path.to_str().unwrap());
        let templates = loader.load_templates().unwrap();
        assert_eq!(templates.get("json").unwrap(), "json template");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_file_template_loader_read_templates_invalid_toml_returns_empty() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("crawlrs_test_llm_loader3");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("invalid.toml");
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(f, "this is not valid toml {{{{").unwrap();

        let loader = FileTemplateLoader::new(file_path.to_str().unwrap());
        let templates = loader.load_templates().unwrap();
        assert!(templates.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_file_template_loader_read_templates_partial_toml_only_json() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("crawlrs_test_llm_loader4");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("partial.toml");
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(
            f,
            r#"
[extraction]
json = 'json only'
"#
        )
        .unwrap();

        let loader = FileTemplateLoader::new(file_path.to_str().unwrap());
        let templates = loader.load_templates().unwrap();
        assert_eq!(templates.len(), 1);
        assert!(templates.contains_key("json"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_file_template_loader_read_templates_no_extraction_key() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("crawlrs_test_llm_loader5");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("no_extraction.toml");
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(f, "[other]\nkey = \"value\"").unwrap();

        let loader = FileTemplateLoader::new(file_path.to_str().unwrap());
        let templates = loader.load_templates().unwrap();
        assert!(templates.is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_file_template_loader_load_templates_returns_clone() {
        use std::io::Write;
        let dir = std::env::temp_dir().join("crawlrs_test_llm_loader6");
        let _ = std::fs::create_dir_all(&dir);
        let file_path = dir.join("clone_test.toml");
        let mut f = std::fs::File::create(&file_path).unwrap();
        writeln!(f, "[extraction]\njson = 'test'").unwrap();

        let loader = FileTemplateLoader::new(file_path.to_str().unwrap());
        let mut t1 = loader.load_templates().unwrap();
        t1.insert("new".to_string(), "val".to_string());
        let t2 = loader.load_templates().unwrap();
        assert!(!t2.contains_key("new"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    // ===== LLMService constructor tests =====

    #[test]
    fn test_llm_service_new_with_config_creates_instance() {
        let settings = make_test_settings(llm_settings_default());
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new_with_config(
            "test-key".to_string(),
            "gpt-4".to_string(),
            "https://api.openai.com/v1".to_string(),
            http_client,
        );
        assert_eq!(service.model, "gpt-4");
        assert_eq!(service.provider, "openai");
    }

    #[test]
    fn test_llm_service_new_with_config_and_loader_creates_instance() {
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new_with_config_and_loader(
            "test-key".to_string(),
            "gpt-4".to_string(),
            "https://api.openai.com/v1".to_string(),
            http_client,
            loader,
        );
        assert_eq!(service.model, "gpt-4");
    }

    #[test]
    fn test_llm_service_new_with_config_has_default_templates() {
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new_with_config(
            "test-key".to_string(),
            "gpt-4".to_string(),
            "https://api.openai.com/v1".to_string(),
            http_client,
        );
        assert!(service.templates.contains_key("json"));
        assert!(service.templates.contains_key("markdown"));
    }

    #[test]
    fn test_llm_service_clone_preserves_fields() {
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new_with_config(
            "key".to_string(),
            "model-x".to_string(),
            "https://api.example.com".to_string(),
            http_client,
        );
        let cloned = service.clone();
        assert_eq!(cloned.model, "model-x");
        assert_eq!(
            cloned.api_base_url,
            Some("https://api.example.com".to_string())
        );
    }

    // ===== Helper functions =====

    fn llm_settings_default() -> crate::config::settings::LLMSettings {
        crate::config::settings::LLMSettings {
            provider: Some("openai".to_string()),
            api_key: Some("test-key".to_string()),
            model: Some("gpt-3.5-turbo".to_string()),
            api_base_url: None,
        }
    }

    fn make_test_settings(llm: crate::config::settings::LLMSettings) -> Settings {
        Settings {
            llm,
            ..Settings::default()
        }
    }

    #[test]
    fn test_new_with_template_loader_default_settings() {
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        let http_client = Arc::new(reqwest::Client::new());
        let settings = make_test_settings(llm_settings_default());
        let service = LLMService::new_with_template_loader(&settings, http_client, loader);
        assert_eq!(service.provider, "openai");
        assert_eq!(service.model, "gpt-3.5-turbo");
    }

    #[test]
    fn test_new_with_template_loader_with_provider_and_model() {
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        let http_client = Arc::new(reqwest::Client::new());
        let mut llm = llm_settings_default();
        llm.provider = Some("anthropic".to_string());
        llm.model = Some("claude-2".to_string());
        let settings = make_test_settings(llm);
        let service = LLMService::new_with_template_loader(&settings, http_client, loader);
        assert_eq!(service.provider, "anthropic");
        assert_eq!(service.model, "claude-2");
    }

    #[test]
    fn test_new_with_template_loader_ollama_port_forces_openai() {
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        let http_client = Arc::new(reqwest::Client::new());
        let mut llm = llm_settings_default();
        llm.provider = Some("ollama".to_string());
        llm.api_base_url = Some("http://localhost:11434".to_string());
        let settings = make_test_settings(llm);
        let service = LLMService::new_with_template_loader(&settings, http_client, loader);
        // OllamaStrategy should resolve to "openai" for localhost:11434
        assert_eq!(service.provider, "openai");
    }

    #[test]
    fn test_new_with_template_loader_172_24_160_1_forces_openai() {
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        let http_client = Arc::new(reqwest::Client::new());
        let mut llm = llm_settings_default();
        llm.provider = Some("ollama".to_string());
        llm.api_base_url = Some("http://172.24.160.1:8080".to_string());
        let settings = make_test_settings(llm);
        let service = LLMService::new_with_template_loader(&settings, http_client, loader);
        assert_eq!(service.provider, "openai");
    }

    #[test]
    fn test_new_with_template_loader_no_api_base_url_skips_ollama_check() {
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        let http_client = Arc::new(reqwest::Client::new());
        let mut llm = llm_settings_default();
        llm.provider = Some("ollama".to_string());
        llm.api_base_url = None;
        let settings = make_test_settings(llm);
        let service = LLMService::new_with_template_loader(&settings, http_client, loader);
        // Without api_base_url, OllamaStrategy doesn't transform
        assert_eq!(service.provider, "ollama");
    }

    #[test]
    fn test_new_with_template_loader_failing_loader_uses_empty_templates() {
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        let http_client = Arc::new(reqwest::Client::new());
        let settings = make_test_settings(llm_settings_default());
        let service = LLMService::new_with_template_loader(&settings, http_client, loader);
        // Loader returns empty templates, service should still construct
        assert!(service.templates.is_empty());
    }

    #[test]
    fn test_new_with_file_template_loader_default() {
        let http_client = Arc::new(reqwest::Client::new());
        let settings = make_test_settings(llm_settings_default());
        let service = LLMService::new(&settings, http_client);
        // FileTemplateLoader::default() tries to load from config/prompts.toml
        // which may or may not exist; service should still construct
        assert_eq!(service.model, "gpt-3.5-turbo");
    }
}
