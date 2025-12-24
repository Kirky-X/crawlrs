// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::engines::circuit_breaker::CircuitBreaker;
use crate::engines::traits::{EngineError, ScrapeRequest, ScrapeResponse, ScraperEngine};
use rand::seq::SliceRandom;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// 引擎性能统计
#[derive(Debug, Clone)]
pub struct EngineStats {
    /// 成功率 (0.0 - 1.0)
    pub success_rate: f64,
    /// 平均响应时间
    pub avg_response_time: Duration,
    /// 最近使用时间
    pub last_used: Option<Instant>,
    /// 使用次数
    pub usage_count: u64,
}

impl Default for EngineStats {
    fn default() -> Self {
        Self {
            success_rate: 1.0,
            avg_response_time: Duration::from_millis(500),
            last_used: None,
            usage_count: 0,
        }
    }
}

/// 负载均衡策略
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum LoadBalancingStrategy {
    /// 轮询
    RoundRobin,
    /// 加权轮询 (基于成功率)
    WeightedRoundRobin,
    /// 最少连接/最少使用
    LeastConnections,
    /// 最快响应时间
    FastestResponse,
    /// 随机
    Random,
    /// 智能混合 (默认)
    SmartHybrid,
}

/// 引擎路由器
///
/// 负责根据请求特征和负载均衡策略选择合适的抓取引擎
pub struct EngineRouter {
    /// 引擎列表
    engines: Vec<Arc<dyn ScraperEngine>>,
    /// 熔断器
    circuit_breaker: Arc<CircuitBreaker>,
    /// 引擎性能统计
    engine_stats: Arc<parking_lot::RwLock<std::collections::HashMap<String, EngineStats>>>,
    /// 当前轮询索引
    round_robin_index: Arc<parking_lot::Mutex<usize>>,
    /// 负载均衡策略
    strategy: LoadBalancingStrategy,
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
        let mut engine_stats = std::collections::HashMap::new();
        for engine in &engines {
            engine_stats.insert(engine.name().to_string(), EngineStats::default());
        }

