// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Provider Adapter — LLM 提供商的请求适配与响应解析。
//!
//! 封装两种 LLM 调用路径：
//! 1. **直接 HTTP**（显式 `api_base_url` 时）：通过 `EngineClient` 发送 OpenAI 兼容请求
//! 2. **genai crate**（无显式地址时）：通过 genai 库的 provider 适配器
//!
//! `LlmRequest` / `LlmResponse` 为中间表示，解耦 prompt 构造与实际发送。

use crate::engines::engine_client::{EngineClient, HttpMethod, ScrapeOptions, ScrapeRequest};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;

use super::TokenUsage;

/// LLM 请求的中间表示
pub struct LlmRequest {
    pub url: String,
    pub model: String,
    pub prompt: String,
    pub api_key: Option<String>,
}

/// LLM 响应的中间表示
pub struct LlmResponse {
    pub content: String,
    pub usage: TokenUsage,
}

/// 构建直接 HTTP 请求的 URL（OpenAI 兼容格式）。
///
/// # Rules
///
/// - 以 `/v1` 或 `/v1/` 结尾 → 追加 `/chat/completions`
/// - 包含 `:11434`（Ollama 默认端口）→ 追加 `/v1/chat/completions`
/// - 其他 → 追加 `/chat/completions`
pub fn build_request_url(base_url: &str) -> String {
    if base_url.ends_with("/v1") || base_url.ends_with("/v1/") {
        format!("{}/chat/completions", base_url.trim_end_matches('/'))
    } else if base_url.contains(":11434") {
        format!("{}/v1/chat/completions", base_url.trim_end_matches('/'))
    } else {
        format!("{}/chat/completions", base_url.trim_end_matches('/'))
    }
}

/// 构建直接 HTTP 请求的 headers。
pub fn build_request_headers(api_key: Option<&str>) -> HashMap<String, String> {
    let mut headers = HashMap::new();
    headers.insert("Content-Type".to_string(), "application/json".to_string());
    if let Some(key) = api_key {
        headers.insert("Authorization".to_string(), format!("Bearer {}", key));
    } else {
        headers.insert("Authorization".to_string(), "Bearer ollama".to_string());
    }
    headers
}

/// 通过 `EngineClient` 发送直接 HTTP 请求到 LLM 提供商。
///
/// # Arguments
///
/// * `engine_client` - HTTP 引擎客户端
/// * `url` - 请求 URL
/// * `model` - 模型名称
/// * `prompt` - 完整 prompt 文本
/// * `api_key` - 可选 API 密钥
pub async fn send_direct_request(
    engine_client: &Arc<EngineClient>,
    url: &str,
    model: &str,
    prompt: &str,
    api_key: Option<&str>,
) -> Result<LlmResponse> {
    let body = json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.0
    });

    let headers = build_request_headers(api_key);

    let request = ScrapeRequest::new(url).with_options(
        ScrapeOptions::builder()
            .method(HttpMethod::Post)
            .headers(headers)
            .body(body.to_string())
            .build(),
    );

    let res = engine_client
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
        completion_tokens: res_json["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
        total_tokens: res_json["usage"]["total_tokens"].as_u64().unwrap_or(0) as u32,
    };

    Ok(LlmResponse { content, usage })
}

/// 通过 genai crate 发送请求（当无显式 api_base_url 时使用）。
///
/// # Errors
///
/// 当 `llm` feature 未启用时返回错误
#[allow(dead_code)]
pub async fn send_genai_request(provider: &str, model: &str, prompt: &str) -> Result<LlmResponse> {
    #[cfg(feature = "llm")]
    {
        use genai::chat::{ChatMessage, ChatRequest};

        let chat_req = ChatRequest::new(vec![ChatMessage::user(prompt.to_string())]);
        let model_id = format!("{}:{}", provider, model);

        // genai Client 是短生命周期的，每次创建
        let client = genai::Client::default();
        let chat_res = client
            .exec_chat(&model_id, chat_req, None)
            .await
            .map_err(|e| anyhow::anyhow!("LLM call failed for model {}: {:?}", model_id, e))?;

        let content = chat_res
            .first_text()
            .ok_or_else(|| anyhow::anyhow!("LLM returned empty content"))?
            .to_string();

        let usage = TokenUsage {
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
        };

        Ok(LlmResponse { content, usage })
    }
    #[cfg(not(feature = "llm"))]
    {
        let _ = (provider, model, prompt);
        Err(anyhow::anyhow!(
            "LLM provider requires 'llm' feature to be enabled. \
             Please rebuild with --features llm"
        ))
    }
}

/// 创建 EngineClient（从 reqwest::Client）。
pub fn create_engine_client(http_client: Arc<reqwest::Client>) -> Arc<EngineClient> {
    use crate::engines::client::reqwest::ReqwestEngine;
    use crate::engines::router::{EngineRouter, EngineRouterTrait};

    let reqwest_engine = ReqwestEngine::new(http_client);
    let router: Arc<dyn EngineRouterTrait> =
        Arc::new(EngineRouter::new(vec![Arc::new(reqwest_engine)]));
    Arc::new(EngineClient::with_router(router))
}

/// 从 Settings 解析 provider，应用 OllamaStrategy 规则。
pub fn resolve_provider(settings: &crate::config::settings::Settings) -> String {
    use super::super::llm_provider_strategy::{OllamaStrategy, ProviderStrategy};

    let fallback = settings
        .llm
        .provider
        .clone()
        .unwrap_or_else(|| "openai".to_string());
    OllamaStrategy::new(fallback).resolve_provider(settings.llm.api_base_url.as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_request_url_with_v1_suffix() {
        assert_eq!(
            build_request_url("https://api.example.com/v1"),
            "https://api.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_build_request_url_with_v1_trailing_slash() {
        assert_eq!(
            build_request_url("https://api.example.com/v1/"),
            "https://api.example.com/v1/chat/completions"
        );
    }

    #[test]
    fn test_build_request_url_ollama_port() {
        assert_eq!(
            build_request_url("http://localhost:11434"),
            "http://localhost:11434/v1/chat/completions"
        );
    }

    #[test]
    fn test_build_request_url_plain() {
        assert_eq!(
            build_request_url("https://api.example.com"),
            "https://api.example.com/chat/completions"
        );
    }

    #[test]
    fn test_build_request_headers_with_key() {
        let headers = build_request_headers(Some("sk-test"));
        assert_eq!(headers.get("Authorization").unwrap(), "Bearer sk-test");
        assert_eq!(headers.get("Content-Type").unwrap(), "application/json");
    }

    #[test]
    fn test_build_request_headers_without_key() {
        let headers = build_request_headers(None);
        assert_eq!(headers.get("Authorization").unwrap(), "Bearer ollama");
    }
}
