// Copyright (c) 2025 Kirky.X
//
// Licensed under MIT License
// See LICENSE file in the project root for full license information.

//! Unified HTTP Client
//! Provides a single, simple interface for HTTP requests

use crate::engines::search_health::{SearchEngineHealth, SearchHealthChecker};
use crate::engines::user_agent::get_rotated_user_agent;
use once_cell::sync::Lazy;
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};

// ============== 统一输入输出模板 ==============

#[derive(Debug, Clone)]
pub struct HttpInput {
    pub url: String,
    pub method: HttpMethod,
    pub headers: Option<Vec<(String, String)>>,
    pub body: Option<String>,
    pub timeout: Option<u64>,
    pub retry: bool,
}

impl Default for HttpInput {
    fn default() -> Self {
        Self {
            url: String::new(),
            method: HttpMethod::GET,
            headers: None,
            body: None,
            timeout: Some(30),
            retry: true,
        }
    }
}

impl HttpInput {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
            ..Default::default()
        }
    }

    pub fn with_method(mut self, method: HttpMethod) -> Self {
        self.method = method;
        self
    }

    pub fn with_body(mut self, body: &str) -> Self {
        self.body = Some(body.to_string());
        self
    }

    pub fn with_timeout(mut self, seconds: u64) -> Self {
        self.timeout = Some(seconds);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HttpMethod {
    GET,
    POST,
    PUT,
    DELETE,
    PATCH,
}

impl HttpMethod {
    pub fn as_str(&self) -> &str {
        match self {
            HttpMethod::GET => "GET",
            HttpMethod::POST => "POST",
            HttpMethod::PUT => "PUT",
            HttpMethod::DELETE => "DELETE",
            HttpMethod::PATCH => "PATCH",
        }
    }
}

#[derive(Debug, Clone)]
pub struct HttpOutput {
    pub status_code: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub url: String,
    pub response_time_ms: u64,
}

#[derive(Debug)]
pub struct HttpResponse {
    pub output: HttpOutput,
    pub is_success: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum HttpError {
    #[error("Connection failed: {0}")]
    Connection(String),
    #[error("Timeout after {0}s")]
    Timeout(u64),
    #[error("HTTP {0}")]
    Status(u16),
    #[error("Request failed: {0}")]
    Failed(String),
}

// ============== 统一客户端 ==============

pub struct HttpClient {
    client: Client,
    health_checker: Arc<SearchHealthChecker>,
}

impl HttpClient {
    pub fn global() -> &'static Self {
        static INSTANCE: Lazy<HttpClient> = Lazy::new(|| HttpClient::new());
        &INSTANCE
    }

    fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            health_checker: Arc::new(SearchHealthChecker::new()),
        }
    }

    pub async fn request(&self, input: &HttpInput) -> Result<HttpResponse, HttpError> {
        let start_time = std::time::Instant::now();

        debug!("HTTP {} {}", input.method.as_str(), input.url);

        // Check URL health
        let domain = extract_domain(&input.url);
        let health = self.health_checker.check_health(&domain, &input.url).await;

        if !health.is_healthy {
            // 根据健康状态严重程度使用不同日志级别
            match health.status {
                SearchEngineHealth::Unhealthy => {
                    tracing::error!("Domain {} is unhealthy: {:?}", domain, health.error_message);
                }
                SearchEngineHealth::Degraded => {
                    tracing::warn!("Domain {} is degraded: {:?}", domain, health.error_message);
                }
                SearchEngineHealth::Healthy => {
                    tracing::debug!("Domain {} is healthy", domain);
                }
            }
        }

        // Execute with retry
        let result = self.execute_with_retry(input).await;
        let response_time = start_time.elapsed().as_millis() as u64;

        match result {
            Ok((status, headers, body)) => {
                let is_success = status >= 200 && status < 300;
                Ok(HttpResponse {
                    output: HttpOutput {
                        status_code: status,
                        headers,
                        body,
                        url: input.url.clone(),
                        response_time_ms: response_time,
                    },
                    is_success,
                })
            }
            Err(e) => Err(e),
        }
    }

    async fn execute_with_retry(
        &self,
        input: &HttpInput,
    ) -> Result<(u16, Vec<(String, String)>, String), HttpError> {
        let max_retries = if input.retry { 3 } else { 1 };
        let mut last_error = None;

        for attempt in 1..=max_retries {
            match self.execute_once(input).await {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_error = Some(e);
                    if attempt < max_retries {
                        warn!("HTTP request attempt {} failed, retrying...", attempt);
                        tokio::time::sleep(Duration::from_millis(100 * attempt as u64)).await;
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| HttpError::Failed("Unknown error".to_string())))
    }

    async fn execute_once(
        &self,
        input: &HttpInput,
    ) -> Result<(u16, Vec<(String, String)>, String), HttpError> {
        let ua = get_rotated_user_agent();

        let mut request = match input.method {
            HttpMethod::GET => self.client.get(&input.url),
            HttpMethod::POST => self.client.post(&input.url),
            HttpMethod::PUT => self.client.put(&input.url),
            HttpMethod::DELETE => self.client.delete(&input.url),
            HttpMethod::PATCH => self.client.patch(&input.url),
        };

        request = request.header("User-Agent", ua);

        // Add headers
        if let Some(headers) = &input.headers {
            for (k, v) in headers {
                request = request.header(k, v);
            }
        }

        // Add body
        if let Some(body) = &input.body {
            request = request.body(body.to_string());
        }

        // Set timeout
        if let Some(timeout) = input.timeout {
            request = request.timeout(Duration::from_secs(timeout));
        }

        // Send request
        let response = request
            .send()
            .await
            .map_err(|e| HttpError::Connection(e.to_string()))?;

        let status = response.status().as_u16();

        let headers: Vec<_> = response
            .headers()
            .iter()
            .filter_map(|(k, v)| v.to_str().ok().map(|s| (k.to_string(), s.to_string())))
            .collect();

        let body = response
            .text()
            .await
            .map_err(|e| HttpError::Failed(e.to_string()))?;

        Ok((status, headers, body))
    }

    pub async fn get_domain_health(
        &self,
        domain: &str,
    ) -> crate::engines::search_health::SearchHealthCheckResult {
        self.health_checker
            .check_health(domain, &format!("https://{}", domain))
            .await
    }
}

