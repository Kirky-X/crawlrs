// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! [`SearxngTransport`] 的 reqwest 实现
//!
//! 出站复用全局 `reqwest::Client`（自带 SSRF 安全重定向策略与 IPv4 解析），
//! 本层只负责：统一 UA、逐请求超时、响应体大小上限、非 2xx 不解析。

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use crate::utils::http_client::{read_body_limited, BodyReadError};

use super::{SearxngError, SearxngTransport};

/// 响应体上限（8 MiB）：searx.space 数据集约 1-2 MiB，留足余量同时防 OOM
const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// 基于全局 reqwest 客户端的传输实现
#[derive(Clone)]
pub struct ReqwestSearxngTransport {
    client: Arc<reqwest::Client>,
    user_agent: String,
}

impl ReqwestSearxngTransport {
    /// 构造传输实现
    pub fn new(client: Arc<reqwest::Client>, user_agent: impl Into<String>) -> Self {
        Self {
            client,
            user_agent: user_agent.into(),
        }
    }
}

#[async_trait]
impl SearxngTransport for ReqwestSearxngTransport {
    async fn get_json(&self, url: &str, timeout: Duration) -> Result<(u16, Value), SearxngError> {
        let response = self
            .client
            .get(url)
            .timeout(timeout)
            .header(reqwest::header::USER_AGENT, &self.user_agent)
            .header(reqwest::header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    SearxngError::Timeout(timeout)
                } else {
                    SearxngError::Network(e.to_string())
                }
            })?;

        let status = response.status().as_u16();
        if !(200..300).contains(&status) {
            // 非 2xx 不解析响应体：错误分类由调用方按状态码决定
            return Ok((status, Value::Null));
        }

        let body = read_body_limited(response, MAX_RESPONSE_BYTES)
            .await
            .map_err(|e| match e {
                BodyReadError::LimitExceeded { max_bytes } => {
                    SearxngError::Parse(format!("response body exceeds {max_bytes} byte limit"))
                }
                BodyReadError::Network(e) => SearxngError::Network(e.to_string()),
            })?;

        let value: Value = serde_json::from_str(&body)
            .map_err(|e| SearxngError::Parse(format!("invalid json from {url}: {e}")))?;

        Ok((status, value))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_keeps_user_agent() {
        let transport = ReqwestSearxngTransport::new(
            Arc::new(reqwest::Client::new()),
            "crawlrs-test-agent/1.0",
        );
        assert_eq!(transport.user_agent, "crawlrs-test-agent/1.0");
    }

    #[test]
    fn test_new_accepts_owned_user_agent() {
        let transport =
            ReqwestSearxngTransport::new(Arc::new(reqwest::Client::new()), String::from("ua"));
        assert_eq!(transport.user_agent, "ua");
    }
}
