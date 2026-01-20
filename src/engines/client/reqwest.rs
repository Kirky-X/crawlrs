// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::engines::engine_client::{
    EngineError, InternalScrapeRequest, InternalScrapeResponse, ScraperEngine,
};
use crate::engines::validators;
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 默认超时时间
const DEFAULT_TIMEOUT_SECONDS: u64 = 30;

/// 抓取引擎
///
/// 基于reqwest实现的基本HTTP抓取引擎
pub struct ReqwestEngine {
    /// HTTP 客户端（通过依赖注入，支持连接复用）
    http_client: Arc<reqwest::Client>,
    /// 全局代理URL（如果配置）
    proxy_url: Option<String>,
}

impl ReqwestEngine {
    /// 创建新的 ReqwestEngine 实例
    pub fn new(http_client: Arc<reqwest::Client>) -> Self {
        Self {
            http_client,
            proxy_url: None,
        }
    }

    /// 创建带代理配置的 ReqwestEngine 实例
    pub fn with_proxy(http_client: Arc<reqwest::Client>, proxy_url: impl Into<String>) -> Self {
        Self {
            http_client,
            proxy_url: Some(proxy_url.into()),
        }
    }

    /// 获取HTTP客户端
    fn get_client(&self, proxy: &Option<String>) -> reqwest::Client {
        // 如果请求指定了代理，或者引擎有全局代理配置
        let proxy_url = proxy.as_ref().or(self.proxy_url.as_ref());

        if let Some(url) = proxy_url {
            // 创建带代理的客户端
            let builder = reqwest::Client::builder()
                .timeout(Duration::from_secs(DEFAULT_TIMEOUT_SECONDS))
                .cookie_store(true);

            match reqwest::Proxy::http(url) {
                Ok(proxy) => {
                    if let Ok(client) = builder.proxy(proxy).build() {
                        tracing::debug!("Using HTTP proxy: {}", url);
                        return client;
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to configure HTTP proxy: {}", e);
                }
            }
        }

        // 返回注入的 HTTP 客户端（Arc 被克隆但引用计数不变）
        (*self.http_client).clone()
    }
}

#[async_trait]
impl ScraperEngine for ReqwestEngine {
    /// 执行HTTP抓取
    ///
    /// # 参数
    ///
    /// * `request` - 抓取请求
    ///
    /// # 返回值
    ///
    /// * `Ok(InternalScrapeResponse)` - 抓取响应
    /// * `Err(EngineError)` - 抓取过程中出现的错误
    async fn scrape(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        // SSRF protection: validate all URLs to prevent access to internal services
        validators::validate_url(&request.url)
            .await
            .map_err(|e| EngineError::Other(format!("SSRF protection: {}", e)))?;

        // Build headers
        let mut headers = HeaderMap::new();
        for (k, v) in &request.headers {
            if let (Ok(header_name), Ok(header_value)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(v),
            ) {
                headers.insert(header_name, header_value);
            }
        }

        // Use shared HTTP client for connection reuse, with proxy support
        let client = self.get_client(&request.proxy);

        // Create request builder
        let mut request_builder = if request.mobile {
            client
                .get(&request.url)
                .header("User-Agent", "Mozilla/5.0 (iPhone; CPU iPhone OS 14_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0 Mobile/15E148 Safari/604.1")
        } else {
            client.get(&request.url).header(
                "User-Agent",
                "Mozilla/5.0 (compatible; crawlrs/1.0; +http://crawlrs.dev)",
            )
        };

        // Add custom headers
        request_builder = request_builder.headers(headers);

        // Set timeout
        request_builder = request_builder.timeout(request.timeout);

        let start = Instant::now();
        let response_result = request_builder.send().await;

        let response = match response_result {
            Ok(resp) => resp,
            Err(e) if e.is_timeout() => return Err(EngineError::Timeout(request.timeout)),
            Err(e) => return Err(EngineError::RequestFailed(e.to_string())),
        };

        let status_code = response.status().as_u16();
        let content_type = response
            .headers()
            .get("content-type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("text/html")
            .to_string();

        // Ensure content_type is not empty
        let content_type = if content_type.trim().is_empty() {
            "text/html".to_string()
        } else {
            content_type
        };

        let mut response_headers = std::collections::HashMap::with_capacity(32);
        for (k, v) in response.headers() {
            if let Ok(v_str) = v.to_str() {
                response_headers.insert(k.as_str().to_string(), v_str.to_string());
            }
        }

        let content = response
            .text()
            .await
            .map_err(|e| EngineError::RequestFailed(e.to_string()))?;

        // 同步等待
        if request.sync_wait_ms > 0 {
            tokio::time::sleep(Duration::from_millis(request.sync_wait_ms as u64)).await;
        }

        Ok(InternalScrapeResponse {
            status_code,
            content,
            screenshot: None,
            content_type,
            headers: response_headers,
            response_time_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// 计算对请求的支持分数
    ///
    /// # 参数
    ///
    /// * `request` - 抓取请求
    ///
    /// # 返回值
    ///
    /// 支持分数（0-100），不支持JS和截图的请求返回100分
    fn support_score(&self, request: &InternalScrapeRequest) -> u8 {
        if request.needs_js || request.needs_screenshot {
            return 10; // Low priority for unsupported features
        }
        100 // Highest priority (fastest)
    }

    /// 获取引擎名称
    ///
    /// # 返回值
    ///
    /// 引擎名称
    fn name(&self) -> &'static str {
        "reqwest"
    }
}
