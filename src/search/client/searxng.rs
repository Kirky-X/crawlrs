// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! SearXNG 搜索引擎 — 公共实例 JSON API 适配器
//!
//! 引擎不直连实例数据库与 searx.space 数据集：实例经 [`SearxngInstanceService`]
//! 端口轮转获取，请求结果同样回填给该端口（熔断记账 + 逐出晋升）。单次 `search()`
//! 最多消费 `max_failover_attempts` 个不同实例。

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;

use crate::domain::services::searxng::{
    InstanceOutcome, PoolMember, SearxngError, SearxngInstanceService, SearxngTransport,
};
use crate::search::engine_trait::{SearchEngine, SearchRequest};
use crate::search::error::SearchError;
use crate::search::response::{Response, ResponseItem};
use crate::search::types::{EngineHealth, SearchEngineType};

/// 公共实例的默认每页条数（SearXNG `/search` 分页口径）
const RESULTS_PER_PAGE: u32 = 20;

/// SearXNG 搜索引擎
pub struct SearxngSearchEngine {
    service: Arc<dyn SearxngInstanceService>,
    transport: Arc<dyn SearxngTransport>,
    /// 单次请求超时（`search.timeout_seconds`）
    timeout: Duration,
    /// 单次 `search()` 最多消费的不同实例数
    max_failover_attempts: u32,
}

impl SearxngSearchEngine {
    /// 构造引擎
    pub fn new(
        service: Arc<dyn SearxngInstanceService>,
        transport: Arc<dyn SearxngTransport>,
        timeout: Duration,
        max_failover_attempts: u32,
    ) -> Self {
        Self {
            service,
            transport,
            timeout,
            max_failover_attempts: max_failover_attempts.max(1),
        }
    }

    /// 构造实例搜索 URL：`{base}/search?q=..&format=json&pageno=N[&language=..]`
    fn build_url(base: &str, request: &SearchRequest) -> String {
        let page = request.offset / RESULTS_PER_PAGE + 1;
        let mut url = format!(
            "{}/search?q={}&format=json&pageno={page}",
            base.trim_end_matches('/'),
            urlencoding::encode(&request.query)
        );
        if let Some(lang) = request.lang.as_deref().filter(|l| !l.trim().is_empty()) {
            url.push_str(&format!("&language={}", urlencoding::encode(lang)));
        }
        url
    }

    /// 取一个实例：空池即 `NoEngineAvailable`
    fn acquire(&self, tried: &[String]) -> Result<Arc<PoolMember>, SearchError> {
        self.service
            .acquire(tried)
            .ok_or(SearchError::NoEngineAvailable)
    }

    /// 解析响应体并按请求的 offset/limit 切片
    fn parse_items(
        &self,
        body: &serde_json::Value,
        offset: u32,
        limit: u32,
    ) -> Result<(Vec<ResponseItem>, Option<u64>), SearchError> {
        let response: SearxngResponse = serde_json::from_value(body.clone())
            .map_err(|e| SearchError::Parse(format!("searxng response parse failed: {e}")))?;

        let skip = (offset % RESULTS_PER_PAGE) as usize;
        let items: Vec<ResponseItem> = response
            .results
            .into_iter()
            .skip(skip)
            .take(limit as usize)
            .map(|raw| ResponseItem {
                title: raw.title.unwrap_or_default(),
                url: raw.url,
                description: raw.content.unwrap_or_default(),
                engine: SearchEngineType::Searxng,
            })
            .collect();

        Ok((items, response.number_of_results))
    }
}

/// 构造一个已 elapsed 的超时错误
///
/// `SearchError::Timeout` 持有 `tokio::time::error::Elapsed`（不可直接构造），
/// 用零时长超时 + 永不完成的 future 取一个等价实例。
async fn elapsed_error() -> tokio::time::error::Elapsed {
    tokio::time::timeout(Duration::ZERO, std::future::pending::<()>())
        .await
        .expect_err("pending future never completes")
}

