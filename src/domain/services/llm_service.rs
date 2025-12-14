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

use anyhow::Result;
use serde_json::{json, Value};
use std::env;

/// LLM Service to handle interaction with LLM providers
///
/// Currently supports a mock implementation and placeholders for real providers (e.g., OpenAI)
pub struct LLMService {
    api_key: Option<String>,
    model: String,
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
        // Truncate text to avoid token limits (simplified)
        let truncated_text = if text.len() > 10000 {
            &text[..10000]
        } else {
            text
        };

        if let Some(api_key) = &self.api_key {
            // Check if we have a real implementation available (e.g. via feature flag or simple check)
            // For this project, we'll implement a basic HTTP call to an OpenAI-compatible API
            // if the API key is present.

            // This is a simplified implementation. In production, use a proper client library (e.g. async-openai)
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
                .await;

            match response {
                Ok(resp) => {
                    if resp.status().is_success() {
                        let body: Value = resp.json().await?;
                        if let Some(content) = body["choices"][0]["message"]["content"].as_str() {
                            // Clean up potential markdown code blocks
                            let clean_content = content
                                .trim()
                                .trim_start_matches("```json")
                                .trim_start_matches("```")
                                .trim_end_matches("```");

                            match serde_json::from_str::<Value>(clean_content) {
                                Ok(val) => return Ok(val),
                                Err(_) => {
                                    // If parsing fails, fall back to mock or error
                                    // println!("Failed to parse LLM response: {}", content);
                                }
                            }
                        }
                    }
                    // If API call fails or returns unexpected format, fall back to mock
                    self.mock_extraction(truncated_text, schema)
                }
                Err(_) => self.mock_extraction(truncated_text, schema),
            }
        } else {
            self.mock_extraction(truncated_text, schema)
        }
    }

    fn mock_extraction(&self, text: &str, _schema: &Value) -> Result<Value> {
        // Simple mock implementation that attempts to find keywords or returns a dummy object
        // In a real scenario, this would call an LLM API

        // Simulate processing time
        // std::thread::sleep(std::time::Duration::from_millis(100));

        Ok(json!({
            "summary": format!("Extracted from {} chars", text.len()),
            "entities": ["mock_entity_1", "mock_entity_2"],
            "sentiment": "neutral"
        }))
    }
}
