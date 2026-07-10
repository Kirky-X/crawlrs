// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use log::warn;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

use crate::engines::engine_client::{
    EngineError, InternalScrapeRequest, InternalScrapeResponse, ScraperEngine,
};

/// 引擎健康状态
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EngineHealth {
    /// 健康
    Healthy,
    /// 降级
    Degraded,
    /// 不可用
    Unhealthy,
}

/// 引擎健康检查信息
#[derive(Debug, Clone)]
pub struct HealthCheckInfo {
    /// 引擎名称
    pub engine_name: String,
    /// 健康状态
    pub health: EngineHealth,
    /// 最后检查时间
    pub last_check: DateTime<Utc>,
    /// 连续失败次数
    pub consecutive_failures: u32,
    /// 平均响应时间（毫秒）
    pub avg_response_time_ms: Option<u64>,
    /// 错误信息
    pub error_message: Option<String>,
}

/// 健康检查配置
#[derive(Debug, Clone)]
pub struct HealthCheckConfig {
    /// 检查间隔
    pub check_interval: Duration,
    /// 超时时间
    pub timeout: Duration,
    /// 最大连续失败次数
    pub max_consecutive_failures: u32,
    /// 降级阈值（平均响应时间，毫秒）
    pub degraded_threshold_ms: u64,
    /// 不健康阈值（平均响应时间，毫秒）
    pub unhealthy_threshold_ms: u64,
    /// 目标URL
    pub target_url: String,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        // 使用配置服务获取健康检查 URL，如果不可用则回退到环境变量
        let target_url = std::env::var("CRAWLRS_HEALTH_CHECK_URL")
            .or_else(|_| std::env::var("APP_ENVIRONMENT"))
            .unwrap_or_else(|_| "https://www.google.com".to_string());

        Self {
            check_interval: Duration::from_secs(60),
            timeout: Duration::from_secs(10),
            max_consecutive_failures: 3,
            degraded_threshold_ms: 2000,
            unhealthy_threshold_ms: 5000,
            target_url,
        }
    }
}

/// 引擎健康监控器
pub struct EngineHealthMonitor {
    /// 引擎列表
    engines: Vec<Arc<dyn ScraperEngine>>,
    /// 健康状态
    health_status: Arc<RwLock<HashMap<String, HealthCheckInfo>>>,
    /// 配置
    config: HealthCheckConfig,
}

impl EngineHealthMonitor {
    /// 创建新的健康监控器
    pub fn new(engines: Vec<Arc<dyn ScraperEngine>>) -> Self {
        let health_status = Self::initialize_health_status(&engines);

        Self {
            engines,
            health_status: Arc::new(RwLock::new(health_status)),
            config: HealthCheckConfig::default(),
        }
    }

    /// 使用自定义配置创建新的健康监控器
    pub fn new_with_config(
        engines: Vec<Arc<dyn ScraperEngine>>,
        config: HealthCheckConfig,
    ) -> Self {
        let health_status = Self::initialize_health_status(&engines);

        Self {
            engines,
            health_status: Arc::new(RwLock::new(health_status)),
            config,
        }
    }

    /// 内部初始化方法 - 提取公共的 health_status 初始化逻辑
    fn initialize_health_status(
        engines: &[Arc<dyn ScraperEngine>],
    ) -> HashMap<String, HealthCheckInfo> {
        let mut health_status = HashMap::with_capacity(8);

        for engine in engines {
            let engine_name = engine.name().to_string();
            health_status.insert(
                engine_name.clone(),
                HealthCheckInfo {
                    engine_name,
                    health: EngineHealth::Healthy,
                    last_check: Utc::now(),
                    consecutive_failures: 0,
                    avg_response_time_ms: None,
                    error_message: None,
                },
            );
        }

        health_status
    }

    /// 获取所有引擎的健康状态
    pub async fn get_all_health_status(&self) -> HashMap<String, HealthCheckInfo> {
        self.health_status.read().await.clone()
    }

    /// 获取特定引擎的健康状态
    pub async fn get_engine_health(&self, engine_name: &str) -> Option<HealthCheckInfo> {
        self.health_status.read().await.get(engine_name).cloned()
    }

    /// 执行健康检查
    pub async fn perform_health_check(&self) {
        for engine in &self.engines {
            let engine_name = engine.name().to_string();
            let health_info = self.check_engine_health(engine).await;

            let mut status = self.health_status.write().await;
            status.insert(engine_name, health_info);
        }
    }