/// `SearxngError` → `SearchError`（保留既有分类语义）
async fn map_transport_error(error: SearxngError) -> SearchError {
    match error {
        SearxngError::Timeout(_) => SearchError::Timeout(elapsed_error().await),
        SearxngError::RateLimited => SearchError::RateLimited("searxng".to_string()),
        SearxngError::BadHttpStatus(code) => {
            SearchError::BadHttpStatus("searxng".to_string(), code)
        }
        SearxngError::Parse(message) => SearchError::Parse(message),
        SearxngError::Network(message) => SearchError::EngineFailed(format!("searxng: {message}")),
        SearxngError::NoInstanceAvailable => SearchError::NoEngineAvailable,
        SearxngError::Disabled => SearchError::EngineFailed("searxng disabled".to_string()),
        SearxngError::DatasetFetch(message) => {
            SearchError::EngineFailed(format!("searxng dataset: {message}"))
        }
        SearxngError::Repository(message) => {
            SearchError::EngineFailed(format!("searxng registry: {message}"))
        }
    }
}

#[async_trait]
impl SearchEngine for SearxngSearchEngine {
    fn name(&self) -> &'static str {
        "SearxNG"
    }

    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Searxng
    }

    fn health(&self) -> EngineHealth {
        self.service.health()
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        let mut tried: Vec<String> = Vec::new();
        let mut last_error: Option<SearchError> = None;

        for _ in 0..self.max_failover_attempts {
            let member = match self.acquire(&tried) {
                Ok(member) => member,
                Err(e) => {
                    // 首次取不到实例 = 池空；已试过其它实例则返回最后一次失败原因
                    return Err(last_error.unwrap_or(e));
                }
            };

            let url = Self::build_url(member.url(), request);
            let started = std::time::Instant::now();
            match self.transport.get_json(&url, self.timeout).await {
                Ok((200, body)) => match self.parse_items(&body, request.offset, request.limit) {
                    Ok((items, total)) => {
                        self.service
                            .report(
                                member.url(),
                                InstanceOutcome::Success {
                                    latency_ms: started.elapsed().as_millis() as i64,
                                },
                            )
                            .await;
                        return Ok(Response {
                            items,
                            total_results: total,
                            engine: SearchEngineType::Searxng,
                        });
                    }
                    Err(e) => {
                        self.service
                            .report(
                                member.url(),
                                InstanceOutcome::Failure {
                                    error: SearxngError::Parse(e.to_string()),
                                },
                            )
                            .await;
                        last_error = Some(e);
                    }
                },
                Ok((status, _)) => {
                    let error = SearxngError::from_status(status);
                    self.service
                        .report(
                            member.url(),
                            InstanceOutcome::Failure {
                                error: error.clone(),
                            },
                        )
                        .await;
                    last_error = Some(map_transport_error(error).await);
                }
                Err(error) => {
                    self.service
                        .report(
                            member.url(),
                            InstanceOutcome::Failure {
                                error: error.clone(),
                            },
                        )
                        .await;
                    last_error = Some(map_transport_error(error).await);
                }
            }

            tried.push(member.url().to_string());
        }

        Err(last_error.unwrap_or(SearchError::NoEngineAvailable))
    }
}

