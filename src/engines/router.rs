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
use crate::utils::hedge::HedgeController;
use crate::utils::retry::{RetryDirective, RetryReason, RetryTracker};
use crate::utils::ua_pool::UaPool;
use dashmap::DashMap;
use log::{debug, info, warn};
use rand::seq::SliceRandom;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub use crate::engines::router_metrics::{EngineStats, RouterMetrics};

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
    /// 引擎性能统计（DashMap 优化并发读写，避免 RwLock 借用）
    engine_stats: Arc<DashMap<String, EngineStats>>,
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
    /// UA 池（性能审查 H-1 修复：原在 route() 内每请求 UaPool::new() 分配 44 个 profile）
    ///
    /// UaPool 内部全 `&'static str`，构造后只读，可安全跨线程共享（Send+Sync 自动派生）。
    /// 用 `Arc` 是因为 `EngineRouter` 本身可能被 Clone 共享。
    ua_pool: Arc<UaPool>,
    /// Hedge 控制器（design.md §17，T070/R-runtime-004）
    ///
    /// 记录 race 胜出引擎延迟，估算 P84 阈值，供未来顺序路径决策是否发送副本请求。
    /// `HedgeController` 内部全 `Atomic*`，无锁线程安全，可直接共享无需 `Arc`。
    hedge_controller: HedgeController,
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
        let engine_stats = DashMap::with_capacity(8);
        for engine in &engines {
            engine_stats.insert(engine.name().to_string(), EngineStats::default());
        }

        Self {
            engines,
            circuit_breaker: Arc::new(CircuitBreaker::new()),
            engine_stats: Arc::new(engine_stats),
            round_robin_index: Arc::new(parking_lot::Mutex::new(0)),
            strategy: LoadBalancingStrategy::SmartHybrid,
            metrics: Arc::new(RouterMetrics::new()),
            max_engine_attempts: 3,
            max_retries: 5,                                     // 默认最大重试次数
            feature_filter_enabled: true,                       // 默认启用特征检测过滤
            race_mode_enabled: false,                           // 默认禁用并发竞速模式
            dynamic_threshold_factor: 1.0,                      // 默认动态阈值因子
            ua_pool: Arc::new(UaPool::new()),                   // 性能审查 H-1：构造一次共享
            hedge_controller: HedgeController::with_defaults(), // T070：默认参数
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
        let engine_stats = DashMap::with_capacity(8);
        for engine in &engines {
            engine_stats.insert(engine.name().to_string(), EngineStats::default());
        }

        Self {
            engines,
            circuit_breaker,
            engine_stats: Arc::new(engine_stats),
            round_robin_index: Arc::new(parking_lot::Mutex::new(0)),
            strategy,
            metrics: Arc::new(RouterMetrics::new()),
            max_engine_attempts: 3,
            max_retries: 5,
            feature_filter_enabled: true,
            race_mode_enabled: false,
            dynamic_threshold_factor: 1.0,
            ua_pool: Arc::new(UaPool::new()), // 性能审查 H-1：构造一次共享
            hedge_controller: HedgeController::with_defaults(), // T070：默认参数
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

    /// 获取 Hedge 控制器引用（design.md §17，T070）
    ///
    /// 返回 `&HedgeController`，外部可读取 P84 阈值、样本数等观测值，
    /// 也可调用 `should_hedge` 决策是否发起副本（未来顺序路径用）。
    /// `record_latency` / `reset` 已限定为 `pub(crate)`，外部无法篡改状态
    /// （架构审查 M-1：接口隔离修复）。
    pub fn hedge_controller(&self) -> &HedgeController {
        &self.hedge_controller
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

        // PERF-04/MEDIUM-2：一次性收集 DashMap 为 HashMap，避免循环内多次 Ref 借用，
        // 同时供 Second pass（calculate_engine_score）和 sort_candidates_by_strategy 复用，
        // DashMap 全局只遍历一次。
        let stats: std::collections::HashMap<String, EngineStats> = self
            .engine_stats
            .iter()
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect();

        // Second pass: calculate scores（从 HashMap 取 EngineStats，无 DashMap 借用开销）
        let mut scored_candidates = Vec::new();

        for (support_score, engine_name, engine) in candidates {
            // 性能审查 M-1 修复：循环内不 clone EngineStats，直接借用 stats HashMap
            // （原 .cloned().unwrap_or_default() 每次循环都分配 EngineStats）
            let engine_stat = stats.get(&engine_name);
            let default_stat;
            let engine_stat_ref: &EngineStats = match engine_stat {
                Some(s) => s,
                None => {
                    default_stat = EngineStats::default();
                    &default_stat
                }
            };

            // Apply dynamic threshold factor
            let adjusted_score = support_score * self.dynamic_threshold_factor;

            // Calculate final score
            let final_score = self.calculate_engine_score(adjusted_score, engine_stat_ref);

            scored_candidates.push((final_score, engine));
        }

        // Sort by strategy（复用上方已收集的 stats，无需再次遍历 DashMap）
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
        // DashMap::get_mut 返回 RefMut guard，作用域结束自动释放
        if let Some(mut stat) = self.engine_stats.get_mut(engine_name) {
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

    /// H-1 修复：提取 RetryTracker 上限检查的公共逻辑（DRY）
    ///
    /// 在 AntiBot 和 retryable error 两个分支中，原代码有以下重复：
    /// 1. `if !tracker.should_retry(reason)` → warn + metrics + return
    /// 2. `if total_attempts >= max_retries` → warn + metrics + return
    ///
    /// 本方法封装两个检查，返回 `true` 表示应停止重试。
    /// 调用方在调用前已 `tracker.record(reason)`，调用后根据返回值决定 `return`。
    ///
    /// # 参数
    ///
    /// - `tracker`: 重试跟踪器（已 record 过 reason）
    /// - `reason`: 本次失败的 RetryReason（用于日志）
    /// - `total_attempts`: 当前总尝试次数
    /// - `max_retries`: 最大重试次数
    ///
    /// # 返回值
    ///
    /// - `true`: 应停止重试（已记录指标 + warn 日志）
    /// - `false`: 可继续重试
    fn should_stop_after_retry_check(
        &self,
        tracker: &RetryTracker,
        reason: RetryReason,
        total_attempts: usize,
        max_retries: usize,
    ) -> bool {
        if !tracker.should_retry(reason) {
            warn!(
                "RetryTracker blocked {:?} after total={} (anti_bot={}, feature_toggle={}), stopping",
                reason,
                tracker.total(),
                tracker.anti_bot(),
                tracker.feature_toggle()
            );
            self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
            self.metrics.failed_requests.fetch_add(1, Ordering::Relaxed);
            return true;
        }
        if total_attempts >= max_retries {
            warn!(
                "Max retries {} reached after {:?}, stopping",
                max_retries, reason
            );
            self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
            self.metrics.failed_requests.fetch_add(1, Ordering::Relaxed);
            return true;
        }
        false
    }

    /// Internal route implementation without timeout
    #[allow(unused_variables)]
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

        debug!(
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
        // T013（R-antibot-003）：anti-bot 检测命中后，后续 attempt 强制 needs_js=true，
        // 使浏览器/FlareSolverr 引擎走 JS 渲染路径突破反爬挑战。
        // 仅 `antibot` 特性启用时 `force_needs_js` 会被改写；关闭时需抑制 unused_mut。
        #[cfg_attr(not(feature = "antibot"), allow(unused_mut))]
        let mut force_needs_js = false;

        // T028（R-identity-002）：智能重试基础设施
        // - RetryTracker：各 reason 独立计数与上限（AntiBot cap=2、FeatureToggle cap=3、total=5）
        // - UaPool：按 attempt 稳定轮换 UA（pick_seeded）
        // - last_reason：上次失败原因，用于计算下次 attempt 的 RetryDirective
        let mut tracker = RetryTracker::new_default();
        // 性能审查 H-1：复用结构体字段 ua_pool，避免每请求分配 44 个 profile
        let ua_pool = &self.ua_pool;
        let mut last_reason: Option<RetryReason> = None;

        for (score, engine) in candidates.into_iter().take(max_attempts) {
            total_attempts += 1;
            let engine_name = engine.name();

            // 记录引擎选择
            self.metrics.record_engine_selection(engine_name);
            self.metrics.record_attempt();

            debug!(
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

            // T028（R-identity-002）：计算本次 attempt 的身份升级指令
            // - 首次尝试（total_attempts=1）用 default（无升级）
            // - 第 N 次重试（total_attempts=N+1, N>=1）基于上次失败 reason 升级
            //   directive_attempt = total_attempts - 2（0-indexed：0=首次重试、1=第二次重试...）
            let directive = if total_attempts == 1 {
                RetryDirective::default()
            } else {
                let directive_attempt = (total_attempts - 2) as u32;
                RetryDirective::for_attempt(
                    last_reason.unwrap_or(RetryReason::Transient),
                    directive_attempt,
                )
            };

            // T028：按 directive.rotate_ua 轮换 UA（C-1 修复：同步轮换所有指纹相关 header）
            //
            // C-1 问题：原实现仅覆盖 User-Agent，未同步轮换 Accept-Language 与 sec-ch-ua，
            //          导致重试时 UA 与 Accept-Language/sec-ch-ua 不一致（指纹矛盾），
            //          反爬服务可识别为「指纹不一致的爬虫」。
            //
            // C-1 修复：将 profile 的所有指纹相关 header 一次性写入：
            //   - User-Agent：所有 profile 必设
            //   - Accept-Language：所有 profile 必设
            //   - sec-ch-ua：仅 Chromium-based 浏览器非空时设置；为空时移除原值，
            //     避免残留 Firefox/Safari UA 但保留 Chromium sec-ch-ua 的矛盾。
            let mut headers = request.headers.clone();
            if directive.rotate_ua {
                let profile = ua_pool.pick_seeded((total_attempts - 1) as u64, request.mobile);
                headers.insert("User-Agent".to_string(), profile.ua.to_string());
                headers.insert(
                    "Accept-Language".to_string(),
                    profile.accept_language.to_string(),
                );
                if profile.sec_ch_ua.is_empty() {
                    headers.remove("sec-ch-ua");
                } else {
                    headers.insert("sec-ch-ua".to_string(), profile.sec_ch_ua.to_string());
                }
                debug!(
                    "Attempt {}: rotated UA via pick_seeded(seed={}) -> {} ({}, AL={}, sec-ch-ua={})",
                    total_attempts,
                    total_attempts - 1,
                    profile.ua,
                    if profile.mobile { "mobile" } else { "desktop" },
                    profile.accept_language,
                    if profile.sec_ch_ua.is_empty() {
                        "<none>"
                    } else {
                        profile.sec_ch_ua
                    }
                );
            }

            let attempt_request = InternalScrapeRequest {
                url: request.url.clone(),
                method: request.method,
                headers,
                timeout: remaining,
                // T013 + T028：force_needs_js（anti-bot）或 directive.force_browser 都强制 needs_js
                needs_js: request.needs_js || force_needs_js || directive.force_browser,
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
                block_ads: request.block_ads,
                block_media: request.block_media,
                session_id: request.session_id.clone(),
                wait_for: request.wait_for.clone(),
            };

            let engine_start = Instant::now();

            // T062（design.md §14）：瀑布式 MRT 超时包裹
            //
            // 单引擎调用以 `min(remaining, engine.max_response_time())` 包裹：
            // - `remaining` = 请求整体剩余时间（request.timeout - 已耗时）
            // - `mrt` = 引擎级最大响应时间（engine.max_response_time()）
            //
            // 取 min 确保：
            // 1. remaining < mrt：请求整体超时优先（不浪费 MRT 配额）
            // 2. mrt < remaining：超 MRT 即切下一引擎（瀑布式 fallback）
            //
            // race_mode 路径不受影响（保留作为可选模式，在 route_race_mode 中独立处理）。
            let engine_mrt = engine.max_response_time();
            let effective_timeout = std::cmp::min(remaining, engine_mrt);

            match tokio::time::timeout(effective_timeout, engine.scrape(&attempt_request)).await {
                Ok(Ok(response)) => {
                    let response_time = engine_start.elapsed();

                    // T013（R-antibot-003）：引擎返回"成功"响应后，检查是否为反爬挑战页。
                    // 命中 needs_browser 时将当前结果视为失败，强制后续 attempt needs_js=true，
                    // 使浏览器/FlareSolverr 引擎走 JS 渲染路径突破反爬。
                    #[cfg(feature = "antibot")]
                    {
                        if let Some(detection) = check_antibot_response(&response, &request.url) {
                            if detection.needs_browser {
                                self.update_engine_stats(engine_name, false, response_time);
                                self.metrics
                                    .record_engine_failure(engine_name, &detection.reason);
                                warn!(
                                    "Engine {} returned anti-bot challenge ({:?}): {}, \
                                     forcing needs_js for subsequent attempts",
                                    engine_name, detection.tech, detection.reason
                                );
                                last_error = Some(EngineError::AntiBotDetected(detection.reason));
                                force_needs_js = true;

                                // T028（R-identity-002）：记录 AntiBot 失败到 tracker
                                let reason = RetryReason::AntiBot;
                                tracker.record(reason);
                                last_reason = Some(reason);

                                // T028：检查 tracker 上限（anti_bot cap=2）+ max_retries
                                // H-1 修复：使用 should_stop_after_retry_check 消除重复（DRY）
                                if self.should_stop_after_retry_check(
                                    &tracker,
                                    reason,
                                    total_attempts,
                                    max_retries,
                                ) {
                                    return Err(last_error.unwrap_or_else(|| {
                                        EngineError::AllEnginesFailed(
                                            "Max retries reached".to_string(),
                                        )
                                    }));
                                }
                                continue;
                            }
                        }
                    }

                    // T015（R-jsrender-001）：流式 HTTP→Chrome 升级探测
                    //
                    // HTTP 引擎（needs_js==false）返回"成功"响应后，检查是否疑似 SPA 空壳。
                    // 若 probe 判定 upgrade，将当前结果视为失败（不返回空壳给用户），
                    // 以 needs_js=true 重新 route_internal 改派浏览器引擎（Playwright）渲染。
                    //
                    // 防递归：递归调用时 request.needs_js=true，attempt_request.needs_js=true，
                    // 故 `!attempt_request.needs_js` 为 false，probe 检查自然跳过。
                    if !attempt_request.needs_js {
                        let verdict = check_js_upgrade_probe(&response);
                        if verdict.upgrade {
                            self.update_engine_stats(engine_name, false, response_time);
                            self.metrics.record_engine_failure(
                                engine_name,
                                &format!("js-upgrade-probe: {}", verdict.reason),
                            );
                            debug!(
                                "Engine {} returned SPA shell (probe score={}, reason={}); \
                                 re-routing with needs_js=true to dispatch browser engine",
                                engine_name, verdict.score, verdict.reason
                            );

                            let mut js_request = request.clone();
                            js_request.needs_js = true;
                            return Box::pin(self.route_internal(&js_request)).await;
                        }
                    }

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
                Ok(Err(e)) => {
                    let response_time = engine_start.elapsed();
                    self.update_engine_stats(engine_name, false, response_time);

                    // 记录失败指标
                    self.metrics
                        .record_engine_failure(engine_name, &e.to_string());

                    if e.is_retryable() {
                        self.circuit_breaker.record_failure(engine_name);

                        // T028（R-identity-002）：记录失败原因到 tracker（在 e move 之前取 reason）
                        let reason = e.retry_reason();
                        tracker.record(reason);
                        last_reason = Some(reason);

                        // T028：检查 tracker 上限（AntiBot cap=2、FeatureToggle cap=3、total=5）+ max_retries
                        // H-1 修复：使用 should_stop_after_retry_check 消除重复（DRY）
                        if self.should_stop_after_retry_check(
                            &tracker,
                            reason,
                            total_attempts,
                            max_retries,
                        ) {
                            return Err(e);
                        }

                        warn!(
                            "Engine {} failed with retryable error: {}, trying next engine \
                             (reason={:?}, tracker total={})",
                            engine_name,
                            e,
                            reason,
                            tracker.total()
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
                Err(_elapsed) => {
                    // T062：MRT 瀑布式超时——`tokio::time::timeout` 在 effective_timeout 后
                    // 取消 engine.scrape() future，进入此分支。
                    //
                    // 架构审查 MEDIUM-2 修复：根据 effective_timeout 的来源区分两种语义：
                    // - `effective_timeout == remaining`（remaining <= engine_mrt）：
                    //   请求整体超时（剩余时间耗尽），返回 `EngineError::Timeout`。
                    //   此时即使切下一引擎也无力回天，由上层 route_sequential 循环下轮
                    //   的 `remaining.is_zero()` 检查兜底（L776）。
                    // - `effective_timeout == engine_mrt`（engine_mrt < remaining）：
                    //   真正的引擎 MRT 超时，返回 `EngineError::EngineMrtExceeded`，
                    //   router 触发瀑布式 fallback 切下一引擎继续。
                    //
                    // 边界情况 `remaining == engine_mrt`：按 Timeout 处理（保守语义，
                    // 避免误认为仍有剩余时间可 fallback）。
                    let response_time = engine_start.elapsed();
                    self.update_engine_stats(engine_name, false, response_time);

                    // 判断本次超时来源（边界 == 走 Timeout 分支）
                    let is_overall_timeout = effective_timeout <= remaining;
                    let timeout_err = if is_overall_timeout {
                        // remaining 耗尽（remaining <= engine_mrt）→ 请求整体超时
                        warn!(
                            "Request overall timeout (remaining={:?} <= mrt={:?}); \
                             engine={} cancelled at effective_timeout={:?}",
                            remaining, engine_mrt, engine_name, effective_timeout
                        );
                        EngineError::Timeout(effective_timeout)
                    } else {
                        // engine_mrt < remaining → 引擎级 MRT 超时
                        let mrt_err = EngineError::EngineMrtExceeded {
                            engine: engine_name.to_string(),
                            mrt: effective_timeout,
                        };
                        warn!(
                            "Engine {} exceeded MRT (effective_timeout={:?}, mrt={:?}, remaining={:?}); \
                             waterfall fallback to next engine",
                            engine_name, effective_timeout, engine_mrt, remaining
                        );
                        mrt_err
                    };

                    self.metrics
                        .record_engine_failure(engine_name, &timeout_err.to_string());

                    self.circuit_breaker.record_failure(engine_name);

                    let reason = timeout_err.retry_reason();
                    tracker.record(reason);
                    last_reason = Some(reason);

                    if self.should_stop_after_retry_check(
                        &tracker,
                        reason,
                        total_attempts,
                        max_retries,
                    ) {
                        return Err(timeout_err);
                    }

                    last_error = Some(timeout_err);
                    continue;
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

        debug!(
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
                block_ads: request.block_ads,
                block_media: request.block_media,
                session_id: request.session_id.clone(),
                wait_for: request.wait_for.clone(),
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

                        // T070/§17：记录胜出引擎延迟到 Hedge 控制器，
                        // 为未来顺序路径提供 P84 阈值估算（接入 race 路径为可选增强）
                        self.hedge_controller.record_latency(response_time);

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
        // SSRF 防护：与 _route_impl 保持一致，避免 aggregate 绕过内网地址校验
        if let Err(e) = validate_url(&request.url).await {
            return Err(EngineError::SsrfProtection(e.to_string()));
        }

        let candidates = self.select_optimal_engines(request);
        if candidates.is_empty() {
            // 错误消息与 _route_impl 保持一致，避免 EngineRouterTrait::aggregate
            // 与 route 行为不一致（LSP）
            return Err(EngineError::AllEnginesFailed(
                "No suitable engines available".to_string(),
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
        // DashMap → HashMap 一次性收集（trait 契约要求返回 HashMap）
        self.engine_stats
            .iter()
            .map(|r| (r.key().clone(), r.value().clone()))
            .collect()
    }

    /// 重置引擎统计信息
    pub fn _reset_engine_stats_impl(&self, engine_name: &str) {
        // DashMap::get_mut 返回 RefMut guard，作用域结束自动释放
        if let Some(mut stat) = self.engine_stats.get_mut(engine_name) {
            *stat = EngineStats::default();
        }
    }

    /// 注册引擎
    pub fn register_engine(&mut self, engine: Arc<dyn ScraperEngine>) {
        let name = engine.name().to_string();
        self.engines.push(engine);
        // DashMap::insert 直接替换/插入，无需获取写锁
        self.engine_stats
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

/// T013（R-antibot-003）：检查引擎"成功"响应是否为反爬挑战页。
///
/// 将 `InternalScrapeResponse` 的 `HashMap<String,String>` headers 转为
/// `reqwest::header::HeaderMap` 后调用 `antibot::classify`。仅在 `antibot`
/// feature 启用时编译；关闭时 route_internal 的检测块被 cfg 移除，此函数也不存在。
#[cfg(feature = "antibot")]
fn check_antibot_response(
    response: &InternalScrapeResponse,
    url: &str,
) -> Option<crate::engines::antibot::Detection> {
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

    let mut header_map = HeaderMap::new();
    for (k, v) in &response.headers {
        if let Ok(name) = HeaderName::from_bytes(k.as_bytes()) {
            if let Ok(value) = HeaderValue::from_str(v) {
                header_map.append(name, value);
            }
        }
    }
    crate::engines::antibot::classify(response.status_code, &response.content, &header_map, url)
}

/// T015（R-jsrender-001）：对引擎"成功"响应运行 JS 升级探测。
///
/// 将 `InternalScrapeResponse` 的 headers 转为 `reqwest::header::HeaderMap` 后
/// 调用 [`crate::engines::upgrade_probe::JsUpgradeProbe::evaluate`]。返回
/// [`crate::engines::upgrade_probe::ProbeVerdict`]，由 `route_internal` 消费
/// `upgrade=true` 时以 `needs_js=true` 重新改派浏览器引擎。
///
/// 与 [`check_antibot_response`] 不同，此函数**不** feature-gate：`upgrade_probe`
/// 模块是纯 Rust 无外部依赖，始终编译；SPA 空壳探测是通用能力，不应受 `antibot` 特性开关影响。
fn check_js_upgrade_probe(
    response: &InternalScrapeResponse,
) -> crate::engines::upgrade_probe::ProbeVerdict {
    use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

    let probe = crate::engines::upgrade_probe::JsUpgradeProbe::default();
    let mut header_map = HeaderMap::new();
    for (k, v) in &response.headers {
        if let Ok(name) = HeaderName::from_bytes(k.as_bytes()) {
            if let Ok(value) = HeaderValue::from_str(v) {
                header_map.append(name, value);
            }
        }
    }
    // 性能审查 HIGH-1 修复：evaluate docstring 明确「body_prefix」语义，
    // 传入完整 body 会让多次 `contains`/`find` 退化为 O(body_len)。
    // 截取前 PROBE_PREFIX_LEN 字节，覆盖典型 SPA 空壳的 head+顶层 body。
    let prefix_end = response
        .content
        .len()
        .min(crate::engines::upgrade_probe::PROBE_PREFIX_LEN);
    let body_prefix = &response.content[..prefix_end];
    probe.evaluate(&header_map, body_prefix)
}


#[cfg(test)]
#[path = "tests/router_test.rs"]
mod tests;

#[cfg(test)]
#[path = "tests/router_tests_impl.rs"]
mod tests_impl;
