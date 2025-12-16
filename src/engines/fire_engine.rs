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
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Fire Engine 实现 (基于 Flaresolverr)
///
/// Fire Engine 是一个高级抓取引擎，支持 TLS 指纹对抗和 CDP 控制。
/// 这里使用 Flaresolverr 作为后端服务。
pub struct FireEngine {
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

impl FireEngine {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: std::env::var("FIRE_ENGINE_URL")
                .unwrap_or_else(|_| "http://localhost:8191/v1".to_string()),
        }
    }
}

impl Default for FireEngine {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ScraperEngine for FireEngine {
    async fn scrape(&self, request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
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

        // Handle screenshot: Flaresolverr doesn't natively return screenshot in the basic request.get command usually.
        // But for this task we assume it returns HTML.
        // If we really need screenshot, we might need a different command or Flaresolverr doesn't support it directly in this mode.
        // Assuming Fire Engine (Flaresolverr) is primarily for HTML/TLS.
        // If screenshot is requested, this engine might not be the best unless we extend it.
        // But the trait requires we return a ScrapeResponse.

        let screenshot = None; // Flaresolverr basic API doesn't return screenshot

        Ok(ScrapeResponse {
            status_code: solution.status,
            content: solution.response,
            screenshot,
            content_type: "text/html".to_string(), // Assumed
            headers,
            response_time_ms: start.elapsed().as_millis() as u64,
        })
    }

    fn support_score(&self, request: &ScrapeRequest) -> u8 {
        // 如果明确请求使用 Fire Engine 或需要 TLS 指纹，给予最高优先级
        if request.use_fire_engine || request.needs_tls_fingerprint {
            return 100;
        }

        // 如果需要 JS，Fire Engine (Flaresolverr) 可以处理
        if request.needs_js {
            return 90;
        }

        // 如果需要截图，Flaresolverr 标准API不支持，所以给低分
        if request.needs_screenshot {
            return 10;
        }

        // 对于普通请求，Fire Engine 成本较高，优先级较低
        50
    }

    fn name(&self) -> &'static str {
        "fire_engine"
    }
}
