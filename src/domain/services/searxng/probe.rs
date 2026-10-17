// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 实例探活器
//!
//! 判活标准是**端到端**验证真正要用的能力：`GET {base}/search?q=..&format=json`
//! 返回 200 且 `results` 为数组。`/healthz` 之类只证明进程存活，大量公共实例
//! 关闭 JSON API，因此不作为判据。

use std::time::{Duration, Instant};

use crate::config::SearxngHealthSettings;
use crate::domain::repositories::searxng_instance_repository::SearxngInstance;

use super::{SearxngError, SearxngTransport};

/// 一次探测结果
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeOutcome {
    /// 是否存活（200 + `results` 数组）
    pub alive: bool,
    /// 存活时的时延（毫秒）
    pub latency_ms: Option<i64>,
    /// 失败原因（存活为 None）
    pub error: Option<String>,
}

impl ProbeOutcome {
    /// 存活结果
    fn alive(latency_ms: i64) -> Self {
        Self {
            alive: true,
            latency_ms: Some(latency_ms),
            error: None,
        }
    }

    /// 失败结果
    fn failed(error: impl ToString) -> Self {
        Self {
            alive: false,
            latency_ms: None,
            error: Some(error.to_string()),
        }
    }
}

/// 构造探测 URL：`{base}/search?q={probe_query}&format=json`
pub fn probe_url(base_url: &str, probe_query: &str) -> String {
    format!(
        "{}/search?q={}&format=json",
        base_url.trim_end_matches('/'),
        urlencoding::encode(probe_query)
    )
}

/// 探测一个实例。
///
/// 判活：HTTP 200 且响应体 `results` 为 JSON 数组；429/非 2xx/超时/解析失败均为失败，
/// 错误分类保留在 [`ProbeOutcome::error`]（写入登记簿 `last_error`）。
pub async fn probe_instance(
    transport: &dyn SearxngTransport,
    instance: &SearxngInstance,
    cfg: &SearxngHealthSettings,
) -> ProbeOutcome {
    let url = probe_url(&instance.url, &cfg.probe_query);
    let timeout = Duration::from_secs(cfg.probe_timeout_seconds);
    let started = Instant::now();

    match transport.get_json(&url, timeout).await {
        Ok((200, body)) => {
            if body.get("results").is_some_and(|r| r.is_array()) {
                let latency = started.elapsed().as_millis() as i64;
                ProbeOutcome::alive(latency)
            } else {
                ProbeOutcome::failed("probe response missing `results` array")
            }
        }
        Ok((status, _)) => ProbeOutcome::failed(SearxngError::from_status(status)),
        Err(e) => ProbeOutcome::failed(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::repositories::searxng_instance_repository::{
        SearxngInstanceSource, SearxngInstanceState,
    };
    use crate::domain::services::searxng::test_support::{Scripted, ScriptedTransport};
    use chrono::Utc;

    fn instance(url: &str) -> SearxngInstance {
        let now = Utc::now();
        SearxngInstance {
            url: url.to_string(),
            source: SearxngInstanceSource::Seed,
            state: SearxngInstanceState::Candidate,
            network_type: None,
            version: None,
            api_grade: None,
            http_grade: None,
            tls_grade: None,
            cdn_grade: None,
            uptime_day: None,
            uptime_month: None,
            latency_ms: None,
            consecutive_failures: 0,
            last_success_at: None,
            last_failure_at: None,
            last_error: None,
            dataset_miss_count: 0,
            created_at: now,
            updated_at: now,
        }
    }

    fn cfg() -> SearxngHealthSettings {
        SearxngHealthSettings::default()
    }

    #[tokio::test]
    async fn test_probe_alive_on_200_with_results_array() {
        let transport = ScriptedTransport::new(vec![Scripted::Json {
            status: 200,
            body: serde_json::json!({ "results": [{ "url": "https://example.com" }] }),
        }]);
        let outcome = probe_instance(&transport, &instance("https://searx.example"), &cfg()).await;
        assert!(outcome.alive);
        assert!(outcome.latency_ms.is_some_and(|l| l >= 0));
        assert!(outcome.error.is_none());
        assert_eq!(
            transport.received_urls(),
            vec!["https://searx.example/search?q=crawlrs&format=json".to_string()]
        );
    }

    #[tokio::test]
    async fn test_probe_empty_results_array_is_alive() {
        let transport = ScriptedTransport::new(vec![Scripted::Json {
            status: 200,
            body: serde_json::json!({ "results": [] }),
        }]);
        let outcome = probe_instance(&transport, &instance("https://searx.example"), &cfg()).await;
        assert!(
            outcome.alive,
            "an empty result list still proves the JSON API works"
        );
    }

    #[tokio::test]
    async fn test_probe_fails_when_results_missing() {
        let transport = ScriptedTransport::new(vec![Scripted::Json {
            status: 200,
            body: serde_json::json!({ "answers": [] }),
        }]);
        let outcome = probe_instance(&transport, &instance("https://searx.example"), &cfg()).await;
        assert!(!outcome.alive);
        assert!(outcome
            .error
            .as_deref()
            .is_some_and(|e| e.contains("results")));
    }

    #[tokio::test]
    async fn test_probe_fails_on_429() {
        let transport = ScriptedTransport::new(vec![Scripted::Json {
            status: 429,
            body: serde_json::Value::Null,
        }]);
        let outcome = probe_instance(&transport, &instance("https://searx.example"), &cfg()).await;
        assert!(!outcome.alive);
        let err = outcome.error.expect("error");
        assert!(err.contains("rate limited"), "got: {err}");
    }

    #[tokio::test]
    async fn test_probe_fails_on_non_2xx() {
        let transport = ScriptedTransport::new(vec![Scripted::Json {
            status: 503,
            body: serde_json::Value::Null,
        }]);
        let outcome = probe_instance(&transport, &instance("https://searx.example"), &cfg()).await;
        assert!(!outcome.alive);
        assert!(outcome.error.expect("error").contains("503"));
    }

    #[tokio::test]
    async fn test_probe_maps_timeout() {
        let transport = ScriptedTransport::new(vec![Scripted::Timeout]);
        let outcome = probe_instance(&transport, &instance("https://searx.example"), &cfg()).await;
        assert!(!outcome.alive);
        let err = outcome.error.expect("error");
        assert!(err.contains("timed out"), "got: {err}");
    }

    #[tokio::test]
    async fn test_probe_url_encodes_query_and_strips_trailing_slash() {
        assert_eq!(
            probe_url("https://searx.example/", "hello world"),
            "https://searx.example/search?q=hello%20world&format=json"
        );
    }
}
