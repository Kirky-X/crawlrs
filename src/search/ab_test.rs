// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use log::info;
use rand::Rng;
use std::sync::Arc;

use crate::search::engine_trait::{SearchEngine, SearchRequest};
use crate::search::error::SearchError;
use crate::search::response::{Response, ResponseItem};
use crate::search::types::{EngineHealth, SearchEngineType};

/// 搜索算法 A/B 测试框架
///
/// 该框架允许在不同的搜索引擎（或搜索策略）之间进行流量分配，
/// 并记录结果以供后续性能和相关性分析。
pub struct SearchABTestEngine {
    variant_a: Arc<dyn SearchEngine>,
    variant_b: Arc<dyn SearchEngine>,
    /// Variant B 的流量权重 (0.0 到 1.0)
    variant_b_weight: f64,
}

impl SearchABTestEngine {
    pub fn new(
        variant_a: Arc<dyn SearchEngine>,
        variant_b: Arc<dyn SearchEngine>,
        variant_b_weight: f64,
    ) -> Self {
        Self {
            variant_a,
            variant_b,
            variant_b_weight: variant_b_weight.clamp(0.0, 1.0),
        }
    }

    /// 根据权重选择要使用的引擎
    fn select_engine(&self) -> (Arc<dyn SearchEngine>, &'static str) {
        let mut rng = rand::rng();
        if rng.random_bool(self.variant_b_weight) {
            (self.variant_b.clone(), "variant_b")
        } else {
            (self.variant_a.clone(), "variant_a")
        }
    }
}

