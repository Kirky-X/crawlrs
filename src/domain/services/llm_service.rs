// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! LLMService - LLM provider interaction handling

use crate::config::settings::Settings;
use crate::engines::client::reqwest::ReqwestEngine;
use crate::engines::engine_client::{EngineClient, HttpMethod, ScrapeOptions, ScrapeRequest};
use crate::engines::router::{EngineRouter, EngineRouterTrait};
use anyhow::{Context, Result};
use async_trait::async_trait;
#[cfg(feature = "experimental")]
use genai::chat::{ChatMessage, ChatRequest};
#[cfg(feature = "experimental")]
use genai::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

/// Token usage tracking for LLM calls
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

/// Trait for template loading - enables DI and testing
pub trait TemplateLoaderTrait: Send + Sync {
    /// Load all templates
    fn load_templates(&self) -> Result<HashMap<String, String>>;
}

/// Template loader wrapped in Arc for Clone support
pub type TemplateLoader = Arc<dyn TemplateLoaderTrait>;

/// File-based template loader (production implementation)
pub struct FileTemplateLoader {
    templates: HashMap<String, String>,
}

impl FileTemplateLoader {
    pub fn new(file_path: impl Into<String>) -> Self {
        let path = file_path.into();
        let templates = Self::read_templates(&path).unwrap_or_else(|e| {
            log::error!("Failed to load templates from {}: {}", path, e);
            HashMap::new()
        });

        Self { templates }
    }

    /// Create with default path
    pub fn default_path() -> Self {
        Self::new("config/prompts.toml")
    }

    fn read_templates(file_path: &str) -> Result<HashMap<String, String>> {
        let content = std::fs::read_to_string(file_path)?;
        let v: Value = toml::from_str(&content)?;

        let mut templates = HashMap::new();
        if let Some(extraction) = v.get("extraction") {
            if let Some(json_tpl) = extraction.get("json").and_then(|t| t.as_str()) {
                templates.insert("json".to_string(), json_tpl.to_string());
            }
            if let Some(md_tpl) = extraction.get("markdown").and_then(|t| t.as_str()) {
                templates.insert("markdown".to_string(), md_tpl.to_string());
            }
        }
        Ok(templates)
    }
}

impl Default for FileTemplateLoader {
    fn default() -> Self {
        Self::default_path()
    }
}

impl TemplateLoaderTrait for FileTemplateLoader {
    fn load_templates(&self) -> Result<HashMap<String, String>> {
        Ok(self.templates.clone())
    }
}

/// In-memory template loader (for testing)
#[derive(Debug, Default, Clone)]
pub struct InMemoryTemplateLoader {
    templates: HashMap<String, String>,
}

impl InMemoryTemplateLoader {
    pub fn new() -> Self {
        Self {
            templates: HashMap::new(),
        }
    }

    pub fn with_template(mut self, name: impl Into<String>, content: impl Into<String>) -> Self {
        self.templates.insert(name.into(), content.into());
        self
    }

    /// Add default extraction templates
    pub fn with_default_templates(mut self) -> Self {
        self.templates.insert(
            "json".to_string(),
            "Extract JSON from {{text}} using schema {{schema}}".to_string(),
        );
        self.templates.insert(
            "markdown".to_string(),
            "Extract Markdown from {{text}}".to_string(),
        );
        self
    }
}

impl TemplateLoaderTrait for InMemoryTemplateLoader {
    fn load_templates(&self) -> Result<HashMap<String, String>> {
        Ok(self.templates.clone())
    }
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
#[allow(dead_code)]
pub struct LLMService {
    engine_client: Arc<EngineClient>,
    /// LLM 客户端
    #[cfg(feature = "experimental")]
    client: Client,
    /// 使用的模型
    model: String,
    /// 提供商
    provider: String,
    /// API 基础 URL
    api_base_url: Option<String>,
    /// API 密钥
    api_key: Option<String>,
    /// 提示模板加载器
    #[allow(dead_code)]
    template_loader: TemplateLoader,
    /// 提示模板（缓存）
    templates: HashMap<String, String>,
}

impl LLMService {
    fn create_engine_client(http_client: Arc<reqwest::Client>) -> Arc<EngineClient> {
        let reqwest_engine = ReqwestEngine::new(http_client);
        let router: Arc<dyn EngineRouterTrait> =
            Arc::new(EngineRouter::new(vec![Arc::new(reqwest_engine)]));
        Arc::new(EngineClient::with_router(router))
    }

