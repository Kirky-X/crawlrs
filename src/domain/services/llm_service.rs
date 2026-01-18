// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! LLMService - LLM provider interaction handling

#![allow(deprecated)]

use crate::config::settings::Settings;
use crate::utils::http_client::HTTP_CLIENT;
use anyhow::{Context, Result};
use async_trait::async_trait;
use genai::chat::{ChatMessage, ChatRequest};
use genai::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;

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
    ) -> Result<(Value, TokenUsage)>;
}

/// LLM服务 - 处理与LLM提供商的交互
pub struct LLMService {
    client: Client,
    model: String,
    provider: String,
    api_base_url: Option<String>,
    api_key: Option<String>,
    templates: HashMap<String, String>,
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
    pub fn new(settings: &Settings) -> Self {
        let templates = Self::load_templates().unwrap_or_default();
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

        // 核心研究成果应用：如果检测到是本地 Ollama 地址，我们“假装”它是 OpenAI
        // 这样可以避开 genai 内置适配器对 localhost 的强行转换
        if let Some(url) = &api_base_url {
            if url.contains("172.24.160.1") || url.contains(":11434") {
                provider = "openai".to_string();
            }
        }

        Self {
            client: Client::default(),
            model,
            provider,
            api_base_url,
            api_key,
            templates,
        }
    }

    pub fn new_with_config(_api_key: String, model: String, api_base_url: String) -> Self {
        let mut templates = HashMap::new();
        // Fallback templates if loading fails
        templates.insert(
            "json".to_string(),
            "Extract JSON from {{text}} using schema {{schema}}".to_string(),
        );
        templates.insert(
            "markdown".to_string(),
            "Extract Markdown from {{text}}".to_string(),
        );

        Self {
            client: Client::default(),
            model,
            provider: "openai".to_string(),
            api_base_url: Some(api_base_url),
            api_key: Some(_api_key),
            templates,
        }
    }

    fn load_templates() -> Result<HashMap<String, String>> {
        let content = fs::read_to_string("config/prompts.toml")?;
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

            let mut request = HTTP_CLIENT.post(&url).json(&body);
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
                .content_text_as_str()
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
