// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::engines::client::flaresolverr_types::{
    FlareSolverrRequest, FlareSolverrResponse, FlareSolverrSolution,
};
use crate::engines::traits::{EngineError, ScrapeRequest, ScrapeResponse, ScraperEngine};
use async_trait::async_trait;
use std::time::Instant;

/// Fire Engine (CDP) 实现
///
/// 支持完整的浏览器自动化，包括 JS 渲染、截图和 TLS 指纹对抗。
/// 成本较高，速度较慢。
pub struct FireEngineCdp {
    client: reqwest::Client,
    base_url: String,
    /// 代理配置
    proxy_url: Option<String>,
}

impl FireEngineCdp {
    /// 创建新的 FireEngineCdp 实例
    pub fn new() -> Self {
        let client = reqwest::Client::builder().build().unwrap_or_default();
        Self {
            client,
            base_url: String::new(),
            proxy_url: None,
        }
    }

    /// 创建带有代理配置的 FireEngineCdp 实例
    pub fn with_proxy(proxy: &str) -> Self {
        let proxy_result = reqwest::Proxy::https(proxy);
        let client_builder = reqwest::Client::builder();
        let client_builder = match proxy_result {
            Ok(p) => client_builder.proxy(p),
            Err(_) => client_builder,
        };
        let client = client_builder.build().unwrap_or_default();
        Self {
            client,
            base_url: String::new(),
            proxy_url: Some(proxy.to_string()),
        }
    }

    /// 创建带有 base URL 和代理配置的实例
    pub fn with_url_and_proxy(base_url: &str, proxy: Option<&str>) -> Self {
        let proxy_result = proxy.map(|p| reqwest::Proxy::https(p)).transpose();
        let client_builder = reqwest::Client::builder();
        let client_builder = match proxy_result {
            Ok(Some(p)) => client_builder.proxy(p),
            _ => client_builder,
        };
        let client = client_builder.build().unwrap_or_default();
        Self {
            client,
            base_url: base_url.to_string(),
            proxy_url: proxy.map(|s| s.to_string()),
        }
    }
}

impl Default for FireEngineCdp {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ScraperEngine for FireEngineCdp {
    async fn scrape(&self, request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
        let start = Instant::now();

        // Determine proxy to use: request-level override or engine-level default
        let proxy_url = request.proxy.as_ref().or(self.proxy_url.as_ref());

        // Prepare custom headers with proxy info if configured
        let custom_headers = proxy_url.map(|proxy| {
            let mut headers = std::collections::HashMap::new();
            headers.insert("X-Proxy-URL".to_string(), proxy.clone());
            tracing::debug!("FireEngineCdp using proxy: {}", proxy);
            headers
        });

        let req_body = FlareSolverrRequest::new(
            "request.get".to_string(),
            request.url.clone(),
            request.timeout.as_millis() as u64,
        )
        .with_screenshot(request.needs_screenshot)
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

        Ok(ScrapeResponse {
            status_code: solution.status,
            content: solution.response,
            screenshot: None, // FlareSolverr doesn't return screenshots in this struct
            content_type,
            headers,
            response_time_ms: start.elapsed().as_millis() as u64,
        })
    }

    fn support_score(&self, request: &ScrapeRequest) -> u8 {
        // 如果需要 TLS 指纹且需要截图，这是最佳选择
        if request.needs_tls_fingerprint && request.needs_screenshot {
            return 100;
        }

        // 如果明确请求使用 Fire Engine
        if request.use_fire_engine {
            return 100;
        }

        // 如果需要截图，但不需要 TLS，Playwright 可能更好，但这个也能做
        if request.needs_screenshot {
            return 80;
        }

        // 如果需要 JS，支持
        if request.needs_js {
            return 90;
        }

        // 如果有交互动作，支持
        if !request.actions.is_empty() {
            return 90;
        }

        // 成本较高，默认优先级低
        40
    }

    fn name(&self) -> &'static str {
        "fire_engine_cdp"
    }
}