/// SearXNG JSON 响应（宽容解析）
#[derive(Debug, Deserialize)]
struct SearxngResponse {
    #[serde(default)]
    results: Vec<SearxngRawResult>,
    /// 实例报告的结果总数（缺失时为 None）
    #[serde(default)]
    number_of_results: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct SearxngRawResult {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    url: String,
    #[serde(default)]
    content: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::repositories::searxng_instance_repository::SearxngInstanceState;
    use crate::domain::services::searxng::test_support::{
        shared_repo, test_settings, InMemorySearxngRepo, Scripted, ScriptedTransport,
    };
    use crate::domain::services::searxng::SearxngInstanceServiceImpl;

    fn alive_body() -> serde_json::Value {
        serde_json::json!({ "results": [] })
    }

    fn search_body() -> serde_json::Value {
        serde_json::json!({
            "number_of_results": 1234,
            "results": [
                { "url": "https://r0.example", "title": "T0", "content": "C0" },
                { "url": "https://r1.example", "title": "T1", "content": "C1" },
                { "url": "https://r2.example", "title": "T2", "content": "C2" },
                { "url": "https://r3.example", "title": "T3", "content": "C3" },
                { "url": "https://r4.example", "title": "T4", "content": "C4" },
                { "url": "https://r5.example", "title": "T5", "content": "C5" },
                { "url": "https://r6.example", "title": "T6", "content": "C6" },
                { "url": "https://r7.example", "title": "T7", "content": "C7" }
            ]
        })
    }

    struct Harness {
        repo: Arc<InMemorySearxngRepo>,
        transport: Arc<ScriptedTransport>,
        service: Arc<SearxngInstanceServiceImpl>,
        engine: SearxngSearchEngine,
    }

    /// 布线：候选 → bootstrap 探活入池 → 引擎消费后续脚本文本
    async fn harness(
        instances: &[(&str, f64)],
        script: Vec<Scripted>,
        max_failover_attempts: u32,
    ) -> Harness {
        let repo = shared_repo();
        for (url, uptime) in instances {
            let mut instance =
                InMemorySearxngRepo::dataset_instance(url, SearxngInstanceState::Candidate);
            instance.uptime_day = Some(*uptime);
            repo.insert_raw(instance);
        }

        let mut all_script: Vec<Scripted> = instances
            .iter()
            .map(|_| Scripted::Json {
                status: 200,
                body: alive_body(),
            })
            .collect();
        all_script.extend(script);

        let transport = Arc::new(ScriptedTransport::new(all_script));
        let service = Arc::new(SearxngInstanceServiceImpl::new(
            repo.clone(),
            transport.clone(),
            test_settings(),
        ));
        service.bootstrap().await;

        let engine = SearxngSearchEngine::new(
            service.clone(),
            transport.clone(),
            Duration::from_secs(5),
            max_failover_attempts,
        );
        Harness {
            repo,
            transport,
            service,
            engine,
        }
    }

    #[test]
    fn test_engine_identity() {
        let engine = SearxngSearchEngine::new(
            Arc::new(SearxngInstanceServiceImpl::new(
                shared_repo(),
                Arc::new(ScriptedTransport::new(vec![])),
                test_settings(),
            )),
            Arc::new(ScriptedTransport::new(vec![])),
            Duration::from_secs(1),
            2,
        );
        assert_eq!(engine.name(), "SearxNG");
        assert_eq!(engine.engine_type(), SearchEngineType::Searxng);
    }

    #[test]
    fn test_build_url_has_format_and_page() {
        let request = SearchRequest::new("rust web").with_offset(20).with_limit(5);
        let url = SearxngSearchEngine::build_url("https://searx.example/", &request);
        assert_eq!(
            url,
            "https://searx.example/search?q=rust%20web&format=json&pageno=2"
        );
    }

    #[test]
    fn test_build_url_appends_language_only_when_provided() {
        let base = SearchRequest::new("q");
        assert!(!SearxngSearchEngine::build_url("https://a.example", &base).contains("language="));

        let with_lang = SearchRequest::new("q").with_lang("zh-CN");
        assert!(
            SearxngSearchEngine::build_url("https://a.example", &with_lang)
                .contains("language=zh-CN")
        );

        let blank_lang = SearchRequest::new("q").with_lang("   ");
        assert!(
            !SearxngSearchEngine::build_url("https://a.example", &blank_lang).contains("language=")
        );
    }

    #[tokio::test]
    async fn test_search_maps_and_slices_results() {
        let h = harness(
            &[("https://a.example", 99.0)],
            vec![Scripted::Json {
                status: 200,
                body: search_body(),
            }],
            2,
        )
        .await;

        let request = SearchRequest::new("q").with_offset(5).with_limit(3);
        let response = h.engine.search(&request).await.expect("search succeeds");

        assert_eq!(response.engine, SearchEngineType::Searxng);
        assert_eq!(response.total_results, Some(1234));
        assert_eq!(response.items.len(), 3);
        assert_eq!(response.items[0].title, "T5");
        assert_eq!(response.items[0].url, "https://r5.example");
        assert_eq!(response.items[0].description, "C5");
        assert_eq!(response.items[0].engine, SearchEngineType::Searxng);
        assert_eq!(response.items[2].url, "https://r7.example");

        let urls = h.transport.received_urls();
        let last = urls.last().expect("search request recorded");
        assert!(last.contains("format=json"));
        assert!(last.contains("pageno=1"));
    }

    #[tokio::test]
    async fn test_search_passes_language_parameter() {
        let h = harness(
            &[("https://a.example", 99.0)],
            vec![Scripted::Json {
                status: 200,
                body: search_body(),
            }],
            2,
        )
        .await;

        let request = SearchRequest::new("q").with_lang("zh");
        let response = h.engine.search(&request).await.expect("search succeeds");
        assert_eq!(response.items.len(), 8);
        let urls = h.transport.received_urls();
        assert!(urls.last().expect("url").contains("language=zh"));
    }

    #[tokio::test]
    async fn test_failover_to_second_instance_reports_both() {
        let h = harness(
            &[("https://a.example", 99.0), ("https://b.example", 98.0)],
            vec![
                Scripted::Json {
                    status: 500,
                    body: serde_json::Value::Null,
                },
                Scripted::Json {
                    status: 200,
                    body: search_body(),
                },
            ],
            2,
        )
        .await;

        let response = h
            .engine
            .search(&SearchRequest::new("q").with_limit(2))
            .await
            .expect("second instance succeeds");

        assert_eq!(response.items.len(), 2);

        // 两个实例各被使用一次
        let all_urls = h.transport.received_urls();
        let search_urls: Vec<&String> = all_urls.iter().filter(|u| u.contains("pageno=")).collect();
        assert_eq!(search_urls.len(), 2);
        assert!(
            search_urls[0].contains("https://a.example"),
            "first attempt: {:?}",
            search_urls
        );
        assert!(
            search_urls[1].contains("https://b.example"),
            "failover attempt: {:?}",
            search_urls
        );

        // report 回填：A 记失败、B 记成功时延
        let a = h.repo.get("https://a.example").expect("a row");
        assert_eq!(a.consecutive_failures, 1);
        assert!(a.last_error.is_some());
        let b = h.repo.get("https://b.example").expect("b row");
        assert_eq!(b.consecutive_failures, 0);
        assert!(b.latency_ms.is_some());
    }

    #[tokio::test]
    async fn test_all_instances_rate_limited_returns_rate_limited() {
        let h = harness(
            &[("https://a.example", 99.0), ("https://b.example", 98.0)],
            vec![
                Scripted::Json {
                    status: 429,
                    body: serde_json::Value::Null,
                },
                Scripted::Json {
                    status: 429,
                    body: serde_json::Value::Null,
                },
            ],
            2,
        )
        .await;

        let err = h
            .engine
            .search(&SearchRequest::new("q"))
            .await
            .expect_err("all instances rate limited");
        match err {
            SearchError::RateLimited(engine) => assert_eq!(engine, "searxng"),
            other => panic!("expected RateLimited, got {other:?}"),
        }

        let attempts = h
            .transport
            .received_urls()
            .iter()
            .filter(|u| u.contains("pageno="))
            .count();
        assert_eq!(attempts, 2, "must not exceed max_failover_attempts");
    }

    #[tokio::test]
    async fn test_empty_pool_returns_no_engine_available_without_network() {
        let repo = shared_repo();
        let transport = Arc::new(ScriptedTransport::new(vec![]));
        let service = Arc::new(SearxngInstanceServiceImpl::new(
            repo,
            transport.clone(),
            test_settings(),
        ));
        service.mark_bootstrap_done();
        let engine =
            SearxngSearchEngine::new(service, transport.clone(), Duration::from_secs(1), 2);

        let err = engine
            .search(&SearchRequest::new("q"))
            .await
            .expect_err("empty pool");
        assert!(matches!(err, SearchError::NoEngineAvailable));
        assert_eq!(
            transport.calls.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "empty pool must not issue any network call"
        );
    }

    #[tokio::test]
    async fn test_bad_json_maps_to_parse_error() {
        let h = harness(
            &[("https://a.example", 99.0)],
            vec![Scripted::Json {
                status: 200,
                body: serde_json::json!("not-an-object"),
            }],
            1,
        )
        .await;

        let err = h
            .engine
            .search(&SearchRequest::new("q"))
            .await
            .expect_err("parse failure");
        assert!(matches!(err, SearchError::Parse(_)), "got: {err:?}");
    }

    #[tokio::test]
    async fn test_timeout_maps_to_timeout_error() {
        let h = harness(&[("https://a.example", 99.0)], vec![Scripted::Timeout], 1).await;

        let err = h
            .engine
            .search(&SearchRequest::new("q"))
            .await
            .expect_err("timeout");
        assert!(matches!(err, SearchError::Timeout(_)), "got: {err:?}");
    }

    #[tokio::test]
    async fn test_health_delegates_to_service() {
        let h = harness(&[("https://a.example", 99.0)], vec![], 2).await;
        assert_eq!(h.engine.health(), EngineHealth::Degraded);
        assert_eq!(h.service.health(), EngineHealth::Degraded);
    }
}
