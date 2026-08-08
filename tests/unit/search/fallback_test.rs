// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Fallback 搜索引擎集成级测试
//!
//! 使用 mock 引擎验证 FallbackSearchEngine 的完整行为链。

use async_trait::async_trait;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use crawlrs::search::engine_trait::{SearchEngine, SearchRequest};
use crawlrs::search::error::SearchError;
use crawlrs::search::fallback::FallbackSearchEngine;
use crawlrs::search::response::{Response, ResponseItem};
use crawlrs::search::types::{EngineHealth, SearchEngineType};

/// 可追踪调用次数的 mock 引擎
struct CountingMockEngine {
    name: &'static str,
    call_count: Arc<AtomicU32>,
    behavior: MockBehavior,
}

enum MockBehavior {
    Success(Vec<ResponseItem>),
    Empty,
    Error(String),
}

impl CountingMockEngine {
    fn success(name: &'static str, count: usize) -> Self {
        let items = (0..count)
            .map(|i| ResponseItem {
                title: format!("{} Result {}", name, i + 1),
                url: format!("https://{}.example.com/{}", name, i + 1),
                description: format!("Description from {}", name),
                engine: SearchEngineType::Auto,
            })
            .collect();
        Self {
            name,
            call_count: Arc::new(AtomicU32::new(0)),
            behavior: MockBehavior::Success(items),
        }
    }

    fn empty(name: &'static str) -> Self {
        Self {
            name,
            call_count: Arc::new(AtomicU32::new(0)),
            behavior: MockBehavior::Empty,
        }
    }

    fn error(name: &'static str, msg: &'static str) -> Self {
        Self {
            name,
            call_count: Arc::new(AtomicU32::new(0)),
            behavior: MockBehavior::Error(msg.to_string()),
        }
    }

    fn call_count(&self) -> u32 {
        self.call_count.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl SearchEngine for CountingMockEngine {
    fn name(&self) -> &'static str {
        self.name
    }

    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Auto
    }

    fn health(&self) -> EngineHealth {
        EngineHealth::Healthy
    }

    async fn search(
        &self,
        _request: &SearchRequest,
    ) -> Result<Response<ResponseItem>, SearchError> {
        self.call_count.fetch_add(1, Ordering::SeqCst);

        match &self.behavior {
            MockBehavior::Success(items) => Ok(Response {
                items: items.clone(),
                total_results: Some(items.len() as u64),
                engine: SearchEngineType::Auto,
            }),
            MockBehavior::Empty => Ok(Response {
                items: vec![],
                total_results: Some(0),
                engine: SearchEngineType::Auto,
            }),
            MockBehavior::Error(msg) => Err(SearchError::EngineFailed(msg.clone())),
        }
    }
}

fn make_request() -> SearchRequest {
    SearchRequest::new("integration test query").with_limit(5)
}

// ========== 完整行为链测试 ==========

#[tokio::test]
async fn test_primary_success_does_not_call_fallbacks() {
    let primary = Arc::new(CountingMockEngine::success("Primary", 3));
    let fallback1 = Arc::new(CountingMockEngine::success("Fallback1", 2));
    let fallback2 = Arc::new(CountingMockEngine::success("Fallback2", 1));

    let engine =
        FallbackSearchEngine::new(primary.clone(), vec![fallback1.clone(), fallback2.clone()]);

    let response = engine.search(&make_request()).await.unwrap();
    assert_eq!(response.items.len(), 3);
    assert_eq!(primary.call_count(), 1);
    assert_eq!(fallback1.call_count(), 0, "fallback1 should not be called");
    assert_eq!(fallback2.call_count(), 0, "fallback2 should not be called");
}

#[tokio::test]
async fn test_primary_failure_calls_first_fallback_only() {
    let primary = Arc::new(CountingMockEngine::error("Primary", "down"));
    let fallback1 = Arc::new(CountingMockEngine::success("Fallback1", 2));
    let fallback2 = Arc::new(CountingMockEngine::success("Fallback2", 1));

    let engine =
        FallbackSearchEngine::new(primary.clone(), vec![fallback1.clone(), fallback2.clone()]);

    let response = engine.search(&make_request()).await.unwrap();
    assert_eq!(response.items.len(), 2);
    assert!(response.items[0].title.contains("Fallback1"));
    assert_eq!(primary.call_count(), 1);
    assert_eq!(fallback1.call_count(), 1);
    assert_eq!(fallback2.call_count(), 0, "fallback2 should not be called");
}

#[tokio::test]
async fn test_primary_empty_then_first_fallback_empty_calls_second() {
    let primary = Arc::new(CountingMockEngine::empty("Primary"));
    let fallback1 = Arc::new(CountingMockEngine::empty("Fallback1"));
    let fallback2 = Arc::new(CountingMockEngine::success("Fallback2", 1));

    let engine =
        FallbackSearchEngine::new(primary.clone(), vec![fallback1.clone(), fallback2.clone()]);

    let response = engine.search(&make_request()).await.unwrap();
    assert_eq!(response.items.len(), 1);
    assert!(response.items[0].title.contains("Fallback2"));
    assert_eq!(primary.call_count(), 1);
    assert_eq!(fallback1.call_count(), 1);
    assert_eq!(fallback2.call_count(), 1);
}

#[tokio::test]
async fn test_all_fail_returns_all_engines_failed() {
    let primary = Arc::new(CountingMockEngine::error("Primary", "down"));
    let fallback1 = Arc::new(CountingMockEngine::error("Fallback1", "also down"));
    let fallback2 = Arc::new(CountingMockEngine::empty("Fallback2"));

    let engine =
        FallbackSearchEngine::new(primary.clone(), vec![fallback1.clone(), fallback2.clone()]);

    let result = engine.search(&make_request()).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), SearchError::AllEnginesFailed));
    assert_eq!(primary.call_count(), 1);
    assert_eq!(fallback1.call_count(), 1);
    assert_eq!(
        fallback2.call_count(),
        1,
        "empty fallback should still be called"
    );
}

#[tokio::test]
async fn test_mixed_errors_and_empty_in_chain() {
    let primary = Arc::new(CountingMockEngine::error("Primary", "timeout"));
    let fallback1 = Arc::new(CountingMockEngine::error("Fallback1", "rate limited"));
    let fallback2 = Arc::new(CountingMockEngine::empty("Fallback2"));
    let fallback3 = Arc::new(CountingMockEngine::success("Fallback3", 1));

    let engine = FallbackSearchEngine::new(
        primary.clone(),
        vec![fallback1.clone(), fallback2.clone(), fallback3.clone()],
    );

    let response = engine.search(&make_request()).await.unwrap();
    assert_eq!(response.items.len(), 1);
    assert!(response.items[0].title.contains("Fallback3"));
    assert_eq!(fallback3.call_count(), 1);
}
