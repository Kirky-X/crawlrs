// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::engines::client::reqwest::ReqwestEngine;
use crate::engines::engine_client::{EngineClient, ScrapeOptions, ScrapeRequest};
use crate::engines::router::{EngineRouter, EngineRouterTrait};
use crate::search::engine_trait::{SearchEngine, SearchRequest};
use anyhow::Result;
use std::sync::Arc;

#[derive(Debug, Default, Clone)]
pub struct TestResult {
    pub total: usize,
    pub accessible: usize,
    pub inaccessible: usize,
}

pub async fn run_engine_test_with_output<E: SearchEngine>(
    name: &str,
    engine: E,
    query: Option<&str>,
    timeout_secs: u64,
    limit: Option<u32>,
) -> Result<TestResult> {
    use tokio::time::timeout;

    let start_time = std::time::Instant::now();
    let query_str = query.unwrap_or("test query");

    let request = SearchRequest::new(query_str).with_limit(limit.unwrap_or(10));

    let result = timeout(
        std::time::Duration::from_secs(timeout_secs),
        engine.search(&request),
    )
    .await;

    let elapsed = start_time.elapsed();

    let engine_client = build_test_engine_client();

    match result {
        Ok(Ok(response)) => {
            let total = response.items.len();
            let mut accessible = 0;
            let mut inaccessible = 0;

            for entry in response.items {
                let url = &entry.url;
                let is_accessible = match engine_client.as_ref() {
                    Some(client) => check_url_accessible(client, url).await,
                    None => false,
                };
                if is_accessible {
                    accessible += 1;
                } else {
                    inaccessible += 1;
                }
            }

            log::info!(
                "[{}] Search completed in {:.2}s",
                name,
                elapsed.as_secs_f64()
            );
            log::info!("[{}] Total results: {}", name, total);

            Ok(TestResult {
                total,
                accessible,
                inaccessible,
            })
        }
        Ok(Err(e)) => {
            log::error!("[{}] Search failed: {:?}", name, e);
            Err(e.into())
        }
        Err(_) => {
            log::error!("[{}] Search timed out after {}s", name, timeout_secs);
            Err(anyhow::anyhow!("Search timed out"))
        }
    }
}

fn build_test_engine_client() -> Option<Arc<EngineClient>> {
    let http_client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
        .ok()?;
    let reqwest_engine = ReqwestEngine::new(Arc::new(http_client));
    let router: Arc<dyn EngineRouterTrait> =
        Arc::new(EngineRouter::new(vec![Arc::new(reqwest_engine)]));
    Some(Arc::new(EngineClient::with_router(router)))
}

