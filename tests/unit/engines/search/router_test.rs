// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use crawlrs::domain::models::search_result::SearchResult;
use crawlrs::domain::search::engine::{SearchEngine, SearchError};
use crawlrs::infrastructure::search::aggregator::SearchAggregator;
use futures::Future;
use mockall::mock;
use std::pin::Pin;
use std::sync::Arc;
use tokio::time::{self, Duration};

mock! {
    pub SearchEngine {
        fn search<'a>(&'a self, query: &'a str, limit: u32, lang: Option<&'a str>, country: Option<&'a str>) -> Pin<Box<dyn Future<Output = Result<Vec<SearchResult>, SearchError>> + Send + 'static>>;
        fn name(&self) -> &'static str;
    }
}

#[async_trait]
impl SearchEngine for MockSearchEngine {
    async fn search(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        self.search(query, limit, lang, country).await
    }

    fn name(&self) -> &'static str {
        self.name()
    }
}

#[tokio::test]
async fn test_search_router_concurrent_aggregation() {
    // Given: 两个搜索引擎，一个快一个慢
    let mut fast_engine = MockSearchEngine::new();
    fast_engine.expect_search().returning(|query, _, _, _| {
        let query = query.to_string();
        Box::pin(async move {
            Ok(vec![SearchResult {
                url: "https://fast.com".to_string(),
                title: format!("Fast Engine Result for {}", query),
                description: None,
                score: 0.0,
                published_time: None,
                engine: "fast".to_string(),
            }])
        })
    });
    fast_engine.expect_name().return_const("fast_engine");

    let mut slow_engine = MockSearchEngine::new();
    slow_engine.expect_search().returning(|query, _, _, _| {
        let query = query.to_string();
        Box::pin(async move {
            time::sleep(Duration::from_millis(100)).await;
            Ok(vec![SearchResult {
                url: "https://slow.com".to_string(),
                title: format!("Slow Engine Result for {}", query),
                description: None,
                score: 0.0,
                published_time: None,
                engine: "slow".to_string(),
            }])
        })
    });
    slow_engine.expect_name().return_const("slow_engine");

    let aggregator = SearchAggregator::new(
        vec![Arc::new(fast_engine), Arc::new(slow_engine)],
        1000,
    );

    // When: 执行搜索
    let results = aggregator
        .search("test", 5, None, None)
        .await
        .unwrap();

    // Then: 应该聚合两个引擎的结果
    assert_eq!(results.len(), 2);
    assert!(results
        .iter()
        .any(|r| r.title == "Fast Engine Result for test"));
    assert!(results
        .iter()
        .any(|r| r.title == "Slow Engine Result for test"));
}
