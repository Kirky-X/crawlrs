// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use std::sync::Arc;
use tracing::info;

use crate::domain::models::search_result::SearchResult;
use crate::domain::search::engine::{SearchEngine, SearchError};

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
        if rand::random_bool(self.variant_b_weight) {
            (self.variant_b.clone(), "variant_b")
        } else {
            (self.variant_a.clone(), "variant_a")
        }
    }
}

#[async_trait]
impl SearchEngine for SearchABTestEngine {
    async fn search(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let (engine, variant_name) = self.select_engine();

        info!(
            "A/B Test: Selected {} ({}) for query: {}",
            variant_name,
            engine.name(),
            query
        );

        let start_time = std::time::Instant::now();
        let result = engine.search(query, limit, lang, country).await;
        let duration = start_time.elapsed();

        match &result {
            Ok(results) => {
                info!(
                    "A/B Test: {} completed in {:?}, returned {} results",
                    variant_name,
                    duration,
                    results.len()
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

    fn name(&self) -> &'static str {
        "ab_test_engine"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::models::search_result::SearchResult;
    use crate::domain::search::engine::SearchError;

    struct SampleEngine {
        name: &'static str,
    }

    #[async_trait]
    impl SearchEngine for SampleEngine {
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
            self.name
        }
    }

    #[tokio::test]
    async fn test_ab_test_selection() {
        let engine_a = Arc::new(SampleEngine { name: "engine_a" });
        let engine_b = Arc::new(SampleEngine { name: "engine_b" });

        // 100% 流量分配给 B
        let ab_engine = SearchABTestEngine::new(engine_a.clone(), engine_b.clone(), 1.0);
        let (selected, variant) = ab_engine.select_engine();
        assert_eq!(selected.name(), "engine_b");
        assert_eq!(variant, "variant_b");

        // 0% 流量分配给 B (即 100% 给 A)
        let ab_engine = SearchABTestEngine::new(engine_a, engine_b, 0.0);
        let (selected, variant) = ab_engine.select_engine();
        assert_eq!(selected.name(), "engine_a");
        assert_eq!(variant, "variant_a");
    }
}
