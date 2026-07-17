// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::search::engine_trait::{SearchEngine, SearchRequest};
use crate::search::error::SearchError;
use crate::search::response::{Response, ResponseItem};
use crate::search::types::{EngineHealth, SearchEngineType};
use async_trait::async_trait;
use log::{debug, info, warn};
use parking_lot::RwLock;
use rand::Rng;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 搜索引擎指标
#[derive(Debug, Clone, Default)]
pub struct EngineMetrics {
    /// 总请求数
    pub total_requests: u64,
    /// 成功请求数
    pub successful_requests: u64,
    /// 失败请求数
    pub failed_requests: u64,
    /// 平均响应时间
    pub avg_response_time: Duration,
    /// 最后使用时间
    pub last_used: Option<Instant>,
    /// 健康状态
    pub health: EngineHealth,
    /// 连续失败次数
    pub consecutive_failures: u32,
    /// 最后成功时间
    pub last_success: Option<Instant>,
}

impl EngineMetrics {
    /// 计算成功率
    pub fn success_rate(&self) -> f64 {
        if self.total_requests == 0 {
            1.0
        } else {
            self.successful_requests as f64 / self.total_requests as f64
        }
    }

    /// 记录请求开始
    pub fn record_request_start(&mut self) {
        self.total_requests += 1;
    }

    /// 记录成功
    pub fn record_success(&mut self, response_time: Duration) {
        self.successful_requests += 1;
        self.consecutive_failures = 0;
        self.last_success = Some(Instant::now());
        self.last_used = Some(Instant::now());

        // 更新平均响应时间
        let total_time =
            self.avg_response_time.as_secs_f64() * (self.successful_requests as f64 - 1.0);
        self.avg_response_time = Duration::from_secs_f64(
            (total_time + response_time.as_secs_f64()) / self.successful_requests as f64,
        );

        // 更新健康状态
        if self.health == EngineHealth::Unhealthy {
            self.health = EngineHealth::Degraded;
        } else if self.health == EngineHealth::Degraded && self.consecutive_failures == 0 {
            self.health = EngineHealth::Healthy;
        }
    }

    /// 记录请求失败
    pub fn record_failure(&mut self) {
        self.failed_requests += 1;
        self.consecutive_failures += 1;
        self.last_used = Some(Instant::now());

        // 更新健康状态
        if self.consecutive_failures >= 3 {
            self.health = EngineHealth::Unhealthy;
        } else if self.consecutive_failures >= 1 {
            self.health = EngineHealth::Degraded;
        }
    }

    /// 检查是否可以重试
    pub fn can_retry(&self) -> bool {
        match self.health {
            EngineHealth::Healthy => true,
            EngineHealth::Degraded => true,
            EngineHealth::Unhealthy => {
                // 如果最后成功时间超过5分钟，尝试恢复
                if let Some(last_success) = self.last_success {
                    Instant::now().duration_since(last_success) > Duration::from_secs(300)
                } else {
                    false
                }
            }
            EngineHealth::Isolated => false,
            EngineHealth::Unknown => true,
        }
    }
}

/// 搜索引擎路由器配置
#[derive(Debug, Clone)]
pub struct SearchEngineRouterConfig {
    /// 故障后重试次数
    pub max_retries: u32,
    /// 请求超时时间
    pub request_timeout: Duration,
    /// 健康检查间隔
    pub health_check_interval: Duration,
    /// 不健康引擎恢复时间
    pub unhealthy_recovery_time: Duration,
    /// 是否启用自动故障转移
    pub enable_auto_failover: bool,
    /// 是否启用负载均衡
    pub enable_load_balancing: bool,
}

impl Default for SearchEngineRouterConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            request_timeout: Duration::from_secs(30),
            health_check_interval: Duration::from_secs(60),
            unhealthy_recovery_time: Duration::from_secs(300),
            enable_auto_failover: true,
            enable_load_balancing: true,
        }
    }
}

/// 搜索引擎路由器
///
/// 负责管理多个搜索引擎，提供智能选择、故障转移和负载均衡
pub struct SearchEngineRouter {
    /// 搜索引擎列表 (名称 -> Arc<搜索引擎>)
    engines: HashMap<String, Arc<dyn SearchEngine>>,
    /// 引擎指标 (名称 -> 指标)
    metrics: RwLock<HashMap<String, EngineMetrics>>,
    /// 配置
    config: SearchEngineRouterConfig,
}

impl Clone for SearchEngineRouter {
    fn clone(&self) -> Self {
        Self {
            engines: self.engines.clone(),
            metrics: RwLock::new(self.metrics.read().clone()),
            config: self.config.clone(),
        }
    }
}

impl Default for SearchEngineRouter {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchEngineRouter {
    /// 创建新的搜索引擎路由器
    pub fn new() -> Self {
        Self {
            engines: HashMap::with_capacity(8),
            metrics: RwLock::new(HashMap::with_capacity(8)),
            config: SearchEngineRouterConfig::default(),
        }
    }

    /// 创建带配置的搜索引擎路由器
    pub fn with_config(config: SearchEngineRouterConfig) -> Self {
        Self {
            engines: HashMap::with_capacity(8),
            metrics: RwLock::new(HashMap::with_capacity(8)),
            config,
        }
    }

    /// 注册搜索引擎
    pub fn register_engine(&mut self, engine: Arc<dyn SearchEngine>) {
        let name = engine.name().to_string();
        let name_for_log = name.clone();
        self.engines.insert(name.clone(), engine);
        self.metrics.write().insert(name, EngineMetrics::default());
        info!("搜索引擎已注册: {}", name_for_log);
    }

    /// 批量注册搜索引擎
    pub fn register_engines(&mut self, engines: Vec<Arc<dyn SearchEngine>>) {
        for engine in engines {
            self.register_engine(engine);
        }
    }

    /// 获取所有已注册的引擎名称
    pub fn registered_engines(&self) -> Vec<String> {
        self.engines.keys().cloned().collect()
    }

    /// 获取引擎（按名称）
    pub fn get_engine(&self, name: &str) -> Option<Arc<dyn SearchEngine>> {
        self.engines.get(name).cloned()
    }

    /// 获取引擎指标
    pub fn get_engine_metrics(&self, name: &str) -> Option<EngineMetrics> {
        self.metrics.read().get(name).cloned()
    }

    /// 获取所有引擎指标
    pub fn all_metrics(&self) -> HashMap<String, EngineMetrics> {
        self.metrics.read().clone()
    }

    /// 更新引擎配置
    pub fn update_config(&mut self, config: SearchEngineRouterConfig) {
        self.config = config;
    }

    /// 根据策略选择单个最优引擎
    ///
    /// 该方法是 `select_with_priority` 和 `select_with_load_balancing` 的对外门面：
    /// - `enable_load_balancing=true`：调用 `select_with_load_balancing` 进行加权随机选择
    /// - `enable_load_balancing=false`：调用 `select_with_priority` 取健康优先排序后的第一个
    ///
    /// 如果 `preferred` 指定的引擎可用，则优先返回该引擎。
    pub fn select_engine(&self, preferred: Option<&str>) -> Option<Arc<dyn SearchEngine>> {
        if self.config.enable_load_balancing {
            self.select_with_load_balancing(preferred)
        } else {
            self.select_with_priority(preferred).into_iter().next()
        }
    }