        Self {
            engines,
            circuit_breaker: Arc::new(CircuitBreaker::new()),
            engine_stats: Arc::new(parking_lot::RwLock::new(engine_stats)),
            round_robin_index: Arc::new(parking_lot::Mutex::new(0)),
            strategy: LoadBalancingStrategy::SmartHybrid,
        }
    }

    /// 使用指定熔断器和策略创建引擎路由器
    ///
    /// # 参数
    ///
    /// * `engines` - 引擎列表
    /// * `circuit_breaker` - 熔断器
    /// * `strategy` - 负载均衡策略
    ///
    /// # 返回值
    ///
    /// 返回新的引擎路由器实例
    pub fn with_circuit_breaker_and_strategy(
        engines: Vec<Arc<dyn ScraperEngine>>,
        circuit_breaker: Arc<CircuitBreaker>,
        strategy: LoadBalancingStrategy,
    ) -> Self {
        let mut engine_stats = std::collections::HashMap::new();
        for engine in &engines {
            engine_stats.insert(engine.name().to_string(), EngineStats::default());
        }

        Self {
            engines,
            circuit_breaker,
            engine_stats: Arc::new(parking_lot::RwLock::new(engine_stats)),
            round_robin_index: Arc::new(parking_lot::Mutex::new(0)),
            strategy,
        }
    }

    /// 设置负载均衡策略
    pub fn set_strategy(&mut self, strategy: LoadBalancingStrategy) {
        self.strategy = strategy;
    }

    /// 选择最优引擎
    ///
    /// # 参数
    ///
    /// * `request` - 抓取请求
    ///
    /// # 返回值
    ///
    /// 返回最优引擎列表（按优先级排序）
    fn select_optimal_engines(
        &self,
        request: &ScrapeRequest,
    ) -> Vec<(f64, Arc<dyn ScraperEngine>)> {
        let mut candidates = Vec::new();
        let stats = self.engine_stats.read();

        for engine in &self.engines {
            let engine_name = engine.name();

            // 检查熔断器状态
            if self.circuit_breaker.is_open(engine_name) {
                continue;
            }

            // 获取支持分数
            let support_score = engine.support_score(request) as f64;
            if support_score == 0.0 {
                continue;
            }

            // 获取引擎统计信息
            let default_stats = EngineStats::default();
            let engine_stat = stats.get(engine_name).unwrap_or(&default_stats);

            // 计算综合评分
            let final_score = self.calculate_engine_score(support_score, engine_stat);

            candidates.push((final_score, engine.clone()));
        }

        // 根据策略排序
        self.sort_candidates_by_strategy(&mut candidates, &stats);

        candidates
    }

    /// 计算引擎综合评分
    fn calculate_engine_score(&self, support_score: f64, stats: &EngineStats) -> f64 {
        let mut score = support_score;

        // 成功率权重 (30%)
        score *= 0.3 + (stats.success_rate * 0.7);

        // 响应时间权重 (20%)
        let response_time_score = 1.0 - (stats.avg_response_time.as_secs_f64() / 10.0).min(1.0);
        score *= 0.8 + (response_time_score * 0.2);

        // 使用频率权重 (10%)
        let usage_penalty = (stats.usage_count as f64 / 1000.0).min(0.1);
        score *= 1.0 - usage_penalty;

        score
    }

    /// 根据策略排序候选引擎
    fn sort_candidates_by_strategy(
        &self,
        candidates: &mut Vec<(f64, Arc<dyn ScraperEngine>)>,
        stats: &std::collections::HashMap<String, EngineStats>,
    ) {
        match self.strategy {
            LoadBalancingStrategy::RoundRobin => {
                // 保持原有顺序，由外部轮询索引控制
            }
            LoadBalancingStrategy::WeightedRoundRobin => {
                // 按综合评分排序
                candidates
                    .sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
            }
            LoadBalancingStrategy::LeastConnections => {
                // 按使用次数升序排序
                candidates.sort_by(|a, b| {
                    let usage_a = stats.get(a.1.name()).map(|s| s.usage_count).unwrap_or(0);
                    let usage_b = stats.get(b.1.name()).map(|s| s.usage_count).unwrap_or(0);
                    usage_a.cmp(&usage_b)
                });
            }
            LoadBalancingStrategy::FastestResponse => {
                // 按响应时间升序排序
                candidates.sort_by(|a, b| {
                    let time_a = stats
                        .get(a.1.name())
                        .map(|s| s.avg_response_time)
                        .unwrap_or(Duration::MAX);
                    let time_b = stats
                        .get(b.1.name())
                        .map(|s| s.avg_response_time)
                        .unwrap_or(Duration::MAX);
                    time_a.cmp(&time_b)
                });
            }
            LoadBalancingStrategy::Random => {
                // 随机打乱
                candidates.shuffle(&mut rand::rng());
            }
            LoadBalancingStrategy::SmartHybrid => {
                // 智能混合策略：综合评分 + 最少使用 + 响应时间
                candidates.sort_by(|a, b| {
                    let score_a = a.0;
                    let score_b = b.0;

                    let usage_a = stats.get(a.1.name()).map(|s| s.usage_count).unwrap_or(0);
                    let usage_b = stats.get(b.1.name()).map(|s| s.usage_count).unwrap_or(0);

                    let time_a = stats
                        .get(a.1.name())
                        .map(|s| s.avg_response_time)
                        .unwrap_or(Duration::MAX);
                    let time_b = stats
                        .get(b.1.name())
                        .map(|s| s.avg_response_time)
                        .unwrap_or(Duration::MAX);

                    // 综合排序：评分优先，然后使用次数，最后响应时间
                    score_b
                        .partial_cmp(&score_a)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then_with(|| usage_a.cmp(&usage_b))
                        .then_with(|| time_a.cmp(&time_b))
                });
            }
        }
    }

    /// 更新引擎统计信息
    fn update_engine_stats(&self, engine_name: &str, success: bool, response_time: Duration) {
        let mut stats = self.engine_stats.write();
        if let Some(stat) = stats.get_mut(engine_name) {
            // 更新成功率
            let alpha = 0.1; // 平滑因子
            let current_success = if success { 1.0 } else { 0.0 };
            stat.success_rate = stat.success_rate * (1.0 - alpha) + current_success * alpha;

            // 更新平均响应时间
            let current_avg_ns = stat.avg_response_time.as_nanos() as f64;
            let response_ns = response_time.as_nanos() as f64;
            let new_avg_ns = current_avg_ns * (1.0 - alpha) + response_ns * alpha;
            stat.avg_response_time = Duration::from_nanos(new_avg_ns as u64);

            // 更新使用信息
            stat.last_used = Some(Instant::now());
            stat.usage_count += 1;
        }
    }

    /// 获取下一个轮询索引
    fn get_next_round_robin_index(&self, max_index: usize) -> usize {
        let mut index = self.round_robin_index.lock();
        let current = *index;
        *index = (*index + 1) % max_index;
        current
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
        let start_time = Instant::now();
        let mut last_error = None;

        // 选择最优引擎
        let mut candidates = self.select_optimal_engines(request);

        if candidates.is_empty() {
            warn!("No suitable engines available for request");
            return Err(EngineError::AllEnginesFailed);
        }

        // 轮询策略特殊处理
        if self.strategy == LoadBalancingStrategy::RoundRobin {
            let start_index = self.get_next_round_robin_index(candidates.len());
            candidates.rotate_left(start_index);
        }

        info!(
            "Selected {} candidate engines using {:?} strategy",
            candidates.len(),
            self.strategy
        );

        // 尝试每个引擎
        for (score, engine) in candidates {
            let engine_name = engine.name();
            info!(
                "Trying engine {} with score {:.2} for request to {}",
                engine_name, score, request.url
            );

            let engine_start = Instant::now();
            match engine.scrape(request).await {
                Ok(response) => {
                    let response_time = engine_start.elapsed();
                    self.update_engine_stats(engine_name, true, response_time);
                    self.circuit_breaker.record_success(engine_name);

                    info!(
                        "Engine {} succeeded in {:?}, total time: {:?}",
                        engine_name,
                        response_time,
                        start_time.elapsed()
                    );

                    return Ok(response);
                }
                Err(e) => {
                    let response_time = engine_start.elapsed();
                    self.update_engine_stats(engine_name, false, response_time);

                    if e.is_retryable() {
                        self.circuit_breaker.record_failure(engine_name);
                        warn!(
                            "Engine {} failed with retryable error: {}, trying next engine",
                            engine_name, e
                        );
                        last_error = Some(e);
                        continue;
                    }

                    warn!(
                        "Engine {} failed with non-retryable error: {}",
                        engine_name, e
                    );
                    return Err(e);
                }
            }
        }

        warn!("All engines failed for request to {}", request.url);
        Err(last_error.unwrap_or(EngineError::AllEnginesFailed))
    }

    /// 聚合多个引擎的搜索结果
    ///
    /// # 参数
    ///
    /// * `request` - 抓取请求
    ///
    /// # 返回值
    ///
    /// * `Ok(ScrapeResponse)` - 聚合后的抓取响应
    /// * `Err(EngineError)` - 如果所有引擎都失败
    pub async fn aggregate(&self, request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
        let candidates = self.select_optimal_engines(request);
        if candidates.is_empty() {
            return Err(EngineError::AllEnginesFailed);
        }

        let mut results = Vec::new();
        let mut errors = Vec::new();

        for (_, engine) in candidates {
            match engine.scrape(request).await {
                Ok(response) => results.push((engine.name().to_string(), response)),
                Err(e) => errors.push((engine.name().to_string(), e)),
            }
        }

        if results.is_empty() {
            return Err(EngineError::AllEnginesFailed);
        }

        // 简单的结果聚合：取第一个成功的结果，但在实际应用中可以合并多个结果
        // 这里我们选择第一个成功的结果作为基础，并记录其他成功的结果数量
        let (primary_name, primary_response) = results.remove(0);
        info!(
            "Aggregation: Primary result from {}, {} other successes",
            primary_name,
            results.len()
        );

        // 更新统计信息
        self.update_engine_stats(&primary_name, true, Duration::from_millis(0)); // 这里没有准确的时间，暂且忽略
        self.circuit_breaker.record_success(&primary_name);

        for (name, _) in results {
            self.update_engine_stats(&name, true, Duration::from_millis(0));
            self.circuit_breaker.record_success(&name);
        }

        for (name, _) in errors {
            self.update_engine_stats(&name, false, Duration::from_millis(0));
            // 注意：这里没有检查是否可重试，简单记录失败
            self.circuit_breaker.record_failure(&name);
        }

        Ok(primary_response)
    }

    /// 获取引擎统计信息
    pub fn get_engine_stats(&self) -> std::collections::HashMap<String, EngineStats> {
        self.engine_stats.read().clone()
    }

    /// 重置引擎统计信息
    pub fn reset_engine_stats(&self, engine_name: &str) {
        let mut stats = self.engine_stats.write();
        if let Some(stat) = stats.get_mut(engine_name) {
            *stat = EngineStats::default();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::reqwest_engine::ReqwestEngine;

    #[tokio::test]
    async fn test_engine_router_creation() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![Arc::new(ReqwestEngine)];
        let router = EngineRouter::new(engines);

        assert_eq!(router.strategy, LoadBalancingStrategy::SmartHybrid);
    }

    #[test]
    fn test_engine_score_calculation() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![];
        let router = EngineRouter::new(engines);

        let mut stats = EngineStats::default();
        stats.success_rate = 0.9;
        stats.avg_response_time = Duration::from_millis(200);
        stats.usage_count = 10;

        let score = router.calculate_engine_score(1.0, &stats);
        assert!(score > 0.8 && score <= 1.0);
    }
}

