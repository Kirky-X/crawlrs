// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::warn;

use crate::engines::traits::{EngineError, ScrapeRequest, ScrapeResponse, ScraperEngine};

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
        Self {
            check_interval: Duration::from_secs(60),
            timeout: Duration::from_secs(10),
            max_consecutive_failures: 3,
            degraded_threshold_ms: 2000,
            unhealthy_threshold_ms: 5000,
            target_url: "https://www.google.com".to_string(),
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
        let mut health_status = HashMap::new();

        for engine in &engines {
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
        let mut health_status = HashMap::new();

        for engine in &engines {
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

        Self {
            engines,
            health_status: Arc::new(RwLock::new(health_status)),
            config,
        }
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
        let test_request = ScrapeRequest {
            url: self.config.target_url.clone(),
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
                    warn!("ALARM: Engine {} is unhealthy after {} consecutive failures", engine_name, consecutive_failures);
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
}

#[async_trait]
impl ScraperEngine for EngineHealthMonitor {
    async fn scrape(&self, _request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
        // 健康监控器本身不执行抓取，只提供监控功能
        Err(EngineError::Other(
            "Health monitor cannot perform scraping".to_string(),
        ))
    }

    fn support_score(&self, _request: &ScrapeRequest) -> u8 {
        0
    }

    fn name(&self) -> &'static str {
        "health_monitor"
    }
}
