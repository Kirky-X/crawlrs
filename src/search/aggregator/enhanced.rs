use async_trait::async_trait;
use log::warn;
use std::sync::Arc;
use std::time::Duration;

use crate::domain::models::search_result::SearchResult;
use crate::domain::search::engine::{SearchEngine, SearchError};
use crate::infrastructure::oxcache::generate_search_key;

#[allow(dead_code)]
pub struct EnhancedSearchAggregator {
    engines: Vec<Arc<dyn SearchEngine>>,
    timeout: Duration,
}

impl EnhancedSearchAggregator {
    pub fn new(engines: Vec<Arc<dyn SearchEngine>>, timeout_ms: u64) -> Self {
        Self {
            engines,
            timeout: Duration::from_millis(timeout_ms),
        }
    }

    #[allow(dead_code)]
    fn generate_cache_key(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> String {
        generate_search_key(query, limit, lang, country)
    }

    async fn search_with_cache(
        &self,
        _query: &str,
        _limit: u32,
        _lang: Option<&str>,
        _country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        // Note: Cache functionality will be implemented when OxCacheComponent is ready
        // For now, fall through to engine search
        self.search_engines(_query, _limit, _lang, _country).await
    }

    async fn search_engines(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let mut all_results = Vec::new();

        for engine in &self.engines {
            match engine.search(query, limit, lang, country).await {
                Ok(results) => {
                    all_results.extend(results);
                }
                Err(e) => {
                    warn!("Engine search failed: {}", e);
                }
            }
        }

        all_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        all_results.truncate(limit as usize);

        Ok(all_results)
    }
}

#[async_trait]
impl SearchEngine for EnhancedSearchAggregator {
    async fn search(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        self.search_with_cache(query, limit, lang, country).await
    }

    fn name(&self) -> &'static str {
        "aggregator"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_key_generation() {
        let aggregator = EnhancedSearchAggregator::new(vec![], 5000);
        let key1 = aggregator.generate_cache_key("rust", 10, Some("en"), Some("US"));
        assert!(key1.starts_with("search:rust"));

        let key2 = aggregator.generate_cache_key("python", 5, None, None);
        assert!(key2.starts_with("search:python"));
    }

    #[tokio::test]
    async fn test_search_with_empty_engines_returns_empty() {
        let aggregator = EnhancedSearchAggregator::new(vec![], 5000);
        let results = aggregator
            .search("test query", 10, None, None)
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    #[test]
    fn test_aggregator_name() {
        let aggregator = EnhancedSearchAggregator::new(vec![], 5000);
        assert_eq!(aggregator.name(), "aggregator");
    }

    #[test]
    fn test_new_constructor_with_timeout() {
        let aggregator = EnhancedSearchAggregator::new(vec![], 3000);
        let key = aggregator.generate_cache_key("test", 5, None, None);
        assert!(key.starts_with("search:test"));
    }

    #[tokio::test]
    async fn test_search_with_zero_limit() {
        let aggregator = EnhancedSearchAggregator::new(vec![], 5000);
        let results = aggregator.search("test", 0, None, None).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_search_with_lang_and_country() {
        let aggregator = EnhancedSearchAggregator::new(vec![], 5000);
        let results = aggregator
            .search("test", 10, Some("en"), Some("US"))
            .await
            .unwrap();
        assert!(results.is_empty());
    }

    // ===== Mock engine for aggregation tests =====

    struct MockAggregatorEngine {
        name: &'static str,
        results: Vec<SearchResult>,
        should_error: bool,
    }

    impl MockAggregatorEngine {
        fn ok(name: &'static str, results: Vec<SearchResult>) -> Arc<Self> {
            Arc::new(Self {
                name,
                results,
                should_error: false,
            })
        }
        fn failing(name: &'static str) -> Arc<Self> {
            Arc::new(Self {
                name,
                results: vec![],
                should_error: true,
            })
        }
    }

    #[async_trait]
    impl SearchEngine for MockAggregatorEngine {
        async fn search(
            &self,
            _query: &str,
            _limit: u32,
            _lang: Option<&str>,
            _country: Option<&str>,
        ) -> Result<Vec<SearchResult>, SearchError> {
            if self.should_error {
                Err(SearchError::EngineError(format!("{} failed", self.name)))
            } else {
                Ok(self.results.clone())
            }
        }
        fn name(&self) -> &'static str {
            self.name
        }
    }

    fn make_result(title: &str, score: f64) -> SearchResult {
        let mut r = SearchResult::new(
            title.to_string(),
            format!("https://example.com/{}", title),
            None,
            "mock".to_string(),
        );
        r.score = score;
        r
    }

    #[tokio::test]
    async fn test_search_aggregates_multiple_engines() {
        let engine_a = MockAggregatorEngine::ok("a", vec![make_result("a1", 0.5)]);
        let engine_b = MockAggregatorEngine::ok("b", vec![make_result("b1", 0.8)]);
        let aggregator = EnhancedSearchAggregator::new(vec![engine_a, engine_b], 5000);
        let results = aggregator.search("q", 10, None, None).await.unwrap();
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_search_sorts_by_score_descending() {
        let engine = MockAggregatorEngine::ok(
            "a",
            vec![
                make_result("low", 0.1),
                make_result("high", 0.9),
                make_result("mid", 0.5),
            ],
        );
        let aggregator = EnhancedSearchAggregator::new(vec![engine], 5000);
        let results = aggregator.search("q", 10, None, None).await.unwrap();
        assert_eq!(results[0].title, "high");
        assert_eq!(results[1].title, "mid");
        assert_eq!(results[2].title, "low");
    }

    #[tokio::test]
    async fn test_search_truncates_to_limit() {
        let engine = MockAggregatorEngine::ok(
            "a",
            vec![
                make_result("r1", 0.9),
                make_result("r2", 0.8),
                make_result("r3", 0.7),
            ],
        );
        let aggregator = EnhancedSearchAggregator::new(vec![engine], 5000);
        let results = aggregator.search("q", 2, None, None).await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].title, "r1");
        assert_eq!(results[1].title, "r2");
    }

    #[tokio::test]
    async fn test_search_failing_engine_continues_with_others() {
        let ok_engine = MockAggregatorEngine::ok("ok", vec![make_result("ok1", 0.5)]);
        let fail_engine = MockAggregatorEngine::failing("fail");
        let aggregator = EnhancedSearchAggregator::new(vec![fail_engine, ok_engine], 5000);
        let results = aggregator.search("q", 10, None, None).await.unwrap();
        // Failing engine is skipped; ok engine results are returned
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "ok1");
    }

    #[tokio::test]
    async fn test_search_all_engines_fail_returns_empty() {
        let fail_a = MockAggregatorEngine::failing("a");
        let fail_b = MockAggregatorEngine::failing("b");
        let aggregator = EnhancedSearchAggregator::new(vec![fail_a, fail_b], 5000);
        let results = aggregator.search("q", 10, None, None).await.unwrap();
        assert!(results.is_empty());
    }
}