#[cfg(test)]
mod tests_impl {
    use super::*;
    use crate::engines::traits::EngineError;
    use crate::engines::traits::ScrapeRequest;
    use crate::engines::traits::ScrapeResponse;
    use crate::engines::traits::ScraperEngine;
    use async_trait::async_trait;
    use std::sync::Mutex;

    // A simple test engine that is a controllable implementation
    struct TestScraperEngineImpl {
        name: &'static str,
        _supported_domains: Vec<String>,
        _weight: u8,
        response_result: Mutex<Option<Result<ScrapeResponse, EngineError>>>,
    }

    impl TestScraperEngineImpl {
        fn new(
            name: &'static str,
            supported_domains: Vec<String>,
            weight: u8,
            result: Result<ScrapeResponse, EngineError>,
        ) -> Self {
            Self {
                name,
                _supported_domains: supported_domains,
                _weight: weight,
                response_result: Mutex::new(Some(result)),
            }
        }
    }

    #[async_trait]
    impl ScraperEngine for TestScraperEngineImpl {
        async fn scrape(&self, _request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
            let mut lock = self.response_result.lock().unwrap();
            if let Some(res) = lock.take() {
                // For concurrent tests, we might want to clone if we needed multiple calls,
                // but here we just return the pre-configured result.
                // Since Result is not Clone by default for EngineError, we have to be careful.
                // For this simple test implementation, we just consume it.
                // In a real scenario we might clone data.
                return res;
            }
            // Default if called more times than configured
            Ok(ScrapeResponse::new("http://example.com", "Default Result"))
        }

