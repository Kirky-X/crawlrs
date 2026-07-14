// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! EngineRouter - Internal routing logic for engine selection
//!
//! This module handles the internal routing logic for selecting appropriate
//! scraping engines based on request requirements.
//! This is an internal implementation detail.

use crate::engines::circuit_breaker::CircuitBreaker;
use crate::engines::engine_client::{
    EngineError, InternalScrapeRequest, InternalScrapeResponse, ScraperEngine,
};
use crate::engines::validators::validate_url;
use dashmap::DashMap;
use log::{info, warn};
use rand::seq::SliceRandom;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

// === Section: EngineRouterTrait Definition ===

/// Trait for EngineRouter - enables dependency injection
#[async_trait::async_trait]
pub trait EngineRouterTrait: Send + Sync {
    /// Route a request to the optimal engine
    async fn route(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError>;

    /// Aggregate results from multiple engines
    async fn aggregate(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError>;

    /// Get engine statistics
    fn get_engine_stats(&self) -> std::collections::HashMap<String, EngineStats>;

    /// Reset statistics for a specific engine
    fn reset_engine_stats(&self, engine_name: &str);

    /// Get list of registered engine names
    fn registered_engines(&self) -> Vec<String>;
}

/// 路由层指标收集器
///
/// 收集引擎路由过程中的各种指标，用于监控和优化
///
/// # 安全提示
///
/// 所有字段都是内部实现细节，仅对 crate 可见。
/// 外部模块应使用提供的公共方法访问聚合统计数据。
#[derive(Debug, Default)]
pub struct RouterMetrics {
    /// 总请求数
    pub(crate) total_requests: AtomicU64,
    /// 成功请求数
    pub(crate) successful_requests: AtomicU64,
    /// 失败请求数
    pub(crate) failed_requests: AtomicU64,
    /// 候选引擎数量统计
    pub(crate) candidate_count_total: AtomicU64,
    /// 尝试次数统计
    pub(crate) attempt_count_total: AtomicU64,
    /// 引擎选择次数
    pub(crate) engine_selection_total: AtomicU64,
    /// 按引擎名称的延迟统计 (引擎名 -> 总延迟纳秒) - 使用 DashMap 优化并发性能
    pub(crate) engine_latencies: Arc<DashMap<String, u64>>,
    /// 按引擎名称的成功次数 - 使用 DashMap 优化并发性能
    pub(crate) engine_success_count: Arc<DashMap<String, u64>>,
    /// 按引擎名称的失败次数 - 使用 DashMap 优化并发性能
    pub(crate) engine_failure_count: Arc<DashMap<String, u64>>,
    /// 失败类型统计 (错误类型 -> 次数) - 使用 DashMap 优化并发性能
    pub(crate) failure_classification: Arc<DashMap<String, u64>>,
}

impl RouterMetrics {
    /// 创建新的指标收集器
    pub fn new() -> Self {
        Self {
            total_requests: AtomicU64::new(0),
            successful_requests: AtomicU64::new(0),
            failed_requests: AtomicU64::new(0),
            candidate_count_total: AtomicU64::new(0),
            attempt_count_total: AtomicU64::new(0),
            engine_selection_total: AtomicU64::new(0),
            engine_latencies: Arc::new(DashMap::with_capacity(8)),
            engine_success_count: Arc::new(DashMap::with_capacity(8)),
            engine_failure_count: Arc::new(DashMap::with_capacity(8)),
            failure_classification: Arc::new(DashMap::with_capacity(8)),
        }
    }

    /// 安全获取 latencies (DashMap 不需要 async 锁)
    fn latencies(&self) -> &DashMap<String, u64> {
        &self.engine_latencies
    }

    /// 安全获取 success_count (DashMap 不需要 async 锁)
    fn success_count(&self) -> &DashMap<String, u64> {
        &self.engine_success_count
    }

    /// 安全获取 failure_count (DashMap 不需要 async 锁)
    fn failure_count(&self) -> &DashMap<String, u64> {
        &self.engine_failure_count
    }

    /// 安全获取 classification (DashMap 不需要 async 锁)
    fn classification(&self) -> &DashMap<String, u64> {
        &self.failure_classification
    }

    /// 对错误进行分类
    fn classify_error(error_type: &str) -> String {
        let lower = error_type.to_lowercase();
        if lower.contains("timeout") {
            "timeout".to_string()
        } else if lower.contains("ssrf") {
            "ssrf_protection".to_string()
        } else if lower.contains("network") {
            "network_error".to_string()
        } else if lower.contains("circuit") {
            "circuit_breaker".to_string()
        } else if lower.contains("browser") {
            "browser_error".to_string()
        } else {
            "other".to_string()
        }
    }

    /// 记录候选引擎数量
    pub fn record_candidates(&self, count: usize) {
        self.candidate_count_total
            .fetch_add(count as u64, Ordering::Relaxed);
    }

    /// 记录单次尝试
    pub fn record_attempt(&self) {
        self.attempt_count_total.fetch_add(1, Ordering::Relaxed);
    }

    /// 记录引擎选择
    pub fn record_engine_selection(&self, engine_name: &str) {
        self.engine_selection_total.fetch_add(1, Ordering::Relaxed);
        // DashMap: 直接插入，自动处理并发
        self.latencies().insert(engine_name.to_string(), 0);
    }

    /// 记录引擎延迟
    pub fn record_engine_latency(&self, engine_name: &str, duration: Duration) {
        // DashMap: 使用 modify 进行原子更新
        let total_ns = duration.as_nanos() as u64;
        if let Some(mut count) = self.latencies().get_mut(engine_name) {
            *count += total_ns;
        }
    }

    /// 记录引擎成功
    pub fn record_engine_success(&self, engine_name: &str) {
        // DashMap: 使用 get_mut 进行原子更新
        if let Some(mut count) = self.success_count().get_mut(engine_name) {
            *count += 1;
        }
    }

    /// 记录引擎失败
    pub fn record_engine_failure(&self, engine_name: &str, error_type: &str) {
        // DashMap: 使用 get_mut 进行原子更新
        if let Some(mut count) = self.failure_count().get_mut(engine_name) {
            *count += 1;
        }

        let error_category = Self::classify_error(error_type);
        if let Some(mut count) = self.classification().get_mut(&error_category) {
            *count += 1;
        }
    }

    /// 获取按引擎名称的平均延迟（纳秒）
    pub fn get_avg_latency_ns(&self, engine_name: &str) -> Option<u64> {
        // DashMap: 并发读取
        let latencies = self.latencies();
        let success_count = self.success_count();

        if let (Some(total_ns), Some(count)) =
            (latencies.get(engine_name), success_count.get(engine_name))
        {
            return total_ns.checked_div(*count);
        }
        None
    }

    /// 获取成功率
    pub fn get_success_rate(&self) -> f64 {
        let total = self.total_requests.load(Ordering::Relaxed);
        if total == 0 {
            return 1.0;
        }
        self.successful_requests.load(Ordering::Relaxed) as f64 / total as f64
    }
}

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
    /// 路由层指标
    metrics: Arc<RouterMetrics>,
    /// 最大引擎尝试次数
    max_engine_attempts: usize,
    /// 最大重试次数 (总请求时间限制)
    max_retries: usize,
    /// 是否启用特征检测过滤
    feature_filter_enabled: bool,
    /// 是否启用并发竞速模式
    race_mode_enabled: bool,
    /// 动态阈值因子 (根据历史数据调整)
    dynamic_threshold_factor: f64,
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
        let mut engine_stats = std::collections::HashMap::with_capacity(8);
        for engine in &engines {
            engine_stats.insert(engine.name().to_string(), EngineStats::default());
        }

        Self {
            engines,
            circuit_breaker: Arc::new(CircuitBreaker::new()),
            engine_stats: Arc::new(parking_lot::RwLock::new(engine_stats)),
            round_robin_index: Arc::new(parking_lot::Mutex::new(0)),
            strategy: LoadBalancingStrategy::SmartHybrid,
            metrics: Arc::new(RouterMetrics::new()),
            max_engine_attempts: 3,
            max_retries: 5,                // 默认最大重试次数
            feature_filter_enabled: true,  // 默认启用特征检测过滤
            race_mode_enabled: false,      // 默认禁用并发竞速模式
            dynamic_threshold_factor: 1.0, // 默认动态阈值因子
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
        let mut engine_stats = std::collections::HashMap::with_capacity(8);
        for engine in &engines {
            engine_stats.insert(engine.name().to_string(), EngineStats::default());
        }

        Self {
            engines,
            circuit_breaker,
            engine_stats: Arc::new(parking_lot::RwLock::new(engine_stats)),
            round_robin_index: Arc::new(parking_lot::Mutex::new(0)),
            strategy,
            metrics: Arc::new(RouterMetrics::new()),
            max_engine_attempts: 3,
            max_retries: 5,
            feature_filter_enabled: true,
            race_mode_enabled: false,
            dynamic_threshold_factor: 1.0,
        }
    }

    pub fn set_max_engine_attempts(&mut self, attempts: usize) {
        self.max_engine_attempts = attempts.max(1);
    }

    /// 设置最大重试次数 (用于限制总请求时间)
    pub fn set_max_retries(&mut self, retries: usize) {
        self.max_retries = retries.max(1);
    }

    /// 启用/禁用特征检测过滤
    pub fn set_feature_filter_enabled(&mut self, enabled: bool) {
        self.feature_filter_enabled = enabled;
    }

    /// 启用/禁用并发竞速模式
    pub fn set_race_mode_enabled(&mut self, enabled: bool) {
        self.race_mode_enabled = enabled;
    }

    /// 设置动态阈值因子
    pub fn set_dynamic_threshold_factor(&mut self, factor: f64) {
        self.dynamic_threshold_factor = factor.clamp(0.1, 2.0);
    }

    /// 设置负载均衡策略
    pub fn set_strategy(&mut self, strategy: LoadBalancingStrategy) {
        self.strategy = strategy;
    }

    /// 获取路由层指标
    pub fn metrics(&self) -> &Arc<RouterMetrics> {
        &self.metrics
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
        request: &InternalScrapeRequest,
    ) -> Vec<(f64, Arc<dyn ScraperEngine>)> {
        let mut candidates = Vec::new();

        // First pass: collect engine info without holding lock for circuit breaker checks
        let engine_infos: Vec<_> = self.engines.iter().enumerate().collect();

        for (_, engine) in &engine_infos {
            let engine_name = engine.name();

            // Check circuit breaker status FIRST (outside of stats lock)
            if self.circuit_breaker.is_open(engine_name) {
                continue;
            }

            // Feature detection filtering
            if self.feature_filter_enabled {
                if let Some(reason) = self.should_filter_by_feature(request, engine) {
                    log::debug!(
                        "Engine {} filtered by feature detection: {}",
                        engine_name,
                        reason
                    );
                    continue;
                }
            }

            // Get support score
            let support_score = engine.support_score(request) as f64;
            if support_score == 0.0 {
                continue;
            }

            candidates.push((support_score, engine_name.to_string(), Arc::clone(engine)));
        }

        // Second pass: calculate scores with stats (short lock hold)
        let stats = self.engine_stats.read();
        let mut scored_candidates = Vec::new();

        for (support_score, engine_name, engine) in candidates {
            // Get engine stats
            let default_stats = EngineStats::default();
            let engine_stat = stats.get(&engine_name).unwrap_or(&default_stats);

            // Apply dynamic threshold factor
            let adjusted_score = support_score * self.dynamic_threshold_factor;

            // Calculate final score
            let final_score = self.calculate_engine_score(adjusted_score, engine_stat);

            scored_candidates.push((final_score, engine));
        }

        // Sort by strategy
        self.sort_candidates_by_strategy(&mut scored_candidates, &stats);

        scored_candidates
    }

    /// 特征检测过滤
    /// 根据请求特征直接过滤不适合的引擎（使用能力方法替代硬编码引擎名）
    fn should_filter_by_feature(
        &self,
        request: &InternalScrapeRequest,
        engine: &Arc<dyn ScraperEngine>,
    ) -> Option<String> {
        // 如果需要截图，排除得分很低的引擎
        if request.needs_screenshot && engine.support_score(request) < 50 {
            return Some(format!(
                "Engine {} does not support screenshots",
                engine.name()
            ));
        }

        // 如果需要 JS 或交互动作，排除得分很低的引擎
        if (request.needs_js || !request.actions.is_empty()) && engine.support_score(request) < 50 {
            return Some(format!(
                "Engine {} does not support JavaScript",
                engine.name()
            ));
        }

        // 如果明确需要 TLS 指纹，检查得分
        if request.needs_tls_fingerprint && engine.support_score(request) < 50 {
            return Some(format!(
                "Engine {} is not optimized for TLS fingerprinting",
                engine.name()
            ));
        }

        None
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
    pub async fn _route_impl(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        if let Err(e) = validate_url(&request.url).await {
            return Err(EngineError::SsrfProtection(e.to_string()));
        }

        let timeout = request.timeout;

        // Wrap the entire operation with timeout
        tokio::time::timeout(timeout, self.route_internal(request))
            .await
            .map_err(|_| EngineError::Timeout(timeout))
            .and_then(|result| result)
    }

    /// Internal route implementation without timeout
    async fn route_internal(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        let start_time = Instant::now();

        // 选择最优引擎
        let mut candidates = self.select_optimal_engines(request);

        // 记录候选引擎数量
        self.metrics.record_candidates(candidates.len());

        if candidates.is_empty() {
            warn!("No suitable engines available for request");
            self.metrics.failed_requests.fetch_add(1, Ordering::Relaxed);
            return Err(EngineError::AllEnginesFailed(
                "No suitable engines available".to_string(),
            ));
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

        // 并发竞速模式
        if self.race_mode_enabled && candidates.len() > 1 {
            return self.route_race_mode(request, candidates, start_time).await;
        }

        // 传统顺序模式 (带 max_retries 限制)
        let max_attempts = self.max_engine_attempts.max(1).min(candidates.len());
        let max_retries = self.max_retries.max(1);
        let mut total_attempts = 0;
        let mut last_error = None;

        for (score, engine) in candidates.into_iter().take(max_attempts) {
            total_attempts += 1;
            let engine_name = engine.name();

            // 记录引擎选择
            self.metrics.record_engine_selection(engine_name);
            self.metrics.record_attempt();

            info!(
                "Trying engine {} with score {:.2} for request to {}",
                engine_name, score, request.url
            );

            let remaining = request
                .timeout
                .checked_sub(start_time.elapsed())
                .unwrap_or(Duration::from_millis(0));
            if remaining.is_zero() {
                return Err(EngineError::Timeout(request.timeout));
            }

            let attempt_request = InternalScrapeRequest {
                url: request.url.clone(),
                method: request.method,
                headers: request.headers.clone(),
                timeout: remaining,
                needs_js: request.needs_js,
                needs_screenshot: request.needs_screenshot,
                screenshot_config: request.screenshot_config.clone(),
                mobile: request.mobile,
                proxy: request.proxy.clone(),
                skip_tls_verification: request.skip_tls_verification,
                needs_tls_fingerprint: request.needs_tls_fingerprint,
                use_fire_engine: request.use_fire_engine,
                actions: request.actions.clone(),
                body: request.body.clone(),
                sync_wait_ms: request.sync_wait_ms,
            };

            let engine_start = Instant::now();
            match engine.scrape(&attempt_request).await {
                Ok(response) => {
                    let response_time = engine_start.elapsed();
                    self.update_engine_stats(engine_name, true, response_time);
                    self.circuit_breaker.record_success(engine_name);

                    // 记录成功指标
                    self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
                    self.metrics
                        .successful_requests
                        .fetch_add(1, Ordering::Relaxed);
                    self.metrics
                        .record_engine_latency(engine_name, response_time);
                    self.metrics.record_engine_success(engine_name);

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

                    // 记录失败指标
                    self.metrics
                        .record_engine_failure(engine_name, &e.to_string());

                    if e.is_retryable() {
                        self.circuit_breaker.record_failure(engine_name);
                        warn!(
                            "Engine {} failed with retryable error: {}, trying next engine",
                            engine_name, e
                        );
                        last_error = Some(e);

                        // 检查是否超过最大重试次数
                        if total_attempts >= max_retries {
                            warn!("Max retries {} reached, failing request", max_retries);
                            self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
                            self.metrics.failed_requests.fetch_add(1, Ordering::Relaxed);
                            return Err(last_error.unwrap_or_else(|| {
                                EngineError::AllEnginesFailed("Max retries reached".to_string())
                            }));
                        }
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
        self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
        self.metrics.failed_requests.fetch_add(1, Ordering::Relaxed);
        Err(last_error
            .unwrap_or_else(|| EngineError::AllEnginesFailed("All engines failed".to_string())))
    }

    /// 并发竞速模式：同时发起多个引擎请求，返回最快成功的那个
    async fn route_race_mode(
        &self,
        request: &InternalScrapeRequest,
        candidates: Vec<(f64, Arc<dyn ScraperEngine>)>,
        start_time: Instant,
    ) -> Result<InternalScrapeResponse, EngineError> {
        use futures::future;
        use tokio::time;

        let remaining = request
            .timeout
            .checked_sub(start_time.elapsed())
            .unwrap_or(Duration::from_millis(0));

        if remaining.is_zero() {
            return Err(EngineError::Timeout(request.timeout));
        }

        // 限制竞速引擎数量
        let race_candidates: Vec<_> = candidates.into_iter().take(3).collect();

        info!(
            "Race mode: launching {} engines concurrently for {}",
            race_candidates.len(),
            request.url
        );

        // 创建竞速任务 (使用 Box::pin 解决 Unpin 问题)
        let mut race_futures: Vec<std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>>> =
            Vec::new();

        for (_score, engine) in race_candidates {
            let engine_name = engine.name().to_string();
            let engine_clone = engine.clone();

            let remaining_clone = remaining;
            let request_clone = InternalScrapeRequest {
                url: request.url.clone(),
                method: request.method,
                headers: request.headers.clone(),
                timeout: remaining_clone,
                needs_js: request.needs_js,
                needs_screenshot: request.needs_screenshot,
                screenshot_config: request.screenshot_config.clone(),
                mobile: request.mobile,
                proxy: request.proxy.clone(),
                skip_tls_verification: request.skip_tls_verification,
                needs_tls_fingerprint: request.needs_tls_fingerprint,
                use_fire_engine: request.use_fire_engine,
                actions: request.actions.clone(),
                body: request.body.clone(),
                sync_wait_ms: request.sync_wait_ms,
            };

            let race_future: std::pin::Pin<Box<dyn std::future::Future<Output = _> + Send>> =
                Box::pin(async move {
                    let engine_start = Instant::now();
                    match engine_clone.scrape(&request_clone).await {
                        Ok(response) => Ok((engine_name, response, engine_start.elapsed())),
                        Err(e) => Err((engine_name, e)),
                    }
                });

            race_futures.push(race_future);
        }

        // 并发执行，返回最快成功的
        let timeout_duration = remaining.max(Duration::from_millis(100));

        // 使用 SelectAll 进行竞速
        let select_all_future = future::select_all(race_futures);

        match time::timeout(timeout_duration, select_all_future).await {
            Ok((result, _index, _others)) => {
                match result {
                    Ok((engine_name, response, response_time)) => {
                        self.update_engine_stats(&engine_name, true, response_time);
                        self.circuit_breaker.record_success(&engine_name);
                        self.metrics
                            .successful_requests
                            .fetch_add(1, Ordering::Relaxed);
                        self.metrics
                            .record_engine_latency(&engine_name, response_time);
                        self.metrics.record_engine_success(&engine_name);

                        info!(
                            "Race mode: {} won in {:?}, total time: {:?}",
                            engine_name,
                            response_time,
                            start_time.elapsed()
                        );

                        // 取消其他正在进行的任务
                        Ok(response)
                    }
                    Err((engine_name, e)) => {
                        self.metrics
                            .record_engine_failure(&engine_name, &e.to_string());

                        if e.is_retryable() {
                            self.circuit_breaker.record_failure(&engine_name);
                            Err(e)
                        } else {
                            Err(e)
                        }
                    }
                }
            }
            Err(_) => {
                // 超时
                warn!(
                    "Race mode timed out after {:?} for request to {}",
                    timeout_duration, request.url
                );
                Err(EngineError::Timeout(timeout_duration))
            }
        }
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
    pub async fn _aggregate_impl(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        let candidates = self.select_optimal_engines(request);
        if candidates.is_empty() {
            return Err(EngineError::AllEnginesFailed(
                "All engines failed".to_string(),
            ));
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
            return Err(EngineError::AllEnginesFailed(
                "All engines failed in aggregate".to_string(),
            ));
        }

        // 简单的结果聚合：取第一个成功的结果，但在实际应用中可以合并多个结果
        // 这里我们选择第一个成功的结果作为基础，并记录其他成功的结果数量
        let (primary_name, primary_response) = results.remove(0);
        info!(
            "Aggregation: Primary result from {}, {} other successes",
            primary_name,
            results.len()
        );

        self.circuit_breaker.record_success(&primary_name);

        for (name, _) in results {
            self.circuit_breaker.record_success(&name);
        }

        for (name, error) in errors {
            if error.is_retryable() {
                self.circuit_breaker.record_failure(&name);
            }
        }

        Ok(primary_response)
    }

    /// 获取引擎统计信息
    pub fn _get_engine_stats_impl(&self) -> std::collections::HashMap<String, EngineStats> {
        self.engine_stats.read().clone()
    }

    /// 重置引擎统计信息
    pub fn _reset_engine_stats_impl(&self, engine_name: &str) {
        let mut stats = self.engine_stats.write();
        if let Some(stat) = stats.get_mut(engine_name) {
            *stat = EngineStats::default();
        }
    }

    /// 注册引擎
    pub fn register_engine(&mut self, engine: Arc<dyn ScraperEngine>) {
        let name = engine.name().to_string();
        self.engines.push(engine);
        self.engine_stats
            .write()
            .insert(name.clone(), EngineStats::default());
        info!("引擎已注册: {}", name);
    }

    /// 获取所有已注册的引擎名称
    pub fn _registered_engines_impl(&self) -> Vec<String> {
        self.engines.iter().map(|e| e.name().to_string()).collect()
    }

    /// Get all registered engines (internal use only)
    #[doc(hidden)]
    pub fn get_engines(&self) -> &Vec<Arc<dyn ScraperEngine>> {
        &self.engines
    }

    /// Public wrapper for route (for backward compatibility)
    pub async fn route(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        self._route_impl(request).await
    }

    /// Public wrapper for aggregate (for backward compatibility)
    pub async fn aggregate(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        self._aggregate_impl(request).await
    }

    /// Public wrapper for get_engine_stats (for backward compatibility)
    pub fn get_engine_stats(&self) -> std::collections::HashMap<String, EngineStats> {
        self._get_engine_stats_impl()
    }

    /// Public wrapper for reset_engine_stats (for backward compatibility)
    pub fn reset_engine_stats(&self, engine_name: &str) {
        self._reset_engine_stats_impl(engine_name)
    }

    /// Public wrapper for registered_engines (for backward compatibility)
    pub fn registered_engines(&self) -> Vec<String> {
        self._registered_engines_impl()
    }
}

#[async_trait::async_trait]
impl EngineRouterTrait for EngineRouter {
    async fn route(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        self._route_impl(request).await
    }

    async fn aggregate(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        self._aggregate_impl(request).await
    }

    fn get_engine_stats(&self) -> std::collections::HashMap<String, EngineStats> {
        self._get_engine_stats_impl()
    }

    fn reset_engine_stats(&self, engine_name: &str) {
        self._reset_engine_stats_impl(engine_name)
    }

    fn registered_engines(&self) -> Vec<String> {
        self._registered_engines_impl()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::client::reqwest::ReqwestEngine;
    use async_trait::async_trait;
    use std::collections::HashMap;

    #[tokio::test]
    async fn test_engine_router_creation() {
        let http_client = Arc::new(
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap(),
        );
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![Arc::new(ReqwestEngine::new(http_client))];
        let router = EngineRouter::new(engines);

        assert_eq!(router.strategy, LoadBalancingStrategy::SmartHybrid);
    }

    #[tokio::test]
    async fn test_route_respects_max_engine_attempts() {
        struct CountingEngine {
            name: &'static str,
            calls: Arc<std::sync::atomic::AtomicU32>,
            ok: bool,
        }

        #[async_trait]
        impl ScraperEngine for CountingEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                if self.ok {
                    Ok(InternalScrapeResponse {
                        status_code: 200,
                        content: "ok".to_string(),
                        screenshot: None,
                        content_type: "text/html".to_string(),
                        headers: HashMap::new(),
                        response_time_ms: 10,
                    })
                } else {
                    Err(EngineError::Timeout(Duration::from_millis(10)))
                }
            }

            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }

            fn name(&self) -> &'static str {
                self.name
            }
        }

        let c1 = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let c2 = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let c3 = Arc::new(std::sync::atomic::AtomicU32::new(0));

        let e1: Arc<dyn ScraperEngine> = Arc::new(CountingEngine {
            name: "e1",
            calls: c1.clone(),
            ok: false,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(CountingEngine {
            name: "e2",
            calls: c2.clone(),
            ok: false,
        });
        let e3: Arc<dyn ScraperEngine> = Arc::new(CountingEngine {
            name: "e3",
            calls: c3.clone(),
            ok: true,
        });

        let mut router = EngineRouter::new(vec![e1, e2, e3]);
        router.set_strategy(LoadBalancingStrategy::RoundRobin);
        router.set_max_engine_attempts(2);

        let request = InternalScrapeRequest {
            url: "http://1.1.1.1".to_string(),
            method: crate::engines::engine_client::HttpMethod::Get,
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
            actions: Vec::new(),
            body: None,
            sync_wait_ms: 0,
        };
        let result = router.route(&request).await;

        assert!(result.is_err());
        assert_eq!(c1.load(Ordering::SeqCst), 1);
        assert_eq!(c2.load(Ordering::SeqCst), 1);
        assert_eq!(c3.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_engine_score_calculation() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![];
        let router = EngineRouter::new(engines);

        let stats = EngineStats {
            success_rate: 0.9,
            avg_response_time: Duration::from_millis(200),
            usage_count: 10,
            last_used: None,
        };

        let score = router.calculate_engine_score(1.0, &stats);
        assert!(score > 0.8 && score <= 1.0);
    }

    // === Mock engine with controllable support score ===

    struct MockEngine {
        engine_name: &'static str,
        score: u8,
    }

    #[async_trait]
    impl ScraperEngine for MockEngine {
        async fn scrape(
            &self,
            _request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            Ok(InternalScrapeResponse {
                status_code: 200,
                content: "mock".to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 10,
            })
        }

        fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
            self.score
        }

        fn name(&self) -> &'static str {
            self.engine_name
        }
    }

    fn make_request() -> InternalScrapeRequest {
        InternalScrapeRequest {
            url: "http://example.com".to_string(),
            method: crate::engines::engine_client::HttpMethod::Get,
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
            actions: Vec::new(),
            body: None,
            sync_wait_ms: 0,
        }
    }

    // === should_filter_by_feature tests ===

    #[test]
    fn test_should_filter_by_feature_screenshot_low_score() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "low-score",
            score: 30,
        });
        let router = EngineRouter::new(vec![]);
        let mut request = make_request();
        request.needs_screenshot = true;
        let result = router.should_filter_by_feature(&request, &engine);
        assert!(result.is_some());
        assert!(result.unwrap().contains("screenshots"));
    }

    #[test]
    fn test_should_filter_by_feature_screenshot_high_score() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "high-score",
            score: 80,
        });
        let router = EngineRouter::new(vec![]);
        let mut request = make_request();
        request.needs_screenshot = true;
        let result = router.should_filter_by_feature(&request, &engine);
        assert!(result.is_none());
    }

