// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::models::search_result::SearchResult;
use crate::domain::search::engine::{SearchEngine, SearchError};
use async_trait::async_trait;
use parking_lot::RwLock;
use rand::seq::SliceRandom;
use rand::Rng;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// 搜索引擎状态
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum EngineHealth {
    /// 健康
    #[default]
    Healthy,
    /// 降级（部分失败）
    Degraded,
    /// 不健康（连续失败）
    Unhealthy,
    /// 隔离（暂时禁用）
    Isolated,
}

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

    /// 记录请求成功
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
    /// 负载均衡索引
    load_balance_index: RwLock<usize>,
}

impl Clone for SearchEngineRouter {
    fn clone(&self) -> Self {
        Self {
            engines: self.engines.clone(),
            metrics: RwLock::new(self.metrics.read().clone()),
            config: self.config.clone(),
            load_balance_index: RwLock::new(*self.load_balance_index.read()),
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
            engines: HashMap::new(),
            metrics: RwLock::new(HashMap::new()),
            config: SearchEngineRouterConfig::default(),
            load_balance_index: RwLock::new(0),
        }
    }

    /// 创建带配置的搜索引擎路由器
    pub fn with_config(config: SearchEngineRouterConfig) -> Self {
        Self {
            engines: HashMap::new(),
            metrics: RwLock::new(HashMap::new()),
            config,
            load_balance_index: RwLock::new(0),
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

    /// 根据策略选择引擎
    #[allow(dead_code)]
    fn select_engine(&self, preferred: Option<&str>) -> Option<Arc<dyn SearchEngine>> {
        let metrics = self.metrics.read();
        let mut candidates: Vec<(String, Arc<dyn SearchEngine>, &EngineMetrics)> = Vec::new();

        for (name, engine) in &self.engines {
            if let Some(metric) = metrics.get(name) {
                if metric.can_retry() {
                    candidates.push((name.clone(), engine.clone(), metric));
                }
            }
        }

        if candidates.is_empty() {
            warn!("没有可用的搜索引擎");
            return None;
        }

        // 如果有首选引擎且可用，优先使用
        if let Some(pref) = preferred {
            if let Some((_name, engine, metric)) = candidates.iter().find(|(n, _, _)| n == pref) {
                if metric.can_retry() {
                    info!("使用首选搜索引擎: {}", pref);
                    return Some(engine.clone());
                }
            }
        }

        // 根据配置选择引擎
        if self.config.enable_load_balancing {
            self.select_with_load_balancing(candidates)
        } else {
            self.select_with_priority(candidates)
        }
    }

    /// 负载均衡选择
    #[allow(dead_code)]
    fn select_with_load_balancing(
        &self,
        candidates: Vec<(String, Arc<dyn SearchEngine>, &EngineMetrics)>,
    ) -> Option<Arc<dyn SearchEngine>> {
        // 基于成功率和响应时间计算权重
        let total_weight: f64 = candidates
            .iter()
            .map(|(_, _, m)| {
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
            .sum();

        if total_weight == 0.0 {
            return candidates.first().map(|(_, e, _)| e.clone());
        }

        let mut rng = rand::rng();
        let mut random_weight = rng.random_range(0.0..total_weight);

        for (_, engine, _) in &candidates {
            let success_rate = self
                .metrics
                .read()
                .get(engine.name())
                .map(|m| m.success_rate())
                .unwrap_or(1.0);

            let weight = success_rate * 100.0;
            random_weight -= weight;
            if random_weight <= 0.0 {
                return Some(engine.clone());
            }
        }

        candidates.first().map(|(_, e, _)| e.clone())
    }

    /// 优先级选择（优先选择健康状态最好的）
    #[allow(dead_code)]
    fn select_with_priority(
        &self,
        candidates: Vec<(String, Arc<dyn SearchEngine>, &EngineMetrics)>,
    ) -> Option<Arc<dyn SearchEngine>> {
        // 按健康状态和成功率排序
        let mut sorted: Vec<_> = candidates;
        sorted.sort_by(|a, b| {
            let health_order = |h: &EngineHealth| match h {
                EngineHealth::Healthy => 0,
                EngineHealth::Degraded => 1,
                EngineHealth::Unhealthy => 2,
                EngineHealth::Isolated => 3,
            };

            let a_order = health_order(&a.2.health);
            let b_order = health_order(&b.2.health);

            if a_order != b_order {
                a_order.cmp(&b_order)
            } else {
                // 成功率相同时，选择响应时间短的
                b.2.avg_response_time.cmp(&a.2.avg_response_time)
            }
        });

        sorted.first().map(|(_, e, _)| e.clone())
    }

    /// 搜索（带智能路由）
    pub async fn search(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
        preferred: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        let start_time = Instant::now();
        let mut last_error: Option<SearchError> = None;
        let attempted_engines: Vec<String> = Vec::new();

        // 收集所有可用的引擎
        let available_engines: Vec<(String, Arc<dyn SearchEngine>)> = {
            let metrics = self.metrics.read();
            self.engines
                .iter()
                .filter(|(name, _)| {
                    if let Some(metric) = metrics.get(*name) {
                        metric.can_retry()
                    } else {
                        true
                    }
                })
                .map(|(n, e)| (n.clone(), e.clone()))
                .collect()
        };

        // 按优先级尝试引擎
        let mut sorted_engines: Vec<(String, Arc<dyn SearchEngine>)> = available_engines;

        // 首选引擎优先
        if let Some(pref) = preferred {
            if let Some(pos) = sorted_engines.iter().position(|(n, _)| *n == pref) {
                if pos > 0 {
                    sorted_engines.swap(0, pos);
                }
            }
        }

        // 如果启用了负载均衡，随机打乱（除了第一个首选引擎）
        if self.config.enable_load_balancing && sorted_engines.len() > 1 {
            let mut rng = rand::rng();
            if sorted_engines.len() > 1 {
                sorted_engines[1..].shuffle(&mut rng);
            }
        }

        for (name, engine) in &sorted_engines {
            // 记录请求开始
            {
                let mut metrics = self.metrics.write();
                if let Some(m) = metrics.get_mut(name) {
                    m.record_request_start();
                }
            }

            match tokio::time::timeout(
                self.config.request_timeout,
                engine.search(query, limit, lang, country),
            )
            .await
            {
                Ok(Ok(results)) => {
                    // 记录成功
                    let response_time = start_time.elapsed();
                    {
                        let mut metrics = self.metrics.write();
                        if let Some(m) = metrics.get_mut(name) {
                            m.record_success(response_time);
                        }
                    }

                    info!(
                        "搜索引擎 {} 成功返回 {} 个结果 (耗时: {:?})",
                        name,
                        results.len(),
                        response_time
                    );

                    return Ok(results);
                }
                Ok(Err(e)) => {
                    last_error = Some(e.clone());
                    {
                        let mut metrics = self.metrics.write();
                        if let Some(m) = metrics.get_mut(name) {
                            m.record_failure();
                        }
                    }
                    warn!("搜索引擎 {} 失败: {}", name, e);
                }
                Err(_) => {
                    let timeout_secs = self.config.request_timeout.as_secs();
                    last_error = Some(SearchError::TimeoutError(timeout_secs));
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

        // 所有引擎都失败了
        warn!(
            "所有搜索引擎都失败了，已尝试的引擎: {:?}",
            attempted_engines
        );

        Err(last_error
            .unwrap_or_else(|| SearchError::EngineError("所有搜索引擎都失败".to_string())))
    }

    /// 简单的搜索方法（使用默认引擎）
    pub async fn simple_search(
        &self,
        query: &str,
        limit: u32,
    ) -> Result<Vec<SearchResult>, SearchError> {
        self.search(query, limit, None, None, None).await
    }

    /// 检查引擎健康状态
    pub fn check_engine_health(&self, name: &str) -> EngineHealth {
        self.metrics
            .read()
            .get(name)
            .map(|m| m.health.clone())
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
    async fn search(
        &self,
        query: &str,
        limit: u32,
        lang: Option<&str>,
        country: Option<&str>,
    ) -> Result<Vec<SearchResult>, SearchError> {
        // 如果有路由器，使用路由器进行搜索
        if let Some(router) = &self.router {
            return router
                .search(query, limit, lang, country, Some(self.name))
                .await;
        }

        // 否则直接使用内部搜索引擎
        self.inner.search(query, limit, lang, country).await
    }

    fn name(&self) -> &'static str {
        self.name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::Arc;

    #[derive(Clone)]
    struct MockSearchEngine {
        name: &'static str,
        should_fail: bool,
        response_time: std::time::Duration,
    }

    impl MockSearchEngine {
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
        async fn search(
            &self,
            _query: &str,
            _limit: u32,
            _lang: Option<&str>,
            _country: Option<&str>,
        ) -> Result<Vec<SearchResult>, SearchError> {
            tokio::time::sleep(self.response_time).await;

            if self.should_fail {
                Err(SearchError::EngineError(format!(
                    "{} engine failed",
                    self.name
                )))
            } else {
                Ok(vec![SearchResult {
                    title: format!("Result from {}", self.name),
                    url: format!("https://example.com/{}", self.name),
                    description: Some(format!("Description from {}", self.name)),
                    engine: self.name.to_string(),
                    ..Default::default()
                }])
            }
        }

        fn name(&self) -> &'static str {
            self.name
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
            ..Default::default()
        };
        let router = SearchEngineRouter::with_config(config.clone());
        assert_eq!(router.config.enable_load_balancing, true);
    }

    #[tokio::test]
    async fn test_router_registration() {
        let mut router = SearchEngineRouter::new();
        let engine = Arc::new(MockSearchEngine::new("test_engine", false, 100));
        router.register_engine(engine);
        assert_eq!(router.registered_engines().len(), 1);
        assert!(router
            .registered_engines()
            .contains(&"test_engine".to_string()));
    }

    #[tokio::test]
    async fn test_router_multiple_engines() {
        let mut router = SearchEngineRouter::new();
        let engine1 = Arc::new(MockSearchEngine::new("engine1", false, 100));
        let engine2 = Arc::new(MockSearchEngine::new("engine2", false, 100));
        let engine3 = Arc::new(MockSearchEngine::new("engine3", false, 100));

        router.register_engine(engine1);
        router.register_engine(engine2);
        router.register_engine(engine3);

        assert_eq!(router.registered_engines().len(), 3);
    }

    #[tokio::test]
    async fn test_successful_search_with_single_engine() {
        let mut router = SearchEngineRouter::new();
        let engine = Arc::new(MockSearchEngine::new("success_engine", false, 50));
        router.register_engine(engine);

        let result = router.search("test query", 10, None, None, None).await;
        assert!(result.is_ok());
        let results = result.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].engine, "success_engine");
    }

    #[tokio::test]
    async fn test_failed_engine_fallback() {
        let mut router = SearchEngineRouter::new();
        let engine1 = Arc::new(MockSearchEngine::new("failing_engine", true, 50));
        let engine2 = Arc::new(MockSearchEngine::new("success_engine", false, 50));

        router.register_engine(engine1);
        router.register_engine(engine2);

        let result = router.search("test query", 10, None, None, None).await;
        assert!(result.is_ok());
        let results = result.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].engine, "success_engine");
    }

    #[tokio::test]
    async fn test_all_engines_fail() {
        let mut router = SearchEngineRouter::new();
        let config = SearchEngineRouterConfig {
            enable_auto_failover: true,
            ..Default::default()
        };
        router.update_config(config);

        let engine1 = Arc::new(MockSearchEngine::new("failing1", true, 50));
        let engine2 = Arc::new(MockSearchEngine::new("failing2", true, 50));

        router.register_engine(engine1);
        router.register_engine(engine2);

        let result = router.search("test query", 10, None, None, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_preferred_engine_selection() {
        let mut router = SearchEngineRouter::new();
        let engine1 = Arc::new(MockSearchEngine::new("engine1", false, 50));
        let engine2 = Arc::new(MockSearchEngine::new("engine2", false, 50));

        router.register_engine(engine1);
        router.register_engine(engine2);

        let result = router
            .search("test query", 10, None, None, Some("engine2"))
            .await;
        assert!(result.is_ok());
        let results = result.unwrap();
        assert_eq!(results[0].engine, "engine2");
    }

    #[tokio::test]
    async fn test_metrics_recording() {
        let mut router = SearchEngineRouter::new();
        let engine = Arc::new(MockSearchEngine::new("metrics_engine", false, 100));
        router.register_engine(engine);

        let _ = router.search("test query", 10, None, None, None).await;

        let metrics = router.get_engine_metrics("metrics_engine");
        assert!(metrics.is_some());
        let m = metrics.unwrap();
        assert_eq!(m.total_requests, 1);
        assert_eq!(m.successful_requests, 1);
    }

    #[tokio::test]
    async fn test_router_clone() {
        let router = SearchEngineRouter::new();
        let cloned = router.clone();
        assert_eq!(cloned.registered_engines().len(), 0);
    }

    #[tokio::test]
    async fn test_get_engine() {
        let mut router = SearchEngineRouter::new();
        let engine = Arc::new(MockSearchEngine::new("get_engine", false, 50));
        router.register_engine(engine.clone());

        let retrieved = router.get_engine("get_engine");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name(), "get_engine");
    }

    #[tokio::test]
    async fn test_get_nonexistent_engine() {
        let router = SearchEngineRouter::new();
        let retrieved = router.get_engine("nonexistent");
        assert!(retrieved.is_none());
    }

    #[tokio::test]
    async fn test_smart_search_engine_wrapper() {
        let router = SearchEngineRouter::new();
        let mock_engine = Arc::new(MockSearchEngine::new("wrapper_engine", false, 50));
        let wrapper = SmartSearchEngineWrapper::new(mock_engine, Some(Arc::new(router)));
        assert_eq!(wrapper.name(), "wrapper_engine");
    }
}
