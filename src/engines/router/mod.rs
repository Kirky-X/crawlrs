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
use crate::utils::retry::{RetryReason, RetryTracker};
use crate::utils::ua_pool::UaPool;
use dashmap::DashMap;
use log::{info, warn};
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

    // T034: select_optimal_engines, should_filter_by_feature, calculate_engine_score,
    // sort_candidates_by_strategy 已拆分到 engine_selector.rs (partial impl block)

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

    // T035: route_internal 已拆分到 route_sequential.rs (partial impl block)
    // T035: route_race_mode 已拆分到 route_race.rs (partial impl block)

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
#[cfg(feature = "content")]
pub(super) fn check_antibot_response(
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
pub(super) fn check_js_upgrade_probe(
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
#[path = "../tests/router_test.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/router_tests_impl.rs"]
mod tests_impl;

// T034: 引擎选择逻辑拆分
mod engine_selector;
// T035: 路由模式拆分
mod route_race;
mod route_sequential;
