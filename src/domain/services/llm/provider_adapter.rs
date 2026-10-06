// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Provider Adapter — LLM 提供商的请求适配与响应解析。
//!
//! 封装两种 LLM 调用路径：
//! 1. **直接 HTTP**（显式 `api_base_url` 时）：经共享 reqwest 客户端直连
//!    OpenAI 兼容端点（运维自配地址，不走用户输入的 SSRF 校验）
//! 2. **genai crate**（无显式地址时）：通过 genai 库的 provider 适配器
//!
//! `LlmRequest` / `LlmResponse` 为中间表示，解耦 prompt 构造与实际发送。

use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::collections::HashMap;

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

/// 向 LLM 提供商发送直接 HTTP 请求（POST，OpenAI 兼容格式）。
///
/// 使用共享 `reqwest::Client` 直连而非 `EngineClient`：`[llm] api_base_url`
/// 是运维自配的服务端端点（默认 `http://localhost:11434/v1` Ollama），不属于
/// 用户抓取输入，不经过面向用户输入的 SSRF 校验——否则本机 LLM 开箱即被拦
/// （"Hostname 'localhost' is blocked"）。
///
/// # Arguments
///
/// * `http_client` - 共享 HTTP 客户端
/// * `url` - 请求 URL
/// * `model` - 模型名称
/// * `prompt` - 完整 prompt 文本
/// * `api_key` - 可选 API 密钥
/// * `max_tokens` - 可选单次回答 token 上限（确定性封顶生成时长，防无界生成挂死任务）
/// * `timeout_secs` - 单请求超时秒数（须覆盖自托管端点冷加载与长生成）
pub async fn send_direct_request(
    http_client: &reqwest::Client,
    url: &str,
    model: &str,
    prompt: &str,
    api_key: Option<&str>,
    max_tokens: Option<u64>,
    timeout_secs: u64,
) -> Result<LlmResponse> {
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

    let mut body = json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "temperature": 0.0
    });
    if let Some(cap) = max_tokens {
        body["max_tokens"] = json!(cap);
    }

    let mut headers = HeaderMap::new();
    for (k, v) in build_request_headers(api_key) {
        if let (Ok(header_name), Ok(header_value)) = (
            HeaderName::from_bytes(k.as_bytes()),
            HeaderValue::from_str(&v),
        ) {
            headers.insert(header_name, header_value);
        }
    }

    let resp = http_client
        .post(url)
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .headers(headers)
        .body(body.to_string())
        .send()
        .await
        .context("Direct LLM call failed")?;

    let status = resp.status();
    let text = resp
        .text()
        .await
        .context("Failed to read LLM response body")?;
    if !status.is_success() {
        return Err(anyhow::anyhow!("LLM returned error: {}", text));
    }

    let res_json: Value =
        serde_json::from_str(&text).context("Failed to parse LLM JSON response")?;

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

    // ===== send_direct_request / resolve_provider =====

    /// 启动返回固定 (status, body) 的 OpenAI 兼容 mock，路径 /chat。
    async fn start_llm_mock(status: u16, body: String) -> String {
        use axum::http::StatusCode;
        use axum::routing::any;
        use axum::Router;
        use tokio::net::TcpListener;
        let app = Router::new().route(
            "/chat",
            any(move || {
                let b = body.clone();
                async move { (StatusCode::from_u16(status).expect("status"), b) }
            }),
        );
        let l = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = l.local_addr().expect("addr");
        tokio::spawn(async move {
            let _ = axum::serve(l, app).await;
        });
        format!("http://{}/chat", addr)
    }

    #[tokio::test]
    async fn send_direct_request_success_parses_content_and_usage() {
        let url = start_llm_mock(
            200,
            r#"{"choices":[{"message":{"content":"hi"}}],"usage":{"prompt_tokens":5,"completion_tokens":2,"total_tokens":7}}"#
                .to_string(),
        )
        .await;
        let resp = send_direct_request(
            &reqwest::Client::new(),
            &url,
            "gpt-x",
            "prompt",
            Some("sk-test"),
            Some(64),
            10,
        )
        .await
        .expect("success");
        assert_eq!(resp.content, "hi");
        assert_eq!(resp.usage.prompt_tokens, 5);
        assert_eq!(resp.usage.completion_tokens, 2);
        assert_eq!(resp.usage.total_tokens, 7);
    }

    #[tokio::test]
    async fn send_direct_request_no_api_key_no_max_tokens() {
        let url = start_llm_mock(
            200,
            r#"{"choices":[{"message":{"content":"ok"}}]}"#.to_string(),
        )
        .await;
        let resp = send_direct_request(&reqwest::Client::new(), &url, "m", "p", None, None, 10)
            .await
            .expect("success (ollama default header)");
        assert_eq!(resp.content, "ok");
        // 无 usage 字段时归零
        assert_eq!(resp.usage.total_tokens, 0);
    }

    #[tokio::test]
    async fn send_direct_request_error_status_maps_to_err() {
        let url = start_llm_mock(500, "boom".to_string()).await;
        let err = send_direct_request(&reqwest::Client::new(), &url, "m", "p", None, None, 10)
            .await
            .err()
            .expect("500 must err");
        assert!(err.to_string().contains("LLM returned error"), "got: {err}");
    }

    #[tokio::test]
    async fn send_direct_request_invalid_json_maps_to_err() {
        let url = start_llm_mock(200, "not-json".to_string()).await;
        assert!(
            send_direct_request(&reqwest::Client::new(), &url, "m", "p", None, None, 10,)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn send_direct_request_empty_choices_maps_to_err() {
        let url = start_llm_mock(200, r#"{"choices":[]}"#.to_string()).await;
        let err = send_direct_request(&reqwest::Client::new(), &url, "m", "p", None, None, 10)
            .await
            .err()
            .expect("empty content must err");
        assert!(err.to_string().contains("Empty content"), "got: {err}");
    }

    #[test]
    fn resolve_provider_returns_non_empty_with_default_settings() {
        let settings = crate::config::settings::Settings::default();
        let provider = resolve_provider(&settings);
        assert!(!provider.is_empty(), "provider should not be empty");
    }
}
