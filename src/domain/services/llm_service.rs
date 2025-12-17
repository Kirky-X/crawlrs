// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::env;

use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
}

#[async_trait]
pub trait LLMServiceTrait: Send + Sync {
    async fn extract_data(&self, text: &str, schema: &Value) -> Result<(Value, TokenUsage)>;
}

/// LLM服务 - 处理与LLM提供商的交互
///
/// # 功能
///
/// 提供与大型语言模型（LLM）提供商的交互接口，支持数据提取功能
///
/// # 配置
///
/// 通过环境变量进行配置：
/// - `LLM_API_KEY` - LLM API密钥
/// - `LLM_MODEL` - 使用的模型名称（默认为 gpt-3.5-turbo）
/// - `LLM_API_BASE_URL` - LLM API基础URL
pub struct LLMService {
    api_key: Option<String>,
    model: String,
    api_base_url: String,
}

#[async_trait]
impl LLMServiceTrait for LLMService {
    async fn extract_data(&self, text: &str, schema: &Value) -> Result<(Value, TokenUsage)> {
        LLMService::extract_data(self, text, schema).await
    }
}

impl Default for LLMService {
    fn default() -> Self {
        Self::new()
    }
}

impl LLMService {
    pub fn new() -> Self {
        Self {
            api_key: env::var("LLM_API_KEY").ok(),
            model: env::var("LLM_MODEL").unwrap_or_else(|_| "gpt-3.5-turbo".to_string()),
            api_base_url: env::var("LLM_API_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
        }
    }

    pub fn new_with_config(api_key: String, model: String, api_base_url: String) -> Self {
        Self {
            api_key: Some(api_key),
            model,
            api_base_url,
        }
    }

    /// 使用LLM从文本中提取结构化数据
    ///
    /// # 参数
    /// * `text` - 输入文本（例如HTML内容或纯文本）
    /// * `schema` - JSON模式，描述期望的输出结构
    ///
    /// # 返回值
    /// * `Result<(Value, TokenUsage)>` - 提取的数据和令牌使用情况
    ///
    /// # 错误
    /// * 当LLM API密钥未配置时返回错误
    /// * 当LLM服务调用失败时返回错误
    pub async fn extract_data(&self, text: &str, schema: &Value) -> Result<(Value, TokenUsage)> {
        let api_key = self
            .api_key
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("LLM API key not configured"))?;

        // Truncate text to avoid token limits (simplified)
        let truncated_text = if text.len() > 10000 {
            &text[..10000]
        } else {
            text
        };

        let client = reqwest::Client::new();
        let prompt = format!(
            "Extract data from the following text according to this JSON schema: {}. \
            Return ONLY the valid JSON object, no markdown formatting. \
            Text: {}",
            schema, truncated_text
        );

        let request_body = json!({
            "model": self.model,
            "messages": [
                {
                    "role": "system",
                    "content": "You are a helpful data extraction assistant. You output only valid JSON."
                },
                {
                    "role": "user",
                    "content": prompt
                }
            ],
            "temperature": 0.0
        });

        let url = format!("{}/chat/completions", self.api_base_url);
        let response = client
            .post(url)
            .header("Authorization", format!("Bearer {}", api_key))
            .json(&request_body)
            .send()
            .await
            .context("Failed to send request to LLM API")?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_default();
            return Err(anyhow::anyhow!(
                "LLM API returned error: {} - {}",
                status,
                error_text
            ));
        }

        let body: Value = response
            .json()
            .await
            .context("Failed to parse LLM API response")?;

        let usage = if let Some(usage_val) = body.get("usage") {
            TokenUsage {
                prompt_tokens: usage_val["prompt_tokens"].as_u64().unwrap_or(0) as u32,
                completion_tokens: usage_val["completion_tokens"].as_u64().unwrap_or(0) as u32,
                total_tokens: usage_val["total_tokens"].as_u64().unwrap_or(0) as u32,
            }
        } else {
            TokenUsage::default()
        };

        if let Some(content) = body["choices"][0]["message"]["content"].as_str() {
            // Clean up potential markdown code blocks
            let clean_content = content
                .trim()
                .trim_start_matches("```json")
                .trim_start_matches("```")
                .trim_end_matches("```");

            let data = serde_json::from_str::<Value>(clean_content)
                .context("Failed to parse extracted JSON content")?;
            Ok((data, usage))
        } else {
            Err(anyhow::anyhow!("Invalid response format from LLM API"))
        }
    }
}
