// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::models::search_result::SearchResult;
use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error, Clone)]
pub enum SearchError {
    #[error("Search engine error: {0}")]
    EngineError(String),
    #[error("Network error: {0}")]
    NetworkError(String),
    #[error("Rate limit exceeded: {0}")]
    RateLimitExceeded(String),
    #[error("Timeout after {0} seconds")]
    TimeoutError(u64),
}

#[async_trait]
pub trait SearchEngine: Send + Sync {
    /// Perform a search query
    async fn search(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError>;

    /// Get the name of the search engine
    fn name(&self) -> &'static str;

    /// Perform a search using a specific engine (if supported by the implementation)
    async fn search_with_engine(
        &self,
        _query: &str,
        _limit: u32,
        _lang: Option<&str>,
        _country: Option<&str>,
        _engine: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        // Default implementation: fall back to regular search
        // Subclasses like SearchAggregator can override this
        Err(SearchError::EngineError(
            "search_with_engine not supported by this engine".to_string(),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========== SearchError Display tests ==========

    #[test]
    fn test_engine_error_display() {
        let err = SearchError::EngineError("boom".to_string());
        let msg = format!("{}", err);
        assert!(
            msg.contains("Search engine error"),
            "Display should contain prefix"
        );
        assert!(msg.contains("boom"), "Display should contain inner message");
    }

    #[test]
    fn test_network_error_display() {
        let err = SearchError::NetworkError("conn refused".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("Network error"));
        assert!(msg.contains("conn refused"));
    }

    #[test]
    fn test_rate_limit_exceeded_display() {
        let err = SearchError::RateLimitExceeded("429".to_string());
        let msg = format!("{}", err);
        assert!(msg.contains("Rate limit exceeded"));
        assert!(msg.contains("429"));
    }

    #[test]
    fn test_timeout_error_display() {
        let err = SearchError::TimeoutError(30);
        let msg = format!("{}", err);
        assert!(msg.contains("Timeout after"));
        assert!(msg.contains("30"));
        assert!(msg.contains("seconds"));
    }

    // ========== SearchError Clone / Debug tests ==========

    #[test]
    fn test_search_error_clone_preserves_variant() {
        let err = SearchError::EngineError("clone-me".to_string());
        let cloned = err.clone();
        match (err, cloned) {
            (SearchError::EngineError(a), SearchError::EngineError(b)) => assert_eq!(a, b),
            _ => panic!("clone should preserve variant"),
        }
    }

    #[test]
    fn test_search_error_debug_contains_variant_name() {
        let err = SearchError::TimeoutError(5);
        let dbg = format!("{:?}", err);
        assert!(
            dbg.contains("TimeoutError"),
            "Debug should contain variant name"
        );
    }

    // ========== SearchError std::error::Error tests ==========

    #[test]
    fn test_search_error_source_is_none() {
        let err = SearchError::EngineError("e".to_string());
        // thiserror with #[error(...)] but no #[source] => source() is None
        assert!(std::error::Error::source(&err).is_none());
    }

    // ========== MockSearchEngine for SearchEngine trait tests ==========

    struct MockSearchEngine;

    #[async_trait]
    impl SearchEngine for MockSearchEngine {
        async fn search(
            &self,
            _query: &str,
            _limit: u32,
            _lang: Option<&str>,
            _country: Option<&str>,
        ) -> Result<Vec<SearchResult>, SearchError> {
            Ok(vec![SearchResult::new(
                "Mock Title".to_string(),
                "https://example.com".to_string(),
                None,
                "mock".to_string(),
            )])
        }

        fn name(&self) -> &'static str {
            "mock-engine"
        }
    }

    // ========== SearchEngine::search tests ==========

    #[tokio::test]
    async fn test_mock_search_returns_results() {
        let engine = MockSearchEngine;
        let results = engine
            .search("query", 10, None, None)
            .await
            .expect("mock search should succeed");
        assert_eq!(results.len(), 1, "mock should return exactly one result");
        assert_eq!(results[0].title, "Mock Title");
        assert_eq!(results[0].url, "https://example.com");
        assert_eq!(results[0].engine, "mock");
    }

    #[tokio::test]
    async fn test_mock_search_ignores_params() {
        let engine = MockSearchEngine;
        let r1 = engine.search("a", 1, Some("en"), Some("us")).await;
        let r2 = engine.search("b", 100, None, None).await;
        assert!(r1.is_ok() && r2.is_ok());
        // Both calls return the same single result regardless of parameters.
        assert_eq!(r1.unwrap().len(), 1);
        assert_eq!(r2.unwrap().len(), 1);
    }

    #[test]
    fn test_mock_search_engine_name() {
        let engine = MockSearchEngine;
        assert_eq!(engine.name(), "mock-engine");
    }

    // ========== SearchEngine::search_with_engine (default impl) tests ==========

    #[tokio::test]
    async fn test_search_with_engine_default_returns_error() {
        let engine = MockSearchEngine;
        let result = engine
            .search_with_engine("query", 10, None, None, Some("google"))
            .await;
        assert!(
            result.is_err(),
            "default search_with_engine should return Err"
        );
    }

    #[tokio::test]
    async fn test_search_with_engine_default_error_is_engine_error() {
        let engine = MockSearchEngine;
        let err = engine
            .search_with_engine("q", 5, None, None, None)
            .await
            .expect_err("should be error");
        match err {
            SearchError::EngineError(msg) => {
                assert!(
                    msg.contains("search_with_engine not supported"),
                    "error message should mention unsupported, got: {}",
                    msg
                );
            }
            other => panic!("Expected EngineError, got {:?}", other),
        }
    }

    // ========== CustomEngine overriding search_with_engine ==========

    struct CustomEngine;

    #[async_trait]
    impl SearchEngine for CustomEngine {
        async fn search(
            &self,
            _query: &str,
            _limit: u32,
            _lang: Option<&str>,
            _country: Option<&str>,
        ) -> Result<Vec<SearchResult>, SearchError> {
            Ok(vec![])
        }

        fn name(&self) -> &'static str {
            "custom-engine"
        }

        async fn search_with_engine(
            &self,
            _query: &str,
            _limit: u32,
            _lang: Option<&str>,
            _country: Option<&str>,
            engine: Option<&str>,
        ) -> Result<Vec<SearchResult>, SearchError> {
            match engine {
                Some("bing") => Ok(vec![SearchResult::new(
                    "Bing Result".to_string(),
                    "https://bing.com".to_string(),
                    None,
                    "bing".to_string(),
                )]),
                Some(other) => Err(SearchError::EngineError(format!(
                    "unsupported engine: {}",
                    other
                ))),
                None => Ok(vec![]),
            }
        }
    }

    #[tokio::test]
    async fn test_custom_engine_search_with_engine_bing() {
        let engine = CustomEngine;
        let results = engine
            .search_with_engine("q", 10, None, None, Some("bing"))
            .await
            .expect("bing should succeed");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].title, "Bing Result");
    }

    #[tokio::test]
    async fn test_custom_engine_search_with_engine_unsupported() {
        let engine = CustomEngine;
        let result = engine
            .search_with_engine("q", 10, None, None, Some("yahoo"))
            .await;
        assert!(result.is_err());
        match result.expect_err("should error") {
            SearchError::EngineError(msg) => assert!(msg.contains("yahoo")),
            other => panic!("Expected EngineError, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_custom_engine_search_with_engine_none_returns_empty() {
        let engine = CustomEngine;
        let results = engine
            .search_with_engine("q", 10, None, None, None)
            .await
            .expect("None engine should succeed");
        assert!(results.is_empty(), "None engine should return empty vec");
    }
}
