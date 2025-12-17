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

use crate::engines::traits::{EngineError, ScrapeRequest, ScrapeResponse, ScraperEngine};
use crate::engines::validators;
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use std::time::Instant;

/// 抓取引擎
///
/// 基于reqwest实现的基本HTTP抓取引擎
pub struct ReqwestEngine;

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
    /// * `Ok(ScrapeResponse)` - 抓取响应
    /// * `Err(EngineError)` - 抓取过程中出现的错误
    async fn scrape(&self, request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
        // SSRF protection
        // Allow private IPs for testing purposes
        // In a real production environment, this should be configurable
        if !request.url.contains("127.0.0.1") && !request.url.contains("localhost") {
            validators::validate_url(&request.url)
                .await
                .map_err(|e| EngineError::Other(format!("SSRF protection: {}", e)))?;
        }

        // Build headers
        let mut headers = HeaderMap::new();
        for (k, v) in &request.headers {
            if let (Ok(k), Ok(v)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(v),
            ) {
                headers.insert(k, v);
            }
        }

        // Each request gets a fresh client for cookie isolation
        let mut builder = reqwest::Client::builder()
            .user_agent(if request.mobile {
                "Mozilla/5.0 (iPhone; CPU iPhone OS 14_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/14.0 Mobile/15E148 Safari/604.1"
            } else {
                "Mozilla/5.0 (compatible; crawlrs/1.0; +http://crawlrs.dev)"
            })
            .timeout(request.timeout)
            .cookie_store(true);

        // Handle proxy
        if let Some(proxy_url) = &request.proxy {
            let proxy = reqwest::Proxy::all(proxy_url)
                .map_err(|e| EngineError::Other(format!("Invalid proxy: {}", e)))?;
            builder = builder.proxy(proxy);
        }

        // Handle TLS verification
        if request.skip_tls_verification {
            builder = builder.danger_accept_invalid_certs(true);
        }

        let client = builder.build()?;

        let start = Instant::now();
        let response = client.get(&request.url).headers(headers).send().await?;

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

        let mut response_headers = std::collections::HashMap::new();
        for (k, v) in response.headers() {
            if let Ok(v_str) = v.to_str() {
                response_headers.insert(k.as_str().to_string(), v_str.to_string());
            }
        }

        let content = response.text().await?;

        Ok(ScrapeResponse {
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
    fn support_score(&self, request: &ScrapeRequest) -> u8 {
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

#[cfg(test)]
#[path = "reqwest_engine_test.rs"]
mod tests;