async fn check_url_accessible(engine_client: &EngineClient, url: &str) -> bool {
    let options = ScrapeOptions::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build();
    let request = ScrapeRequest::new(url).with_options(options);
    engine_client
        .scrape(&request)
        .await
        .map(|response| response.is_success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== TestResult::default tests ==========

    #[test]
    fn test_test_result_default_total_is_zero() {
        let r = TestResult::default();
        assert_eq!(r.total, 0, "default total should be 0");
    }

    #[test]
    fn test_test_result_default_accessible_is_zero() {
        let r = TestResult::default();
        assert_eq!(r.accessible, 0, "default accessible should be 0");
    }

    #[test]
    fn test_test_result_default_inaccessible_is_zero() {
        let r = TestResult::default();
        assert_eq!(r.inaccessible, 0, "default inaccessible should be 0");
    }

    #[test]
    fn test_test_result_default_all_fields_zero() {
        let r = TestResult::default();
        assert_eq!((r.total, r.accessible, r.inaccessible), (0, 0, 0));
    }

    // ========== TestResult construction tests ==========

    #[test]
    fn test_test_result_construction_all_accessible() {
        let r = TestResult {
            total: 10,
            accessible: 10,
            inaccessible: 0,
        };
        assert_eq!(r.total, 10);
        assert_eq!(r.accessible, 10);
        assert_eq!(r.inaccessible, 0);
    }

    #[test]
    fn test_test_result_construction_mixed() {
        let r = TestResult {
            total: 10,
            accessible: 7,
            inaccessible: 3,
        };
        assert_eq!(r.total, 10);
        assert_eq!(r.accessible, 7);
        assert_eq!(r.inaccessible, 3);
    }

    #[test]
    fn test_test_result_construction_all_inaccessible() {
        let r = TestResult {
            total: 5,
            accessible: 0,
            inaccessible: 5,
        };
        assert_eq!(r.accessible, 0);
        assert_eq!(r.inaccessible, 5);
    }

    // ========== TestResult Clone tests ==========

    #[test]
    fn test_test_result_clone_preserves_fields() {
        let original = TestResult {
            total: 8,
            accessible: 5,
            inaccessible: 3,
        };
        let cloned = original.clone();
        assert_eq!(cloned.total, original.total);
        assert_eq!(cloned.accessible, original.accessible);
        assert_eq!(cloned.inaccessible, original.inaccessible);
    }

    // ========== TestResult Debug tests ==========

    #[test]
    fn test_test_result_debug_contains_struct_name() {
        let r = TestResult::default();
        let dbg = format!("{:?}", r);
        assert!(
            dbg.contains("TestResult"),
            "Debug should contain struct name"
        );
    }

    #[test]
    fn test_test_result_debug_contains_field_values() {
        let r = TestResult {
            total: 42,
            accessible: 40,
            inaccessible: 2,
        };
        let dbg = format!("{:?}", r);
        assert!(dbg.contains("42"), "Debug should contain total value");
    }

    // ========== TestResult field-by-field comparison tests ==========

    #[test]
    fn test_test_result_same_values_match_field_by_field() {
        let a = TestResult {
            total: 3,
            accessible: 2,
            inaccessible: 1,
        };
        let b = TestResult {
            total: 3,
            accessible: 2,
            inaccessible: 1,
        };
        // TestResult does not derive PartialEq, so compare field by field.
        assert_eq!(a.total, b.total);
        assert_eq!(a.accessible, b.accessible);
        assert_eq!(a.inaccessible, b.inaccessible);
    }

    #[test]
    fn test_test_result_different_total_differs_field_by_field() {
        let a = TestResult {
            total: 3,
            accessible: 2,
            inaccessible: 1,
        };
        let b = TestResult {
            total: 4,
            accessible: 2,
            inaccessible: 1,
        };
        assert_ne!(a.total, b.total, "different total should differ");
    }

    // ========== TestResult edge cases ==========

    #[test]
    fn test_test_result_zero_total() {
        let r = TestResult {
            total: 0,
            accessible: 0,
            inaccessible: 0,
        };
        assert_eq!(r.total + r.accessible + r.inaccessible, 0);
    }

    #[test]
    fn test_test_result_large_values() {
        let r = TestResult {
            total: usize::MAX,
            accessible: usize::MAX / 2,
            inaccessible: usize::MAX / 2,
        };
        assert_eq!(r.total, usize::MAX);
    }

    #[test]
    fn test_test_result_default_is_zero_struct() {
        let default = TestResult::default();
        let explicit = TestResult {
            total: 0,
            accessible: 0,
            inaccessible: 0,
        };
        assert_eq!(default.total, explicit.total);
        assert_eq!(default.accessible, explicit.accessible);
        assert_eq!(default.inaccessible, explicit.inaccessible);
    }

    #[test]
    fn test_test_result_debug_contains_all_field_names() {
        let r = TestResult {
            total: 1,
            accessible: 2,
            inaccessible: 3,
        };
        let dbg = format!("{:?}", r);
        assert!(dbg.contains("total"), "Debug should contain 'total' field");
        assert!(
            dbg.contains("accessible"),
            "Debug should contain 'accessible' field"
        );
        assert!(
            dbg.contains("inaccessible"),
            "Debug should contain 'inaccessible' field"
        );
    }

    #[test]
    fn test_test_result_clone_independent_after_clone() {
        let original = TestResult {
            total: 5,
            accessible: 3,
            inaccessible: 2,
        };
        let cloned = original.clone();
        // cloned should have same values
        assert_eq!(cloned.total, 5);
        assert_eq!(cloned.accessible, 3);
        assert_eq!(cloned.inaccessible, 2);
    }

    #[test]
    fn test_test_result_debug_format_consistency() {
        let r = TestResult::default();
        let dbg = format!("{:?}", r);
        let dbg_alt = format!("{:#?}", r);
        // Both should contain the struct name
        assert!(dbg.contains("TestResult"));
        assert!(dbg_alt.contains("TestResult"));
    }

    // ========== build_test_engine_client tests ==========

    #[test]
    fn test_build_test_engine_client_returns_some() {
        let client = build_test_engine_client();
        assert!(
            client.is_some(),
            "build_test_engine_client should return Some"
        );
    }

    #[test]
    fn test_build_test_engine_client_creates_valid_client() {
        let client = build_test_engine_client();
        assert!(client.is_some());
        let client = client.unwrap();
        // The client should be a valid Arc<EngineClient>
        let _ref: &EngineClient = &client;
    }

    #[test]
    fn test_build_test_engine_client_has_one_engine() {
        // build_test_engine_client registers exactly one ReqwestEngine.
        let client = build_test_engine_client().expect("client should be Some");
        assert_eq!(
            client.engine_count(),
            1,
            "exactly one engine (reqwest) should be registered"
        );
    }

    #[test]
    fn test_build_test_engine_client_registered_engines_contains_reqwest() {
        let client = build_test_engine_client().expect("client should be Some");
        let names = client.registered_engines();
        assert!(
            names.iter().any(|n| n == "reqwest"),
            "registered engines should contain 'reqwest', got {:?}",
            names
        );
    }

    #[test]
    fn test_build_test_engine_client_registered_engines_count_matches_engine_count() {
        let client = build_test_engine_client().expect("client should be Some");
        let names = client.registered_engines();
        assert_eq!(
            names.len(),
            client.engine_count(),
            "registered_engines().len() should equal engine_count()"
        );
    }

    #[test]
    fn test_build_test_engine_client_deterministic_returns_some() {
        // Repeated calls must always succeed (the builder is deterministic and
        // does not depend on external state).
        for i in 0..5 {
            assert!(
                build_test_engine_client().is_some(),
                "call #{} should return Some",
                i
            );
        }
    }

    #[test]
    fn test_build_test_engine_client_creates_independent_instances() {
        // Each call constructs a fresh EngineClient; the returned Arcs must not
        // share the same allocation.
        let a = build_test_engine_client().expect("first call should be Some");
        let b = build_test_engine_client().expect("second call should be Some");
        assert!(
            !Arc::ptr_eq(&a, &b),
            "each call should produce an independent EngineClient instance"
        );
    }

    // ========== run_engine_test_with_output tests (mock SearchEngine) ==========

    use crate::search::{EngineHealth, Response, ResponseItem, SearchEngineType, SearchError};
    use async_trait::async_trait;
    use std::time::Duration;

    /// Mock engine that returns an empty result set.
    struct MockEngineEmpty;

    #[async_trait]
    impl SearchEngine for MockEngineEmpty {
        fn name(&self) -> &'static str {
            "MockEmpty"
        }
        fn engine_type(&self) -> SearchEngineType {
            SearchEngineType::Google
        }
        fn health(&self) -> EngineHealth {
            EngineHealth::Healthy
        }
        async fn search(
            &self,
            _request: &SearchRequest,
        ) -> Result<Response<ResponseItem>, SearchError> {
            Ok(Response {
                items: vec![],
                total_results: Some(0),
                engine: SearchEngineType::Google,
            })
        }
    }

    /// Mock engine that returns items with internal URLs (blocked by SSRF).
    struct MockEngineWithItems {
        item_count: usize,
    }

    #[async_trait]
    impl SearchEngine for MockEngineWithItems {
        fn name(&self) -> &'static str {
            "MockWithItems"
        }
        fn engine_type(&self) -> SearchEngineType {
            SearchEngineType::Google
        }
        fn health(&self) -> EngineHealth {
            EngineHealth::Healthy
        }
        async fn search(
            &self,
            _request: &SearchRequest,
        ) -> Result<Response<ResponseItem>, SearchError> {
            let items = (0..self.item_count)
                .map(|i| ResponseItem {
                    title: format!("Result {}", i + 1),
                    // Use localhost URLs so SSRF validation blocks them
                    // without making real HTTP requests.
                    url: format!("http://127.0.0.1:1/page-{}", i + 1),
                    description: format!("Description {}", i + 1),
                    engine: SearchEngineType::Google,
                })
                .collect();
            Ok(Response {
                items,
                total_results: Some(self.item_count as u64),
                engine: SearchEngineType::Google,
            })
        }
    }

    /// Mock engine that always returns a search error.
    struct MockEngineError;

    #[async_trait]
    impl SearchEngine for MockEngineError {
        fn name(&self) -> &'static str {
            "MockError"
        }
        fn engine_type(&self) -> SearchEngineType {
            SearchEngineType::Google
        }
        fn health(&self) -> EngineHealth {
            EngineHealth::Healthy
        }
        async fn search(
            &self,
            _request: &SearchRequest,
        ) -> Result<Response<ResponseItem>, SearchError> {
            Err(SearchError::Engine("mock engine failure".to_string()))
        }
    }

    /// Mock engine that sleeps longer than the test timeout.
    struct MockEngineTimeout;

    #[async_trait]
    impl SearchEngine for MockEngineTimeout {
        fn name(&self) -> &'static str {
            "MockTimeout"
        }
        fn engine_type(&self) -> SearchEngineType {
            SearchEngineType::Google
        }
        fn health(&self) -> EngineHealth {
            EngineHealth::Healthy
        }
        async fn search(
            &self,
            _request: &SearchRequest,
        ) -> Result<Response<ResponseItem>, SearchError> {
            // Sleep longer than the test timeout to trigger a timeout.
            tokio::time::sleep(Duration::from_secs(10)).await;
            Ok(Response {
                items: vec![],
                total_results: Some(0),
                engine: SearchEngineType::Google,
            })
        }
    }

    #[tokio::test]
    async fn test_run_engine_test_success_empty_results() {
        let result = run_engine_test_with_output(
            "MockEmpty",
            MockEngineEmpty,
            Some("test query"),
            5,
            Some(10),
        )
        .await;

        assert!(result.is_ok(), "empty result search should succeed");
        let tr = result.unwrap();
        assert_eq!(tr.total, 0, "empty result should have 0 total");
        assert_eq!(tr.accessible, 0);
        assert_eq!(tr.inaccessible, 0);
    }

    #[tokio::test]
    async fn test_run_engine_test_success_with_items_all_inaccessible() {
        let result = run_engine_test_with_output(
            "MockWithItems",
            MockEngineWithItems { item_count: 3 },
            Some("test query"),
            5,
            Some(10),
        )
        .await;

        assert!(result.is_ok(), "search with items should succeed");
        let tr = result.unwrap();
        assert_eq!(tr.total, 3, "should have 3 total results");
        assert_eq!(tr.accessible, 0, "all localhost URLs should be inaccessible");
        assert_eq!(tr.inaccessible, 3, "all 3 items should be inaccessible");
    }

    #[tokio::test]
    async fn test_run_engine_test_search_error() {
        let result = run_engine_test_with_output(
            "MockError",
            MockEngineError,
            Some("test query"),
            5,
            Some(10),
        )
        .await;

        assert!(
            result.is_err(),
            "search error should propagate as Err"
        );
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("mock engine failure"),
            "error message should contain the engine error, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_run_engine_test_timeout() {
        let result = run_engine_test_with_output(
            "MockTimeout",
            MockEngineTimeout,
            Some("test query"),
            1,
            Some(10),
        )
        .await;

        assert!(result.is_err(), "timeout should produce an error");
        let err_msg = format!("{}", result.unwrap_err());
        assert!(
            err_msg.contains("timed out"),
            "error message should mention timeout, got: {}",
            err_msg
        );
    }

    #[tokio::test]
    async fn test_run_engine_test_uses_default_query_when_none() {
        // When query is None, the function should use "test query" as default.
        let result = run_engine_test_with_output(
            "MockEmpty",
            MockEngineEmpty,
            None,
            5,
            None,
        )
        .await;

        assert!(result.is_ok(), "should succeed with default query and limit");
        let tr = result.unwrap();
        assert_eq!(tr.total, 0);
    }

    // ========== check_url_accessible tests ==========

    #[tokio::test]
    async fn test_check_url_accessible_returns_false_for_internal_url() {
        // Internal URLs (127.0.0.1) are blocked by SSRF validation in
        // EngineClient::scrape(), so check_url_accessible should return false.
        let client = build_test_engine_client().expect("client should be Some");
        let result = check_url_accessible(&client, "http://127.0.0.1:1/nonexistent").await;
        assert!(
            !result,
            "internal URL should not be accessible (SSRF blocked)"
        );
    }

    #[tokio::test]
    async fn test_check_url_accessible_returns_false_for_localhost() {
        let client = build_test_engine_client().expect("client should be Some");
        let result = check_url_accessible(&client, "http://localhost:1/page").await;
        assert!(
            !result,
            "localhost URL should not be accessible (SSRF blocked)"
        );
    }

    #[tokio::test]
    async fn test_check_url_accessible_returns_false_for_private_ip() {
        let client = build_test_engine_client().expect("client should be Some");
        let result = check_url_accessible(&client, "http://10.0.0.1:8080/internal").await;
        assert!(
            !result,
            "private IP URL should not be accessible (SSRF blocked)"
        );
    }
}