    /// 检查特定引擎的健康状态
    async fn check_engine_health(&self, engine: &Arc<dyn ScraperEngine>) -> HealthCheckInfo {
        let engine_name = engine.name().to_string();
        let start_time = std::time::Instant::now();

        // 创建测试请求
        let test_request = InternalScrapeRequest {
            url: self.config.target_url.clone(),
            method: crate::engines::engine_client::HttpMethod::Get,
            headers: std::collections::HashMap::new(),
            timeout: self.config.timeout,
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

        match engine.scrape(&test_request).await {
            Ok(response) => {
                let response_time = start_time.elapsed().as_millis() as u64;
                let health = self.determine_health_status(response_time, response.status_code);

                HealthCheckInfo {
                    engine_name,
                    health,
                    last_check: Utc::now(),
                    consecutive_failures: 0,
                    avg_response_time_ms: Some(response_time),
                    error_message: None,
                }
            }
            Err(error) => {
                let current_status = self.health_status.read().await;
                let current_info = current_status.get(&engine_name);
                let consecutive_failures = current_info
                    .map(|info| info.consecutive_failures + 1)
                    .unwrap_or(1);

                let health = if consecutive_failures >= self.config.max_consecutive_failures {
                    // Alert P3-Low: Single Engine Failure
                    warn!(
                        "ALARM: Engine {} is unhealthy after {} consecutive failures",
                        engine_name, consecutive_failures
                    );
                    EngineHealth::Unhealthy
                } else {
                    EngineHealth::Degraded
                };

                HealthCheckInfo {
                    engine_name,
                    health,
                    last_check: Utc::now(),
                    consecutive_failures,
                    avg_response_time_ms: None,
                    error_message: Some(error.to_string()),
                }
            }
        }
    }

    /// 根据响应时间和状态码确定健康状态
    fn determine_health_status(&self, response_time_ms: u64, status_code: u16) -> EngineHealth {
        if status_code >= 500 || response_time_ms > self.config.unhealthy_threshold_ms {
            EngineHealth::Unhealthy
        } else if response_time_ms > self.config.degraded_threshold_ms {
            EngineHealth::Degraded
        } else {
            EngineHealth::Healthy
        }
    }

    /// 获取健康的引擎列表
    pub async fn get_healthy_engines(&self) -> Vec<Arc<dyn ScraperEngine>> {
        let status = self.health_status.read().await;
        self.engines
            .iter()
            .filter(|engine| {
                status
                    .get(engine.name())
                    .map(|info| info.health == EngineHealth::Healthy)
                    .unwrap_or(true)
            })
            .cloned()
            .collect()
    }

    /// 获取所有可用的引擎（包括降级状态的引擎）
    pub async fn get_available_engines(&self) -> Vec<Arc<dyn ScraperEngine>> {
        let status = self.health_status.read().await;
        self.engines
            .iter()
            .filter(|engine| {
                status
                    .get(engine.name())
                    .map(|info| info.health != EngineHealth::Unhealthy)
                    .unwrap_or(true)
            })
            .cloned()
            .collect()
    }

    /// 执行所有引擎的健康检查
    pub async fn perform_all_health_checks(&self) {
        self.perform_health_check().await;
    }

    /// 获取聚合的健康状态
    pub async fn get_aggregate_status(&self) -> AggregateHealthStatus {
        let status = self.health_status.read().await;

        let mut healthy_count = 0;
        let mut degraded_count = 0;
        let mut unhealthy_count = 0;
        let mut unhealthy_engines = Vec::new();

        for engine in &self.engines {
            if let Some(info) = status.get(engine.name()) {
                match info.health {
                    EngineHealth::Healthy => healthy_count += 1,
                    EngineHealth::Degraded => {
                        degraded_count += 1;
                        unhealthy_engines.push(engine.name().to_string());
                    }
                    EngineHealth::Unhealthy => {
                        unhealthy_count += 1;
                        unhealthy_engines.push(engine.name().to_string());
                    }
                }
            } else {
                // Engine not in status map, count as unknown
                healthy_count += 1;
            }
        }

        if unhealthy_count > 0 && healthy_count == 0 {
            AggregateHealthStatus::Unavailable
        } else if degraded_count > 0 || unhealthy_count > 0 {
            AggregateHealthStatus::Degraded(unhealthy_engines)
        } else {
            AggregateHealthStatus::Healthy
        }
    }
}

/// Aggregate health status for EngineClient
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum AggregateHealthStatus {
    #[default]
    Healthy,
    Degraded(Vec<String>),
    Unavailable,
}

#[async_trait]
impl ScraperEngine for EngineHealthMonitor {
    async fn scrape(
        &self,
        _request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError> {
        // 健康监控器本身不执行抓取，只提供监控功能
        Err(EngineError::Other(
            "Health monitor cannot perform scraping".to_string(),
        ))
    }

    fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
        0
    }

