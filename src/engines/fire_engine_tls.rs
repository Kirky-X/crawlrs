// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::engines::traits::{EngineError, ScrapeRequest, ScrapeResponse, ScraperEngine};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Fire Engine (TLS) 实现
///
/// 专注于 TLS 指纹对抗，速度较快，但不支持截图和复杂的 JS 交互。
pub struct FireEngineTls {
    client: reqwest::Client,
    base_url: String,
}

#[derive(Serialize)]
struct FlaresolverrRequest {
    cmd: String,
    url: String,
    #[serde(rename = "maxTimeout")]
    max_timeout: u64,
}

#[derive(Deserialize, Debug)]
struct FlaresolverrResponse {
    status: String,
    message: String,
    solution: Option<FlaresolverrSolution>,
    #[serde(rename = "startTimestamp")]
    #[allow(dead_code)]
    start_timestamp: u64,
    #[serde(rename = "endTimestamp")]
    #[allow(dead_code)]
    end_timestamp: u64,
    #[allow(dead_code)]
    version: String,
}

#[derive(Deserialize, Debug)]
struct FlaresolverrSolution {
    #[allow(dead_code)]
    url: String,
    status: u16,
    headers: serde_json::Value,
    response: String,
    #[allow(dead_code)]
    cookies: Vec<serde_json::Value>,
    #[serde(rename = "userAgent")]
    #[allow(dead_code)]
    user_agent: String,
}

impl FireEngineTls {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: std::env::var("FIRE_ENGINE_TLS_URL")
                .or_else(|_| std::env::var("FIRE_ENGINE_URL"))
                .unwrap_or_else(|_| "http://localhost:8191/v1".to_string()),
        }
    }
}

impl Default for FireEngineTls {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ScraperEngine for FireEngineTls {
    async fn scrape(&self, request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
        // TLS Engine explicitly rejects screenshot requests
        if request.needs_screenshot {
            return Err(EngineError::Other(
                "FireEngineTls does not support screenshots".to_string(),
            ));
        }

        let start = Instant::now();

        let req_body = FlaresolverrRequest {
            cmd: "request.get".to_string(),
            url: request.url.clone(),
            max_timeout: request.timeout.as_millis() as u64,
        };

        let resp = self
            .client
            .post(&self.base_url)
            .json(&req_body)
            .send()
            .await
            .map_err(EngineError::RequestFailed)?;

        let flare_resp: FlaresolverrResponse =
            resp.json().await.map_err(EngineError::RequestFailed)?;

        if flare_resp.status == "error" {
            return Err(EngineError::Other(format!(
                "Flaresolverr error: {}",
                flare_resp.message
            )));
        }

        let solution = flare_resp
            .solution
            .ok_or_else(|| EngineError::Other("Flaresolverr returned no solution".to_string()))?;

        // Convert headers
        let mut headers = std::collections::HashMap::new();
        if let serde_json::Value::Object(map) = solution.headers {
            for (k, v) in map {
                if let Some(s) = v.as_str() {
                    headers.insert(k, s.to_string());
                }
            }
        }

        Ok(ScrapeResponse {
            status_code: solution.status,
            content: solution.response,
            screenshot: None,
            content_type: "text/html".to_string(),
            headers,
            response_time_ms: start.elapsed().as_millis() as u64,
        })
    }

    fn support_score(&self, request: &ScrapeRequest) -> u8 {
        // 如果需要截图，直接不支持
        if request.needs_screenshot {
            return 0;
        }

        // 如果明确请求使用 Fire Engine 或需要 TLS 指纹
        if request.use_fire_engine || request.needs_tls_fingerprint {
            return 100;
        }

        // 如果只是需要 JS，但不需要截图，可以支持
        if request.needs_js {
            return 80;
        }

        // 普通请求
        50
    }

    fn name(&self) -> &'static str {
        "fire_engine_tls"
    }
}

#[cfg(test)]
#[path = "fire_engine_tls_test.rs"]
mod tests;
