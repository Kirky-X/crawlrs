// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::engines::client::flaresolverr_types::{FlareSolverrRequest, FlareSolverrResponse};
use crate::engines::engine_client::{
    EngineError, InternalScrapeRequest, InternalScrapeResponse, ScraperEngine,
};
use async_trait::async_trait;
use std::time::Instant;

use std::sync::Arc;

/// Fire Engine (TLS) 实现
///
/// 专注于 TLS 指纹对抗，速度较快，但不支持截图和复杂的 JS 交互。
pub struct FireEngineTls {
    client: Arc<reqwest::Client>,
    base_url: String,
    /// 代理配置
    proxy_url: Option<String>,
}

impl FireEngineTls {
    /// 创建新的 FireEngineTls 实例
    pub fn new(client: Arc<reqwest::Client>) -> Self {
        Self {
            client,
            base_url: String::new(),
            proxy_url: None,
        }
    }

    /// 创建带有代理配置的 FireEngineTls 实例
    pub fn with_proxy(client: Arc<reqwest::Client>, proxy: &str) -> Self {
        Self {
            client,
            base_url: String::new(),
            proxy_url: Some(proxy.to_string()),
        }
    }

    /// 创建带有 base URL 和代理配置的实例
    pub fn with_url_and_proxy(
        client: Arc<reqwest::Client>,
        base_url: &str,
        proxy: Option<&str>,
    ) -> Self {
        Self {
            client,
            base_url: base_url.to_string(),
            proxy_url: proxy.map(|s| s.to_string()),
        }
    }
}

#[async_trait]
impl ScraperEngine for FireEngineTls {
    async fn scrape(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        // TLS Engine explicitly rejects screenshot requests
        if request.needs_screenshot {
            return Err(EngineError::Other(
                "FireEngineTls does not support screenshots".to_string(),
            ));
        }

        let start = Instant::now();

        // Determine proxy to use: request-level override or engine-level default
        let proxy_url = request.proxy.as_ref().or(self.proxy_url.as_ref());

        // Prepare custom headers with proxy info if configured
        let custom_headers = proxy_url.map(|proxy| {
            let mut headers = std::collections::HashMap::new();
            headers.insert("X-Proxy-URL".to_string(), proxy.clone());
            tracing::debug!("FireEngineTls using proxy: {}", proxy);
            headers
        });

        let req_body = FlareSolverrRequest::new(
            "request.get".to_string(),
            request.url.clone(),
            request.timeout.as_millis() as u64,
        )
        .with_headers(custom_headers.unwrap_or_default());

        let resp = self
            .client
            .post(&self.base_url)
            .json(&req_body)
            .send()
            .await
            .map_err(|e| EngineError::RequestFailed(e.to_string()))?;

        let flare_resp: FlareSolverrResponse = resp
            .json()
            .await
            .map_err(|e| EngineError::RequestFailed(e.to_string()))?;

        if flare_resp.status == "error" {
            return Err(EngineError::Other(format!(
                "Flaresolverr error: {}",
                flare_resp.message
            )));
        }

        let solution = flare_resp
            .solution
            .ok_or_else(|| EngineError::Other("Flaresolverr returned no solution".to_string()))?;

        // Convert headers - headers is already HashMap<String, String>
        let headers = solution.headers;

        let content_type = headers
            .get("content-type")
            .or_else(|| headers.get("Content-Type"))
            .cloned()
            .unwrap_or_else(|| "text/html".to_string());

        Ok(InternalScrapeResponse {
            status_code: solution.status,
            content: solution.response,
            screenshot: None,
            content_type,
            headers,
            response_time_ms: start.elapsed().as_millis() as u64,
        })
    }

    fn support_score(&self, request: &InternalScrapeRequest) -> u8 {
        // 如果明确请求使用 Fire Engine (TLS 模式)
        if request.use_fire_engine {
            return 95;
        }

        // 如果需要 TLS 指纹但不需要截图，TLS Engine 是最佳选择
        if request.needs_tls_fingerprint {
            return 90;
        }

        // 如果需要 JS，支持
        if request.needs_js {
            return 60;
        }

        // 如果有交互动作，支持但不是最佳
        if !request.actions.is_empty() {
            return 50;
        }

        // 成本低，速度快，默认优先级高
        70
    }

    fn name(&self) -> &'static str {
        "fire_engine_tls"
    }
}
