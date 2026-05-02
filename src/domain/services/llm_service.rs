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
pub trait TemplateLoaderTrait: shaku::Interface + Send + Sync {
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
            tracing::error!("Failed to load templates from {}: {}", path, e);
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
pub trait LLMServiceTrait: shaku::Interface + Send + Sync {
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