    /// Create LLMService with settings and custom template loader
    pub fn new_with_template_loader(
        settings: &Settings,
        http_client: Arc<reqwest::Client>,
        template_loader: TemplateLoader,
    ) -> Self {
        let templates = template_loader.load_templates().unwrap_or_default();
        let engine_client = Self::create_engine_client(http_client);
        let mut provider = settings
            .llm
            .provider
            .clone()
            .unwrap_or_else(|| "openai".to_string());
        let model = settings
            .llm
            .model
            .clone()
            .unwrap_or_else(|| "gpt-3.5-turbo".to_string());
        let api_base_url = settings.llm.api_base_url.clone();
        let api_key = settings.llm.api_key.clone();

        // 核心研究成果应用：如果检测到是本地 Ollama 地址，我们"假装"它是 OpenAI
        // 这样可以避开 genai 内置适配器对 localhost 的强行转换
        if let Some(url) = &api_base_url {
            if url.contains("172.24.160.1") || url.contains(":11434") {
                provider = "openai".to_string();
            }
        }

        Self {
            engine_client,
            #[cfg(feature = "experimental")]
            client: Client::default(),
            model,
            provider,
            api_base_url,
            api_key,
            template_loader,
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
        let engine_client = Self::create_engine_client(http_client);

        Self {
            engine_client,
            #[cfg(feature = "experimental")]
            client: Client::default(),
            model,
            provider: "openai".to_string(),
            api_base_url: Some(api_base_url),
            api_key: Some(_api_key),
            template_loader,
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
        let engine_client = Self::create_engine_client(http_client);

        Self {
            engine_client,
            #[cfg(feature = "experimental")]
            client: Client::default(),
            model,
            provider: "openai".to_string(),
            api_base_url: Some(api_base_url),
            api_key: Some(_api_key),
            template_loader,
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

        let prompt = template
            .replace("{{text}}", text)
            .replace("{{schema}}", &serde_json::to_string_pretty(schema)?);

        let (content, usage) = if let Some(base_url) = &self.api_base_url {
            // 如果提供了显式地址，直接使用 reqwest 发送 OpenAI 兼容请求，避开 genai 的环境变干扰
            let url = if base_url.ends_with("/v1") || base_url.ends_with("/v1/") {
                format!("{}/chat/completions", base_url.trim_end_matches('/'))
            } else if base_url.contains(":11434") {
                // 如果是 Ollama 端口但没带 /v1，自动补全
                format!("{}/v1/chat/completions", base_url.trim_end_matches('/'))
            } else {
                format!("{}/chat/completions", base_url.trim_end_matches('/'))
            };

            let body = json!({
                "model": self.model,
                "messages": [{"role": "user", "content": prompt}],
                "temperature": 0.0
            });

            let mut headers = HashMap::new();
            headers.insert("Content-Type".to_string(), "application/json".to_string());
            if let Some(key) = &self.api_key {
                headers.insert("Authorization".to_string(), format!("Bearer {}", key));
            } else {
                headers.insert("Authorization".to_string(), "Bearer ollama".to_string());
            }

            let request = ScrapeRequest::new(&url).with_options(
                ScrapeOptions::builder()
                    .method(HttpMethod::Post)
                    .headers(headers)
                    .body(body.to_string())
                    .build(),
            );

            let res = self
                .engine_client
                .scrape(&request)
                .await
                .map_err(|e| anyhow::anyhow!("Direct LLM call failed: {}", e))?;
            if !res.is_success() {
                return Err(anyhow::anyhow!("LLM returned error: {}", res.content));
            }

            let res_json: Value =
                serde_json::from_str(&res.content).context("Failed to parse LLM JSON response")?;

            let content = res_json["choices"][0]["message"]["content"]
                .as_str()
                .ok_or_else(|| anyhow::anyhow!("Empty content from LLM"))?
                .to_string();

            let usage = TokenUsage {
                prompt_tokens: res_json["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
                completion_tokens: res_json["usage"]["completion_tokens"].as_u64().unwrap_or(0)
                    as u32,
                total_tokens: res_json["usage"]["total_tokens"].as_u64().unwrap_or(0) as u32,
            };

            (content, usage)
        } else {
            #[cfg(feature = "experimental")]
            {
                // 否则使用 genai 默认逻辑
                let chat_req = ChatRequest::new(vec![ChatMessage::user(prompt)]);

                let model_id = format!("{}:{}", self.provider, self.model);

                let chat_res = match self.client.exec_chat(&model_id, chat_req, None).await {
                    Ok(res) => res,
                    Err(e) => {
                        return Err(anyhow::anyhow!(
                            "LLM call failed for model {}: {:?}",
                            model_id,
                            e
                        ));
                    }
                };

                let content = chat_res
                    .first_text()
                    .ok_or_else(|| anyhow::anyhow!("LLM returned empty content"))?
                    .to_string();

                // genai 0.5.0 的 TokenUsage 获取方式
                let usage = TokenUsage {
                    prompt_tokens: 0, // genai 0.5.0 暂未直接暴露此结构，保持兼容
                    completion_tokens: 0,
                    total_tokens: 0,
                };

                (content, usage)
            }
            #[cfg(not(feature = "experimental"))]
            {
                return Err(anyhow::anyhow!(
                    "LLM provider requires 'experimental' feature to be enabled. \
                     Please rebuild with --features experimental"
                ));
            }
        };

        if format == "json" {
            let clean_content = content
                .trim()
                .trim_start_matches("```json")
                .trim_start_matches("```")
                .trim_end_matches("```")
                .trim();
            let data = serde_json::from_str::<Value>(clean_content).context(format!(
                "Failed to parse LLM JSON response: {}",
                clean_content
            ))?;
            Ok((data, usage))
        } else {
            Ok((json!({ "content": content }), usage))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::Arc;

    // ========== TokenUsage ==========

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
        let s = serde_json::to_string(&usage).expect("serialize");
        let back: TokenUsage = serde_json::from_str(&s).expect("deserialize");
        assert_eq!(back.prompt_tokens, 100);
        assert_eq!(back.completion_tokens, 50);
        assert_eq!(back.total_tokens, 150);
    }

    #[test]
    fn test_token_usage_clone_preserves_values() {
        let usage = TokenUsage {
            prompt_tokens: 7,
            completion_tokens: 3,
            total_tokens: 10,
        };
        let cloned = usage.clone();
        assert_eq!(cloned.prompt_tokens, usage.prompt_tokens);
        assert_eq!(cloned.completion_tokens, usage.completion_tokens);
        assert_eq!(cloned.total_tokens, usage.total_tokens);
    }

    // ========== InMemoryTemplateLoader ==========

    #[test]
    fn test_in_memory_template_loader_new_empty() {
        let loader = InMemoryTemplateLoader::new();
        let templates = loader.load_templates().expect("load");
        assert!(templates.is_empty());
    }

    #[test]
    fn test_in_memory_template_loader_with_template_adds_entry() {
        let loader =
            InMemoryTemplateLoader::new().with_template("custom", "template content {{text}}");
        let templates = loader.load_templates().expect("load");
        assert_eq!(
            templates.get("custom"),
            Some(&"template content {{text}}".to_string())
        );
    }

    #[test]
    fn test_in_memory_template_loader_with_default_templates_adds_two() {
        let loader = InMemoryTemplateLoader::new().with_default_templates();
        let templates = loader.load_templates().expect("load");
        assert_eq!(templates.len(), 2);
        assert!(templates.contains_key("json"));
        assert!(templates.contains_key("markdown"));
        assert!(templates["json"].contains("{{text}}"));
        assert!(templates["markdown"].contains("{{text}}"));
    }

    #[test]
    fn test_in_memory_template_loader_load_templates_returns_clone() {
        let loader = InMemoryTemplateLoader::new().with_template("k", "v");
        let t1 = loader.load_templates().expect("load");
        let t2 = loader.load_templates().expect("load");
        assert_eq!(t1, t2);
    }

    #[test]
    fn test_in_memory_template_loader_default_empty() {
        let loader = InMemoryTemplateLoader::default();
        let templates = loader.load_templates().expect("load");
        assert!(templates.is_empty());
    }

    #[test]
    fn test_in_memory_template_loader_builder_chain() {
        let loader = InMemoryTemplateLoader::new()
            .with_template("a", "1")
            .with_template("b", "2")
            .with_default_templates();
        let templates = loader.load_templates().expect("load");
        assert_eq!(templates.len(), 4);
        assert_eq!(templates.get("a"), Some(&"1".to_string()));
        assert_eq!(templates.get("b"), Some(&"2".to_string()));
        assert!(templates.contains_key("json"));
        assert!(templates.contains_key("markdown"));
    }

    // ========== FileTemplateLoader ==========

    #[test]
    fn test_file_template_loader_new_valid_file_loads_templates() {
        // Write a valid TOML file to a temp path
        let toml_content = r#"
[extraction]
json = "Extract JSON: {{text}} schema={{schema}}"
markdown = "Extract MD: {{text}}"
"#;
        let mut tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        std::io::Write::write_all(&mut tmp, toml_content.as_bytes()).expect("write");
        let path = tmp.path().to_str().expect("path str").to_string();

        let loader = FileTemplateLoader::new(&path);
        let templates = loader.load_templates().expect("load");
        assert_eq!(templates.len(), 2);
        assert!(templates.contains_key("json"));
        assert!(templates.contains_key("markdown"));
        assert!(templates["json"].contains("{{text}}"));
    }

    #[test]
    fn test_file_template_loader_new_invalid_path_returns_empty() {
        // Non-existent path: new() swallows the error and returns empty HashMap
        let loader = FileTemplateLoader::new("/nonexistent/path/that/does/not/exist.toml");
        let templates = loader.load_templates().expect("load");
        assert!(templates.is_empty());
    }

    #[test]
    fn test_file_template_loader_read_templates_valid_toml() {
        let toml_content = r#"
[extraction]
json = "J"
markdown = "M"
"#;
        let mut tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        std::io::Write::write_all(&mut tmp, toml_content.as_bytes()).expect("write");
        let path = tmp.path().to_str().expect("path str");

        let templates = FileTemplateLoader::read_templates(path).expect("read");
        assert_eq!(templates.len(), 2);
        assert_eq!(templates.get("json"), Some(&"J".to_string()));
        assert_eq!(templates.get("markdown"), Some(&"M".to_string()));
    }

    #[test]
    fn test_file_template_loader_read_templates_invalid_toml_returns_error() {
        let invalid_toml = "this is not valid toml {{{{";
        let mut tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        std::io::Write::write_all(&mut tmp, invalid_toml.as_bytes()).expect("write");
        let path = tmp.path().to_str().expect("path str");

        let result = FileTemplateLoader::read_templates(path);
        assert!(result.is_err());
    }

    #[test]
    fn test_file_template_loader_read_templates_partial_toml_only_json() {
        let toml_content = r#"
[extraction]
json = "Only JSON template"
"#;
        let mut tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        std::io::Write::write_all(&mut tmp, toml_content.as_bytes()).expect("write");
        let path = tmp.path().to_str().expect("path str");

        let templates = FileTemplateLoader::read_templates(path).expect("read");
        assert_eq!(templates.len(), 1);
        assert!(templates.contains_key("json"));
        assert!(!templates.contains_key("markdown"));
    }

    #[test]
    fn test_file_template_loader_read_templates_no_extraction_key() {
        let toml_content = r#"
[other]
foo = "bar"
"#;
        let mut tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        std::io::Write::write_all(&mut tmp, toml_content.as_bytes()).expect("write");
        let path = tmp.path().to_str().expect("path str");

        let templates = FileTemplateLoader::read_templates(path).expect("read");
        assert!(templates.is_empty(), "no extraction key → empty templates");
    }

    #[test]
    fn test_file_template_loader_load_templates_returns_clone() {
        let toml_content = r#"
[extraction]
json = "J"
"#;
        let mut tmp = tempfile::NamedTempFile::new().expect("create tempfile");
        std::io::Write::write_all(&mut tmp, toml_content.as_bytes()).expect("write");
        let path = tmp.path().to_str().expect("path str").to_string();

        let loader = FileTemplateLoader::new(&path);
        let t1 = loader.load_templates().expect("load");
        let t2 = loader.load_templates().expect("load");
        assert_eq!(t1, t2);
    }

    #[test]
    fn test_file_template_loader_default() {
        // Default uses config/prompts.toml; in test env this may or may not exist.
        // We just verify it doesn't panic and returns a HashMap.
        let loader = FileTemplateLoader::default();
        let _templates = loader.load_templates().expect("load");
    }

    #[test]
    fn test_file_template_loader_default_path_does_not_panic() {
        let loader = FileTemplateLoader::default_path();
        let _templates = loader.load_templates().expect("load");
    }

    // ========== LLMService construction ==========

    #[test]
    fn test_llm_service_new_with_config_creates_instance() {
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new_with_config(
            "test-key".to_string(),
            "gpt-4".to_string(),
            "http://localhost:8080".to_string(),
            http_client,
        );
        assert_eq!(service.model, "gpt-4");
        assert_eq!(service.provider, "openai");
        assert_eq!(
            service.api_base_url,
            Some("http://localhost:8080".to_string())
        );
        assert_eq!(service.api_key, Some("test-key".to_string()));
    }

    #[test]
    fn test_llm_service_new_with_config_and_loader_creates_instance() {
        let http_client = Arc::new(reqwest::Client::new());
        let loader: TemplateLoader = Arc::new(
            InMemoryTemplateLoader::new()
                .with_template("json", "J {{text}}")
                .with_template("markdown", "M {{text}}"),
        );
        let service = LLMService::new_with_config_and_loader(
            "key".to_string(),
            "m".to_string(),
            "http://x:1234".to_string(),
            http_client,
            loader,
        );
        assert_eq!(service.model, "m");
        assert_eq!(service.provider, "openai");
        assert_eq!(service.api_base_url, Some("http://x:1234".to_string()));
        assert_eq!(service.api_key, Some("key".to_string()));
        let templates = service.templates.clone();
        assert_eq!(templates.len(), 2);
        assert!(templates.contains_key("json"));
        assert!(templates.contains_key("markdown"));
    }

    #[test]
    fn test_llm_service_new_with_config_has_default_templates() {
        // new_with_config uses InMemoryTemplateLoader::new().with_default_templates()
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new_with_config(
            "k".to_string(),
            "m".to_string(),
            "http://localhost:1".to_string(),
            http_client,
        );
        assert!(service.templates.contains_key("json"));
        assert!(service.templates.contains_key("markdown"));
    }

    #[tokio::test]
    async fn test_llm_service_engine_client_is_constructed() {
        // Verify create_engine_client produces a usable engine client via new_with_config
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new_with_config(
            "k".to_string(),
            "m".to_string(),
            "http://localhost:2".to_string(),
            http_client,
        );
        // engine_client is private but we can verify it exists by calling extract_data_internal
        // which uses it. We test template-not-found path (no HTTP call needed).
        let result = service
            .extract_data_internal("text", &json!({}), "nonexistent_format")
            .await;
        assert!(result.is_err());
    }

    // ========== extract_data_internal: error paths ==========

    #[tokio::test]
    async fn test_extract_data_internal_template_not_found_returns_error() {
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new_with_config(
            "k".to_string(),
            "m".to_string(),
            "http://localhost:3".to_string(),
            http_client,
        );
        let result = service
            .extract_data_internal("text", &json!({}), "unknown_format")
            .await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("Template not found"), "msg: {}", msg);
    }

    #[tokio::test]
    async fn test_extract_data_internal_no_api_base_url_without_experimental_returns_error() {
        // In default feature (no experimental), api_base_url=None should return error.
        let http_client = Arc::new(reqwest::Client::new());
        let mut service = LLMService::new_with_config(
            "k".to_string(),
            "m".to_string(),
            "http://localhost:4".to_string(),
            http_client,
        );
        service.api_base_url = None; // override to test the None branch

        let result = service
            .extract_data_internal("text", &json!({}), "json")
            .await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("experimental") || msg.contains("experimental"),
            "should mention experimental feature, got: {}",
            msg
        );
    }

    // ========== extract_data_internal: URL construction paths ==========
    // EngineClient::scrape() always validates URLs via SSRF protection,
    // which blocks 127.0.0.1/localhost. These tests verify that the URL
    // construction branches are exercised by checking the SSRF error is
    // returned (proving the code reached the scrape call, past template
    // lookup, prompt construction, URL construction, and request building).

    #[tokio::test]
    async fn test_extract_data_internal_default_url_construction_hits_ssrf() {
        // Default branch: base_url without /v1 or :11434
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new_with_config(
            "k".to_string(),
            "m".to_string(),
            "http://127.0.0.1:9999".to_string(),
            http_client,
        );

        let result = service
            .extract_data_internal("text", &json!({}), "json")
            .await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("SSRF protection"),
            "should fail at SSRF (proving default URL construction ran), got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn test_extract_data_internal_v1_url_construction_hits_ssrf() {
        // /v1 branch: base_url ends with /v1
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new_with_config(
            "k".to_string(),
            "m".to_string(),
            "http://127.0.0.1:9999/v1".to_string(),
            http_client,
        );

        let result = service
            .extract_data_internal("text", &json!({}), "json")
            .await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("SSRF protection"),
            "should fail at SSRF (proving /v1 URL construction ran), got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn test_extract_data_internal_v1_trailing_slash_url_construction_hits_ssrf() {
        // /v1/ branch: base_url ends with /v1/
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new_with_config(
            "k".to_string(),
            "m".to_string(),
            "http://127.0.0.1:9999/v1/".to_string(),
            http_client,
        );

        let result = service
            .extract_data_internal("text", &json!({}), "json")
            .await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("SSRF protection"),
            "should fail at SSRF (proving /v1/ URL construction ran), got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn test_extract_data_internal_ollama_port_url_construction_hits_ssrf() {
        // :11434 branch: base_url contains :11434
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new_with_config(
            "k".to_string(),
            "m".to_string(),
            "http://127.0.0.1:11434".to_string(),
            http_client,
        );

        let result = service
            .extract_data_internal("text", &json!({}), "json")
            .await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("SSRF protection"),
            "should fail at SSRF (proving :11434 URL construction ran), got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn test_extract_data_internal_ssrf_error_not_template_error() {
        // Verify the error is SSRF (code reached scrape), not template-not-found
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new_with_config(
            "k".to_string(),
            "m".to_string(),
            "http://127.0.0.1:9999".to_string(),
            http_client,
        );

        let result = service
            .extract_data_internal("text", &json!({}), "json")
            .await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            !msg.contains("Template not found"),
            "should NOT be template error, got: {}",
            msg
        );
        assert!(
            msg.contains("SSRF protection") || msg.contains("Direct LLM call failed"),
            "should be SSRF/scrape error, got: {}",
            msg
        );
    }

    // ========== new_with_template_loader ==========

    #[tokio::test]
    async fn test_new_with_template_loader_with_custom_templates() {
        let http_client = Arc::new(reqwest::Client::new());
        let loader: TemplateLoader = Arc::new(
            InMemoryTemplateLoader::new()
                .with_template("json", "Custom J {{text}}")
                .with_template("markdown", "Custom M {{text}}"),
        );
        // We can't easily construct Settings, but we can test new_with_config_and_loader
        // which has a similar code path.
        let service = LLMService::new_with_config_and_loader(
            "key".to_string(),
            "model".to_string(),
            "http://127.0.0.1:1".to_string(),
            http_client,
            loader,
        );
        assert_eq!(service.templates.len(), 2);
        assert_eq!(
            service.templates.get("json"),
            Some(&"Custom J {{text}}".to_string())
        );
    }

    #[tokio::test]
    async fn test_new_with_config_and_loader_with_failing_loader() {
        // When template loader fails, templates should be empty (unwrap_or_default)
        struct FailingLoader;
        #[async_trait]
        impl TemplateLoaderTrait for FailingLoader {
            fn load_templates(&self) -> Result<HashMap<String, String>> {
                Err(anyhow::anyhow!("simulated load failure"))
            }
        }

        let http_client = Arc::new(reqwest::Client::new());
        let loader: TemplateLoader = Arc::new(FailingLoader);
        let service = LLMService::new_with_config_and_loader(
            "key".to_string(),
            "model".to_string(),
            "http://127.0.0.1:1".to_string(),
            http_client,
            loader,
        );
        // Failing loader → unwrap_or_default → empty HashMap
        assert!(service.templates.is_empty());
    }

    // ========== LLMService Clone ==========

    #[test]
    fn test_llm_service_clone_preserves_fields() {
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new_with_config(
            "key".to_string(),
            "model-x".to_string(),
            "http://127.0.0.1:1".to_string(),
            http_client,
        );
        let cloned = service.clone();
        assert_eq!(cloned.model, "model-x");
        assert_eq!(cloned.provider, "openai");
        assert_eq!(cloned.api_key, Some("key".to_string()));
        assert_eq!(cloned.api_base_url, Some("http://127.0.0.1:1".to_string()));
        assert!(cloned.templates.contains_key("json"));
    }

    // ========== LLMServiceTrait impl ==========

    #[tokio::test]
    async fn test_llm_service_trait_impl_returns_ssrf_error() {
        // The trait method delegates to extract_data_internal.
        // We verify delegation by checking the same SSRF error path.
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new_with_config(
            "k".to_string(),
            "m".to_string(),
            "http://127.0.0.1:9999".to_string(),
            http_client,
        );

        let trait_ref: &dyn LLMServiceTrait = &service;
        let result = trait_ref.extract_data("text", &json!({}), "json").await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("SSRF protection") || msg.contains("Direct LLM call failed"),
            "trait impl should delegate to internal, got: {}",
            msg
        );
    }

    // ========== new_with_template_loader (with real Settings) ==========

    use crate::config::settings::LLMSettings;

    fn make_test_settings(llm: LLMSettings) -> Settings {
        use crate::config::settings::*;
        Settings {
            server: ServerSettings::default(),
            database: DatabaseSettings::default(),
            cors: CorsSettings::default(),
            rate_limiting: RateLimitingSettings::default(),
            concurrency: ConcurrencySettings::default(),
            webhook: WebhookSettings::default(),
            bing_search: BingSearchSettings::default(),
            search: SearchSettings::default(),
            llm,
            proxy: ProxySettings::default(),
            engines: EngineSettings::default(),
            logging: LoggingSettings::default(),
            workers: WorkerSettings::default(),
            timeouts: TimeoutSettings::default(),
            cache: CacheSettings::default(),
            trusted_proxies: TrustedProxySettings::default(),
        }
    }

    #[test]
    fn test_new_with_template_loader_default_settings() {
        let settings = make_test_settings(LLMSettings::default());
        let http_client = Arc::new(reqwest::Client::new());
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        let service = LLMService::new_with_template_loader(&settings, http_client, loader);
        // LLMSettings::default(): provider=None→"openai", model=Some("gpt-3.5-turbo"),
        // api_base_url=Some("https://api.openai.com/v1"), api_key=None
        assert_eq!(service.provider, "openai");
        assert_eq!(service.model, "gpt-3.5-turbo");
        assert_eq!(
            service.api_base_url.as_deref(),
            Some("https://api.openai.com/v1")
        );
        assert!(service.api_key.is_none());
    }

    #[test]
    fn test_new_with_template_loader_with_provider_and_model() {
        let llm = LLMSettings {
            provider: Some("anthropic".to_string()),
            api_key: Some("sk-test".to_string()),
            model: Some("claude-3".to_string()),
            api_base_url: Some("https://api.anthropic.com".to_string()),
        };
        let settings = make_test_settings(llm);
        let http_client = Arc::new(reqwest::Client::new());
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        let service = LLMService::new_with_template_loader(&settings, http_client, loader);
        assert_eq!(service.provider, "anthropic");
        assert_eq!(service.model, "claude-3");
        assert_eq!(
            service.api_base_url.as_deref(),
            Some("https://api.anthropic.com")
        );
        assert_eq!(service.api_key.as_deref(), Some("sk-test"));
    }

    #[test]
    fn test_new_with_template_loader_ollama_port_forces_openai() {
        let llm = LLMSettings {
            provider: Some("ollama".to_string()),
            api_key: None,
            model: None,
            api_base_url: Some("http://192.168.1.5:11434".to_string()),
        };
        let settings = make_test_settings(llm);
        let http_client = Arc::new(reqwest::Client::new());
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        let service = LLMService::new_with_template_loader(&settings, http_client, loader);
        assert_eq!(service.provider, "openai");
        assert_eq!(service.model, "gpt-3.5-turbo");
    }

    #[test]
    fn test_new_with_template_loader_172_24_160_1_forces_openai() {
        let llm = LLMSettings {
            provider: Some("ollama".to_string()),
            api_key: None,
            model: None,
            api_base_url: Some("http://172.24.160.1:8080".to_string()),
        };
        let settings = make_test_settings(llm);
        let http_client = Arc::new(reqwest::Client::new());
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        let service = LLMService::new_with_template_loader(&settings, http_client, loader);
        assert_eq!(service.provider, "openai");
    }

    #[test]
    fn test_new_with_template_loader_no_api_base_url_skips_ollama_check() {
        let llm = LLMSettings {
            provider: Some("ollama".to_string()),
            api_key: None,
            model: None,
            api_base_url: None,
        };
        let settings = make_test_settings(llm);
        let http_client = Arc::new(reqwest::Client::new());
        let loader: TemplateLoader = Arc::new(InMemoryTemplateLoader::new());
        let service = LLMService::new_with_template_loader(&settings, http_client, loader);
        assert_eq!(service.provider, "ollama");
    }

    #[test]
    fn test_new_with_template_loader_failing_loader_uses_empty_templates() {
        struct FailingLoader;
        impl TemplateLoaderTrait for FailingLoader {
            fn load_templates(&self) -> Result<HashMap<String, String>> {
                Err(anyhow::anyhow!("load fail"))
            }
        }
        let settings = make_test_settings(LLMSettings::default());
        let http_client = Arc::new(reqwest::Client::new());
        let loader: TemplateLoader = Arc::new(FailingLoader);
        let service = LLMService::new_with_template_loader(&settings, http_client, loader);
        assert!(service.templates.is_empty());
    }

    #[test]
    fn test_new_with_file_template_loader_default() {
        let settings = make_test_settings(LLMSettings::default());
        let http_client = Arc::new(reqwest::Client::new());
        let service = LLMService::new(&settings, http_client);
        assert_eq!(service.provider, "openai");
        assert_eq!(service.model, "gpt-3.5-turbo");
    }

    // ========== api_key=None header construction ==========

    #[tokio::test]
    async fn test_extract_data_internal_api_key_none_uses_bearer_ollama() {
        // When api_key is None, the Authorization header should be "Bearer ollama".
        // We verify this by checking that the code reaches the scrape call (SSRF error)
        // rather than failing at an earlier stage. The header construction code runs
        // before the scrape call, so line 320 (Bearer ollama) is covered.
        let http_client = Arc::new(reqwest::Client::new());
        let mut service = LLMService::new_with_config(
            "key".to_string(),
            "m".to_string(),
            "http://127.0.0.1:9999".to_string(),
            http_client,
        );
        // Override api_key to None to exercise the else branch (Bearer ollama)
        service.api_key = None;

        let result = service
            .extract_data_internal("text", &json!({}), "json")
            .await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        // Should reach SSRF (proving header construction with api_key=None ran)
        assert!(
            msg.contains("SSRF protection") || msg.contains("Direct LLM call failed"),
            "should fail at SSRF (proving api_key=None header code ran), got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn test_extract_data_internal_api_key_none_ollama_port() {
        // Combination: api_key=None + :11434 port → exercises both the ollama
        // header branch and the ollama URL construction branch.
        let http_client = Arc::new(reqwest::Client::new());
        let mut service = LLMService::new_with_config(
            "key".to_string(),
            "m".to_string(),
            "http://127.0.0.1:11434".to_string(),
            http_client,
        );
        service.api_key = None;

        let result = service
            .extract_data_internal("text", &json!({}), "json")
            .await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(
            msg.contains("SSRF protection") || msg.contains("Direct LLM call failed"),
            "should reach SSRF with api_key=None + ollama port, got: {}",
            msg
        );
    }
}