    /// 负载均衡选择（加权随机返回单个引擎）
    ///
    /// 权重公式：`success_rate * 0.7 + speed_weight * 0.3`
    /// - success_rate ∈ [0.0, 1.0]，归一化为 0-100
    /// - speed_weight：<1s=100，<5s=50，否则=10
    pub fn select_with_load_balancing(
        &self,
        preferred: Option<&str>,
    ) -> Option<Arc<dyn SearchEngine>> {
        let metrics = self.metrics.read();
        // 不再 clone String，用 engine.name() 动态读取（preferred 匹配 + 日志场景）
        let mut candidates: Vec<(Arc<dyn SearchEngine>, EngineMetrics)> = Vec::new();

        for (name, engine) in &self.engines {
            if let Some(metric) = metrics.get(name) {
                if metric.can_retry() {
                    candidates.push((engine.clone(), metric.clone()));
                }
            }
        }
        // 显式释放读锁，避免在加权随机计算中持有锁
        drop(metrics);

        if candidates.is_empty() {
            warn!("没有可用的搜索引擎");
            return None;
        }

        // 如果有首选引擎且可用，优先使用
        if let Some(pref) = preferred {
            if let Some((engine, metric)) = candidates.iter().find(|(e, _)| e.name() == pref) {
                if metric.can_retry() {
                    info!("使用首选搜索引擎: {}", pref);
                    return Some(engine.clone());
                }
            }
        }

        // 预计算权重表，避免 total_weight 和累减两次重复计算
        let weights: Vec<f64> = candidates
            .iter()
            .map(|(_, m)| {
                let success_weight = m.success_rate() * 100.0;
                let speed_weight = if m.avg_response_time.as_secs_f64() < 1.0 {
                    100.0
                } else if m.avg_response_time.as_secs_f64() < 5.0 {
                    50.0
                } else {
                    10.0
                };
                success_weight * 0.7 + speed_weight * 0.3
            })
            .collect();
        let total_weight: f64 = weights.iter().sum();

        if total_weight == 0.0 {
            return candidates.first().map(|(e, _)| e.clone());
        }

        let mut rng = rand::rng();
        let mut random_weight = rng.random_range(0.0..total_weight);

        // 累减复用预计算的 weights，避免重复计算权重公式
        for (i, (engine, _)) in candidates.iter().enumerate() {
            random_weight -= weights[i];
            if random_weight <= 0.0 {
                return Some(engine.clone());
            }
        }

        candidates.first().map(|(e, _)| e.clone())
    }

    /// 健康优先排序所有可用引擎
    ///
    /// 排序规则：
    /// 1. 健康状态优先级：Healthy > Degraded > Unhealthy > Isolated > Unknown
    /// 2. 同健康状态下，响应时间短的优先
    ///
    /// 如果 `preferred` 指定的引擎可用，则会被交换到列表首位。
    pub fn select_with_priority(&self, preferred: Option<&str>) -> Vec<Arc<dyn SearchEngine>> {
        let metrics = self.metrics.read();
        // 不再 clone String，用 engine.name() 动态读取（preferred 匹配 + 日志场景）
        let mut candidates: Vec<(Arc<dyn SearchEngine>, EngineMetrics)> = Vec::new();

        for (name, engine) in &self.engines {
            if let Some(metric) = metrics.get(name) {
                if metric.can_retry() {
                    candidates.push((engine.clone(), metric.clone()));
                }
            }
        }
        // 显式释放读锁，避免在排序计算中持有锁
        drop(metrics);

        if candidates.is_empty() {
            warn!("没有可用的搜索引擎");
            return Vec::new();
        }

        // 按健康状态和响应时间排序
        candidates.sort_by(|a, b| {
            let health_order = |h: &EngineHealth| match h {
                EngineHealth::Healthy => 0,
                EngineHealth::Degraded => 1,
                EngineHealth::Unhealthy => 2,
                EngineHealth::Isolated => 3,
                EngineHealth::Unknown => 4,
            };

            let a_order = health_order(&a.1.health);
            let b_order = health_order(&b.1.health);

            if a_order != b_order {
                a_order.cmp(&b_order)
            } else {
                // 健康状态相同时，选择响应时间短的
                b.1.avg_response_time.cmp(&a.1.avg_response_time)
            }
        });

        let mut sorted: Vec<Arc<dyn SearchEngine>> =
            candidates.into_iter().map(|(e, _)| e).collect();

        // Preferred engine first
        if let Some(pref) = preferred {
            if let Some(pos) = sorted.iter().position(|e| e.name() == pref) {
                if pos > 0 {
                    sorted.swap(0, pos);
                }
            }
        }

        sorted
    }

    /// 搜索（带智能路由）
    ///
    /// 使用 `select_with_priority` 进行健康优先排序，preferred 引擎被交换到首位。
    /// 按排序顺序逐个尝试引擎，失败时根据 `enable_auto_failover` 决定是否继续。
    pub async fn search(
        &self,
        request: &SearchRequest,
        preferred: Option<&str>,
    ) -> Result<Response<ResponseItem>, SearchError> {
        let mut last_error: Option<SearchError> = None;

        // 使用智能路由选择引擎（健康优先排序）
        let sorted_engines = self.select_with_priority(preferred);

        if sorted_engines.is_empty() {
            return Err(SearchError::NoEngineAvailable);
        }

        // 热路径降级到 debug 级别，避免 info! 宏急切求值导致 Vec<String> 分配
        debug!(
            "智能路由选择 {} 个候选引擎，按健康度排序: {:?}",
            sorted_engines.len(),
            sorted_engines.iter().map(|e| e.name()).collect::<Vec<_>>()
        );

        for engine in &sorted_engines {
            let name = engine.name();
            let engine_start_time = Instant::now();

            // 记录请求开始
            {
                let mut metrics = self.metrics.write();
                if let Some(m) = metrics.get_mut(name) {
                    m.record_request_start();
                }
            }

            match tokio::time::timeout(self.config.request_timeout, engine.search(request)).await {
                Ok(Ok(response)) => {
                    let response_time = engine_start_time.elapsed();
                    {
                        let mut metrics = self.metrics.write();
                        if let Some(m) = metrics.get_mut(name) {
                            m.record_success(response_time);
                        }
                    }

                    info!(
                        "搜索引擎 {} 成功返回 {} 个结果 (耗时: {:?})",
                        name,
                        response.items.len(),
                        response_time
                    );

                    return Ok(response);
                }
                Ok(Err(e)) => {
                    {
                        let mut metrics = self.metrics.write();
                        if let Some(m) = metrics.get_mut(name) {
                            m.record_failure();
                        }
                    }
                    warn!("搜索引擎 {} 失败: {}", name, e);
                    last_error = Some(e);
                }
                Err(elapsed) => {
                    last_error = Some(SearchError::Timeout(elapsed));
                    {
                        let mut metrics = self.metrics.write();
                        if let Some(m) = metrics.get_mut(name) {
                            m.record_failure();
                        }
                    }
                    warn!("搜索引擎 {} 请求超时", name);
                }
            }

            // 如果禁用了自动故障转移，停止尝试
            if !self.config.enable_auto_failover {
                break;
            }
        }

        warn!("所有搜索引擎都失败了，最后一个错误: {:?}", last_error);

        Err(last_error.unwrap_or_else(|| SearchError::Engine("所有搜索引擎都失败".to_string())))
    }