// ============== 简单接口 ==============

pub async fn get(url: &str) -> Result<HttpResponse, HttpError> {
    let input = HttpInput::new(url);
    HttpClient::global().request(&input).await
}

pub async fn post(url: &str, body: &str) -> Result<HttpResponse, HttpError> {
    let input = HttpInput::new(url)
        .with_method(HttpMethod::POST)
        .with_body(body);
    HttpClient::global().request(&input).await
}

fn extract_domain(url: &str) -> String {
    url.strip_prefix("https://")
        .or(url.strip_prefix("http://"))
        .unwrap_or(url)
        .split('/')
        .next()
        .unwrap_or(url)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_http_input_builder() {
        let input = HttpInput::new("https://example.com")
            .with_method(HttpMethod::POST)
            .with_body("test")
            .with_timeout(10);

        assert_eq!(input.url, "https://example.com");
        assert_eq!(input.method, HttpMethod::POST);
        assert_eq!(input.body, Some("test".to_string()));
    }

    #[test]
    fn test_http_method_as_str() {
        assert_eq!(HttpMethod::GET.as_str(), "GET");
        assert_eq!(HttpMethod::POST.as_str(), "POST");
    }

    #[test]
    fn test_extract_domain() {
        assert_eq!(extract_domain("https://google.com/search"), "google.com");
        assert_eq!(
            extract_domain("http://api.example.com/v1"),
            "api.example.com"
        );
    }
}
