// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! WebhookSenderImpl - 使用 reqwest 的 webhook 发送实现
//!
//! 此模块提供基于 reqwest 的 WebhookSender 实现。
//! 支持超时控制、错误处理和响应状态检查。

use crate::domain::services::webhook_sender::WebhookSender;
use crate::utils::http_client::create_http_client;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use log::{error, warn};

/// Webhook 发送超时时间（秒）
const WEBHOOK_TIMEOUT_SECS: u64 = 10;

/// WebhookSender 实现 - 使用 reqwest 发送 HTTP 请求
///
/// 此实现使用 reqwest 库发送简单的 HTTP POST 请求，
/// 适用于大多数 webhook 发送场景。
///
/// # Features
///
/// - 支持自定义 HTTP 头
/// - 支持 JSON payload
/// - 可配置超时时间
/// - 响应状态检查
#[derive(Clone)]
pub struct WebhookSenderImpl {
    /// HTTP 客户端
    client: Arc<Client>,
    /// 请求超时时间
    timeout: Duration,
}

impl WebhookSenderImpl {
    /// 创建新的 WebhookSenderImpl
    pub fn new(client: Arc<Client>, timeout: Duration) -> Self {
        Self { client, timeout }
    }

    /// 使用默认配置创建 WebhookSenderImpl
    pub fn with_default_config() -> Self {
        Self::new(
            create_http_client(),
            Duration::from_secs(WEBHOOK_TIMEOUT_SECS),
        )
    }

    /// 构建请求 builder
    async fn build_request<'a>(
        &self,
        url: &str,
        payload: &'a Value,
        headers: Option<&'a HashMap<String, String>>,
    ) -> Result<reqwest::RequestBuilder> {
        let payload_str = serde_json::to_string(payload)
            .map_err(|e| anyhow!("Failed to serialize payload: {}", e))?;

        let mut request_builder = self.client.post(url).body(payload_str);

        // 设置默认 headers
        request_builder = request_builder.header(
            "Content-Type",
            reqwest::header::HeaderValue::from_static("application/json"),
        );

        // 添加自定义 headers
        if let Some(custom_headers) = headers {
            for (key, value) in custom_headers {
                if let Ok(header_value) = reqwest::header::HeaderValue::from_str(value) {
                    request_builder = request_builder.header(key.as_str(), header_value);
                } else {
                    warn!("Invalid header value for '{}': {}", key, value);
                }
            }
        }

        Ok(request_builder)
    }

    /// 检查响应状态是否表示成功
    fn is_success_status(status: u16) -> bool {
        (200..300).contains(&status)
    }
}

#[async_trait]
impl WebhookSender for WebhookSenderImpl {
    async fn send(
        &self,
        url: &str,
        payload: &Value,
        headers: Option<&HashMap<String, String>>,
    ) -> Result<()> {
        self.send_with_status(url, payload, headers).await?;
        Ok(())
    }

    async fn send_with_status(
        &self,
        url: &str,
        payload: &Value,
        headers: Option<&HashMap<String, String>>,
    ) -> Result<u16> {
        let request_builder = self.build_request(url, payload, headers).await?;

        // 添加超时
        let request_builder = request_builder.timeout(self.timeout);

        // 发送请求
        let response = request_builder
            .send()
            .await
            .map_err(|e| anyhow!("Failed to send webhook request: {}", e))?;

        let status = response.status().as_u16();

        // 检查响应状态
        if Self::is_success_status(status) {
            Ok(status)
        } else {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read response body".to_string());

            // 截断过长的响应体
            let truncated_body = if body.len() > 200 {
                format!("{}... (truncated)", &body[..200])
            } else {
                body
            };

            error!(
                "Webhook delivery failed with status {}: {}",
                status, truncated_body
            );

            Err(anyhow!(
                "Webhook delivery failed with status {}: {}",
                status,
                truncated_body
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use wiremock::matchers::{header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn test_send_success() {
        let mock_server = MockServer::start().await;

        // Setup mock response
        Mock::given(method("POST"))
            .and(path("/webhook"))
            .and(header("Content-Type", "application/json"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let client = Arc::new(
            Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
        );
        let sender = WebhookSenderImpl::new(client, Duration::from_secs(5));

        let payload = json!({"test": "data"});
        let headers = HashMap::new();

        let webhook_url = format!("{}/webhook", mock_server.uri());
        let result = sender.send(&webhook_url, &payload, Some(&headers)).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_failure() {
        let mock_server = MockServer::start().await;

        // Setup mock response with error status
        Mock::given(method("POST"))
            .and(path("/webhook"))
            .respond_with(ResponseTemplate::new(500))
            .mount(&mock_server)
            .await;

        let client = Arc::new(
            Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
        );
        let sender = WebhookSenderImpl::new(client, Duration::from_secs(5));

        let payload = json!({"test": "data"});

        let webhook_url = format!("{}/webhook", mock_server.uri());
        let result = sender.send(&webhook_url, &payload, None).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_send_with_custom_headers() {
        let mock_server = MockServer::start().await;

        // Setup mock response
        Mock::given(method("POST"))
            .and(path("/webhook"))
            .and(header("X-Custom-Header", "custom-value"))
            .and(header("X-Crawlrs-Signature", "sig-123"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let client = Arc::new(
            Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
        );
        let sender = WebhookSenderImpl::new(client, Duration::from_secs(5));

        let payload = json!({"test": "data"});
        let mut headers = HashMap::new();
        headers.insert("X-Custom-Header".to_string(), "custom-value".to_string());
        headers.insert("X-Crawlrs-Signature".to_string(), "sig-123".to_string());

        let webhook_url = format!("{}/webhook", mock_server.uri());
        let result = sender.send(&webhook_url, &payload, Some(&headers)).await;

        assert!(result.is_ok());
    }
}
