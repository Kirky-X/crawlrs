// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! WebhookSenderImpl - 使用 reqwest 的 webhook 发送实现
//!
//! 此模块提供基于 reqwest 的 WebhookSender 实现。
//! 支持超时控制、SSRF 出站防护、错误处理和响应状态检查。

use crate::common::metrics_shim::counter;
use crate::domain::services::webhook_sender::WebhookSender;
use crate::infrastructure::security::ssrf::validate_url;
use crate::utils::http_client::create_http_client;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use log::{error, warn};
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

/// Webhook 发送超时时间（秒）
const WEBHOOK_TIMEOUT_SECS: u64 = 10;

/// WebhookSender 实现 - 使用 reqwest 发送 HTTP 请求
///
/// 此实现使用 reqwest 库发送简单的 HTTP POST 请求，
/// 适用于大多数 webhook 发送场景。
///
/// # Features
///
/// - 出站 URL SSRF 防护（每次发送前重新校验，防 DNS 重绑定 / TOCTOU）
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
    /// SSRF 出站校验开关（生产必须开启；仅回环地址测试替身允许关闭）
    ssrf_guard: bool,
}

impl WebhookSenderImpl {
    /// 创建新的 WebhookSenderImpl（默认开启 SSRF 出站校验）
    pub fn new(client: Arc<Client>, timeout: Duration) -> Self {
        Self {
            client,
            timeout,
            ssrf_guard: true,
        }
    }