    fn name(&self) -> &'static str {
        "health_monitor"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engines::engine_client::{
        EngineError, HttpMethod, InternalScrapeRequest, InternalScrapeResponse, ScraperEngine,
    };
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::time::Duration;

    // === Test helper: mock engines ===

    /// A mock engine that always succeeds with a 200 response
    struct MockOkEngine {
        engine_name: &'static str,
        status_code: u16,
    }

    impl MockOkEngine {
        fn new(name: &'static str) -> Self {
            Self {
                engine_name: name,
                status_code: 200,
            }
        }
    }

    #[async_trait]
    impl ScraperEngine for MockOkEngine {
        async fn scrape(
            &self,
            _request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            Ok(InternalScrapeResponse {
                status_code: self.status_code,
                content: "ok".to_string(),
                screenshot: None,
                content_type: "text/html".to_string(),
                headers: HashMap::new(),
                response_time_ms: 5,
            })
        }

        fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
            100
        }

        fn name(&self) -> &'static str {
            self.engine_name
        }
    }

    /// A mock engine that always fails
    struct MockFailEngine {
        engine_name: &'static str,
        error_msg: String,
    }

    impl MockFailEngine {
        fn new(name: &'static str) -> Self {
            Self {
                engine_name: name,
                error_msg: "connection refused".to_string(),
            }
        }
    }

    #[async_trait]
    impl ScraperEngine for MockFailEngine {
        async fn scrape(
            &self,
            _request: &InternalScrapeRequest,
        ) -> Result<InternalScrapeResponse, EngineError> {
            Err(EngineError::RequestFailed(self.error_msg.clone()))
        }

        fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
            100
        }

        fn name(&self) -> &'static str {
            self.engine_name
        }
    }

    fn test_config() -> HealthCheckConfig {
        HealthCheckConfig {
            check_interval: Duration::from_secs(60),
            timeout: Duration::from_secs(10),
            max_consecutive_failures: 3,
            degraded_threshold_ms: 2000,
            unhealthy_threshold_ms: 5000,
            target_url: "https://example.com".to_string(),
        }
    }

    // === EngineHealth enum tests ===

    #[test]
    fn test_engine_health_equality() {
        assert_eq!(EngineHealth::Healthy, EngineHealth::Healthy);
        assert_eq!(EngineHealth::Degraded, EngineHealth::Degraded);
        assert_eq!(EngineHealth::Unhealthy, EngineHealth::Unhealthy);
        assert_ne!(EngineHealth::Healthy, EngineHealth::Degraded);
        assert_ne!(EngineHealth::Degraded, EngineHealth::Unhealthy);
        assert_ne!(EngineHealth::Healthy, EngineHealth::Unhealthy);
    }

    #[test]
    fn test_engine_health_copy() {
        let h1 = EngineHealth::Healthy;
        let h2 = h1; // Copy
        assert_eq!(h1, h2);
    }

    // === AggregateHealthStatus tests ===

    #[test]
    fn test_aggregate_health_status_default() {
        let status = AggregateHealthStatus::default();
        assert_eq!(status, AggregateHealthStatus::Healthy);
    }

    #[test]
    fn test_aggregate_health_status_equality() {
        assert_eq!(
            AggregateHealthStatus::Healthy,
            AggregateHealthStatus::Healthy
        );
        assert_eq!(
            AggregateHealthStatus::Unavailable,
            AggregateHealthStatus::Unavailable
        );
        assert_eq!(
            AggregateHealthStatus::Degraded(vec!["e1".to_string()]),
            AggregateHealthStatus::Degraded(vec!["e1".to_string()])
        );
        assert_ne!(
            AggregateHealthStatus::Healthy,
            AggregateHealthStatus::Unavailable
        );
    }

    // === HealthCheckConfig tests ===

    #[test]
    fn test_health_check_config_fields() {
        let config = test_config();
        assert_eq!(config.check_interval, Duration::from_secs(60));
        assert_eq!(config.timeout, Duration::from_secs(10));
        assert_eq!(config.max_consecutive_failures, 3);
        assert_eq!(config.degraded_threshold_ms, 2000);
        assert_eq!(config.unhealthy_threshold_ms, 5000);
        assert_eq!(config.target_url, "https://example.com");
    }

    // === EngineHealthMonitor creation tests ===

    #[tokio::test]
    async fn test_monitor_new_initializes_health_status() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![Arc::new(MockOkEngine::new("engine_a"))];
        let monitor = EngineHealthMonitor::new(engines);

        let status = monitor.get_all_health_status().await;
        assert_eq!(status.len(), 1);
        assert!(status.contains_key("engine_a"));

        let info = status.get("engine_a").unwrap();
        assert_eq!(info.health, EngineHealth::Healthy);
        assert_eq!(info.consecutive_failures, 0);
        assert!(info.error_message.is_none());
        assert!(info.avg_response_time_ms.is_none());
    }

    #[tokio::test]
    async fn test_monitor_new_with_multiple_engines() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![
            Arc::new(MockOkEngine::new("engine_a")),
            Arc::new(MockOkEngine::new("engine_b")),
            Arc::new(MockOkEngine::new("engine_c")),
        ];
        let monitor = EngineHealthMonitor::new(engines);

        let status = monitor.get_all_health_status().await;
        assert_eq!(status.len(), 3);
        assert!(status.contains_key("engine_a"));
        assert!(status.contains_key("engine_b"));
        assert!(status.contains_key("engine_c"));
    }

    #[tokio::test]
    async fn test_monitor_new_with_config() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![Arc::new(MockOkEngine::new("engine_a"))];
        let config = test_config();
        let monitor = EngineHealthMonitor::new_with_config(engines, config);

        let status = monitor.get_all_health_status().await;
        assert_eq!(status.len(), 1);
        assert!(status.contains_key("engine_a"));
    }

    #[tokio::test]
    async fn test_monitor_new_empty_engines() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![];
        let monitor = EngineHealthMonitor::new(engines);

        let status = monitor.get_all_health_status().await;
        assert!(status.is_empty());
    }

    // === get_engine_health tests ===

    #[tokio::test]
    async fn test_get_engine_health_existing() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![Arc::new(MockOkEngine::new("engine_a"))];
        let monitor = EngineHealthMonitor::new(engines);

        let health = monitor.get_engine_health("engine_a").await;
        assert!(health.is_some());
        let info = health.unwrap();
        assert_eq!(info.engine_name, "engine_a");
        assert_eq!(info.health, EngineHealth::Healthy);
    }

    #[tokio::test]
    async fn test_get_engine_health_nonexistent() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![];
        let monitor = EngineHealthMonitor::new(engines);

        let health = monitor.get_engine_health("nonexistent").await;
        assert!(health.is_none());
    }

    // === determine_health_status tests ===

    #[tokio::test]
    async fn test_determine_health_status_healthy() {
        let monitor = EngineHealthMonitor::new_with_config(vec![], test_config());

        // Fast response, 200 status
        assert_eq!(
            monitor.determine_health_status(100, 200),
            EngineHealth::Healthy
        );
        assert_eq!(
            monitor.determine_health_status(0, 200),
            EngineHealth::Healthy
        );
        assert_eq!(
            monitor.determine_health_status(1999, 200),
            EngineHealth::Healthy
        );
    }

    #[tokio::test]
    async fn test_determine_health_status_degraded() {
        let monitor = EngineHealthMonitor::new_with_config(vec![], test_config());

        // Response time between degraded_threshold and unhealthy_threshold
        assert_eq!(
            monitor.determine_health_status(2001, 200),
            EngineHealth::Degraded
        );
        assert_eq!(
            monitor.determine_health_status(3000, 200),
            EngineHealth::Degraded
        );
        assert_eq!(
            monitor.determine_health_status(4999, 200),
            EngineHealth::Degraded
        );
    }

    #[tokio::test]
    async fn test_determine_health_status_unhealthy_by_response_time() {
        let monitor = EngineHealthMonitor::new_with_config(vec![], test_config());

        // Response time exceeds unhealthy_threshold
        assert_eq!(
            monitor.determine_health_status(5001, 200),
            EngineHealth::Unhealthy
        );
        assert_eq!(
            monitor.determine_health_status(10000, 200),
            EngineHealth::Unhealthy
        );
    }

    #[tokio::test]
    async fn test_determine_health_status_unhealthy_by_status_code() {
        let monitor = EngineHealthMonitor::new_with_config(vec![], test_config());

        // 5xx status codes are unhealthy regardless of response time
        assert_eq!(
            monitor.determine_health_status(10, 500),
            EngineHealth::Unhealthy
        );
        assert_eq!(
            monitor.determine_health_status(10, 503),
            EngineHealth::Unhealthy
        );
        assert_eq!(
            monitor.determine_health_status(10, 599),
            EngineHealth::Unhealthy
        );
    }

    #[tokio::test]
    async fn test_determine_health_status_4xx_not_unhealthy() {
        let monitor = EngineHealthMonitor::new_with_config(vec![], test_config());

        // 4xx status codes are not treated as unhealthy by this logic
        assert_eq!(
            monitor.determine_health_status(10, 404),
            EngineHealth::Healthy
        );
        assert_eq!(
            monitor.determine_health_status(10, 429),
            EngineHealth::Healthy
        );
    }

    // === perform_health_check tests ===

    #[tokio::test]
    async fn test_perform_health_check_success() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![Arc::new(MockOkEngine::new("engine_a"))];
        let monitor = EngineHealthMonitor::new_with_config(engines, test_config());

        monitor.perform_health_check().await;

        let health = monitor.get_engine_health("engine_a").await.unwrap();
        assert_eq!(health.health, EngineHealth::Healthy);
        assert_eq!(health.consecutive_failures, 0);
        assert!(health.error_message.is_none());
        assert!(health.avg_response_time_ms.is_some());
    }

    #[tokio::test]
    async fn test_perform_health_check_failure_degraded() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![Arc::new(MockFailEngine::new("engine_a"))];
        let monitor = EngineHealthMonitor::new_with_config(engines, test_config());

        monitor.perform_health_check().await;

        let health = monitor.get_engine_health("engine_a").await.unwrap();
        // First failure -> Degraded (below max_consecutive_failures=3)
        assert_eq!(health.health, EngineHealth::Degraded);
        assert_eq!(health.consecutive_failures, 1);
        assert!(health.error_message.is_some());
        assert!(health.avg_response_time_ms.is_none());
    }

    #[tokio::test]
    async fn test_perform_health_check_failure_unhealthy_after_threshold() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![Arc::new(MockFailEngine::new("engine_a"))];
        let monitor = EngineHealthMonitor::new_with_config(engines, test_config());

        // Perform health checks until consecutive_failures reaches max (3)
        monitor.perform_health_check().await; // failures = 1 -> Degraded
        monitor.perform_health_check().await; // failures = 2 -> Degraded
        monitor.perform_health_check().await; // failures = 3 -> Unhealthy

        let health = monitor.get_engine_health("engine_a").await.unwrap();
        assert_eq!(health.health, EngineHealth::Unhealthy);
        assert_eq!(health.consecutive_failures, 3);
        assert!(health.error_message.is_some());
    }

    #[tokio::test]
    async fn test_perform_health_check_multiple_engines() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![
            Arc::new(MockOkEngine::new("healthy_engine")),
            Arc::new(MockFailEngine::new("failing_engine")),
        ];
        let monitor = EngineHealthMonitor::new_with_config(engines, test_config());

        monitor.perform_health_check().await;

        let healthy = monitor.get_engine_health("healthy_engine").await.unwrap();
        assert_eq!(healthy.health, EngineHealth::Healthy);

        let failing = monitor.get_engine_health("failing_engine").await.unwrap();
        assert_eq!(failing.health, EngineHealth::Degraded);
        assert_eq!(failing.consecutive_failures, 1);
    }

    #[tokio::test]
    async fn test_perform_all_health_checks_alias() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![Arc::new(MockOkEngine::new("engine_a"))];
        let monitor = EngineHealthMonitor::new_with_config(engines, test_config());

        // perform_all_health_checks should behave same as perform_health_check
        monitor.perform_all_health_checks().await;

        let health = monitor.get_engine_health("engine_a").await.unwrap();
        assert_eq!(health.health, EngineHealth::Healthy);
    }

    // === get_healthy_engines tests ===

    #[tokio::test]
    async fn test_get_healthy_engines_all_healthy() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![
            Arc::new(MockOkEngine::new("engine_a")),
            Arc::new(MockOkEngine::new("engine_b")),
        ];
        let monitor = EngineHealthMonitor::new_with_config(engines, test_config());

        let healthy = monitor.get_healthy_engines().await;
        assert_eq!(healthy.len(), 2);
    }

    #[tokio::test]
    async fn test_get_healthy_engines_after_check() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![
            Arc::new(MockOkEngine::new("healthy_engine")),
            Arc::new(MockFailEngine::new("failing_engine")),
        ];
        let monitor = EngineHealthMonitor::new_with_config(engines, test_config());

        monitor.perform_health_check().await;

        let healthy = monitor.get_healthy_engines().await;
        assert_eq!(healthy.len(), 1);
        assert_eq!(healthy[0].name(), "healthy_engine");
    }

    #[tokio::test]
    async fn test_get_healthy_engines_empty() {
        let monitor = EngineHealthMonitor::new_with_config(vec![], test_config());
        let healthy = monitor.get_healthy_engines().await;
        assert!(healthy.is_empty());
    }

    // === get_available_engines tests ===

    #[tokio::test]
    async fn test_get_available_engines_includes_degraded() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![
            Arc::new(MockOkEngine::new("healthy_engine")),
            Arc::new(MockFailEngine::new("degraded_engine")),
        ];
        let monitor = EngineHealthMonitor::new_with_config(engines, test_config());

        monitor.perform_health_check().await;

        let available = monitor.get_available_engines().await;
        // Both healthy and degraded engines are available
        assert_eq!(available.len(), 2);
    }

    #[tokio::test]
    async fn test_get_available_engines_excludes_unhealthy() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![
            Arc::new(MockOkEngine::new("healthy_engine")),
            Arc::new(MockFailEngine::new("unhealthy_engine")),
        ];
        let monitor = EngineHealthMonitor::new_with_config(engines, test_config());

        // Trigger enough failures to make the failing engine unhealthy
        monitor.perform_health_check().await; // 1 failure
        monitor.perform_health_check().await; // 2 failures
        monitor.perform_health_check().await; // 3 failures -> Unhealthy

        let available = monitor.get_available_engines().await;
        assert_eq!(available.len(), 1);
        assert_eq!(available[0].name(), "healthy_engine");
    }

    // === get_aggregate_status tests ===

    #[tokio::test]
    async fn test_get_aggregate_status_all_healthy() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![
            Arc::new(MockOkEngine::new("engine_a")),
            Arc::new(MockOkEngine::new("engine_b")),
        ];
        let monitor = EngineHealthMonitor::new_with_config(engines, test_config());

        let status = monitor.get_aggregate_status().await;
        assert_eq!(status, AggregateHealthStatus::Healthy);
    }

    #[tokio::test]
    async fn test_get_aggregate_status_with_degraded() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![
            Arc::new(MockOkEngine::new("healthy_engine")),
            Arc::new(MockFailEngine::new("degraded_engine")),
        ];
        let monitor = EngineHealthMonitor::new_with_config(engines, test_config());

        monitor.perform_health_check().await;

        let status = monitor.get_aggregate_status().await;
        match status {
            AggregateHealthStatus::Degraded(engines) => {
                assert!(engines.contains(&"degraded_engine".to_string()));
            }
            _ => panic!("Expected Degraded status"),
        }
    }

    #[tokio::test]
    async fn test_get_aggregate_status_all_unhealthy() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![Arc::new(MockFailEngine::new("engine_a"))];
        let monitor = EngineHealthMonitor::new_with_config(engines, test_config());

        // Trigger enough failures to make engine unhealthy
        monitor.perform_health_check().await;
        monitor.perform_health_check().await;
        monitor.perform_health_check().await;

        let status = monitor.get_aggregate_status().await;
        assert_eq!(status, AggregateHealthStatus::Unavailable);
    }

    #[tokio::test]
    async fn test_get_aggregate_status_mixed_healthy_and_unhealthy() {
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![
            Arc::new(MockOkEngine::new("healthy_engine")),
            Arc::new(MockFailEngine::new("unhealthy_engine")),
        ];
        let monitor = EngineHealthMonitor::new_with_config(engines, test_config());

        monitor.perform_health_check().await;
        monitor.perform_health_check().await;
        monitor.perform_health_check().await;

        let status = monitor.get_aggregate_status().await;
        // Has both healthy and unhealthy, so should be Degraded (not Unavailable)
        match status {
            AggregateHealthStatus::Degraded(engines) => {
                assert!(engines.contains(&"unhealthy_engine".to_string()));
            }
            _ => panic!("Expected Degraded status"),
        }
    }

    #[tokio::test]
    async fn test_get_aggregate_status_empty_engines() {
        let monitor = EngineHealthMonitor::new_with_config(vec![], test_config());
        let status = monitor.get_aggregate_status().await;
        // No engines, so healthy_count stays 0, but no unhealthy either
        // The logic: unhealthy_count == 0 && healthy_count == 0 -> falls through to Healthy
        assert_eq!(status, AggregateHealthStatus::Healthy);
    }

    // === ScraperEngine trait impl tests ===

    #[tokio::test]
    async fn test_health_monitor_scrape_returns_error() {
        let monitor = EngineHealthMonitor::new(vec![]);
        let request = InternalScrapeRequest {
            url: "https://example.com".to_string(),
            method: HttpMethod::Get,
            headers: HashMap::new(),
            timeout: Duration::from_secs(10),
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

        let result = monitor.scrape(&request).await;
        assert!(result.is_err());
        match result.unwrap_err() {
            EngineError::Other(msg) => {
                assert!(msg.contains("Health monitor cannot perform scraping"));
            }
            other => panic!("Expected EngineError::Other, got {:?}", other),
        }
    }

    #[test]
    fn test_health_monitor_support_score_is_zero() {
        let monitor = EngineHealthMonitor::new(vec![]);
        let request = InternalScrapeRequest {
            url: "https://example.com".to_string(),
            method: HttpMethod::Get,
            headers: HashMap::new(),
            timeout: Duration::from_secs(10),
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
        assert_eq!(monitor.support_score(&request), 0);
    }

    #[test]
    fn test_health_monitor_name() {
        let monitor = EngineHealthMonitor::new(vec![]);
        assert_eq!(monitor.name(), "health_monitor");
    }

    // === Recovery scenario test ===

    #[tokio::test]
    async fn test_recovery_resets_consecutive_failures() {
        // An engine that fails first, then succeeds
        struct RecoveringEngine {
            call_count: Arc<std::sync::atomic::AtomicU32>,
        }

        #[async_trait]
        impl ScraperEngine for RecoveringEngine {
            async fn scrape(
                &self,
                _request: &InternalScrapeRequest,
            ) -> Result<InternalScrapeResponse, EngineError> {
                let count = self
                    .call_count
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if count < 2 {
                    Err(EngineError::RequestFailed("temporary".to_string()))
                } else {
                    Ok(InternalScrapeResponse {
                        status_code: 200,
                        content: "recovered".to_string(),
                        screenshot: None,
                        content_type: "text/html".to_string(),
                        headers: HashMap::new(),
                        response_time_ms: 5,
                    })
                }
            }

            fn support_score(&self, _request: &InternalScrapeRequest) -> u8 {
                100
            }

            fn name(&self) -> &'static str {
                "recovering_engine"
            }
        }

        let call_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
        let engines: Vec<Arc<dyn ScraperEngine>> = vec![Arc::new(RecoveringEngine { call_count })];
        let monitor = EngineHealthMonitor::new_with_config(engines, test_config());

        // First check: fails -> Degraded, consecutive_failures = 1
        monitor.perform_health_check().await;
        let health = monitor
            .get_engine_health("recovering_engine")
            .await
            .unwrap();
        assert_eq!(health.health, EngineHealth::Degraded);
        assert_eq!(health.consecutive_failures, 1);

        // Second check: fails -> Degraded, consecutive_failures = 2
        monitor.perform_health_check().await;
        let health = monitor
            .get_engine_health("recovering_engine")
            .await
            .unwrap();
        assert_eq!(health.health, EngineHealth::Degraded);
        assert_eq!(health.consecutive_failures, 2);

        // Third check: succeeds -> Healthy, consecutive_failures reset to 0
        monitor.perform_health_check().await;
        let health = monitor
            .get_engine_health("recovering_engine")
            .await
            .unwrap();
        assert_eq!(health.health, EngineHealth::Healthy);
        assert_eq!(health.consecutive_failures, 0);
        assert!(health.error_message.is_none());
        assert!(health.avg_response_time_ms.is_some());
    }
}
