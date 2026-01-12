// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use rand::Rng;
use std::sync::Arc;
use tracing::info;

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
}