    #[test]
    fn test_should_filter_by_feature_js_low_score() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "low-score",
            score: 20,
        });
        let router = EngineRouter::new(vec![]);
        let mut request = make_request();
        request.needs_js = true;
        let result = router.should_filter_by_feature(&request, &engine);
        assert!(result.is_some());
        assert!(result.unwrap().contains("JavaScript"));
    }

    #[test]
    fn test_should_filter_by_feature_actions_low_score() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "low-score",
            score: 10,
        });
        let router = EngineRouter::new(vec![]);
        let mut request = make_request();
        request.actions = vec![crate::engines::engine_client::InternalPageAction::Click {
            selector: "#btn".to_string(),
        }];
        let result = router.should_filter_by_feature(&request, &engine);
        assert!(result.is_some());
        assert!(result.unwrap().contains("JavaScript"));
    }

    #[test]
    fn test_should_filter_by_feature_tls_fingerprint_low_score() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "low-score",
            score: 40,
        });
        let router = EngineRouter::new(vec![]);
        let mut request = make_request();
        request.needs_tls_fingerprint = true;
        let result = router.should_filter_by_feature(&request, &engine);
        assert!(result.is_some());
        assert!(result.unwrap().contains("TLS fingerprinting"));
    }

    #[test]
    fn test_should_filter_by_feature_tls_fingerprint_high_score() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "high-score",
            score: 60,
        });
        let router = EngineRouter::new(vec![]);
        let mut request = make_request();
        request.needs_tls_fingerprint = true;
        let result = router.should_filter_by_feature(&request, &engine);
        assert!(result.is_none());
    }

    #[test]
    fn test_should_filter_by_feature_no_special_needs() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "any",
            score: 10,
        });
        let router = EngineRouter::new(vec![]);
        let request = make_request();
        let result = router.should_filter_by_feature(&request, &engine);
        assert!(result.is_none());
    }

    // === sort_candidates_by_strategy tests ===

    #[test]
    fn test_sort_round_robin_preserves_order() {
        let e1: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e1",
            score: 100,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e2",
            score: 100,
        });
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_strategy(LoadBalancingStrategy::RoundRobin);
        let stats = std::collections::HashMap::new();
        let mut candidates: Vec<(f64, Arc<dyn ScraperEngine>)> = vec![
            (1.0, router.engines[0].clone()),
            (2.0, router.engines[1].clone()),
        ];
        let original_names: Vec<_> = candidates.iter().map(|(_, e)| e.name()).collect();
        router.sort_candidates_by_strategy(&mut candidates, &stats);
        let sorted_names: Vec<_> = candidates.iter().map(|(_, e)| e.name()).collect();
        assert_eq!(original_names, sorted_names);
    }

    #[test]
    fn test_sort_weighted_round_robin_by_score() {
        let e1: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e1",
            score: 100,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e2",
            score: 100,
        });
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_strategy(LoadBalancingStrategy::WeightedRoundRobin);
        let stats = std::collections::HashMap::new();
        let mut candidates: Vec<(f64, Arc<dyn ScraperEngine>)> = vec![
            (0.5, router.engines[0].clone()),
            (0.9, router.engines[1].clone()),
        ];
        router.sort_candidates_by_strategy(&mut candidates, &stats);
        assert_eq!(candidates[0].1.name(), "e2");
        assert_eq!(candidates[1].1.name(), "e1");
    }

    #[test]
    fn test_sort_least_connections_by_usage() {
        let e1: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e1",
            score: 100,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e2",
            score: 100,
        });
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_strategy(LoadBalancingStrategy::LeastConnections);
        let mut stats = std::collections::HashMap::new();
        stats.insert(
            "e1".to_string(),
            EngineStats {
                usage_count: 100,
                ..Default::default()
            },
        );
        stats.insert(
            "e2".to_string(),
            EngineStats {
                usage_count: 5,
                ..Default::default()
            },
        );
        let mut candidates: Vec<(f64, Arc<dyn ScraperEngine>)> = vec![
            (1.0, router.engines[0].clone()),
            (1.0, router.engines[1].clone()),
        ];
        router.sort_candidates_by_strategy(&mut candidates, &stats);
        assert_eq!(candidates[0].1.name(), "e2");
        assert_eq!(candidates[1].1.name(), "e1");
    }

    #[test]
    fn test_sort_fastest_response_by_time() {
        let e1: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e1",
            score: 100,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e2",
            score: 100,
        });
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_strategy(LoadBalancingStrategy::FastestResponse);
        let mut stats = std::collections::HashMap::new();
        stats.insert(
            "e1".to_string(),
            EngineStats {
                avg_response_time: Duration::from_millis(500),
                ..Default::default()
            },
        );
        stats.insert(
            "e2".to_string(),
            EngineStats {
                avg_response_time: Duration::from_millis(100),
                ..Default::default()
            },
        );
        let mut candidates: Vec<(f64, Arc<dyn ScraperEngine>)> = vec![
            (1.0, router.engines[0].clone()),
            (1.0, router.engines[1].clone()),
        ];
        router.sort_candidates_by_strategy(&mut candidates, &stats);
        assert_eq!(candidates[0].1.name(), "e2");
    }

    #[test]
    fn test_sort_random_shuffles() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![
            Arc::new(MockEngine {
                engine_name: "e1",
                score: 100,
            }),
            Arc::new(MockEngine {
                engine_name: "e2",
                score: 100,
            }),
            Arc::new(MockEngine {
                engine_name: "e3",
                score: 100,
            }),
        ];
        let mut router = EngineRouter::new(engines);
        router.set_strategy(LoadBalancingStrategy::Random);
        let stats = std::collections::HashMap::new();
        let mut candidates: Vec<(f64, Arc<dyn ScraperEngine>)> =
            router.engines.iter().map(|e| (1.0, e.clone())).collect();
        router.sort_candidates_by_strategy(&mut candidates, &stats);
        // Random may or may not change order, just verify no panic
        assert_eq!(candidates.len(), 3);
    }

    #[test]
    fn test_sort_smart_hybrid_combined() {
        let e1: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e1",
            score: 100,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e2",
            score: 100,
        });
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_strategy(LoadBalancingStrategy::SmartHybrid);
        let mut stats = std::collections::HashMap::new();
        stats.insert(
            "e1".to_string(),
            EngineStats {
                success_rate: 0.5,
                avg_response_time: Duration::from_millis(800),
                usage_count: 50,
                last_used: None,
            },
        );
        stats.insert(
            "e2".to_string(),
            EngineStats {
                success_rate: 0.95,
                avg_response_time: Duration::from_millis(100),
                usage_count: 5,
                last_used: None,
            },
        );
        let mut candidates: Vec<(f64, Arc<dyn ScraperEngine>)> = vec![
            (0.6, router.engines[0].clone()),
            (0.9, router.engines[1].clone()),
        ];
        router.sort_candidates_by_strategy(&mut candidates, &stats);
        assert_eq!(candidates[0].1.name(), "e2");
    }

    // === update_engine_stats tests ===

    #[test]
    fn test_update_engine_stats_success() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "test",
            score: 100,
        });
        let router = EngineRouter::new(vec![engine]);
        router.update_engine_stats("test", true, Duration::from_millis(100));
        let stats = router.get_engine_stats();
        let stat = stats.get("test").unwrap();
        assert!(stat.success_rate > 0.9);
        assert_eq!(stat.usage_count, 1);
        assert!(stat.last_used.is_some());
    }

    #[test]
    fn test_update_engine_stats_failure() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "test",
            score: 100,
        });
        let router = EngineRouter::new(vec![engine]);
        router.update_engine_stats("test", false, Duration::from_millis(500));
        let stats = router.get_engine_stats();
        let stat = stats.get("test").unwrap();
        assert!(stat.success_rate < 1.0);
        assert_eq!(stat.usage_count, 1);
    }

    #[test]
    fn test_update_engine_stats_nonexistent() {
        let router = EngineRouter::new(vec![]);
        router.update_engine_stats("nonexistent", true, Duration::from_millis(50));
        // Should not panic
    }

    // === get_next_round_robin_index tests ===

    #[test]
    fn test_get_next_round_robin_index_wraps() {
        let router = EngineRouter::new(vec![]);
        let idx1 = router.get_next_round_robin_index(3);
        let idx2 = router.get_next_round_robin_index(3);
        let idx3 = router.get_next_round_robin_index(3);
        let idx4 = router.get_next_round_robin_index(3);
        assert_eq!(idx1, 0);
        assert_eq!(idx2, 1);
        assert_eq!(idx3, 2);
        assert_eq!(idx4, 0);
    }

    #[test]
    fn test_get_next_round_robin_index_single() {
        let router = EngineRouter::new(vec![]);
        let idx = router.get_next_round_robin_index(1);
        assert_eq!(idx, 0);
    }

    // === reset_engine_stats tests ===

    #[test]
    fn test_reset_engine_stats() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "test",
            score: 100,
        });
        let router = EngineRouter::new(vec![engine]);
        router.update_engine_stats("test", false, Duration::from_millis(500));
        let stats_before = router.get_engine_stats();
        assert_eq!(stats_before.get("test").unwrap().usage_count, 1);
        router.reset_engine_stats("test");
        let stats_after = router.get_engine_stats();
        let stat = stats_after.get("test").unwrap();
        assert_eq!(stat.usage_count, 0);
        assert_eq!(stat.success_rate, 1.0);
    }

    #[test]
    fn test_reset_engine_stats_nonexistent() {
        let router = EngineRouter::new(vec![]);
        router.reset_engine_stats("nonexistent");
        // Should not panic
    }

    // === register_engine tests ===

    #[test]
    fn test_register_engine() {
        let mut router = EngineRouter::new(vec![]);
        assert!(router.get_engine_stats().is_empty());
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "new-engine",
            score: 100,
        });
        router.register_engine(engine);
        assert!(router.get_engine_stats().contains_key("new-engine"));
        assert_eq!(router.registered_engines(), vec!["new-engine".to_string()]);
    }

    // === RouterMetrics tests ===

    #[test]
    fn test_router_metrics_new() {
        let metrics = RouterMetrics::new();
        assert_eq!(metrics.total_requests.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.successful_requests.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.failed_requests.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_router_metrics_record_candidates() {
        let metrics = RouterMetrics::new();
        metrics.record_candidates(5);
        metrics.record_candidates(3);
        assert_eq!(metrics.candidate_count_total.load(Ordering::Relaxed), 8);
    }

    #[test]
    fn test_router_metrics_record_attempt() {
        let metrics = RouterMetrics::new();
        metrics.record_attempt();
        metrics.record_attempt();
        metrics.record_attempt();
        assert_eq!(metrics.attempt_count_total.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_router_metrics_record_engine_selection() {
        let metrics = RouterMetrics::new();
        metrics.record_engine_selection("engine1");
        assert_eq!(metrics.engine_selection_total.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_router_metrics_record_engine_latency() {
        let metrics = RouterMetrics::new();
        metrics.record_engine_selection("engine1");
        metrics.record_engine_latency("engine1", Duration::from_millis(100));
        let avg = metrics.get_avg_latency_ns("engine1");
        // avg_latency = total_ns / success_count; success_count is 0, so None or Some
        // record_engine_selection inserts into latencies with 0, but success_count has no entry
        // get_avg_latency_ns checks both latencies and success_count
        // Since success_count has no entry, avg should be None
        assert!(avg.is_none());
    }

    #[test]
    fn test_router_metrics_record_engine_success() {
        let metrics = RouterMetrics::new();
        // Pre-initialize success_count entry (record_engine_success only increments existing keys)
        metrics
            .engine_success_count
            .insert("engine1".to_string(), 0);
        metrics.record_engine_success("engine1");
        metrics.record_engine_success("engine1");
        let count = metrics.engine_success_count.get("engine1").unwrap();
        assert_eq!(*count, 2);
    }

    #[test]
    fn test_router_metrics_record_engine_failure() {
        let metrics = RouterMetrics::new();
        // Pre-initialize failure_count and failure_classification entries
        // (record_engine_failure only increments existing keys)
        metrics
            .engine_failure_count
            .insert("engine1".to_string(), 0);
        metrics
            .failure_classification
            .insert("timeout".to_string(), 0);
        metrics
            .failure_classification
            .insert("network_error".to_string(), 0);
        metrics.record_engine_failure("engine1", "timeout error");
        metrics.record_engine_failure("engine1", "network error");
        let count = metrics.engine_failure_count.get("engine1").unwrap();
        assert_eq!(*count, 2);
        let timeout_count = metrics.failure_classification.get("timeout").unwrap();
        assert_eq!(*timeout_count, 1);
        let network_count = metrics.failure_classification.get("network_error").unwrap();
        assert_eq!(*network_count, 1);
    }

    #[test]
    fn test_router_metrics_classify_error() {
        assert_eq!(RouterMetrics::classify_error("request timeout"), "timeout");
        assert_eq!(
            RouterMetrics::classify_error("SSRF protection triggered"),
            "ssrf_protection"
        );
        assert_eq!(
            RouterMetrics::classify_error("network unreachable"),
            "network_error"
        );
        assert_eq!(
            RouterMetrics::classify_error("circuit breaker open"),
            "circuit_breaker"
        );
        assert_eq!(
            RouterMetrics::classify_error("browser crashed"),
            "browser_error"
        );
        assert_eq!(RouterMetrics::classify_error("unknown issue"), "other");
    }

    #[test]
    fn test_router_metrics_get_success_rate() {
        let metrics = RouterMetrics::new();
        assert_eq!(metrics.get_success_rate(), 1.0);
        metrics.total_requests.store(10, Ordering::Relaxed);
        metrics.successful_requests.store(7, Ordering::Relaxed);
        assert_eq!(metrics.get_success_rate(), 0.7);
    }

    #[test]
    fn test_router_metrics_get_avg_latency_ns_no_data() {
        let metrics = RouterMetrics::new();
        assert!(metrics.get_avg_latency_ns("nonexistent").is_none());
    }

    #[test]
    fn test_router_metrics_get_avg_latency_ns_with_data() {
        let metrics = RouterMetrics::new();
        // Manually populate both latencies and success_count
        metrics
            .engine_latencies
            .insert("engine1".to_string(), 1_000_000);
        metrics
            .engine_success_count
            .insert("engine1".to_string(), 10);
        let avg = metrics.get_avg_latency_ns("engine1");
        assert_eq!(avg, Some(100_000));
    }

    #[test]
    fn test_router_metrics_record_engine_success_no_key_is_noop() {
        // Verify that record_engine_success is a no-op when key doesn't exist
        let metrics = RouterMetrics::new();
        metrics.record_engine_success("engine1");
        assert!(metrics.engine_success_count.get("engine1").is_none());
    }

    // === calculate_engine_score edge cases ===

    #[test]
    fn test_calculate_engine_score_zero_success_rate() {
        let router = EngineRouter::new(vec![]);
        let stats = EngineStats {
            success_rate: 0.0,
            avg_response_time: Duration::from_secs(5),
            usage_count: 500,
            last_used: None,
        };
        let score = router.calculate_engine_score(1.0, &stats);
        assert!(score < 0.5);
    }

    #[test]
    fn test_calculate_engine_score_perfect_stats() {
        let router = EngineRouter::new(vec![]);
        let stats = EngineStats {
            success_rate: 1.0,
            avg_response_time: Duration::from_millis(10),
            usage_count: 0,
            last_used: None,
        };
        let score = router.calculate_engine_score(1.0, &stats);
        assert!(score > 0.95);
    }

    #[test]
    fn test_calculate_engine_score_high_usage_penalty() {
        let router = EngineRouter::new(vec![]);
        let stats = EngineStats {
            success_rate: 1.0,
            avg_response_time: Duration::from_millis(10),
            usage_count: 2000,
            last_used: None,
        };
        let score = router.calculate_engine_score(1.0, &stats);
        let perfect_stats = EngineStats {
            success_rate: 1.0,
            avg_response_time: Duration::from_millis(10),
            usage_count: 0,
            last_used: None,
        };
        let perfect_score = router.calculate_engine_score(1.0, &perfect_stats);
        assert!(score < perfect_score);
    }

    // === Setter tests ===

    #[test]
    fn test_set_max_engine_attempts() {
        let mut router = EngineRouter::new(vec![]);
        router.set_max_engine_attempts(5);
        assert_eq!(router.max_engine_attempts, 5);
    }

    #[test]
    fn test_set_max_engine_attempts_min_one() {
        let mut router = EngineRouter::new(vec![]);
        router.set_max_engine_attempts(0);
        assert_eq!(router.max_engine_attempts, 1);
    }

    #[test]
    fn test_set_max_retries() {
        let mut router = EngineRouter::new(vec![]);
        router.set_max_retries(10);
        assert_eq!(router.max_retries, 10);
    }

    #[test]
    fn test_set_max_retries_min_one() {
        let mut router = EngineRouter::new(vec![]);
        router.set_max_retries(0);
        assert_eq!(router.max_retries, 1);
    }

    #[test]
    fn test_set_feature_filter_enabled() {
        let mut router = EngineRouter::new(vec![]);
        router.set_feature_filter_enabled(false);
        assert!(!router.feature_filter_enabled);
        router.set_feature_filter_enabled(true);
        assert!(router.feature_filter_enabled);
    }

    #[test]
    fn test_set_race_mode_enabled() {
        let mut router = EngineRouter::new(vec![]);
        router.set_race_mode_enabled(true);
        assert!(router.race_mode_enabled);
    }

    #[test]
    fn test_set_dynamic_threshold_factor() {
        let mut router = EngineRouter::new(vec![]);
        router.set_dynamic_threshold_factor(1.5);
        assert_eq!(router.dynamic_threshold_factor, 1.5);
    }

    #[test]
    fn test_set_dynamic_threshold_factor_clamped() {
        let mut router = EngineRouter::new(vec![]);
        router.set_dynamic_threshold_factor(0.01);
        assert_eq!(router.dynamic_threshold_factor, 0.1);
        router.set_dynamic_threshold_factor(3.0);
        assert_eq!(router.dynamic_threshold_factor, 2.0);
    }

    #[test]
    fn test_set_strategy() {
        let mut router = EngineRouter::new(vec![]);
        router.set_strategy(LoadBalancingStrategy::RoundRobin);
        assert_eq!(router.strategy, LoadBalancingStrategy::RoundRobin);
        router.set_strategy(LoadBalancingStrategy::Random);
        assert_eq!(router.strategy, LoadBalancingStrategy::Random);
    }

    #[test]
    fn test_metrics_accessor() {
        let router = EngineRouter::new(vec![]);
        let _metrics = router.metrics();
    }

    // === with_circuit_breaker_and_strategy constructor test ===

    #[test]
    fn test_with_circuit_breaker_and_strategy() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "test",
            score: 100,
        });
        let cb = Arc::new(CircuitBreaker::new());
        let router = EngineRouter::with_circuit_breaker_and_strategy(
            vec![engine],
            cb,
            LoadBalancingStrategy::LeastConnections,
        );
        assert_eq!(router.strategy, LoadBalancingStrategy::LeastConnections);
        assert!(router.get_engine_stats().contains_key("test"));
    }

    // === get_engines test ===

    #[test]
    fn test_get_engines() {
        let e1: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e1",
            score: 100,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "e2",
            score: 100,
        });
        let router = EngineRouter::new(vec![e1, e2]);
        let engines = router.get_engines();
        assert_eq!(engines.len(), 2);
        assert_eq!(engines[0].name(), "e1");
        assert_eq!(engines[1].name(), "e2");
    }

    // === EngineStats default test ===

    #[test]
    fn test_engine_stats_default() {
        let stats = EngineStats::default();
        assert_eq!(stats.success_rate, 1.0);
        assert_eq!(stats.avg_response_time, Duration::from_millis(500));
        assert!(stats.last_used.is_none());
        assert_eq!(stats.usage_count, 0);
    }

    // === 路由成功路径测试 ===

    #[tokio::test]
    async fn test_route_success_path() {
        // 测试路由成功路径：MockEngine 返回成功响应
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "success-engine",
            score: 100,
        });
        let router = EngineRouter::new(vec![engine]);
        let request = make_request();
        let result = router.route(&request).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status_code, 200);
        assert_eq!(response.content, "mock");
    }

    // === SSRF 保护测试 ===

    #[tokio::test]
    async fn test_route_ssrf_protection() {
        // 测试 SSRF 保护：使用内部 IP 地址应被拒绝
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "test",
            score: 100,
        });
        let router = EngineRouter::new(vec![engine]);
        let mut request = make_request();
        request.url = "http://127.0.0.1".to_string();
        let result = router.route(&request).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, EngineError::SsrfProtection(_)));
    }

    // === 不可重试错误测试 ===

    #[tokio::test]
    async fn test_route_non_retryable_error() {
        // 测试不可重试错误：引擎返回 InvalidUrl 时应立即失败
        struct NonRetryableEngine;
        #[async_trait]
        impl ScraperEngine for NonRetryableEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Err(EngineError::InvalidUrl("bad url".to_string()))
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "non-retryable"
            }
        }
        let engine: Arc<dyn ScraperEngine> = Arc::new(NonRetryableEngine);
        let router = EngineRouter::new(vec![engine]);
        let request = make_request();
        let result = router.route(&request).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, EngineError::InvalidUrl(_)));
    }

    // === 最大重试次数测试 ===

    #[tokio::test]
    async fn test_route_max_retries_reached() {
        // 测试最大重试次数：所有引擎都返回可重试错误，应达到最大重试次数后失败
        struct AlwaysTimeoutEngine;
        #[async_trait]
        impl ScraperEngine for AlwaysTimeoutEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Err(EngineError::Timeout(Duration::from_secs(10)))
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "always-timeout"
            }
        }
        let engine: Arc<dyn ScraperEngine> = Arc::new(AlwaysTimeoutEngine);
        let mut router = EngineRouter::new(vec![engine]);
        router.set_max_retries(1);
        let request = make_request();
        let result = router.route(&request).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, EngineError::Timeout(_)));
    }

    // === 竞速模式测试 ===

    #[tokio::test]
    async fn test_route_race_mode_success() {
        // 测试竞速模式：多个引擎并发，返回最快的成功结果
        struct FastEngine {
            name: &'static str,
            delay_ms: u64,
        }
        #[async_trait]
        impl ScraperEngine for FastEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                tokio::time::sleep(Duration::from_millis(self.delay_ms)).await;
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: format!("from-{}", self.name),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: self.delay_ms as u64,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                self.name
            }
        }
        let e1: Arc<dyn ScraperEngine> = Arc::new(FastEngine {
            name: "slow",
            delay_ms: 500,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(FastEngine {
            name: "fast",
            delay_ms: 10,
        });
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_race_mode_enabled(true);
        let request = make_request();
        let result = router.route(&request).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.content.starts_with("from-"));
    }

    #[tokio::test]
    async fn test_route_race_mode_all_fail() {
        // 测试竞速模式：所有引擎都失败时返回错误
        struct FailingEngine;
        #[async_trait]
        impl ScraperEngine for FailingEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Err(EngineError::RequestFailed("connection refused".to_string()))
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "failing"
            }
        }
        let e1: Arc<dyn ScraperEngine> = Arc::new(FailingEngine);
        let e2: Arc<dyn ScraperEngine> = Arc::new(FailingEngine);
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_race_mode_enabled(true);
        let request = make_request();
        let result = router.route(&request).await;
        assert!(result.is_err());
    }

    // === 聚合测试 ===

    #[tokio::test]
    async fn test_aggregate_no_candidates() {
        // 测试聚合：所有引擎 support_score 为 0，候选列表为空
        struct ZeroScoreEngine;
        #[async_trait]
        impl ScraperEngine for ZeroScoreEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "ok".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 10,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                0
            }
            fn name(&self) -> &'static str {
                "zero-score"
            }
        }
        let engine: Arc<dyn ScraperEngine> = Arc::new(ZeroScoreEngine);
        let router = EngineRouter::new(vec![engine]);
        let request = make_request();
        let result = router.aggregate(&request).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, EngineError::AllEnginesFailed(_)));
    }

    #[tokio::test]
    async fn test_aggregate_all_engines_fail() {
        // 测试聚合：所有引擎都失败
        struct FailingEngine;
        #[async_trait]
        impl ScraperEngine for FailingEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Err(EngineError::RequestFailed("failed".to_string()))
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "failing"
            }
        }
        let e1: Arc<dyn ScraperEngine> = Arc::new(FailingEngine);
        let e2: Arc<dyn ScraperEngine> = Arc::new(FailingEngine);
        let router = EngineRouter::new(vec![e1, e2]);
        let request = make_request();
        let result = router.aggregate(&request).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, EngineError::AllEnginesFailed(_)));
    }

    // === EngineRouterTrait 通过 trait 对象测试 ===

    #[tokio::test]
    async fn test_engine_router_trait_methods() {
        let engine: Arc<dyn ScraperEngine> = Arc::new(MockEngine {
            engine_name: "trait-test",
            score: 100,
        });
        let router = EngineRouter::new(vec![engine]);
        let trait_ref: &dyn EngineRouterTrait = &router;

        // 测试 registered_engines
        let engines = trait_ref.registered_engines();
        assert_eq!(engines, vec!["trait-test".to_string()]);

        // 测试 get_engine_stats
        let stats = trait_ref.get_engine_stats();
        assert!(stats.contains_key("trait-test"));

        // 测试 reset_engine_stats
        trait_ref.reset_engine_stats("trait-test");
        let stats_after = trait_ref.get_engine_stats();
        assert_eq!(stats_after.get("trait-test").unwrap().usage_count, 0);

        // 测试 route 通过 trait
        let request = make_request();
        let result = trait_ref.route(&request).await;
        assert!(result.is_ok());
    }

    // === select_optimal_engines 边界情况 ===

    #[tokio::test]
    async fn test_route_no_engines_available() {
        // 测试没有引擎时返回 AllEnginesFailed
        let router = EngineRouter::new(vec![]);
        let request = make_request();
        let result = router.route(&request).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, EngineError::AllEnginesFailed(_)));
    }

    #[tokio::test]
    async fn test_route_support_score_zero_filtered() {
        // 测试 support_score 为 0 的引擎被过滤
        struct ZeroScoreEngine;
        #[async_trait]
        impl ScraperEngine for ZeroScoreEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "ok".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 10,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                0
            }
            fn name(&self) -> &'static str {
                "zero-score"
            }
        }
        let engine: Arc<dyn ScraperEngine> = Arc::new(ZeroScoreEngine);
        let router = EngineRouter::new(vec![engine]);
        let request = make_request();
        let result = router.route(&request).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            EngineError::AllEnginesFailed(_)
        ));
    }
}