#[async_trait]
impl SearchEngine for SearchABTestEngine {
    fn name(&self) -> &'static str {
        "ab_test_engine"
    }

    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::ABTest
    }

    fn health(&self) -> EngineHealth {
        let health_a = self.variant_a.health();
        let health_b = self.variant_b.health();
        if health_a == EngineHealth::Healthy && health_b == EngineHealth::Healthy {
            EngineHealth::Healthy
        } else if health_a == EngineHealth::Unhealthy && health_b == EngineHealth::Unhealthy {
            EngineHealth::Unhealthy
        } else {
            EngineHealth::Degraded
        }
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        let (engine, variant_name) = self.select_engine();

        info!(
            "A/B Test: Selected {} ({}) for query: {}",
            variant_name,
            engine.name(),
            request.query
        );

        let start_time = std::time::Instant::now();
        let result = engine.search(request).await;
        let duration = start_time.elapsed();

        match &result {
            Ok(response) => {
                info!(
                    "A/B Test: {} completed in {:?}, returned {} results",
                    variant_name,
                    duration,
                    response.items.len()
                );
            }
            Err(e) => {
                info!(
                    "A/B Test: {} failed after {:?}: {}",
                    variant_name, duration, e
                );
            }
        }

        result
    }

    async fn search_with_engine(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
        engine: Option<&str>,
    ) -> Result<Vec<ResponseItem>, SearchError> {
        let (selected_engine, variant_name) = self.select_engine();

        info!(
            "A/B Test: Selected {} ({}) for query: {} with engine_filter: {:?}",
            variant_name,
            selected_engine.name(),
            query,
            engine
        );

        // Delegate to the selected engine's search_with_engine
        selected_engine
            .search_with_engine(query, limit, lang, country, engine)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::response::ResponseItem;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// Mock engine that tracks search calls
    struct MockEngine {
        name: &'static str,
        search_call_count: Arc<AtomicUsize>,
        last_engine_filter: Arc<AtomicUsize>,
    }

    impl MockEngine {
        fn new(name: &'static str) -> (Arc<Self>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
            let search_call_count = Arc::new(AtomicUsize::new(0));
            let last_engine_filter = Arc::new(AtomicUsize::new(0));
            (
                Arc::new(Self {
                    name,
                    search_call_count: search_call_count.clone(),
                    last_engine_filter: last_engine_filter.clone(),
                }),
                search_call_count,
                last_engine_filter,
            )
        }
    }

    #[async_trait]
    impl SearchEngine for MockEngine {
        fn name(&self) -> &'static str {
            self.name
        }

        fn engine_type(&self) -> SearchEngineType {
            SearchEngineType::from_name(self.name).unwrap_or(SearchEngineType::Auto)
        }

        fn health(&self) -> EngineHealth {
            EngineHealth::Healthy
        }

        async fn search(
            &self,
            _request: &SearchRequest,
        ) -> Result<Response<ResponseItem>, SearchError> {
            self.search_call_count.fetch_add(1, Ordering::SeqCst);
            Ok(Response {
                items: vec![ResponseItem {
                    title: format!("Result from {}", self.name),
                    url: format!("https://{}.com/result", self.name),
                    description: format!("Description from {}", self.name),
                    engine: self.engine_type(),
                }],
                total_results: Some(1),
                engine: self.engine_type(),
            })
        }

        async fn search_with_engine(
            &self,
            query: &str,
            limit: u32,
            _lang: Option<&str>,
            _country: Option<&str>,
            engine: Option<&str>,
        ) -> Result<Vec<ResponseItem>, SearchError> {
            self.search_call_count.fetch_add(1, Ordering::SeqCst);
            self.last_engine_filter
                .store(engine.map(|e| e.len()).unwrap_or(0), Ordering::SeqCst);
            self.search(&SearchRequest::new(query).with_limit(limit))
                .await
                .map(|r| r.items)
        }
    }

    #[tokio::test]
    async fn test_ab_test_engine_search_with_engine_delegation() {
        // Create two mock engines
        let (engine_a, count_a, _) = MockEngine::new("engine_a");
        let (engine_b, _count_b, _) = MockEngine::new("engine_b");

        // Create A/B test engine with 100% variant_a to ensure deterministic behavior
        let ab_engine = SearchABTestEngine::new(engine_a, engine_b, 0.0);

        // Test that search_with_engine properly delegates to variant_a with engine filter
        let result = ab_engine
            .search_with_engine("test query", 10, None, None, Some("bing"))
            .await
            .expect("search_with_engine should succeed");

        // Verify the result came from engine_a (variant_a due to weight 0.0)
        assert_eq!(result.len(), 1);
        assert!(result[0].title.contains("engine_a"));

        // Verify that search_with_engine was called on the underlying engine
        // It should be called exactly once (A/B engine delegates to variant_a)
        // Note: The underlying engine's search_with_engine calls search internally
        // so count_a will be 1 (for search_with_engine) + 1 (for search called by it) = 2
        // But we only track search_with_engine calls
        assert!(count_a.load(Ordering::SeqCst) >= 1);
    }

    #[tokio::test]
    async fn test_ab_test_engine_search_with_engine_passes_filter() {
        // Create two mock engines
        let (engine_a, _count_a, last_filter_a) = MockEngine::new("engine_a");
        let (engine_b, _count_b, _last_filter_b) = MockEngine::new("engine_b");

        // Create A/B test engine with 100% variant_a
        let ab_engine = SearchABTestEngine::new(engine_a, engine_b, 0.0);

        // Test with specific engine filter
        let _ = ab_engine
            .search_with_engine("test query", 10, None, None, Some("bing"))
            .await
            .expect("search_with_engine should succeed");

        // Verify the engine filter was passed through
        assert!(last_filter_a.load(Ordering::SeqCst) > 0);
    }

    // ===== Configurable mock for health/error testing =====

    struct ConfigurableEngine {
        name: &'static str,
        health: EngineHealth,
        should_error: bool,
    }

    impl ConfigurableEngine {
        fn healthy(name: &'static str) -> Arc<Self> {
            Arc::new(Self {
                name,
                health: EngineHealth::Healthy,
                should_error: false,
            })
        }
        fn unhealthy(name: &'static str) -> Arc<Self> {
            Arc::new(Self {
                name,
                health: EngineHealth::Unhealthy,
                should_error: false,
            })
        }
        fn failing(name: &'static str) -> Arc<Self> {
            Arc::new(Self {
                name,
                health: EngineHealth::Healthy,
                should_error: true,
            })
        }
    }

    #[async_trait]
    impl SearchEngine for ConfigurableEngine {
        fn name(&self) -> &'static str {
            self.name
        }
        fn engine_type(&self) -> SearchEngineType {
            SearchEngineType::ABTest
        }
        fn health(&self) -> EngineHealth {
            self.health
        }
        async fn search(
            &self,
            _request: &SearchRequest,
        ) -> Result<Response<ResponseItem>, SearchError> {
            if self.should_error {
                return Err(SearchError::EngineFailed("mock failure".to_string()));
            }
            Ok(Response {
                items: vec![ResponseItem {
                    title: format!("Result from {}", self.name),
                    url: format!("https://{}.com/result", self.name),
                    description: format!("Description from {}", self.name),
                    engine: SearchEngineType::ABTest,
                }],
                total_results: Some(1),
                engine: SearchEngineType::ABTest,
            })
        }
    }

    // ===== Constructor and metadata tests =====

    #[test]
    fn test_new_clamps_weight_above_one() {
        let (engine_a, _, _) = MockEngine::new("a");
        let (engine_b, _, _) = MockEngine::new("b");
        // Weight > 1.0 should clamp to 1.0 (variant_b always selected)
        let ab = SearchABTestEngine::new(engine_a, engine_b, 2.0);
        assert_eq!(ab.variant_b_weight, 1.0);
    }

    #[test]
    fn test_new_clamps_weight_below_zero() {
        let (engine_a, _, _) = MockEngine::new("a");
        let (engine_b, _, _) = MockEngine::new("b");
        // Weight < 0.0 should clamp to 0.0 (variant_a always selected)
        let ab = SearchABTestEngine::new(engine_a, engine_b, -0.5);
        assert_eq!(ab.variant_b_weight, 0.0);
    }

    #[test]
    fn test_engine_name() {
        let (engine_a, _, _) = MockEngine::new("a");
        let (engine_b, _, _) = MockEngine::new("b");
        let ab = SearchABTestEngine::new(engine_a, engine_b, 0.5);
        assert_eq!(ab.name(), "ab_test_engine");
    }

    #[test]
    fn test_engine_type_returns_abtest() {
        let (engine_a, _, _) = MockEngine::new("a");
        let (engine_b, _, _) = MockEngine::new("b");
        let ab = SearchABTestEngine::new(engine_a, engine_b, 0.5);
        assert_eq!(ab.engine_type(), SearchEngineType::ABTest);
    }

    // ===== Health aggregation tests =====

    #[test]
    fn test_health_both_healthy() {
        let ab = SearchABTestEngine::new(
            ConfigurableEngine::healthy("a"),
            ConfigurableEngine::healthy("b"),
            0.5,
        );
        assert_eq!(ab.health(), EngineHealth::Healthy);
    }

    #[test]
    fn test_health_both_unhealthy() {
        let ab = SearchABTestEngine::new(
            ConfigurableEngine::unhealthy("a"),
            ConfigurableEngine::unhealthy("b"),
            0.5,
        );
        assert_eq!(ab.health(), EngineHealth::Unhealthy);
    }

    #[test]
    fn test_health_mixed_returns_degraded() {
        let ab = SearchABTestEngine::new(
            ConfigurableEngine::healthy("a"),
            ConfigurableEngine::unhealthy("b"),
            0.5,
        );
        assert_eq!(ab.health(), EngineHealth::Degraded);
    }

    // ===== search() tests =====

    #[tokio::test]
    async fn test_search_returns_ok() {
        let ab = SearchABTestEngine::new(
            ConfigurableEngine::healthy("a"),
            ConfigurableEngine::healthy("b"),
            0.0, // always variant_a
        );
        let request = SearchRequest::new("test query");
        let result = ab.search(&request).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().items.len(), 1);
    }

    #[tokio::test]
    async fn test_search_propagates_error() {
        let ab = SearchABTestEngine::new(
            ConfigurableEngine::failing("a"),
            ConfigurableEngine::failing("b"),
            0.0, // always variant_a which fails
        );
        let request = SearchRequest::new("test query");
        let result = ab.search(&request).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_search_with_engine_uses_variant_b_at_full_weight() {
        let (engine_a, count_a, _) = MockEngine::new("engine_a");
        let (engine_b, count_b, _) = MockEngine::new("engine_b");
        // Weight 1.0 → always variant_b
        let ab = SearchABTestEngine::new(engine_a, engine_b, 1.0);
        let result = ab
            .search_with_engine("test", 5, None, None, None)
            .await
            .expect("should succeed");
        assert!(result[0].title.contains("engine_b"));
        assert_eq!(
            count_a.load(Ordering::SeqCst),
            0,
            "variant_a should not be called"
        );
        assert!(
            count_b.load(Ordering::SeqCst) >= 1,
            "variant_b should be called"
        );
    }
}
