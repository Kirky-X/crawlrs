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
mod tests {
    use super::*;
    use crate::engines::client::reqwest::ReqwestEngine;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::atomic::AtomicU64;

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
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
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
                // T013：内容需 ≥200 字节且可见文本 ≥50 字符，
                // 否则被 antibot::classify Step 5 误判为 near-empty structural block。
                content: "<html><body><h1>Mock Response</h1><p>This is a mock response for testing router logic. It contains enough visible text to avoid being flagged as a near-empty shell by the antibot classifier.</p></body></html>".to_string(),
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
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
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
        // 架构审查 HIGH-1 修复后：record_engine_latency 自带 entry().or_insert() 自动初始化，
        // 不再依赖 record_engine_selection 预初始化 latencies=0
        metrics.record_engine_latency("engine1", Duration::from_millis(100));
        metrics.record_engine_latency("engine1", Duration::from_millis(200));
        // 累计延迟应为 100+200=300ms = 300_000_000ns
        // PERF-004: AtomicU64 load 读取
        let total_ref = metrics.engine_latencies.get("engine1").unwrap();
        assert_eq!(total_ref.load(Ordering::Relaxed), 300_000_000);
        // avg 需要 success_count 同步存在（get_avg_latency_ns 检查两者）
        // 单独记录 latency 不更新 success_count，故 avg 仍为 None
        let avg = metrics.get_avg_latency_ns("engine1");
        assert!(avg.is_none());
    }

    #[test]
    fn test_router_metrics_record_engine_success() {
        let metrics = RouterMetrics::new();
        // 架构审查 HIGH-1 修复后：record_engine_success 自带 entry().or_insert() 自动初始化，
        // 不再需要测试手动 insert 0 预初始化
        metrics.record_engine_success("engine1");
        metrics.record_engine_success("engine1");
        let count_ref = metrics.engine_success_count.get("engine1").unwrap();
        assert_eq!(count_ref.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_router_metrics_record_engine_failure() {
        let metrics = RouterMetrics::new();
        // 架构审查 HIGH-1 修复后：record_engine_failure 自带 entry().or_insert() 自动初始化
        // failure_count 和 failure_classification 都不再需要测试手动 insert 0 预初始化
        metrics.record_engine_failure("engine1", "timeout error");
        metrics.record_engine_failure("engine1", "network error");
        let count_ref = metrics.engine_failure_count.get("engine1").unwrap();
        assert_eq!(count_ref.load(Ordering::Relaxed), 2);
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
            .insert("engine1".to_string(), AtomicU64::new(1_000_000));
        metrics
            .engine_success_count
            .insert("engine1".to_string(), AtomicU64::new(10));
        let avg = metrics.get_avg_latency_ns("engine1");
        assert_eq!(avg, Some(100_000));
    }

    #[test]
    fn test_router_metrics_record_engine_success_initializes_to_one() {
        // Verify that record_engine_success self-initializes the counter to 1
        // when key doesn't exist (架构审查 HIGH-1 修复：原实现 noop when key missing
        // 导致 success_count 永远为 0，与"成功必须被计数"的业务语义冲突)。
        let metrics = RouterMetrics::new();
        metrics.record_engine_success("engine1");
        assert_eq!(
            metrics
                .engine_success_count
                .get("engine1")
                .unwrap()
                .load(Ordering::Relaxed),
            1u64
        );

        // 二次调用应递增，不应重置
        metrics.record_engine_success("engine1");
        assert_eq!(
            metrics
                .engine_success_count
                .get("engine1")
                .unwrap()
                .load(Ordering::Relaxed),
            2u64
        );
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
        assert!(response.content.contains("Mock Response"));
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
                    response_time_ms: self.delay_ms,
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

    /// T070/§17：验证 race 胜出后延迟被记录到 hedge_controller
    #[tokio::test]
    async fn test_route_race_mode_records_hedge_latency() {
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
                    response_time_ms: self.delay_ms,
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
            delay_ms: 100,
        });
        let e2: Arc<dyn ScraperEngine> = Arc::new(FastEngine {
            name: "fast",
            delay_ms: 5,
        });
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_race_mode_enabled(true);

        // 初始：hedge 样本为 0
        assert_eq!(router.hedge_controller().sample_count(), 0);

        // 多次 race 胜出 fast（5ms）
        let request = make_request();
        for _ in 0..12 {
            let _ = router.route(&request).await.unwrap();
        }

        // 12 次 race 后：hedge 样本数应 ≥ DEFAULT_MIN_SAMPLES（10）
        let controller = router.hedge_controller();
        assert!(
            controller.sample_count() >= 10,
            "hedge should have >= 10 samples, got {}",
            controller.sample_count()
        );

        // P84 阈值应可用（fast 5ms + slow 100ms 但 race 总是 fast 胜）
        let threshold = controller
            .p84_threshold()
            .expect("P84 threshold should be available");
        // fast 总是胜出，延迟应近 5ms（容忍调度抖动）
        let threshold_ms = threshold.as_secs_f64() * 1000.0;
        assert!(
            threshold_ms < 50.0,
            "P84 should be near fast engine latency, got {threshold_ms}ms"
        );

        // 已耗时大于阈值：should_hedge 应为 true
        assert!(router
            .hedge_controller()
            .should_hedge(Duration::from_millis(100)));
        // 已耗时小于阈值：should_hedge 应为 false
        assert!(!router
            .hedge_controller()
            .should_hedge(Duration::from_micros(1)));
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

    // === EngineRouterTrait method coverage ===
    // These tests call methods through the trait interface (not the public
    // wrapper methods) to cover the trait impl at lines 1028-1055.

    #[tokio::test]
    async fn test_trait_aggregate_delegates_to_impl() {
        struct SucceedingEngine;
        #[async_trait]
        impl ScraperEngine for SucceedingEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    // T013：同 MockEngine，需 ≥200 字节可见文本避免 antibot 误判
                    content: "<html><body><h1>OK</h1><p>Succeeding engine response for testing trait delegation. It has enough visible text to pass the antibot classifier near-empty check.</p></body></html>".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 10,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "succeeding"
            }
        }
        let engine: Arc<dyn ScraperEngine> = Arc::new(SucceedingEngine);
        let router = EngineRouter::new(vec![engine]);
        let request = make_request();

        // Call through the trait, not the public wrapper method
        let result = EngineRouterTrait::aggregate(&router, &request).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_trait_route_delegates_to_impl() {
        // Also cover the trait route method (line 1030-1034)
        struct SucceedingEngine;
        #[async_trait]
        impl ScraperEngine for SucceedingEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    // T013：同 MockEngine，需 ≥200 字节可见文本避免 antibot 误判
                    content: "<html><body><h1>OK</h1><p>Succeeding engine response for testing trait delegation. It has enough visible text to pass the antibot classifier near-empty check.</p></body></html>".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 10,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "succeeding"
            }
        }
        let engine: Arc<dyn ScraperEngine> = Arc::new(SucceedingEngine);
        let router = EngineRouter::new(vec![engine]);
        let request = make_request();

        let result = EngineRouterTrait::route(&router, &request).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_feature_filter_excludes_low_score_engine_for_screenshot() {
        // Cover the feature_filter_enabled branch (line 407-411):
        // When needs_screenshot=true and engine support_score < 50,
        // the engine should be filtered out.
        struct LowScoreEngine;
        #[async_trait]
        impl ScraperEngine for LowScoreEngine {
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
                10 // Below the 50 threshold
            }
            fn name(&self) -> &'static str {
                "low-score"
            }
        }
        let engine: Arc<dyn ScraperEngine> = Arc::new(LowScoreEngine);
        let mut router = EngineRouter::new(vec![engine]);
        // feature_filter_enabled defaults to true, but set explicitly for clarity
        router.set_feature_filter_enabled(true);

        let request = InternalScrapeRequest {
            url: "http://example.com".to_string(),
            method: crate::engines::engine_client::HttpMethod::Get,
            headers: HashMap::new(),
            timeout: Duration::from_secs(30),
            needs_js: false,
            needs_screenshot: true, // This triggers the feature filter
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
            actions: Vec::new(),
            body: None,
            sync_wait_ms: 0,
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
        };

        // The low-score engine should be filtered out, leaving no candidates
        let result = router.route(&request).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            EngineError::AllEnginesFailed(_)
        ));
    }

    // === circuit breaker open branch (line 402-404) ===

    #[tokio::test]
    async fn test_route_skips_engine_when_circuit_breaker_open() {
        // Cover line 403: when the circuit breaker for an engine is open,
        // select_optimal_engines should `continue` past it, leaving no
        // candidates and producing AllEnginesFailed.
        use crate::engines::circuit_breaker::CircuitConfig;

        struct CountingEngine {
            name: &'static str,
            calls: Arc<std::sync::atomic::AtomicU32>,
        }

        #[async_trait]
        impl ScraperEngine for CountingEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "ok".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 1,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                self.name
            }
        }

        let calls = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let engine: Arc<dyn ScraperEngine> = Arc::new(CountingEngine {
            name: "guarded",
            calls: calls.clone(),
        });
        let router = EngineRouter::new(vec![engine]);

        // Force the circuit breaker open for this engine: a config with
        // failure_threshold = 1 plus a single recorded failure flips it to
        // Open immediately.
        router.circuit_breaker.set_config(
            "guarded",
            CircuitConfig {
                failure_threshold: 1,
                recovery_timeout: Duration::from_secs(60),
                failure_window: Duration::from_secs(60),
            },
        );
        router.circuit_breaker.record_failure("guarded");
        assert!(
            router.circuit_breaker.is_open("guarded"),
            "circuit breaker should be open after 1 failure"
        );

        let request = make_request();
        let result = router.route(&request).await;
        assert!(
            result.is_err(),
            "route should fail when the only engine is open"
        );
        assert!(
            matches!(result.unwrap_err(), EngineError::AllEnginesFailed(_)),
            "expected AllEnginesFailed when circuit breaker blocks the only engine"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "engine must not be invoked when its circuit breaker is open"
        );
    }

    // === route_internal remaining=0 branch (line 690-692) ===

    #[tokio::test]
    async fn test_route_returns_timeout_when_remaining_time_zero() {
        // Cover line 691: after one engine attempt burns the full request
        // timeout, the next iteration computes `remaining = 0` and short-
        // circuits with EngineError::Timeout.
        struct SlowTimeoutEngine;
        #[async_trait]
        impl ScraperEngine for SlowTimeoutEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                // Sleep longer than the request timeout so that, after this
                // attempt, `start_time.elapsed()` exceeds `request.timeout`.
                tokio::time::sleep(Duration::from_millis(120)).await;
                Err(EngineError::Timeout(Duration::from_millis(120)))
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "slow-timeout"
            }
        }

        let e1: Arc<dyn ScraperEngine> = Arc::new(SlowTimeoutEngine);
        let e2: Arc<dyn ScraperEngine> = Arc::new(SlowTimeoutEngine);
        let mut router = EngineRouter::new(vec![e1, e2]);
        // Need at least 2 attempts so the loop iterates after the first
        // failure; otherwise the first Timeout would propagate directly.
        router.set_max_engine_attempts(2);
        router.set_max_retries(5);

        let mut request = make_request();
        request.timeout = Duration::from_millis(10);

        let result = router.route(&request).await;
        assert!(result.is_err(), "route should fail with Timeout");
        match result.unwrap_err() {
            EngineError::Timeout(d) => {
                assert_eq!(
                    d,
                    Duration::from_millis(10),
                    "should report the original request timeout"
                );
            }
            other => panic!("Expected Timeout, got {:?}", other),
        }
    }

    // === T062: MRT 瀑布式超时测试（red → green） ===
    //
    // design.md §14 / T062：router 顺序 fallback 路径用 `min(remaining, engine.mrt())`
    // 包裹单引擎调用，超 MRT 即切下一引擎（瀑布式），不切整体失败。
    // race_mode 路径不受影响（保留作为可选模式）。

    /// T062 red：engine1 的 scrape() 耗时超过其 MRT → router 应通过 tokio::time::timeout
    /// 在 MRT 时刻取消 engine1，记录 Timeout 失败，瀑布式切到 engine2 → engine2 立即成功。
    ///
    /// 未实现 T062 时：engine1 直接 sleep 500ms 后返回 Ok，engine2 永远不会被调用，
    /// 总耗时 ~500ms，测试失败（断言 engine2_called=true 与 elapsed<400ms）。
    #[tokio::test]
    async fn test_route_mrt_waterfall_first_engine_exceeds_mrt_falls_to_second() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::Instant;

        let engine1_called = Arc::new(AtomicBool::new(false));
        let engine2_called = Arc::new(AtomicBool::new(false));

        /// MRT 短但 scrape 耗时长的引擎（用于触发 MRT 超时）
        struct MrtSlowEngine {
            mrt: Duration,
            sleep_dur: Duration,
            called: Arc<AtomicBool>,
        }
        #[async_trait]
        impl ScraperEngine for MrtSlowEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                self.called.store(true, Ordering::SeqCst);
                // 模拟引擎处理耗时超过 MRT
                tokio::time::sleep(self.sleep_dur).await;
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "<html><body><h1>Slow Engine Response</h1><p>This is the slow engine response with sufficient visible text to pass the anti-bot detection threshold of fifty bytes required by the tier3 visible text minimum check.</p></body></html>".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: self.sleep_dur.as_millis() as u64,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100 // 高分 → 优先被选中
            }
            fn name(&self) -> &'static str {
                "mrt-slow"
            }
            fn max_response_time(&self) -> Duration {
                self.mrt
            }
        }

        /// 立即返回成功的引擎（作为 fallback 目标）
        struct FastEngine {
            called: Arc<AtomicBool>,
        }
        #[async_trait]
        impl ScraperEngine for FastEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                self.called.store(true, Ordering::SeqCst);
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "<html><body><h1>Fast Engine Response</h1><p>This is the fast engine response with sufficient visible text to pass the anti-bot detection threshold of fifty bytes required by the tier3 visible text minimum check.</p></body></html>".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 1,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                90 // 较低分 → 作为 fallback
            }
            fn name(&self) -> &'static str {
                "fast"
            }
        }

        // engine1: MRT=50ms, scrape sleeps 500ms（远超 MRT）
        let e1: Arc<dyn ScraperEngine> = Arc::new(MrtSlowEngine {
            mrt: Duration::from_millis(50),
            sleep_dur: Duration::from_millis(500),
            called: engine1_called.clone(),
        });
        // engine2: 立即返回成功
        let e2: Arc<dyn ScraperEngine> = Arc::new(FastEngine {
            called: engine2_called.clone(),
        });

        let mut router = EngineRouter::new(vec![e1, e2]);
        // 允许至少 2 次引擎尝试（瀑布式 fallback）
        router.set_max_engine_attempts(2);
        router.set_max_retries(5);
        // 关闭 race_mode 与特征过滤，确保走顺序 fallback 路径
        router.set_race_mode_enabled(false);

        let request = make_request();
        let start = Instant::now();
        let result = router.route(&request).await;
        let elapsed = start.elapsed();

        // 断言 1：最终成功（通过 engine2）
        assert!(result.is_ok(), "route should succeed via engine2 fallback");
        let resp = result.unwrap();
        assert_eq!(
            resp.content, "<html><body><h1>Fast Engine Response</h1><p>This is the fast engine response with sufficient visible text to pass the anti-bot detection threshold of fifty bytes required by the tier3 visible text minimum check.</p></body></html>",
            "response should come from fast engine (waterfall fallback)"
        );

        // 断言 2：engine1 被调用（首次尝试）
        assert!(
            engine1_called.load(Ordering::SeqCst),
            "engine1 should have been called (first attempt)"
        );
        // 断言 3：engine2 也被调用（MRT 超时后瀑布式切换）
        assert!(
            engine2_called.load(Ordering::SeqCst),
            "engine2 should have been called after engine1 exceeded MRT (waterfall)"
        );

        // 断言 4：总耗时应远小于 engine1 的 500ms sleep
        // （MRT=50ms + engine2 ~1ms + 开销，应 < 400ms）
        assert!(
            elapsed < Duration::from_millis(400),
            "should not wait for engine1's full 500ms sleep; elapsed={:?}",
            elapsed
        );
    }

    /// T062 red：engine 在其 MRT 内完成 → router 不应误超时，直接返回成功。
    ///
    /// 这是一个回归保护测试：确保 MRT 包裹不会破坏正常行为。
    /// 即使未实现 T062，此测试也应通过（因为 engine1 直接返回 Ok）。
    #[tokio::test]
    async fn test_route_mrt_engine_within_mrt_succeeds_normally() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let engine1_called = Arc::new(AtomicBool::new(false));

        struct MrtOkEngine {
            mrt: Duration,
            sleep_dur: Duration,
            called: Arc<AtomicBool>,
        }
        #[async_trait]
        impl ScraperEngine for MrtOkEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                self.called.store(true, Ordering::SeqCst);
                tokio::time::sleep(self.sleep_dur).await;
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "<html><body><h1>Real Content Page</h1><p>This is a real page with sufficient visible text to pass the anti-bot detection threshold of fifty bytes required by the tier3 visible text minimum check.</p></body></html>".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: self.sleep_dur.as_millis() as u64,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "mrt-ok"
            }
            fn max_response_time(&self) -> Duration {
                self.mrt
            }
        }

        // engine1: MRT=1s, scrape sleeps 50ms（在 MRT 内）
        let e1: Arc<dyn ScraperEngine> = Arc::new(MrtOkEngine {
            mrt: Duration::from_secs(1),
            sleep_dur: Duration::from_millis(50),
            called: engine1_called.clone(),
        });

        let mut router = EngineRouter::new(vec![e1]);
        router.set_race_mode_enabled(false);

        let request = make_request();
        let result = router.route(&request).await;

        assert!(result.is_ok(), "engine within MRT should succeed");
        let resp = result.unwrap();
        assert_eq!(resp.content, "<html><body><h1>Real Content Page</h1><p>This is a real page with sufficient visible text to pass the anti-bot detection threshold of fifty bytes required by the tier3 visible text minimum check.</p></body></html>");
        assert!(
            engine1_called.load(Ordering::SeqCst),
            "engine1 should have been called"
        );
    }

    /// T062 red：当 remaining < mrt 时，router 应使用 remaining 作为超时
    /// （即请求整体超时优先于单引擎 MRT）。
    ///
    /// 场景：request.timeout=80ms, engine.mrt=10s
    /// engine1 sleep 200ms → 应在 ~80ms 时被取消（remaining 耗尽），返回 Timeout。
    #[tokio::test]
    async fn test_route_mrt_uses_min_remaining_when_remaining_less_than_mrt() {
        use std::sync::atomic::{AtomicU32, Ordering};

        let engine1_calls = Arc::new(AtomicU32::new(0));

        struct LongMrtSlowEngine {
            mrt: Duration,
            sleep_dur: Duration,
            calls: Arc<AtomicU32>,
        }
        #[async_trait]
        impl ScraperEngine for LongMrtSlowEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(self.sleep_dur).await;
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "should-not-reach-here".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 0,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "long-mrt-slow"
            }
            fn max_response_time(&self) -> Duration {
                self.mrt
            }
        }

        // engine1: MRT=10s（很长），但 request.timeout=80ms（很短）
        // engine1 sleep 200ms → 应在 ~80ms 时被 remaining 超时取消
        let e1: Arc<dyn ScraperEngine> = Arc::new(LongMrtSlowEngine {
            mrt: Duration::from_secs(10),
            sleep_dur: Duration::from_millis(200),
            calls: engine1_calls.clone(),
        });

        let mut router = EngineRouter::new(vec![e1]);
        router.set_max_engine_attempts(1);
        router.set_max_retries(1);
        router.set_race_mode_enabled(false);

        let mut request = make_request();
        request.timeout = Duration::from_millis(80);

        let start = std::time::Instant::now();
        let result = router.route(&request).await;
        let elapsed = start.elapsed();

        assert!(result.is_err(), "should fail with Timeout");
        match result.unwrap_err() {
            EngineError::Timeout(_) => {}
            other => panic!("Expected Timeout, got {:?}", other),
        }
        // 总耗时应 ~80ms（remaining 耗尽），不是 200ms（engine sleep）或 10s（MRT）。
        // 阈值放宽到 2000ms 容忍 CI/容器环境下 tokio 调度抖动（实测容器中可能 500ms+）。
        assert!(
            elapsed < Duration::from_millis(2000),
            "should timeout at ~80ms (remaining); elapsed={:?}",
            elapsed
        );
        assert_eq!(
            engine1_calls.load(Ordering::SeqCst),
            1,
            "engine1 should be called exactly once"
        );
    }

    // === route_race_mode remaining=0 branch (line 796-798) ===

    #[tokio::test]
    async fn test_route_race_mode_returns_timeout_when_remaining_zero() {
        // Cover line 797: when race_mode is enabled and `remaining` is
        // already zero by the time route_race_mode is entered, the function
        // should immediately return EngineError::Timeout(request.timeout).
        struct NeverCalledEngine;
        #[async_trait]
        impl ScraperEngine for NeverCalledEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                panic!("engine must not be called when remaining time is zero");
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "never-called"
            }
        }

        let e1: Arc<dyn ScraperEngine> = Arc::new(NeverCalledEngine);
        let e2: Arc<dyn ScraperEngine> = Arc::new(NeverCalledEngine);
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_race_mode_enabled(true);

        let mut request = make_request();
        // Zero timeout forces `remaining = 0` immediately inside
        // route_race_mode, before any engine future is polled.
        request.timeout = Duration::from_millis(0);

        let result = router.route(&request).await;
        assert!(result.is_err(), "race_mode with zero remaining should fail");
        match result.unwrap_err() {
            EngineError::Timeout(_) => {}
            other => panic!("Expected Timeout, got {:?}", other),
        }
    }

    // === route_race_mode non-retryable error branch (line 884-886) ===

    #[tokio::test]
    async fn test_route_race_mode_non_retryable_error_returns_err() {
        // Cover line 885: when the first race future resolves to a non-
        // retryable error, route_race_mode should return that error as-is
        // instead of recording a circuit-breaker failure.
        struct InvalidUrlEngine;
        #[async_trait]
        impl ScraperEngine for InvalidUrlEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                Err(EngineError::InvalidUrl("malformed url".to_string()))
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "invalid-url"
            }
        }

        let e1: Arc<dyn ScraperEngine> = Arc::new(InvalidUrlEngine);
        let e2: Arc<dyn ScraperEngine> = Arc::new(InvalidUrlEngine);
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_race_mode_enabled(true);

        let request = make_request();
        let result = router.route(&request).await;
        assert!(
            result.is_err(),
            "race_mode with non-retryable error should fail"
        );
        match result.unwrap_err() {
            EngineError::InvalidUrl(msg) => {
                assert_eq!(msg, "malformed url");
            }
            other => panic!("Expected InvalidUrl, got {:?}", other),
        }
    }

    // === route_race_mode select_all timeout branch (line 890-897) ===

    #[tokio::test]
    async fn test_route_race_mode_returns_timeout_on_select_all_timeout() {
        // Cover lines 892 & 896: when every racing engine takes longer than
        // `timeout_duration` to resolve, time::timeout fires the Err(_)
        // branch and route_race_mode returns EngineError::Timeout with the
        // timeout_duration it actually waited.
        struct SlowOkEngine;
        #[async_trait]
        impl ScraperEngine for SlowOkEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                // Sleep much longer than the race timeout window so that
                // select_all never resolves in time.
                tokio::time::sleep(Duration::from_secs(5)).await;
                Ok(InternalScrapeResponse {
                    status_code: 200,
                    content: "ok".to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 5000,
                })
            }
            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }
            fn name(&self) -> &'static str {
                "slow-ok"
            }
        }

        let e1: Arc<dyn ScraperEngine> = Arc::new(SlowOkEngine);
        let e2: Arc<dyn ScraperEngine> = Arc::new(SlowOkEngine);
        let mut router = EngineRouter::new(vec![e1, e2]);
        router.set_race_mode_enabled(true);

        let mut request = make_request();
        // Pick a request.timeout that is comfortably larger than the time
        // route_internal spends before entering route_race_mode (so that
        // `remaining` is non-zero and we don't hit the early-return at
        // line 797), but smaller than the 5s engine sleep so that
        // time::timeout fires the Err(_) branch.
        // timeout_duration = remaining.max(100ms) ≈ 1s here.
        request.timeout = Duration::from_secs(1);

        let result = router.route(&request).await;
        assert!(
            result.is_err(),
            "race_mode with all-slow engines should time out"
        );
        match result.unwrap_err() {
            EngineError::Timeout(d) => {
                // timeout_duration = max(remaining, 100ms). Since
                // request.timeout = 1s and elapsed before route_race_mode
                // is negligible, d should be ~1s, and at minimum 100ms.
                assert!(
                    d >= Duration::from_millis(100),
                    "timeout duration should be at least 100ms, got {:?}",
                    d
                );
            }
            other => panic!("Expected Timeout, got {:?}", other),
        }
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
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
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
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
        };
        let result = router.aggregate(&request).await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.content, "Result 2");
    }

    // === T013（R-antibot-003）：反爬挑战页改派浏览器引擎 ===
    //
    // 验证：HTTP 引擎返回 Cloudflare 挑战页 HTML（status=200），被 antibot::classify 判
    // needs_browser=true，路由将其视为失败、强制后续 attempt needs_js=true，由浏览器引擎
    // 接管并返回正常结果。
    //
    // 仅在 `antibot` feature 启用时编译——`check_antibot_response` 与 cfg 块都依赖该 feature。
    #[cfg(feature = "antibot")]
    #[tokio::test]
    async fn test_t013_antibot_cloudflare_forces_needs_js_for_next_attempt() {
        use std::sync::Mutex;

        /// 记录每次调用时的 `needs_js` 值，用于断言改派行为
        struct NeedsJsRecordingEngine {
            name: &'static str,
            /// 用 Mutex 包装 Vec 以满足 ScraperEngine 的 Send+Sync 约束
            recorded_needs_js: Arc<Mutex<Vec<bool>>>,
            response: InternalScrapeResponse,
        }

        #[async_trait]
        impl ScraperEngine for NeedsJsRecordingEngine {
            async fn scrape(
                &self,
                request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                self.recorded_needs_js
                    .lock()
                    .expect("lock recorded_needs_js")
                    .push(request.needs_js);
                Ok(self.response.clone())
            }

            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }

            fn name(&self) -> &'static str {
                self.name
            }
        }

        // Cloudflare 挑战页：命中 Tier1 /cdn-cgi/challenge-platform/ 标记
        let cloudflare_body = concat!(
            "<html><head><title>Just a moment...</title></head>",
            "<body>",
            "<script src=\"/cdn-cgi/challenge-platform/h/g/orchestrate/jsch/v1\"></script>",
            "</body></html>"
        );

        let http_record = Arc::new(Mutex::new(Vec::new()));
        let http_engine: Arc<dyn ScraperEngine> = Arc::new(NeedsJsRecordingEngine {
            name: "http-reqwest",
            recorded_needs_js: http_record.clone(),
            response: InternalScrapeResponse {
                status_code: 200,
                content: cloudflare_body.to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 50,
            },
        });

        // 浏览器引擎应最终返回正常正文（body 需足够长且可见文本 >= 50 字符，
        // 避免被 antibot Tier3 近空页检测误判为 StructuralBlock）
        let browser_record = Arc::new(Mutex::new(Vec::new()));
        let browser_engine: Arc<dyn ScraperEngine> = Arc::new(NeedsJsRecordingEngine {
            name: "browser-playwright",
            recorded_needs_js: browser_record.clone(),
            response: InternalScrapeResponse {
                status_code: 200,
                content: "<html><body>This is the real rendered content from the browser \
                           engine after JavaScript execution completed successfully.</body></html>"
                    .to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 200,
            },
        });

        let mut router = EngineRouter::new(vec![http_engine, browser_engine]);
        // 关闭特征过滤与竞速，确保走顺序 fallback
        router.set_feature_filter_enabled(false);
        router.set_race_mode_enabled(false);
        router.set_strategy(LoadBalancingStrategy::WeightedRoundRobin);
        router.set_max_engine_attempts(2);
        router.set_max_retries(2);

        let request = InternalScrapeRequest {
            url: "https://example.com/protected".to_string(),
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
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
        };

        let result = router.route(&request).await;
        assert!(
            result.is_ok(),
            "route should succeed via browser engine after antibot block, got: {:?}",
            result.err()
        );
        let resp = result.unwrap();
        assert!(resp
            .content
            .contains("real rendered content from the browser"));

        // HTTP 引擎被调用 1 次，且 needs_js 与原始请求一致（false）
        let http_calls = http_record.lock().unwrap().clone();
        assert_eq!(
            http_calls.len(),
            1,
            "http engine should be called exactly once"
        );
        assert!(
            !http_calls[0],
            "first attempt must have needs_js=false (original request)"
        );

        // 浏览器引擎被调用 1 次，且 needs_js=true（强制升级）
        let browser_calls = browser_record.lock().unwrap().clone();
        assert_eq!(
            browser_calls.len(),
            1,
            "browser engine should be called exactly once"
        );
        assert!(
            browser_calls[0],
            "second attempt must have needs_js=true (force_needs_js after antibot block)"
        );
    }

    /// T013 边界：HTTP 引擎返回正常页面（非反爬挑战），不应触发 force_needs_js
    #[cfg(feature = "antibot")]
    #[tokio::test]
    async fn test_t013_normal_response_does_not_trigger_force_needs_js() {
        use std::sync::Mutex;

        struct SingleCallEngine {
            name: &'static str,
            recorded_needs_js: Arc<Mutex<Vec<bool>>>,
            response: InternalScrapeResponse,
        }

        #[async_trait]
        impl ScraperEngine for SingleCallEngine {
            async fn scrape(
                &self,
                request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                self.recorded_needs_js
                    .lock()
                    .expect("lock recorded_needs_js")
                    .push(request.needs_js);
                Ok(self.response.clone())
            }

            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }

            fn name(&self) -> &'static str {
                self.name
            }
        }

        let http_record = Arc::new(Mutex::new(Vec::new()));
        let http_engine: Arc<dyn ScraperEngine> = Arc::new(SingleCallEngine {
            name: "http-reqwest",
            recorded_needs_js: http_record.clone(),
            response: InternalScrapeResponse {
                status_code: 200,
                content: "<html><body>Normal page with sufficient visible text content \
                           to pass tier3 structural checks.</body></html>"
                    .to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 30,
            },
        });

        // 第二引擎不应被调用
        let browser_record = Arc::new(Mutex::new(Vec::new()));
        let browser_engine: Arc<dyn ScraperEngine> = Arc::new(SingleCallEngine {
            name: "browser-playwright",
            recorded_needs_js: browser_record.clone(),
            response: InternalScrapeResponse {
                status_code: 200,
                content: "browser content".to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 0,
            },
        });

        let mut router = EngineRouter::new(vec![http_engine, browser_engine]);
        router.set_feature_filter_enabled(false);
        router.set_race_mode_enabled(false);
        router.set_strategy(LoadBalancingStrategy::WeightedRoundRobin);
        router.set_max_engine_attempts(2);
        router.set_max_retries(2);

        let request = InternalScrapeRequest {
            url: "https://example.com/normal".to_string(),
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
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
        };

        let result = router.route(&request).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap().content, "<html><body>Normal page with sufficient visible text content to pass tier3 structural checks.</body></html>");

        // HTTP 引擎被调用 1 次，needs_js=false
        let http_calls = http_record.lock().unwrap().clone();
        assert_eq!(http_calls.len(), 1);
        assert!(!http_calls[0]);

        // 浏览器引擎不应被调用
        let browser_calls = browser_record.lock().unwrap().clone();
        assert!(
            browser_calls.is_empty(),
            "browser engine should NOT be called for normal response"
        );
    }

    // === T015（R-jsrender-001）：SPA 空壳响应触发改派浏览器引擎 ===
    //
    // 验证：HTTP 引擎（needs_js==false）返回含 `__NEXT_DATA__` 的 SPA 空壳响应，
    // JsUpgradeProbe 判定 upgrade=true，路由以 needs_js=true 重新 route_internal
    // 改派浏览器引擎，最终返回浏览器引擎渲染后的真实内容。
    //
    // 防递归：递归调用时 request.needs_js=true，attempt_request.needs_js=true，
    // 故 `!attempt_request.needs_js` 为 false，probe 检查自然跳过。
    #[tokio::test]
    async fn test_t015_spa_shell_triggers_js_upgrade_re_dispatch() {
        use std::sync::Mutex;

        struct NeedsJsRecordingEngine {
            name: &'static str,
            recorded_needs_js: Arc<Mutex<Vec<bool>>>,
            response: InternalScrapeResponse,
        }

        #[async_trait]
        impl ScraperEngine for NeedsJsRecordingEngine {
            async fn scrape(
                &self,
                request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                self.recorded_needs_js
                    .lock()
                    .expect("lock recorded_needs_js")
                    .push(request.needs_js);
                Ok(self.response.clone())
            }

            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }

            fn name(&self) -> &'static str {
                self.name
            }
        }

        // SPA 空壳：含 __NEXT_DATA__ 强信号（probe score=10 >= threshold 10）
        // 但可见文本 >= 50 字符，避免被 antibot Tier3 误判为 StructuralBlock
        let spa_shell = concat!(
            r#"<html><head>"#,
            r#"<script id="__NEXT_DATA__" type="application/json">{"props":{}}</script>"#,
            r#"</head><body>"#,
            r#"Loading... please wait while we render the content for you. "#,
            r#"This page requires JavaScript to function properly."#,
            r#"</body></html>"#
        );

        let http_record = Arc::new(Mutex::new(Vec::new()));
        let http_engine: Arc<dyn ScraperEngine> = Arc::new(NeedsJsRecordingEngine {
            name: "http-reqwest",
            recorded_needs_js: http_record.clone(),
            response: InternalScrapeResponse {
                status_code: 200,
                content: spa_shell.to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 30,
            },
        });

        // 浏览器引擎返回渲染后的真实内容（可见文本 >= 50 避免 antibot 误判）
        let browser_record = Arc::new(Mutex::new(Vec::new()));
        let browser_engine: Arc<dyn ScraperEngine> = Arc::new(NeedsJsRecordingEngine {
            name: "browser-playwright",
            recorded_needs_js: browser_record.clone(),
            response: InternalScrapeResponse {
                status_code: 200,
                content: "<html><body>This is the fully rendered content from the browser \
                           engine after JavaScript execution completed successfully.</body></html>"
                    .to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 200,
            },
        });

        let mut router = EngineRouter::new(vec![http_engine, browser_engine]);
        router.set_feature_filter_enabled(false);
        router.set_race_mode_enabled(false);
        router.set_strategy(LoadBalancingStrategy::WeightedRoundRobin);
        router.set_max_engine_attempts(2);
        router.set_max_retries(2);

        let request = InternalScrapeRequest {
            url: "https://example.com/spa-page".to_string(),
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
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
        };

        let result = router.route(&request).await;
        assert!(
            result.is_ok(),
            "route should succeed via browser engine after SPA shell probe, got: {:?}",
            result.err()
        );
        let resp = result.unwrap();
        assert!(
            resp.content
                .contains("fully rendered content from the browser"),
            "should return browser engine's rendered content, got: {}",
            resp.content
        );

        // HTTP 引擎被调用 1 次，needs_js=false
        let http_calls = http_record.lock().unwrap().clone();
        assert_eq!(
            http_calls.len(),
            1,
            "http engine should be called exactly once"
        );
        assert!(
            !http_calls[0],
            "http engine attempt must have needs_js=false (original request)"
        );

        // 浏览器引擎被调用 1 次，needs_js=true（probe 触发改派）
        let browser_calls = browser_record.lock().unwrap().clone();
        assert_eq!(
            browser_calls.len(),
            1,
            "browser engine should be called exactly once"
        );
        assert!(
            browser_calls[0],
            "browser engine attempt must have needs_js=true (probe-triggered re-route)"
        );
    }

    /// T015 边界：HTTP 引擎返回非 SPA 页面（无 JS 框架信号），不应触发改派
    #[tokio::test]
    async fn test_t015_non_spa_response_does_not_trigger_re_dispatch() {
        use std::sync::Mutex;

        struct SingleCallEngine {
            name: &'static str,
            recorded_needs_js: Arc<Mutex<Vec<bool>>>,
            response: InternalScrapeResponse,
        }

        #[async_trait]
        impl ScraperEngine for SingleCallEngine {
            async fn scrape(
                &self,
                request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                self.recorded_needs_js
                    .lock()
                    .expect("lock recorded_needs_js")
                    .push(request.needs_js);
                Ok(self.response.clone())
            }

            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }

            fn name(&self) -> &'static str {
                self.name
            }
        }

        let http_record = Arc::new(Mutex::new(Vec::new()));
        let http_engine: Arc<dyn ScraperEngine> = Arc::new(SingleCallEngine {
            name: "http-reqwest",
            recorded_needs_js: http_record.clone(),
            response: InternalScrapeResponse {
                status_code: 200,
                content: "<html><body>This is a static page with sufficient visible text content \
                           to pass all antibot and probe checks. No SPA framework signals here.</body></html>"
                    .to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 30,
            },
        });

        let browser_record = Arc::new(Mutex::new(Vec::new()));
        let browser_engine: Arc<dyn ScraperEngine> = Arc::new(SingleCallEngine {
            name: "browser-playwright",
            recorded_needs_js: browser_record.clone(),
            response: InternalScrapeResponse {
                status_code: 200,
                content: "browser content".to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 0,
            },
        });

        let mut router = EngineRouter::new(vec![http_engine, browser_engine]);
        router.set_feature_filter_enabled(false);
        router.set_race_mode_enabled(false);
        router.set_strategy(LoadBalancingStrategy::WeightedRoundRobin);
        router.set_max_engine_attempts(2);
        router.set_max_retries(2);

        let request = InternalScrapeRequest {
            url: "https://example.com/static-page".to_string(),
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
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
        };

        let result = router.route(&request).await;
        assert!(result.is_ok());
        assert!(result
            .unwrap()
            .content
            .contains("static page with sufficient visible text"));

        // HTTP 引擎被调用 1 次，needs_js=false
        let http_calls = http_record.lock().unwrap().clone();
        assert_eq!(http_calls.len(), 1);
        assert!(!http_calls[0]);

        // 浏览器引擎不应被调用（非 SPA，不触发 probe）
        let browser_calls = browser_record.lock().unwrap().clone();
        assert!(
            browser_calls.is_empty(),
            "browser engine should NOT be called for non-SPA response"
        );
    }

    /// T028（R-identity-002）：验证 Transient 错误重试时 UA 按 attempt seed 轮换。
    ///
    /// 场景：3 个失败引擎（Transient）+ 1 个成功引擎，max_retries=4。
    /// 预期 directive 序列：
    /// - attempt 1 (total=1)：default，无 UA 轮换
    /// - attempt 2 (total=2, da=0)：Transient attempt=0 → default，无 UA 轮换
    /// - attempt 3 (total=3, da=1)：Transient attempt=1 → rotate_ua=true，seed=2
    /// - attempt 4 (total=4, da=2)：Transient attempt=2 → rotate_ua=true，seed=3
    #[tokio::test]
    async fn test_t028_ua_rotated_across_transient_retries() {
        use std::sync::Mutex;

        /// 记录每次调用的 User-Agent header（None 表示未注入）。
        /// 使用 `error_msg` 标签 + 消息构造错误，避免 `EngineError: Clone` 依赖。
        struct UaRecordingEngine {
            name: &'static str,
            recorded_ua: Arc<Mutex<Vec<Option<String>>>>,
            /// `Some(msg)` → 返回 `EngineError::RequestFailed(msg)`；`None` → 返回成功响应
            error_msg: Option<String>,
            /// 返回 Ok 时的响应
            response: Option<InternalScrapeResponse>,
        }

        #[async_trait]
        impl ScraperEngine for UaRecordingEngine {
            async fn scrape(
                &self,
                request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                let ua = request.headers.get("User-Agent").map(|v| v.to_string());
                self.recorded_ua.lock().expect("lock recorded_ua").push(ua);
                if let Some(ref msg) = self.error_msg {
                    return Err(EngineError::RequestFailed(msg.clone()));
                }
                Ok(self.response.clone().expect("success response must be set"))
            }

            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }

            fn name(&self) -> &'static str {
                self.name
            }
        }

        fn make_failing_engine(
            name: &'static str,
            record: Arc<Mutex<Vec<Option<String>>>>,
            error_msg: &str,
        ) -> Arc<dyn ScraperEngine> {
            Arc::new(UaRecordingEngine {
                name,
                recorded_ua: record,
                error_msg: Some(error_msg.to_string()),
                response: None,
            })
        }

        fn make_success_engine(
            name: &'static str,
            record: Arc<Mutex<Vec<Option<String>>>>,
        ) -> Arc<dyn ScraperEngine> {
            Arc::new(UaRecordingEngine {
                name,
                recorded_ua: record,
                error_msg: None,
                response: Some(InternalScrapeResponse {
                    status_code: 200,
                    content: "<html><body>success content with enough visible text to pass \
                              any antibot or probe checks along the retry path</body></html>"
                        .to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 10,
                }),
            })
        }

        let rec1 = Arc::new(Mutex::new(Vec::new()));
        let rec2 = Arc::new(Mutex::new(Vec::new()));
        let rec3 = Arc::new(Mutex::new(Vec::new()));
        let rec4 = Arc::new(Mutex::new(Vec::new()));

        let engines: Vec<Arc<dyn ScraperEngine>> = vec![
            make_failing_engine("fail-1", rec1.clone(), "transient-1"),
            make_failing_engine("fail-2", rec2.clone(), "transient-2"),
            make_failing_engine("fail-3", rec3.clone(), "transient-3"),
            make_success_engine("success-4", rec4.clone()),
        ];

        let mut router = EngineRouter::new(engines);
        router.set_feature_filter_enabled(false);
        router.set_race_mode_enabled(false);
        router.set_strategy(LoadBalancingStrategy::WeightedRoundRobin);
        router.set_max_engine_attempts(4);
        router.set_max_retries(4);

        let request = InternalScrapeRequest {
            url: "https://example.com/test".to_string(),
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
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
        };

        let result = router.route(&request).await;
        assert!(
            result.is_ok(),
            "route should succeed via 4th engine after 3 transient failures, got: {:?}",
            result.err()
        );

        // attempt 1：default directive，无 UA 轮换
        let r1 = rec1.lock().unwrap().clone();
        assert_eq!(r1.len(), 1, "engine 1 should be called exactly once");
        assert!(
            r1[0].is_none(),
            "attempt 1 must not rotate UA (default directive)"
        );

        // attempt 2：Transient attempt=0 → default，无 UA 轮换
        let r2 = rec2.lock().unwrap().clone();
        assert_eq!(r2.len(), 1, "engine 2 should be called exactly once");
        assert!(
            r2[0].is_none(),
            "attempt 2 must not rotate UA (Transient attempt=0 → default directive)"
        );

        // attempt 3：Transient attempt=1 → rotate_ua=true，seed=2
        let r3 = rec3.lock().unwrap().clone();
        assert_eq!(r3.len(), 1, "engine 3 should be called exactly once");
        assert!(
            r3[0].is_some(),
            "attempt 3 must rotate UA (Transient attempt=1 → rotate_ua=true)"
        );
        let ua3 = r3[0].clone().unwrap();

        // attempt 4：Transient attempt=2 → rotate_ua=true，seed=3
        let r4 = rec4.lock().unwrap().clone();
        assert_eq!(r4.len(), 1, "engine 4 should be called exactly once");
        assert!(
            r4[0].is_some(),
            "attempt 4 must rotate UA (Transient attempt=2 → rotate_ua=true)"
        );
        let ua4 = r4[0].clone().unwrap();

        // 不同 seed 必须返回不同 UA（pick_seeded(2) vs pick_seeded(3)，desktop pool ≥22）
        assert_ne!(
            ua3, ua4,
            "UA must differ across retry attempts (seed=2 vs seed=3)"
        );
    }

    /// C-1 回归测试：重试轮换 UA 时所有指纹相关 header 必须同步一致。
    ///
    /// 场景：3 个失败引擎（Transient）+ 1 个成功引擎，max_retries=4。
    /// 预期：attempt 3/4 触发 `directive.rotate_ua=true` 时，
    ///   - User-Agent / Accept-Language / sec-ch-ua 三者必须来自同一 profile
    ///   - 与 `UaPool::pick_seeded(seed, false)` 返回的 profile 字段严格相等
    ///
    /// 修复前：router 只覆盖 User-Agent，Accept-Language 与 sec-ch-ua 仍是首次 profile 的值，
    ///         导致指纹矛盾（如 Chrome UA + Firefox sec-ch-ua）。
    /// 修复后：三者一次性写入，保证指纹一致。
    #[tokio::test]
    async fn test_c1_fingerprint_headers_rotated_together() {
        use crate::utils::ua_pool::UaPool;
        use std::sync::Mutex;

        /// 记录每次调用全部指纹相关 header（UA / AL / sec-ch-ua）
        struct FingerprintRecordingEngine {
            name: &'static str,
            recorded: Arc<Mutex<Vec<(Option<String>, Option<String>, Option<String>)>>>,
            error_msg: Option<String>,
            response: Option<InternalScrapeResponse>,
        }

        #[async_trait]
        impl ScraperEngine for FingerprintRecordingEngine {
            async fn scrape(
                &self,
                request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                let ua = request.headers.get("User-Agent").map(|v| v.to_string());
                let al = request
                    .headers
                    .get("Accept-Language")
                    .map(|v| v.to_string());
                let ch = request.headers.get("sec-ch-ua").map(|v| v.to_string());
                self.recorded
                    .lock()
                    .expect("lock recorded")
                    .push((ua, al, ch));
                if let Some(ref msg) = self.error_msg {
                    return Err(EngineError::RequestFailed(msg.clone()));
                }
                Ok(self.response.clone().expect("success response must be set"))
            }

            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }

            fn name(&self) -> &'static str {
                self.name
            }
        }

        fn make_failing(
            name: &'static str,
            rec: Arc<Mutex<Vec<(Option<String>, Option<String>, Option<String>)>>>,
        ) -> Arc<dyn ScraperEngine> {
            Arc::new(FingerprintRecordingEngine {
                name,
                recorded: rec,
                error_msg: Some("transient".to_string()),
                response: None,
            })
        }

        fn make_success(
            name: &'static str,
            rec: Arc<Mutex<Vec<(Option<String>, Option<String>, Option<String>)>>>,
        ) -> Arc<dyn ScraperEngine> {
            Arc::new(FingerprintRecordingEngine {
                name,
                recorded: rec,
                error_msg: None,
                response: Some(InternalScrapeResponse {
                    status_code: 200,
                    content: "<html><body>success content with enough visible text to pass \
                              any antibot or probe checks along the retry path</body></html>"
                        .to_string(),
                    screenshot: None,
                    content_type: "text/html".to_string(),
                    headers: HashMap::new(),
                    response_time_ms: 10,
                }),
            })
        }

        let rec1 = Arc::new(Mutex::new(Vec::new()));
        let rec2 = Arc::new(Mutex::new(Vec::new()));
        let rec3 = Arc::new(Mutex::new(Vec::new()));
        let rec4 = Arc::new(Mutex::new(Vec::new()));

        let engines: Vec<Arc<dyn ScraperEngine>> = vec![
            make_failing("fail-1", rec1.clone()),
            make_failing("fail-2", rec2.clone()),
            make_failing("fail-3", rec3.clone()),
            make_success("success-4", rec4.clone()),
        ];

        let mut router = EngineRouter::new(engines);
        router.set_feature_filter_enabled(false);
        router.set_race_mode_enabled(false);
        router.set_strategy(LoadBalancingStrategy::WeightedRoundRobin);
        router.set_max_engine_attempts(4);
        router.set_max_retries(4);

        let request = InternalScrapeRequest {
            url: "https://example.com/test".to_string(),
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
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
        };

        let result = router.route(&request).await;
        assert!(
            result.is_ok(),
            "route should succeed via 4th engine after 3 transient failures, got: {:?}",
            result.err()
        );

        // attempt 3：Transient attempt=1 → rotate_ua=true，seed=2
        let r3 = rec3.lock().unwrap().clone();
        assert_eq!(r3.len(), 1, "engine 3 should be called exactly once");
        let (ua3, al3, ch3) = r3[0].clone();
        assert!(ua3.is_some(), "attempt 3 must rotate User-Agent");
        let ua3 = ua3.expect("ua3 set");

        // attempt 4：Transient attempt=2 → rotate_ua=true，seed=3
        let r4 = rec4.lock().unwrap().clone();
        assert_eq!(r4.len(), 1, "engine 4 should be called exactly once");
        let (ua4, al4, ch4) = r4[0].clone();
        assert!(ua4.is_some(), "attempt 4 must rotate User-Agent");
        let ua4 = ua4.expect("ua4 set");

        // 与 UaPool.pick_seeded 的预期 profile 字段一致
        let pool = UaPool::new();
        let p3 = pool.pick_seeded(2, false);
        let p4 = pool.pick_seeded(3, false);

        // C-1 核心：UA + Accept-Language + sec-ch-ua 三者必须来自同一 profile
        assert_eq!(
            ua3, p3.ua,
            "attempt 3 User-Agent must match pick_seeded(2).ua"
        );
        assert_eq!(
            al3.as_deref(),
            Some(p3.accept_language),
            "attempt 3 Accept-Language must match profile.accept_language (C-1: 同步轮换)"
        );
        assert_eq!(
            ch3.as_deref(),
            if p3.sec_ch_ua.is_empty() {
                None
            } else {
                Some(p3.sec_ch_ua)
            },
            "attempt 3 sec-ch-ua must match profile.sec_ch_ua (C-1: 同步轮换，Firefox/Safari 为 None)"
        );

        assert_eq!(
            ua4, p4.ua,
            "attempt 4 User-Agent must match pick_seeded(3).ua"
        );
        assert_eq!(
            al4.as_deref(),
            Some(p4.accept_language),
            "attempt 4 Accept-Language must match profile.accept_language (C-1: 同步轮换)"
        );
        assert_eq!(
            ch4.as_deref(),
            if p4.sec_ch_ua.is_empty() {
                None
            } else {
                Some(p4.sec_ch_ua)
            },
            "attempt 4 sec-ch-ua must match profile.sec_ch_ua (C-1: 同步轮换)"
        );

        // 不同 seed 必须返回不同 UA
        assert_ne!(
            ua3, ua4,
            "UA must differ across retry attempts (seed=2 vs seed=3)"
        );
    }

    /// T028（R-identity-002）：验证 RetryTracker 在 FeatureToggle cap=3 时停止重试。
    ///
    /// 场景：5 个引擎全部返回 `EngineError::FeatureToggle`，max_retries=5（高于 cap=3）。
    /// 预期：tracker 在第 3 次 record 后 ft=3 → should_retry(FeatureToggle) 返回 false → 停止。
    /// 即只调用前 3 个引擎，返回 FeatureToggle 错误。
    #[tokio::test]
    async fn test_t028_retry_tracker_caps_feature_toggle() {
        use std::sync::Mutex;

        struct FtFailingEngine {
            name: &'static str,
            call_count: Arc<Mutex<u32>>,
        }

        #[async_trait]
        impl ScraperEngine for FtFailingEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                let mut c = self.call_count.lock().unwrap();
                *c += 1;
                Err(EngineError::FeatureToggle(format!("toggle-fail-{}", *c)))
            }

            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }

            fn name(&self) -> &'static str {
                self.name
            }
        }

        let counts: Vec<Arc<Mutex<u32>>> = (0..5).map(|_| Arc::new(Mutex::new(0u32))).collect();

        let engines: Vec<Arc<dyn ScraperEngine>> = (0..5)
            .map(|i| {
                let e: Arc<dyn ScraperEngine> = Arc::new(FtFailingEngine {
                    name: Box::leak(format!("ft-fail-{}", i).into_boxed_str()),
                    call_count: counts[i].clone(),
                });
                e
            })
            .collect();

        let mut router = EngineRouter::new(engines);
        router.set_feature_filter_enabled(false);
        router.set_race_mode_enabled(false);
        router.set_strategy(LoadBalancingStrategy::WeightedRoundRobin);
        router.set_max_engine_attempts(5);
        // max_retries=5 > feature_toggle cap=3，验证 tracker 先于 max_retries 触发
        router.set_max_retries(5);

        let request = InternalScrapeRequest {
            url: "https://example.com/ft-test".to_string(),
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
            block_ads: false,
            block_media: false,
            session_id: None,
            wait_for: None,
        };

        let result = router.route(&request).await;
        assert!(
            result.is_err(),
            "route must fail after RetryTracker caps FeatureToggle"
        );
        let err = result.unwrap_err();
        assert!(
            matches!(err, EngineError::FeatureToggle(_)),
            "error must be FeatureToggle, got: {:?}",
            err
        );

        // 验证只有前 3 个引擎被调用（cap=3 → 3 次 record 后停止）
        for i in 0..3 {
            let c = counts[i].lock().unwrap();
            assert_eq!(
                *c, 1,
                "engine {} should be called exactly once (within cap)",
                i
            );
        }
        for i in 3..5 {
            let c = counts[i].lock().unwrap();
            assert_eq!(
                *c, 0,
                "engine {} should NOT be called (RetryTracker stopped after cap=3)",
                i
            );
        }
    }
}