        fn support_score(&self, _request: &ScrapeRequest) -> u8 {
            100
        }

        fn name(&self) -> &'static str {
            self.name
        }
    }

    #[tokio::test]
    async fn test_aggregate_concurrent_search() {
        let engine1 = TestScraperEngineImpl::new(
            "engine1",
            vec!["example.com".to_string()],
            1,
            Ok(ScrapeResponse::new("http://example.com", "Result 1")),
        );

        let engine2 = TestScraperEngineImpl::new(
            "engine2",
            vec!["example.com".to_string()],
            1,
            Ok(ScrapeResponse::new("http://example.com", "Result 2")),
        );

        let router = EngineRouter::new(vec![Arc::new(engine1), Arc::new(engine2)]);

        let request = ScrapeRequest::new("http://example.com");
        let result = router.aggregate(&request).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.content.contains("Result"));
    }

    #[tokio::test]
    async fn test_aggregate_partial_failure() {
        let engine1 = TestScraperEngineImpl::new(
            "engine1",
            vec!["example.com".to_string()],
            1,
            Err(EngineError::Timeout),
        );

        let engine2 = TestScraperEngineImpl::new(
            "engine2",
            vec!["example.com".to_string()],
            1,
            Ok(ScrapeResponse::new("http://example.com", "Result 2")),
        );

        let router = EngineRouter::new(vec![Arc::new(engine1), Arc::new(engine2)]);

        let request = ScrapeRequest::new("http://example.com");
        let result = router.aggregate(&request).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.content, "Result 2");
    }
}
