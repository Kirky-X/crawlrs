// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use anyhow::{Context, Result};
use async_trait::async_trait;
#[cfg(test)]
use mockall::automock;
use serde_json::{json, Value};
use std::env;

#[cfg_attr(test, automock)]
#[async_trait]
pub trait LLMServiceTrait: Send + Sync {
    async fn extract_data(&self, text: &str, schema: &Value) -> Result<Value>;
}

/// LLM Service to handle interaction with LLM providers
pub struct LLMService {
    api_key: Option<String>,
    model: String,
}

#[async_trait]
impl LLMServiceTrait for LLMService {
    async fn extract_data(&self, text: &str, schema: &Value) -> Result<Value> {
        self.extract_data(text, schema).await
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
        }
    }

    /// Extract structured data from text using LLM
    ///
    /// # Arguments
    /// * `text` - The input text to process (e.g., HTML content or raw text)
    /// * `schema` - A JSON schema describing the desired output structure
    ///
    /// # Returns
    /// * `Result<Value>` - The extracted data as a JSON Value
    pub async fn extract_data(&self, text: &str, schema: &Value) -> Result<Value> {
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

        // Assuming OpenAI-compatible endpoint
        let response = client
            .post("https://api.openai.com/v1/chat/completions")
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

        if let Some(content) = body["choices"][0]["message"]["content"].as_str() {
            // Clean up potential markdown code blocks
            let clean_content = content
                .trim()
                .trim_start_matches("```json")
                .trim_start_matches("```")
                .trim_end_matches("```");

            serde_json::from_str::<Value>(clean_content)
                .context("Failed to parse extracted JSON content")
        } else {
            Err(anyhow::anyhow!("Invalid response format from LLM API"))
        }
    }
}