    /// 创建关闭 SSRF 校验的 WebhookSenderImpl
    ///
    /// 仅用于面向回环地址测试替身（wiremock 等）的单测：
    /// SSRF 校验会拒绝一切 loopback/私网目标，导致本地 mock 不可达。
    /// 生产代码禁止使用此构造器。
    #[cfg(test)]
    pub fn new_without_ssrf_guard(client: Arc<Client>, timeout: Duration) -> Self {
        Self {
            client,
            timeout,
            ssrf_guard: false,
        }
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

    /// SSRF 出站校验（CWE-918）
    ///
    /// 每次发送前重新校验目标 URL：webhook URL 在创建/入队时虽已校验，
    /// 但投递可能发生在数分钟甚至数小时之后（重试队列），
    /// 期间 DNS 记录可能变更为内网地址（DNS 重绑定 / TOCTOU）。
    /// 校验失败的 URL 一律拒绝发送。
    async fn ssrf_check(&self, url: &str) -> Result<()> {
        if !self.ssrf_guard {
            return Ok(());
        }
        if let Err(e) = validate_url(url).await {
            warn!("SSRF protection blocked webhook delivery to {}: {}", url, e);
            counter!(
                "crawlrs_webhook_ssrf_blocked_total",
                "reason" => e.kind().to_string()
            )
            .increment(1);
            return Err(anyhow!(
                "SSRF protection: webhook URL rejected: {}",
                e.kind()
            ));
        }
        Ok(())
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
        // 出站前 SSRF 校验：拒绝 loopback/私网/云元数据等内网目标
        self.ssrf_check(url).await?;

        let request_builder = self.build_request(url, payload, headers).await?;

        // 添加超时
        let request_builder = request_builder.timeout(self.timeout);

        // 发送请求
        let response = match request_builder.send().await {
            Ok(resp) => resp,
            Err(e) => {
                // 网络层失败指标
                counter!(
                    "crawlrs_webhook_delivery_total",
                    "result" => "failure",
                    "status_code" => "0"
                )
                .increment(1);
                return Err(anyhow!("Failed to send webhook request: {}", e));
            }
        };

        let status = response.status().as_u16();

        // 检查响应状态
        if Self::is_success_status(status) {
            // Prometheus webhook 投递指标
            counter!(
                "crawlrs_webhook_delivery_total",
                "result" => "success",
                "status_code" => status.to_string()
            )
            .increment(1);
            Ok(status)
        } else {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Unable to read response body".to_string());

            // 截断过长的响应体（char 边界安全，中文/emoji 不 panic）
            let truncated_body = if body.chars().count() > 200 {
                format!(
                    "{}... (truncated)",
                    crate::common::text_slice::truncate_chars(&body, 200)
                )
            } else {
                body
            };

            error!(
                "Webhook delivery failed with status {}: {}",
                status, truncated_body
            );

            // Prometheus webhook 投递失败指标
            counter!(
                "crawlrs_webhook_delivery_total",
                "result" => "failure",
                "status_code" => status.to_string()
            )
            .increment(1);

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
        let sender = WebhookSenderImpl::new_without_ssrf_guard(client, Duration::from_secs(5));

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
        let sender = WebhookSenderImpl::new_without_ssrf_guard(client, Duration::from_secs(5));

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
        let sender = WebhookSenderImpl::new_without_ssrf_guard(client, Duration::from_secs(5));

        let payload = json!({"test": "data"});
        let mut headers = HashMap::new();
        headers.insert("X-Custom-Header".to_string(), "custom-value".to_string());
        headers.insert("X-Crawlrs-Signature".to_string(), "sig-123".to_string());

        let webhook_url = format!("{}/webhook", mock_server.uri());
        let result = sender.send(&webhook_url, &payload, Some(&headers)).await;

        assert!(result.is_ok());
    }

    #[test]
    fn test_is_success_status_2xx_range() {
        assert!(WebhookSenderImpl::is_success_status(200));
        assert!(WebhookSenderImpl::is_success_status(201));
        assert!(WebhookSenderImpl::is_success_status(204));
        assert!(WebhookSenderImpl::is_success_status(299));
    }

    #[test]
    fn test_is_success_status_non_2xx() {
        assert!(!WebhookSenderImpl::is_success_status(199));
        assert!(!WebhookSenderImpl::is_success_status(300));
        assert!(!WebhookSenderImpl::is_success_status(301));
        assert!(!WebhookSenderImpl::is_success_status(404));
        assert!(!WebhookSenderImpl::is_success_status(500));
        assert!(!WebhookSenderImpl::is_success_status(503));
    }

    #[test]
    fn test_with_default_config_creates_sender() {
        let sender = WebhookSenderImpl::with_default_config();
        assert_eq!(sender.timeout, Duration::from_secs(WEBHOOK_TIMEOUT_SECS));
    }

    #[test]
    fn test_new_enables_ssrf_guard_by_default() {
        let client = Arc::new(Client::new());
        assert!(WebhookSenderImpl::new(client, Duration::from_secs(1)).ssrf_guard);
        let client = Arc::new(Client::new());
        assert!(
            !WebhookSenderImpl::new_without_ssrf_guard(client, Duration::from_secs(1)).ssrf_guard
        );
    }

    // ========== SSRF 出站校验测试 (CWE-918) ==========
    //
    // 守卫开启时，loopback/私网/云元数据目标必须在发起任何网络请求之前被拒绝；
    // 错误消息只含脱敏类别（kind），不泄漏具体 IP/主机名。

    #[tokio::test]
    async fn test_ssrf_guard_blocks_loopback_wiremock_url() {
        let mock_server = MockServer::start().await;
        // wiremock 绑定 127.0.0.1 —— 守卫必须在连接前拒绝
        Mock::given(method("POST"))
            .respond_with(ResponseTemplate::new(200))
            .mount(&mock_server)
            .await;

        let client = Arc::new(Client::builder().build().unwrap());
        let sender = WebhookSenderImpl::new(client, Duration::from_secs(5));

        let webhook_url = format!("{}/webhook", mock_server.uri());
        let result = sender
            .send(&webhook_url, &serde_json::json!({"k": "v"}), None)
            .await;

        assert!(result.is_err(), "loopback webhook URL must be rejected");
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("SSRF protection"),
            "error should be attributed to SSRF guard, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_ssrf_guard_blocks_private_and_metadata_targets() {
        let client = Arc::new(Client::builder().build().unwrap());
        let sender = WebhookSenderImpl::new(client, Duration::from_secs(5));

        for url in [
            "http://10.0.0.1/hook",
            "http://192.168.1.1/hook",
            "http://172.16.0.1/hook",
            "http://169.254.169.254/latest/meta-data/",
            "http://localhost:9000/hook",
            "file:///etc/passwd",
        ] {
            let result = sender.send(url, &serde_json::json!({}), None).await;
            assert!(result.is_err(), "webhook to {} must be rejected", url);
            assert!(
                result.unwrap_err().to_string().contains("SSRF protection"),
                "rejection for {} must come from SSRF guard",
                url
            );
        }
    }

    #[tokio::test]
    async fn test_ssrf_guard_error_message_is_sanitized() {
        let client = Arc::new(Client::builder().build().unwrap());
        let sender = WebhookSenderImpl::new(client, Duration::from_secs(5));

        let result = sender
            .send("http://10.0.0.1/hook", &serde_json::json!({}), None)
            .await;
        let err_msg = result.unwrap_err().to_string();
        // 错误消息只含脱敏类别，不回显目标 URL / IP（防止内网拓扑进入事件记录）
        assert!(!err_msg.contains("10.0.0.1"), "got: {}", err_msg);
        assert!(!err_msg.contains("/hook"), "got: {}", err_msg);
    }

    #[tokio::test]
    async fn test_send_with_invalid_header_value_still_succeeds() {
        let mock_server = MockServer::start().await;

        // Invalid header value: newline character is not allowed in HTTP headers
        // The build_request method should warn and skip the invalid header,
        // but still send the request with the default Content-Type header.
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
        let sender = WebhookSenderImpl::new_without_ssrf_guard(client, Duration::from_secs(5));

        let payload = json!({"test": "data"});
        let mut headers = HashMap::new();
        // Insert a header with an invalid value (contains newline)
        headers.insert("X-Invalid".to_string(), "bad\nvalue".to_string());

        let webhook_url = format!("{}/webhook", mock_server.uri());
        let result = sender.send(&webhook_url, &payload, Some(&headers)).await;

        // Request should still succeed because invalid headers are skipped
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_send_failure_truncates_long_response_body() {
        let mock_server = MockServer::start().await;

        // Create a response body longer than 200 characters
        let long_body = "x".repeat(300);

        Mock::given(method("POST"))
            .and(path("/webhook"))
            .respond_with(ResponseTemplate::new(500).set_body_string(long_body.clone()))
            .mount(&mock_server)
            .await;

        let client = Arc::new(
            Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap(),
        );
        let sender = WebhookSenderImpl::new_without_ssrf_guard(client, Duration::from_secs(5));

        let payload = json!({"test": "data"});
        let webhook_url = format!("{}/webhook", mock_server.uri());
        let result = sender.send(&webhook_url, &payload, None).await;

        assert!(result.is_err());
        let error_msg = result.unwrap_err().to_string();
        // Verify the body was truncated
        assert!(
            error_msg.contains("... (truncated)"),
            "Error should contain truncated marker"
        );
        // Verify the error contains the status code
        assert!(error_msg.contains("500"));
    }
}