#[cfg(test)]
mod tests_impl {
    use super::*;
    use crate::engines::engine_client::{
        EngineError, InternalScrapeRequest, InternalScrapeResponse, ScraperEngine,
    };
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    // A simple test engine that is a controllable implementation
    struct TestScraperEngineImpl {
        name: &'static str,
        _supported_domains: Vec<String>,
        _weight: u8,
        response_content: String,
        is_error: bool,
        call_count: AtomicU32,
        max_calls: u32,
    }

    impl TestScraperEngineImpl {
        fn new(
            name: &'static str,
            supported_domains: Vec<String>,
            weight: u8,
            result: Result<InternalScrapeResponse, EngineError>,
            max_calls: u32,
        ) -> Self {
            match result {
                Ok(resp) => Self {
                    name,
                    _supported_domains: supported_domains,
                    _weight: weight,
                    response_content: resp.content,
                    is_error: false,
                    call_count: AtomicU32::new(0),
                    max_calls,
                },
                Err(_) => Self {
                    name,
                    _supported_domains: supported_domains,
                    _weight: weight,
                    response_content: String::new(),
                    is_error: true,
                    call_count: AtomicU32::new(0),
                    max_calls,
                },
            }
        }
    }

    #[async_trait]
    impl ScraperEngine for TestScraperEngineImpl {
        async fn scrape(
            &self,
            _request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            let call_count = self.call_count.fetch_add(1, Ordering::SeqCst) + 1;

            if call_count <= self.max_calls {
                if self.is_error {
                    return Err(EngineError::Timeout(Duration::from_secs(30)));
                }
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: self.response_content.clone(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 100,
                })
            } else {
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "Default Result".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 100,
                })
            }
        }

        fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
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
            Ok(InternalScrapeResponse {
                status_code: 200,
                content: "Result 1".to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 100,
            }),
            10, // max_calls
        );

        let engine2 = TestScraperEngineImpl::new(
            "engine2",
            vec!["example.com".to_string()],
            1,
            Ok(InternalScrapeResponse {
                status_code: 200,
                content: "Result 2".to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 100,
            }),
            10, // max_calls
        );

        let router = EngineRouter::new(vec![Arc::new(engine1), Arc::new(engine2)]);

        let request = InternalScrapeRequest {
            url: "http://example.com".to_string(),
            method: crate::engines::engine_client::HttpMethod::Get,
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
            actions: Vec::new(),
            body: None,
            sync_wait_ms: 0,
        };
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
            Err(EngineError::Timeout(Duration::from_secs(30))),
            10, // max_calls
        );

        let engine2 = TestScraperEngineImpl::new(
            "engine2",
            vec!["example.com".to_string()],
            1,
            Ok(InternalScrapeResponse {
                status_code: 200,
                content: "Result 2".to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 100,
            }),
            10, // max_calls
        );

        let router = EngineRouter::new(vec![Arc::new(engine1), Arc::new(engine2)]);

        let request = InternalScrapeRequest {
            url: "http://example.com".to_string(),
            method: crate::engines::engine_client::HttpMethod::Get,
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
            actions: Vec::new(),
            body: None,
            sync_wait_ms: 0,
        };
        let result = router.aggregate(&request).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.content, "Result 2");
    }
}
