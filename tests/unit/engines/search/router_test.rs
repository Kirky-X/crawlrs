// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use crawlrs::domain::models::search_result::SearchResult;
use crawlrs::domain::search::engine::{SearchEngine, SearchError};
use crawlrs::infrastructure::search::aggregator::SearchAggregator;
use std::sync::Arc;
use tokio::time::{self, Duration};

// Test implementation for controlled behavior
struct TestSearchEngine {
    name: &'static str,
    results: Vec<SearchResult>,
    delay: Option<Duration>,
}

impl TestSearchEngine {
    fn new(name: &'static str, results: Vec<SearchResult>, delay: Option<Duration>) -> Self {
        Self {
            name,
            results,
            delay,
        }
    }
}

#[async_trait]
impl SearchEngine for TestSearchEngine {
    async fn search(
        &self,
        _query: &str,
        _limit: u32,
        _lang: Option<&str>,
        _country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        if let Some(delay) = self.delay {
            time::sleep(delay).await;
        }
        Ok(self.results.clone())
    }

    fn name(&self) -> &'static str {
        self.name
    }
}

#[tokio::test]
async fn test_search_router_concurrent_aggregation() {
    // Given: 两个搜索引擎，一个快一个慢
    let fast_engine = TestSearchEngine::new(
        "fast_engine",
        vec![SearchResult {
            url: "https://fast.com".to_string(),
            title: "Fast Engine Result for test".to_string(),
            description: None,
            score: 0.0,
            published_time: None,
            engine: "fast".to_string(),
        }],
        None,
    );

    let slow_engine = TestSearchEngine::new(
        "slow_engine",
        vec![SearchResult {
            url: "https://slow.com".to_string(),
            title: "Slow Engine Result for test".to_string(),
            description: None,
            score: 0.0,
            published_time: None,
            engine: "slow".to_string(),
        }],
        Some(Duration::from_millis(100)),
    );

    let aggregator =
        SearchAggregator::new(vec![Arc::new(fast_engine), Arc::new(slow_engine)], 1000);

    // When: 执行搜索
    let results = aggregator.search("test", 5, None, None).await.unwrap();

    // Then: 应该聚合两个引擎的结果
    assert_eq!(results.len(), 2);
    assert!(results
        .iter()
        .any(|r| r.title == "Fast Engine Result for test"));
    assert!(results
        .iter()
        .any(|r| r.title == "Slow Engine Result for test"));
}