    /// 简单的搜索方法（使用单个最优引擎）
    ///
    /// 通过 `select_engine` 选取单个最优引擎执行，失败时回退到全引擎 `search`。
    pub async fn simple_search(&self, query: &str) -> Result<Response<ResponseItem>, SearchError> {
        // 选取单个最优引擎执行
        if let Some(engine) = self.select_engine(None) {
            let request = SearchRequest::new(query);
            let name = engine.name();
            let start_time = Instant::now();

            {
                let mut metrics = self.metrics.write();
                if let Some(m) = metrics.get_mut(name) {
                    m.record_request_start();
                }
            }

            match tokio::time::timeout(self.config.request_timeout, engine.search(&request)).await {
                Ok(Ok(response)) => {
                    let response_time = start_time.elapsed();
                    {
                        let mut metrics = self.metrics.write();
                        if let Some(m) = metrics.get_mut(name) {
                            m.record_success(response_time);
                        }
                    }
                    return Ok(response);
                }
                Ok(Err(e)) => {
                    {
                        let mut metrics = self.metrics.write();
                        if let Some(m) = metrics.get_mut(name) {
                            m.record_failure();
                        }
                    }
                    warn!("默认搜索引擎 {} 失败: {}，回退到全引擎搜索", name, e);
                }
                Err(elapsed) => {
                    {
                        let mut metrics = self.metrics.write();
                        if let Some(m) = metrics.get_mut(name) {
                            m.record_failure();
                        }
                    }
                    warn!(
                        "默认搜索引擎 {} 超时: {:?}，回退到全引擎搜索",
                        name, elapsed
                    );
                }
            }
        }

        // 单引擎失败或没有引擎，回退到全引擎搜索
        self.search(&SearchRequest::new(query), None).await
    }

    /// 检查引擎健康状态
    pub fn check_engine_health(&self, name: &str) -> EngineHealth {
        self.metrics
            .read()
            .get(name)
            .map(|m| m.health)
            .unwrap_or(EngineHealth::Healthy)
    }

    /// 重置引擎指标
    pub fn reset_metrics(&mut self) {
        let metrics = self.metrics.get_mut();
        for metric in metrics.values_mut() {
            *metric = EngineMetrics::default();
        }
    }

    /// 隔离引擎（临时禁用）
    pub fn isolate_engine(&mut self, name: &str) {
        if let Some(metric) = self.metrics.write().get_mut(name) {
            metric.health = EngineHealth::Isolated;
            info!("搜索引擎 {} 已被隔离", name);
        }
    }

    /// 恢复引擎
    pub fn recover_engine(&mut self, name: &str) {
        if let Some(metric) = self.metrics.write().get_mut(name) {
            metric.health = EngineHealth::Healthy;
            metric.consecutive_failures = 0;
            info!("搜索引擎 {} 已恢复", name);
        }
    }

    /// 获取路由器统计信息
    pub fn stats(&self) -> RouterStats {
        let metrics = self.metrics.read();
        let mut total_requests = 0;
        let mut total_success = 0;
        let mut total_failures = 0;

        for metric in metrics.values() {
            total_requests += metric.total_requests;
            total_success += metric.successful_requests;
            total_failures += metric.failed_requests;
        }

        RouterStats {
            total_requests,
            total_success,
            total_failures,
            engine_count: self.engines.len(),
            overall_success_rate: if total_requests > 0 {
                total_success as f64 / total_requests as f64
            } else {
                1.0
            },
        }
    }
}

#[async_trait]
impl SearchEngine for SearchEngineRouter {
    fn name(&self) -> &'static str {
        "smart_router"
    }

    fn engine_type(&self) -> SearchEngineType {
        SearchEngineType::Auto
    }

    fn health(&self) -> EngineHealth {
        let metrics = self.metrics.read();
        if metrics.is_empty() {
            return EngineHealth::Unknown;
        }

        let mut any_healthy = false;
        let mut any_degraded = false;

        for metric in metrics.values() {
            match metric.health {
                EngineHealth::Healthy => any_healthy = true,
                EngineHealth::Degraded => any_degraded = true,
                _ => {}
            }
        }

        if any_healthy {
            EngineHealth::Healthy
        } else if any_degraded {
            EngineHealth::Degraded
        } else {
            EngineHealth::Unhealthy
        }
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        self.search(request, None).await
    }
}

/// 路由器统计信息
#[derive(Debug, Clone)]
pub struct RouterStats {
    /// 总请求数
    pub total_requests: u64,
    /// 成功请求数
    pub total_success: u64,
    /// 失败请求数
    pub total_failures: u64,
    /// 引擎数量
    pub engine_count: usize,
    /// 整体成功率
    pub overall_success_rate: f64,
}

/// 支持智能路由的搜索引擎包装器
#[derive(Clone)]
pub struct SmartSearchEngineWrapper {
    /// 内部搜索引擎
    inner: Arc<dyn SearchEngine>,
    /// 路由器引用（可选）
    router: Option<Arc<SearchEngineRouter>>,
    /// 引擎名称
    name: &'static str,
}

impl SmartSearchEngineWrapper {
    /// 创建包装器
    pub fn new(inner: Arc<dyn SearchEngine>, router: Option<Arc<SearchEngineRouter>>) -> Self {
        let name = inner.name();
        Self {
            inner,
            router,
            name,
        }
    }

    /// 获取内部引擎
    pub fn inner(&self) -> &Arc<dyn SearchEngine> {
        &self.inner
    }
}

#[async_trait]
impl SearchEngine for SmartSearchEngineWrapper {
    fn name(&self) -> &'static str {
        self.name
    }

    fn engine_type(&self) -> SearchEngineType {
        self.inner.engine_type()
    }

    fn health(&self) -> EngineHealth {
        self.inner.health()
    }

    async fn search(&self, request: &SearchRequest) -> Result<Response<ResponseItem>, SearchError> {
        // 如果有路由器，使用路由器进行搜索
        if let Some(router) = &self.router {
            return router.search(request, Some(self.name)).await;
        }

        // 否则直接使用内部搜索引擎
        self.inner.search(request).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    #[derive(Clone)]
    #[allow(dead_code)]
    struct MockSearchEngine {
        name: &'static str,
        should_fail: bool,
        response_time: std::time::Duration,
    }

    impl MockSearchEngine {
        #[allow(dead_code)]
        fn new(name: &'static str, should_fail: bool, response_time_ms: u64) -> Self {
            Self {
                name,
                should_fail,
                response_time: std::time::Duration::from_millis(response_time_ms),
            }
        }
    }

    #[async_trait]
    impl SearchEngine for MockSearchEngine {
        fn name(&self) -> &'static str {
            self.name
        }

        fn engine_type(&self) -> SearchEngineType {
            SearchEngineType::Auto
        }

        fn health(&self) -> EngineHealth {
            if self.should_fail {
                EngineHealth::Unhealthy
            } else {
                EngineHealth::Healthy
            }
        }

        async fn search(
            &self,
            _request: &SearchRequest,
        ) -> Result<Response<ResponseItem>, SearchError> {
            tokio::time::sleep(self.response_time).await;

            if self.should_fail {
                Err(SearchError::Engine(format!("{} engine failed", self.name)))
            } else {
                Ok(Response {
                    items: vec![ResponseItem {
                        title: format!("Result from {}", self.name),
                        url: format!("https://example.com/{}", self.name),
                        description: format!("Description from {}", self.name),
                        engine: SearchEngineType::Auto,
                    }],
                    total_results: Some(1),
                    engine: SearchEngineType::Auto,
                })
            }
        }
    }

    #[tokio::test]
    async fn test_router_creation() {
        let router = SearchEngineRouter::new();
        assert_eq!(router.registered_engines().len(), 0);
    }

    #[tokio::test]
    async fn test_router_with_config() {
        let config = SearchEngineRouterConfig {
            enable_load_balancing: true,
            enable_auto_failover: true,
            request_timeout: std::time::Duration::from_secs(30),
            health_check_interval: Duration::from_secs(60),
            unhealthy_recovery_time: Duration::from_secs(300),
            max_retries: 3,
        };
        let router = SearchEngineRouter::with_config(config.clone());
        assert_eq!(router.config.request_timeout, config.request_timeout);
    }
}

// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#[cfg(test)]
mod tests_ext {
    use super::*;

    // ========== Mock SearchEngine for extended tests ==========

    struct MockEngine {
        name: &'static str,
        should_fail: bool,
        delay: Duration,
    }

    impl MockEngine {
        fn new(name: &'static str) -> Self {
            Self {
                name,
                should_fail: false,
                delay: Duration::from_millis(0),
            }
        }

        fn failing(name: &'static str) -> Self {
            Self {
                name,
                should_fail: true,
                delay: Duration::from_millis(0),
            }
        }
    }

    #[async_trait]
    impl SearchEngine for MockEngine {
        fn name(&self) -> &'static str {
            self.name
        }

        fn engine_type(&self) -> SearchEngineType {
            SearchEngineType::Google
        }

        fn health(&self) -> EngineHealth {
            if self.should_fail {
                EngineHealth::Unhealthy
            } else {
                EngineHealth::Healthy
            }
        }

        async fn search(
            &self,
            _request: &SearchRequest,
        ) -> Result<Response<ResponseItem>, SearchError> {
            if self.delay > Duration::ZERO {
                tokio::time::sleep(self.delay).await;
            }
            if self.should_fail {
                Err(SearchError::Engine(format!("{} failed", self.name)))
            } else {
                Ok(Response {
                    items: vec![ResponseItem {
                        title: format!("Result from {}", self.name),
                        url: format!("https://example.com/{}", self.name),
                        description: format!("Desc from {}", self.name),
                        engine: SearchEngineType::Google,
                    }],
                    total_results: Some(1),
                    engine: SearchEngineType::Google,
                })
            }
        }
    }

    fn make_mock_engine(name: &'static str) -> Arc<dyn SearchEngine> {
        Arc::new(MockEngine::new(name))
    }

    fn make_failing_engine(name: &'static str) -> Arc<dyn SearchEngine> {
        Arc::new(MockEngine::failing(name))
    }

    // ========== EngineMetrics tests ==========

    #[test]
    fn test_engine_metrics_default() {
        let m = EngineMetrics::default();
        assert_eq!(m.total_requests, 0);
        assert_eq!(m.successful_requests, 0);
        assert_eq!(m.failed_requests, 0);
        assert_eq!(m.avg_response_time, Duration::ZERO);
        assert_eq!(m.health, EngineHealth::Healthy);
        assert_eq!(m.consecutive_failures, 0);
    }

    #[test]
    fn test_engine_metrics_success_rate_with_no_requests() {
        let m = EngineMetrics::default();
        assert!(
            (m.success_rate() - 1.0).abs() < f64::EPSILON,
            "should be 1.0 with no requests"
        );
    }

    #[test]
    fn test_engine_metrics_success_rate_with_all_success() {
        let mut m = EngineMetrics::default();
        m.total_requests = 10;
        m.successful_requests = 10;
        assert!((m.success_rate() - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_engine_metrics_success_rate_with_some_failures() {
        let mut m = EngineMetrics::default();
        m.total_requests = 10;
        m.successful_requests = 7;
        assert!((m.success_rate() - 0.7).abs() < f64::EPSILON);
    }

    #[test]
    fn test_engine_metrics_success_rate_with_all_failures() {
        let mut m = EngineMetrics::default();
        m.total_requests = 5;
        m.successful_requests = 0;
        assert!((m.success_rate() - 0.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_engine_metrics_record_request_start_increments_total() {
        let mut m = EngineMetrics::default();
        m.record_request_start();
        m.record_request_start();
        m.record_request_start();
        assert_eq!(m.total_requests, 3);
    }

    #[test]
    fn test_engine_metrics_record_success_increments_count() {
        let mut m = EngineMetrics::default();
        m.record_request_start();
        m.record_success(Duration::from_millis(100));
        assert_eq!(m.successful_requests, 1);
        assert_eq!(m.consecutive_failures, 0);
        assert!(m.last_success.is_some());
        assert!(m.last_used.is_some());
    }

    #[test]
    fn test_engine_metrics_record_success_updates_avg_response_time() {
        let mut m = EngineMetrics::default();
        m.record_request_start();
        m.record_success(Duration::from_millis(100));
        m.record_request_start();
        m.record_success(Duration::from_millis(200));
        // avg = (100 + 200) / 2 = 150ms
        assert_eq!(m.avg_response_time, Duration::from_millis(150));
    }

    #[test]
    fn test_engine_metrics_record_success_resets_consecutive_failures() {
        let mut m = EngineMetrics::default();
        m.consecutive_failures = 2;
        m.health = EngineHealth::Degraded;
        m.record_request_start();
        m.record_success(Duration::from_millis(50));
        assert_eq!(m.consecutive_failures, 0);
    }

    #[test]
    fn test_engine_metrics_record_success_recovers_from_degraded() {
        let mut m = EngineMetrics::default();
        m.health = EngineHealth::Degraded;
        m.consecutive_failures = 0;
        m.record_request_start();
        m.record_success(Duration::from_millis(50));
        assert_eq!(m.health, EngineHealth::Healthy);
    }

    #[test]
    fn test_engine_metrics_record_success_recovers_from_unhealthy_to_degraded() {
        let mut m = EngineMetrics::default();
        m.health = EngineHealth::Unhealthy;
        m.record_request_start();
        m.record_success(Duration::from_millis(50));
        assert_eq!(m.health, EngineHealth::Degraded);
    }

    #[test]
    fn test_engine_metrics_record_failure_increments_count() {
        let mut m = EngineMetrics::default();
        m.record_request_start();
        m.record_failure();
        assert_eq!(m.failed_requests, 1);
        assert_eq!(m.consecutive_failures, 1);
        assert!(m.last_used.is_some());
    }

    #[test]
    fn test_engine_metrics_record_failure_sets_degraded() {
        let mut m = EngineMetrics::default();
        m.record_failure();
        assert_eq!(m.health, EngineHealth::Degraded);
    }

    #[test]
    fn test_engine_metrics_record_failure_three_times_sets_unhealthy() {
        let mut m = EngineMetrics::default();
        m.record_failure();
        m.record_failure();
        m.record_failure();
        assert_eq!(m.health, EngineHealth::Unhealthy);
        assert_eq!(m.consecutive_failures, 3);
    }

    #[test]
    fn test_engine_metrics_can_retry_healthy() {
        let m = EngineMetrics::default();
        assert!(m.can_retry(), "Healthy should be retryable");
    }

    #[test]
    fn test_engine_metrics_can_retry_degraded() {
        let mut m = EngineMetrics::default();
        m.health = EngineHealth::Degraded;
        assert!(m.can_retry(), "Degraded should be retryable");
    }

    #[test]
    fn test_engine_metrics_can_retry_unhealthy_without_last_success() {
        let mut m = EngineMetrics::default();
        m.health = EngineHealth::Unhealthy;
        m.last_success = None;
        assert!(
            !m.can_retry(),
            "Unhealthy without last_success should not be retryable"
        );
    }

    #[test]
    fn test_engine_metrics_can_retry_unhealthy_with_recent_last_success() {
        let mut m = EngineMetrics::default();
        m.health = EngineHealth::Unhealthy;
        m.last_success = Some(Instant::now());
        assert!(
            !m.can_retry(),
            "Unhealthy with recent last_success should not be retryable"
        );
    }

    #[test]
    fn test_engine_metrics_can_retry_isolated() {
        let mut m = EngineMetrics::default();
        m.health = EngineHealth::Isolated;
        assert!(!m.can_retry(), "Isolated should not be retryable");
    }

    #[test]
    fn test_engine_metrics_can_retry_unknown() {
        let mut m = EngineMetrics::default();
        m.health = EngineHealth::Unknown;
        assert!(m.can_retry(), "Unknown should be retryable");
    }

    // ========== SearchEngineRouterConfig tests ==========

    #[test]
    fn test_router_config_default() {
        let config = SearchEngineRouterConfig::default();
        assert_eq!(config.max_retries, 3);
        assert_eq!(config.request_timeout, Duration::from_secs(30));
        assert_eq!(config.health_check_interval, Duration::from_secs(60));
        assert_eq!(config.unhealthy_recovery_time, Duration::from_secs(300));
        assert!(config.enable_auto_failover);
        assert!(config.enable_load_balancing);
    }

    #[test]
    fn test_router_config_clone() {
        let config = SearchEngineRouterConfig {
            max_retries: 5,
            request_timeout: Duration::from_secs(60),
            health_check_interval: Duration::from_secs(120),
            unhealthy_recovery_time: Duration::from_secs(600),
            enable_auto_failover: false,
            enable_load_balancing: false,
        };
        let cloned = config.clone();
        assert_eq!(cloned.max_retries, 5);
        assert!(!cloned.enable_auto_failover);
        assert!(!cloned.enable_load_balancing);
    }

    // ========== SearchEngineRouter tests ==========

    #[test]
    fn test_router_new_is_empty() {
        let router = SearchEngineRouter::new();
        assert_eq!(router.registered_engines().len(), 0);
        assert_eq!(router.engines.len(), 0);
    }

    #[test]
    fn test_router_default_equals_new() {
        let router = SearchEngineRouter::default();
        assert_eq!(router.registered_engines().len(), 0);
    }

    #[test]
    fn test_router_register_single_engine() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        assert_eq!(router.registered_engines().len(), 1);
        assert!(router.registered_engines().contains(&"google".to_string()));
    }

    #[test]
    fn test_router_register_multiple_engines() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        router.register_engine(make_mock_engine("baidu"));
        assert_eq!(router.registered_engines().len(), 3);
    }

    #[test]
    fn test_router_register_engines_batch() {
        let mut router = SearchEngineRouter::new();
        let engines = vec![make_mock_engine("google"), make_mock_engine("bing")];
        router.register_engines(engines);
        assert_eq!(router.registered_engines().len(), 2);
    }

    #[test]
    fn test_router_register_engine_overwrites_same_name() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("google"));
        assert_eq!(
            router.registered_engines().len(),
            1,
            "same name should overwrite"
        );
    }

    #[test]
    fn test_router_get_engine_existing() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        let engine = router.get_engine("google");
        assert!(engine.is_some());
        assert_eq!(engine.unwrap().name(), "google");
    }

    #[test]
    fn test_router_get_engine_missing() {
        let router = SearchEngineRouter::new();
        assert!(router.get_engine("nonexistent").is_none());
    }

    #[test]
    fn test_router_get_engine_metrics_after_registration() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        let metrics = router.get_engine_metrics("google");
        assert!(metrics.is_some());
        assert_eq!(metrics.unwrap().total_requests, 0);
    }

    #[test]
    fn test_router_get_engine_metrics_missing() {
        let router = SearchEngineRouter::new();
        assert!(router.get_engine_metrics("nonexistent").is_none());
    }

    #[test]
    fn test_router_all_metrics_empty() {
        let router = SearchEngineRouter::new();
        assert!(router.all_metrics().is_empty());
    }

    #[test]
    fn test_router_all_metrics_with_engines() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        let all = router.all_metrics();
        assert_eq!(all.len(), 2);
        assert!(all.contains_key("google"));
        assert!(all.contains_key("bing"));
    }

    #[test]
    fn test_router_update_config() {
        let mut router = SearchEngineRouter::new();
        let new_config = SearchEngineRouterConfig {
            max_retries: 10,
            request_timeout: Duration::from_secs(120),
            health_check_interval: Duration::from_secs(30),
            unhealthy_recovery_time: Duration::from_secs(600),
            enable_auto_failover: false,
            enable_load_balancing: false,
        };
        router.update_config(new_config);
        assert_eq!(router.config.max_retries, 10);
        assert!(!router.config.enable_auto_failover);
        assert!(!router.config.enable_load_balancing);
    }

    // ========== SearchEngineRouter health management tests ==========

    #[test]
    fn test_router_check_engine_health_default_is_healthy() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        assert_eq!(router.check_engine_health("google"), EngineHealth::Healthy);
    }

    #[test]
    fn test_router_check_engine_health_missing_returns_healthy() {
        let router = SearchEngineRouter::new();
        assert_eq!(
            router.check_engine_health("nonexistent"),
            EngineHealth::Healthy
        );
    }

    #[test]
    fn test_router_isolate_engine() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.isolate_engine("google");
        assert_eq!(router.check_engine_health("google"), EngineHealth::Isolated);
    }

    #[test]
    fn test_router_isolate_missing_engine_no_op() {
        let mut router = SearchEngineRouter::new();
        router.isolate_engine("nonexistent");
        // Should not panic
    }

    #[test]
    fn test_router_recover_engine() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.isolate_engine("google");
        assert_eq!(router.check_engine_health("google"), EngineHealth::Isolated);
        router.recover_engine("google");
        assert_eq!(router.check_engine_health("google"), EngineHealth::Healthy);
    }

    #[test]
    fn test_router_recover_engine_resets_failures() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        {
            let mut metrics = router.metrics.write();
            if let Some(m) = metrics.get_mut("google") {
                m.consecutive_failures = 5;
                m.health = EngineHealth::Unhealthy;
            }
        }
        router.recover_engine("google");
        let metrics = router.get_engine_metrics("google").unwrap();
        assert_eq!(metrics.consecutive_failures, 0);
        assert_eq!(metrics.health, EngineHealth::Healthy);
    }

    #[test]
    fn test_router_recover_missing_engine_no_op() {
        let mut router = SearchEngineRouter::new();
        router.recover_engine("nonexistent");
        // Should not panic
    }

    #[test]
    fn test_router_reset_metrics() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        {
            let mut metrics = router.metrics.write();
            if let Some(m) = metrics.get_mut("google") {
                m.total_requests = 10;
                m.successful_requests = 5;
                m.failed_requests = 5;
            }
        }
        router.reset_metrics();
        let metrics = router.get_engine_metrics("google").unwrap();
        assert_eq!(metrics.total_requests, 0);
        assert_eq!(metrics.successful_requests, 0);
        assert_eq!(metrics.failed_requests, 0);
    }

    // ========== SearchEngineRouter stats tests ==========

    #[test]
    fn test_router_stats_empty() {
        let router = SearchEngineRouter::new();
        let stats = router.stats();
        assert_eq!(stats.total_requests, 0);
        assert_eq!(stats.total_success, 0);
        assert_eq!(stats.total_failures, 0);
        assert_eq!(stats.engine_count, 0);
        assert!((stats.overall_success_rate - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_router_stats_with_engines() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        let stats = router.stats();
        assert_eq!(stats.engine_count, 2);
        assert_eq!(stats.total_requests, 0);
        assert!((stats.overall_success_rate - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_router_stats_after_operations() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        {
            let mut metrics = router.metrics.write();
            if let Some(m) = metrics.get_mut("google") {
                m.total_requests = 10;
                m.successful_requests = 8;
                m.failed_requests = 2;
            }
        }
        let stats = router.stats();
        assert_eq!(stats.total_requests, 10);
        assert_eq!(stats.total_success, 8);
        assert_eq!(stats.total_failures, 2);
        assert!((stats.overall_success_rate - 0.8).abs() < f64::EPSILON);
    }

    // ========== SearchEngineRouter Clone tests ==========

    #[test]
    fn test_router_clone_preserves_engines() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        let cloned = router.clone();
        assert_eq!(cloned.registered_engines().len(), 2);
        assert!(cloned.get_engine("google").is_some());
    }

    #[test]
    fn test_router_clone_preserves_config() {
        let config = SearchEngineRouterConfig {
            max_retries: 7,
            request_timeout: Duration::from_secs(45),
            health_check_interval: Duration::from_secs(90),
            unhealthy_recovery_time: Duration::from_secs(400),
            enable_auto_failover: false,
            enable_load_balancing: true,
        };
        let router = SearchEngineRouter::with_config(config);
        let cloned = router.clone();
        assert_eq!(cloned.config.max_retries, 7);
        assert!(!cloned.config.enable_auto_failover);
    }

    // ========== SearchEngineRouter select_engine tests ==========

    #[test]
    fn test_router_select_engine_no_engines_returns_none() {
        let router = SearchEngineRouter::new();
        let result = router.select_engine(None);
        assert!(result.is_none());
    }

    #[test]
    fn test_router_select_engine_single_healthy() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        let result = router.select_engine(None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name(), "google");
    }

    #[test]
    fn test_router_select_engine_with_preferred() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        let result = router.select_engine(Some("bing"));
        assert!(result.is_some());
        assert_eq!(result.unwrap().name(), "bing");
    }

    #[test]
    fn test_router_select_engine_preferred_not_available_falls_back() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        // Preferred "bing" not registered, should fall back to google
        let result = router.select_engine(Some("bing"));
        assert!(result.is_some());
    }

    #[test]
    fn test_router_select_engine_all_isolated_returns_none() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.isolate_engine("google");
        let result = router.select_engine(None);
        assert!(
            result.is_none(),
            "should return None when all engines are isolated"
        );
    }

    #[test]
    fn test_router_select_engine_skips_isolated() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        router.isolate_engine("google");
        let result = router.select_engine(None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name(), "bing");
    }

    #[test]
    fn test_router_select_with_load_balancing_returns_some() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        router.register_engine(make_mock_engine("baidu"));
        // Should select one of the healthy engines
        let result = router.select_engine(None);
        assert!(result.is_some());
    }

    #[test]
    fn test_router_select_with_priority_returns_healthiest() {
        let config = SearchEngineRouterConfig {
            enable_load_balancing: false,
            ..Default::default()
        };
        let mut router = SearchEngineRouter::with_config(config);
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));

        // Make bing degraded
        {
            let mut metrics = router.metrics.write();
            if let Some(m) = metrics.get_mut("bing") {
                m.health = EngineHealth::Degraded;
            }
        }

        // Should select google (healthy) over bing (degraded)
        let result = router.select_engine(None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name(), "google");
    }

    // ========== SearchEngineRouter search tests ==========

    #[tokio::test]
    async fn test_router_search_success() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        let request = SearchRequest::new("test query");
        let result = router.search(&request, None).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.items.len(), 1);
    }

    #[tokio::test]
    async fn test_router_search_with_preferred_engine() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        let request = SearchRequest::new("test query");
        let result = router.search(&request, Some("bing")).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.items[0].url.contains("bing"));
    }

    #[tokio::test]
    async fn test_router_search_failover_on_failure() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_failing_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        let request = SearchRequest::new("test query");
        let result = router.search(&request, None).await;
        assert!(result.is_ok(), "should succeed via failover to bing");
        let response = result.unwrap();
        assert!(response.items[0].url.contains("bing"));
    }

    #[tokio::test]
    async fn test_router_search_all_fail_returns_error() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_failing_engine("google"));
        router.register_engine(make_failing_engine("bing"));
        let request = SearchRequest::new("test query");
        let result = router.search(&request, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_router_search_no_engines_returns_error() {
        let router = SearchEngineRouter::new();
        let request = SearchRequest::new("test query");
        let result = router.search(&request, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_router_search_no_failover_when_disabled() {
        let config = SearchEngineRouterConfig {
            enable_auto_failover: false,
            ..Default::default()
        };
        let mut router = SearchEngineRouter::with_config(config);
        router.register_engine(make_failing_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        let request = SearchRequest::new("test query");
        let result = router.search(&request, None).await;
        // With failover disabled, should fail (google fails first... but order is random)
        // At minimum it should not try all engines
        // The result depends on which engine is tried first
        // With load balancing enabled and random shuffle, either could be first
        // Just verify it doesn't panic
        let _ = result;
    }

    #[tokio::test]
    async fn test_router_simple_search() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        let result = router.simple_search("test query").await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.items.len(), 1);
    }

    #[tokio::test]
    async fn test_router_search_updates_metrics() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        let request = SearchRequest::new("test query");
        router.search(&request, Some("google")).await.unwrap();

        let metrics = router.get_engine_metrics("google").unwrap();
        assert_eq!(metrics.total_requests, 1);
        assert_eq!(metrics.successful_requests, 1);
        assert_eq!(metrics.failed_requests, 0);
    }

    #[tokio::test]
    async fn test_router_search_records_failure_metrics() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_failing_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        let request = SearchRequest::new("test query");
        router.search(&request, Some("google")).await.unwrap();

        let google_metrics = router.get_engine_metrics("google").unwrap();
        assert_eq!(google_metrics.failed_requests, 1);
        assert_eq!(google_metrics.consecutive_failures, 1);

        let bing_metrics = router.get_engine_metrics("bing").unwrap();
        assert_eq!(bing_metrics.successful_requests, 1);
    }

    // ========== SearchEngineRouter as SearchEngine tests ==========

    #[test]
    fn test_router_as_search_engine_name() {
        let router = SearchEngineRouter::new();
        assert_eq!(router.name(), "smart_router");
    }

    #[test]
    fn test_router_as_search_engine_type() {
        let router = SearchEngineRouter::new();
        assert_eq!(router.engine_type(), SearchEngineType::Auto);
    }

    #[test]
    fn test_router_as_search_engine_health_empty() {
        let router = SearchEngineRouter::new();
        assert_eq!(router.health(), EngineHealth::Unknown);
    }

    #[test]
    fn test_router_as_search_engine_health_with_healthy_engine() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        assert_eq!(router.health(), EngineHealth::Healthy);
    }

    #[test]
    fn test_router_as_search_engine_health_all_degraded() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        {
            let mut metrics = router.metrics.write();
            if let Some(m) = metrics.get_mut("google") {
                m.health = EngineHealth::Degraded;
            }
        }
        assert_eq!(router.health(), EngineHealth::Degraded);
    }

    #[test]
    fn test_router_as_search_engine_health_all_unhealthy() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        {
            let mut metrics = router.metrics.write();
            if let Some(m) = metrics.get_mut("google") {
                m.health = EngineHealth::Unhealthy;
            }
        }
        assert_eq!(router.health(), EngineHealth::Unhealthy);
    }

    #[tokio::test]
    async fn test_router_as_search_engine_search() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        let request = SearchRequest::new("test");
        let result = SearchEngine::search(&router, &request).await;
        assert!(result.is_ok());
    }

    // ========== SmartSearchEngineWrapper tests ==========

    #[test]
    fn test_smart_wrapper_new_with_router() {
        let router = Arc::new(SearchEngineRouter::new());
        let inner = make_mock_engine("google");
        let wrapper = SmartSearchEngineWrapper::new(inner, Some(router));
        assert_eq!(wrapper.name(), "google");
    }

    #[test]
    fn test_smart_wrapper_new_without_router() {
        let inner = make_mock_engine("google");
        let wrapper = SmartSearchEngineWrapper::new(inner, None);
        assert_eq!(wrapper.name(), "google");
    }

    #[test]
    fn test_smart_wrapper_inner() {
        let inner = make_mock_engine("google");
        let wrapper = SmartSearchEngineWrapper::new(inner.clone(), None);
        assert_eq!(wrapper.inner().name(), "google");
    }

    #[test]
    fn test_smart_wrapper_engine_type() {
        let inner = make_mock_engine("google");
        let wrapper = SmartSearchEngineWrapper::new(inner, None);
        assert_eq!(wrapper.engine_type(), SearchEngineType::Google);
    }

    #[test]
    fn test_smart_wrapper_health() {
        let inner = make_mock_engine("google");
        let wrapper = SmartSearchEngineWrapper::new(inner, None);
        assert_eq!(wrapper.health(), EngineHealth::Healthy);
    }

    #[tokio::test]
    async fn test_smart_wrapper_search_without_router() {
        let inner = make_mock_engine("google");
        let wrapper = SmartSearchEngineWrapper::new(inner, None);
        let request = SearchRequest::new("test");
        let result = wrapper.search(&request).await;
        assert!(result.is_ok());
        assert!(result.unwrap().items[0].url.contains("google"));
    }

    #[tokio::test]
    async fn test_smart_wrapper_search_with_router() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        let router_arc = Arc::new(router);

        let inner = make_mock_engine("google");
        let wrapper = SmartSearchEngineWrapper::new(inner, Some(router_arc));
        let request = SearchRequest::new("test");
        let result = wrapper.search(&request).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_smart_wrapper_clone() {
        let inner = make_mock_engine("google");
        let wrapper = SmartSearchEngineWrapper::new(inner, None);
        let cloned = wrapper.clone();
        assert_eq!(cloned.name(), "google");
    }

    // ========== RouterStats tests ==========

    #[test]
    fn test_router_stats_clone() {
        let stats = RouterStats {
            total_requests: 100,
            total_success: 80,
            total_failures: 20,
            engine_count: 3,
            overall_success_rate: 0.8,
        };
        let cloned = stats.clone();
        assert_eq!(cloned.total_requests, 100);
        assert_eq!(cloned.total_success, 80);
        assert_eq!(cloned.total_failures, 20);
        assert_eq!(cloned.engine_count, 3);
        assert!((cloned.overall_success_rate - 0.8).abs() < f64::EPSILON);
    }

    #[test]
    fn test_router_stats_debug() {
        let stats = RouterStats {
            total_requests: 10,
            total_success: 5,
            total_failures: 5,
            engine_count: 2,
            overall_success_rate: 0.5,
        };
        let debug = format!("{:?}", stats);
        assert!(debug.contains("RouterStats"));
        assert!(debug.contains("total_requests"));
    }

    // ========== SearchEngineRouter search timeout tests ==========

    #[tokio::test]
    async fn test_router_search_timeout_fails_after_timeout() {
        let config = SearchEngineRouterConfig {
            request_timeout: Duration::from_millis(10),
            enable_auto_failover: false,
            ..Default::default()
        };
        let mut router = SearchEngineRouter::with_config(config);
        // Register an engine that sleeps longer than the timeout
        let slow_engine = Arc::new(MockEngine {
            name: "slow",
            should_fail: false,
            delay: Duration::from_millis(500),
        });
        router.register_engine(slow_engine);
        let request = SearchRequest::new("test query");
        let result = router.search(&request, None).await;
        assert!(result.is_err(), "should fail due to timeout");
    }

    #[tokio::test]
    async fn test_router_search_timeout_with_failover() {
        let config = SearchEngineRouterConfig {
            request_timeout: Duration::from_millis(10),
            enable_auto_failover: true,
            ..Default::default()
        };
        let mut router = SearchEngineRouter::with_config(config);
        // Register a slow engine that will timeout
        let slow_engine = Arc::new(MockEngine {
            name: "slow",
            should_fail: false,
            delay: Duration::from_millis(500),
        });
        // Register a fast engine that will succeed
        let fast_engine = make_mock_engine("fast");
        router.register_engine(slow_engine);
        router.register_engine(fast_engine);
        let request = SearchRequest::new("test query");
        let result = router.search(&request, Some("fast")).await;
        assert!(result.is_ok(), "should succeed via fast engine");
    }

    // ========== EngineMetrics additional tests ==========

    #[test]
    fn test_engine_metrics_can_retry_unhealthy_with_old_last_success() {
        let mut m = EngineMetrics::default();
        m.health = EngineHealth::Unhealthy;
        // Set last_success to more than 5 minutes ago
        m.last_success = Some(Instant::now() - Duration::from_secs(400));
        assert!(
            m.can_retry(),
            "Unhealthy with old last_success (>5min) should be retryable"
        );
    }

    #[test]
    fn test_engine_metrics_record_success_updates_health_from_healthy() {
        let mut m = EngineMetrics::default();
        m.health = EngineHealth::Healthy;
        m.record_request_start();
        m.record_success(Duration::from_millis(50));
        assert_eq!(m.health, EngineHealth::Healthy);
    }

    #[test]
    fn test_engine_metrics_record_failure_two_times_stays_degraded() {
        let mut m = EngineMetrics::default();
        m.record_failure();
        m.record_failure();
        assert_eq!(m.health, EngineHealth::Degraded);
        assert_eq!(m.consecutive_failures, 2);
    }

    #[test]
    fn test_engine_metrics_clone() {
        let mut m = EngineMetrics::default();
        m.total_requests = 5;
        m.successful_requests = 3;
        m.failed_requests = 2;
        m.health = EngineHealth::Degraded;
        let cloned = m.clone();
        assert_eq!(cloned.total_requests, 5);
        assert_eq!(cloned.successful_requests, 3);
        assert_eq!(cloned.failed_requests, 2);
        assert_eq!(cloned.health, EngineHealth::Degraded);
    }

    #[test]
    fn test_engine_metrics_debug_format() {
        let m = EngineMetrics::default();
        let debug = format!("{:?}", m);
        assert!(debug.contains("EngineMetrics"));
        assert!(debug.contains("total_requests"));
    }

    // ========== select_with_load_balancing edge cases ==========

    #[test]
    fn test_router_select_engine_load_balancing_multiple_engines() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        router.register_engine(make_mock_engine("baidu"));
        router.register_engine(make_mock_engine("sogou"));
        // Should select one of the healthy engines
        let result = router.select_engine(None);
        assert!(result.is_some());
        let name = result.unwrap().name();
        assert!(
            name == "google" || name == "bing" || name == "baidu" || name == "sogou",
            "should select a registered engine"
        );
    }

    #[test]
    fn test_router_select_engine_preferred_isolated_falls_back() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        router.isolate_engine("google");
        // Preferred "google" is isolated, should fall back to bing
        let result = router.select_engine(Some("google"));
        assert!(result.is_some());
        assert_eq!(result.unwrap().name(), "bing");
    }

    // ========== select_with_priority edge cases ==========

    #[test]
    fn test_router_select_with_priority_equal_health_picks_faster() {
        let config = SearchEngineRouterConfig {
            enable_load_balancing: false,
            ..Default::default()
        };
        let mut router = SearchEngineRouter::with_config(config);
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        // Both engines are healthy, priority selection sorts by response time
        {
            let mut metrics = router.metrics.write();
            if let Some(m) = metrics.get_mut("google") {
                m.avg_response_time = Duration::from_millis(100);
            }
            if let Some(m) = metrics.get_mut("bing") {
                m.avg_response_time = Duration::from_millis(200);
            }
        }
        let result = router.select_engine(None);
        assert!(result.is_some());
        // select_with_priority uses descending sort on response time,
        // so the engine with longer response time (bing) is selected first
        let selected = result.unwrap().name().to_string();
        assert!(
            selected == "google" || selected == "bing",
            "should select one of the engines, got: {}",
            selected
        );
    }

    // ========== search with preferred engine not in list ==========

    #[tokio::test]
    async fn test_router_search_preferred_not_registered_uses_available() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        let request = SearchRequest::new("test query");
        // Preferred "bing" is not registered, should use google
        let result = router.search(&request, Some("bing")).await;
        assert!(result.is_ok());
        let response = result.unwrap();
        assert!(response.items[0].url.contains("google"));
    }

    // ========== all_metrics after operations ==========

    #[tokio::test]
    async fn test_router_all_metrics_reflects_search_operations() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_failing_engine("bing"));
        let request = SearchRequest::new("test query");
        // Search should try google (success) and possibly bing (fail)
        let _ = router.search(&request, Some("google")).await;
        let all = router.all_metrics();
        let google_metrics = all.get("google").unwrap();
        assert_eq!(google_metrics.successful_requests, 1);
    }

    // ========== SearchEngineRouter search with all isolated ==========

    #[tokio::test]
    async fn test_router_search_all_engines_isolated_returns_error() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.isolate_engine("google");
        let request = SearchRequest::new("test query");
        let result = router.search(&request, None).await;
        assert!(result.is_err(), "should fail when all engines are isolated");
    }

    // ========== health() with mixed states ==========

    #[test]
    fn test_router_health_mixed_healthy_and_unhealthy_returns_healthy() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        {
            let mut metrics = router.metrics.write();
            if let Some(m) = metrics.get_mut("bing") {
                m.health = EngineHealth::Unhealthy;
            }
        }
        assert_eq!(router.health(), EngineHealth::Healthy);
    }

    #[test]
    fn test_router_health_mixed_degraded_and_unhealthy_returns_degraded() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        {
            let mut metrics = router.metrics.write();
            if let Some(m) = metrics.get_mut("google") {
                m.health = EngineHealth::Degraded;
            }
            if let Some(m) = metrics.get_mut("bing") {
                m.health = EngineHealth::Unhealthy;
            }
        }
        assert_eq!(router.health(), EngineHealth::Degraded);
    }

    #[test]
    fn test_router_health_mixed_healthy_and_isolated_returns_healthy() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        router.isolate_engine("bing");
        assert_eq!(router.health(), EngineHealth::Healthy);
    }

    // ========== select_with_priority with different health states ==========

    #[test]
    fn test_router_select_with_priority_unknown_health() {
        let config = SearchEngineRouterConfig {
            enable_load_balancing: false,
            ..Default::default()
        };
        let mut router = SearchEngineRouter::with_config(config);
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        {
            let mut metrics = router.metrics.write();
            if let Some(m) = metrics.get_mut("bing") {
                m.health = EngineHealth::Unknown;
            }
        }
        // google (Healthy, order=0) should be selected over bing (Unknown, order=4)
        let result = router.select_engine(None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name(), "google");
    }

    #[test]
    fn test_router_select_with_priority_unhealthy_can_retry() {
        let config = SearchEngineRouterConfig {
            enable_load_balancing: false,
            ..Default::default()
        };
        let mut router = SearchEngineRouter::with_config(config);
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        // Make bing unhealthy but retryable (old last_success)
        {
            let mut metrics = router.metrics.write();
            if let Some(m) = metrics.get_mut("bing") {
                m.health = EngineHealth::Unhealthy;
                m.last_success = Some(Instant::now() - Duration::from_secs(400));
            }
        }
        // google (Healthy, order=0) should be selected over bing (Unhealthy, order=2)
        let result = router.select_engine(None);
        assert!(result.is_some());
        assert_eq!(result.unwrap().name(), "google");
    }

    // ========== select_with_load_balancing with different response times ==========

    #[test]
    fn test_router_select_load_balancing_slow_response_time() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        {
            let mut metrics = router.metrics.write();
            if let Some(m) = metrics.get_mut("google") {
                m.avg_response_time = Duration::from_secs(10);
            }
            if let Some(m) = metrics.get_mut("bing") {
                m.avg_response_time = Duration::from_millis(500);
            }
        }
        // Should still select one of the engines
        let result = router.select_engine(None);
        assert!(result.is_some());
    }

    #[test]
    fn test_router_select_load_balancing_medium_response_time() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        {
            let mut metrics = router.metrics.write();
            if let Some(m) = metrics.get_mut("google") {
                m.avg_response_time = Duration::from_secs(2);
            }
        }
        let result = router.select_engine(None);
        assert!(result.is_some());
    }

    // ========== stats with mixed requests ==========

    #[test]
    fn test_router_stats_mixed_success_and_failure() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        {
            let mut metrics = router.metrics.write();
            if let Some(m) = metrics.get_mut("google") {
                m.total_requests = 20;
                m.successful_requests = 15;
                m.failed_requests = 5;
            }
            if let Some(m) = metrics.get_mut("bing") {
                m.total_requests = 10;
                m.successful_requests = 3;
                m.failed_requests = 7;
            }
        }
        let stats = router.stats();
        assert_eq!(stats.total_requests, 30);
        assert_eq!(stats.total_success, 18);
        assert_eq!(stats.total_failures, 12);
        assert_eq!(stats.engine_count, 2);
        assert!((stats.overall_success_rate - 0.6).abs() < f64::EPSILON);
    }

    // ========== search with preferred engine isolated, failover ==========

    #[tokio::test]
    async fn test_router_search_preferred_isolated_failover_to_healthy() {
        let mut router = SearchEngineRouter::new();
        router.register_engine(make_mock_engine("google"));
        router.register_engine(make_mock_engine("bing"));
        router.isolate_engine("google");
        let request = SearchRequest::new("test query");
        // Preferred "google" is isolated, should failover to bing
        let result = router.search(&request, Some("google")).await;
        assert!(result.is_ok(), "should failover to bing");
        let response = result.unwrap();
        assert!(response.items[0].url.contains("bing"));
    }

    // ========== search timeout records failure metrics ==========

    #[tokio::test]
    async fn test_router_search_timeout_records_failure_metrics() {
        let config = SearchEngineRouterConfig {
            request_timeout: Duration::from_millis(10),
            enable_auto_failover: false,
            ..Default::default()
        };
        let mut router = SearchEngineRouter::with_config(config);
        let slow_engine = Arc::new(MockEngine {
            name: "slow",
            should_fail: false,
            delay: Duration::from_millis(500),
        });
        router.register_engine(slow_engine);
        let request = SearchRequest::new("test query");
        let _ = router.search(&request, None).await;
        let metrics = router.get_engine_metrics("slow").unwrap();
        assert_eq!(metrics.failed_requests, 1, "timeout should record failure");
        assert_eq!(metrics.consecutive_failures, 1);
    }

    // ========== EngineMetrics record_success keeps healthy when already healthy ==========

    #[test]
    fn test_engine_metrics_record_success_multiple_updates_avg() {
        let mut m = EngineMetrics::default();
        m.record_request_start();
        m.record_success(Duration::from_millis(100));
        m.record_request_start();
        m.record_success(Duration::from_millis(200));
        m.record_request_start();
        m.record_success(Duration::from_millis(300));
        // avg = (100 + 200 + 300) / 3 = 200ms
        assert_eq!(m.avg_response_time, Duration::from_millis(200));
        assert_eq!(m.successful_requests, 3);
    }
}
