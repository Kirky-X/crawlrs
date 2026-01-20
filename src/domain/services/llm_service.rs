// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! LLMService - LLM provider interaction handling

use crate::config::settings::Settings;
use anyhow::{Context, Result};
use async_trait::async_trait;
use genai::chat::{ChatMessage, ChatRequest};
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
    file_path: String,
}

impl FileTemplateLoader {
    pub fn new(file_path: impl Into<String>) -> Self {
        Self {
            file_path: file_path.into(),
        }
    }

    /// Create with default path
    pub fn default_path() -> Self {
        Self::new("config/prompts.toml")
    }
}

impl Default for FileTemplateLoader {
    fn default() -> Self {
        Self::default_path()
    }
}

impl TemplateLoaderTrait for FileTemplateLoader {
    fn load_templates(&self) -> Result<HashMap<String, String>> {
        let content = std::fs::read_to_string(&self.file_path)?;
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
    ) -> Result<(Value, TokenUsage)>;
}

/// LLM服务 - 处理与LLM提供商的交互
#[derive(Clone)]
pub struct LLMService {
    /// HTTP 客户端 (通过依赖注入的单例)
    http_client: Arc<reqwest::Client>,
    /// LLM 客户端
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
    /// Create LLMService with settings and custom template loader
    pub fn new_with_template_loader(
        settings: &Settings,
        http_client: Arc<reqwest::Client>,
        template_loader: TemplateLoader,
    ) -> Self {
        let templates = template_loader.load_templates().unwrap_or_default();
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
            http_client,
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

        Self {
            http_client,
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

        Self {
            http_client,
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

            let mut request = self.http_client.post(&url).json(&body);
            if let Some(key) = &self.api_key {
                request = request.bearer_auth(key);
            } else {
                request = request.bearer_auth("ollama");
            }

            let res = request.send().await.context("Direct LLM call failed")?;
            if !res.status().is_success() {
                let err = res.text().await.unwrap_or_default();
                return Err(anyhow::anyhow!("LLM returned error: {}", err));
            }

            let res_json: Value = res
                .json()
                .await
                .context("Failed to parse LLM JSON response")?;

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
