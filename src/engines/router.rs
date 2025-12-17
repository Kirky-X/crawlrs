// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::engines::circuit_breaker::CircuitBreaker;
use crate::engines::traits::{EngineError, ScrapeRequest, ScrapeResponse, ScraperEngine};
use std::sync::Arc;
use tracing::warn;

/// 引擎路由器
///
/// 负责根据请求特征选择合适的抓取引擎
pub struct EngineRouter {
    /// 引擎列表
    engines: Vec<Arc<dyn ScraperEngine>>,
    /// 熔断器
    circuit_breaker: Arc<CircuitBreaker>,
}

impl EngineRouter {
    /// 创建新的引擎路由器
    ///
    /// # 参数
    ///
    /// * `engines` - 引擎列表
    ///
    /// # 返回值
    ///
    /// 返回新的引擎路由器实例
    pub fn new(engines: Vec<Arc<dyn ScraperEngine>>) -> Self {
        Self {
            engines,
            circuit_breaker: Arc::new(CircuitBreaker::new()),
        }
    }

    /// 使用指定熔断器创建引擎路由器
    ///
    /// # 参数
    ///
    /// * `engines` - 引擎列表
    /// * `circuit_breaker` - 熔断器
    ///
    /// # 返回值
    ///
    /// 返回新的引擎路由器实例
    pub fn with_circuit_breaker(
        engines: Vec<Arc<dyn ScraperEngine>>,
        circuit_breaker: Arc<CircuitBreaker>,
    ) -> Self {
        Self {
            engines,
            circuit_breaker,
        }
    }

    /// 路由请求到合适的引擎
    ///
    /// # 参数
    ///
    /// * `request` - 抓取请求
    ///
    /// # 返回值
    ///
    /// * `Ok(ScrapeResponse)` - 抓取响应
    /// * `Err(EngineError)` - 抓取过程中出现的错误
    pub async fn route(&self, request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
        // Sort engines by support score
        let mut scored_engines: Vec<_> = self
            .engines
            .iter()
            .map(|e| (e.support_score(request), e))
            .collect();
        scored_engines.sort_by_key(|(score, _)| std::cmp::Reverse(*score));

        // Try each engine in order
        for (score, engine) in scored_engines {
            if score == 0 {
                continue; // Skip unsupported engines
            }

            if self.circuit_breaker.is_open(engine.name()) {
                warn!("Circuit breaker open for {}", engine.name());
                continue;
            }

            match engine.scrape(request).await {
                Ok(response) => {
                    self.circuit_breaker.record_success(engine.name());
                    return Ok(response);
                }
                Err(e) => {
                    if e.is_retryable() {
                        self.circuit_breaker.record_failure(engine.name());
                        continue;
                    }
                    return Err(e);
                }
            }
        }

        Err(EngineError::AllEnginesFailed)
    }
}
