// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! EngineClient - Unified public API for scraping operations
//!
//! This module provides the single entry point for all scraping operations.
//! All internal implementation details (UA rotation, circuit breaker, engine selection)
//! are encapsulated within EngineClient and not exposed to callers.

// ARC-002: DTO 类型已拆分至 `types.rs`，此处 re-export 保持向后兼容。
pub use crate::engines::types::{
    validate_session_id, HttpMethod, InternalPageAction, InternalScrapeRequest,
    InternalScrapeResponse, InternalScreenshotConfig, PageAction, ScrapeOptions,
    ScrapeOptionsBuilder, ScrapeRequest, ScrapeResponse, ScreenshotConfig, ScrollDirection,
    WaitFor, MAX_SESSION_ID_LEN,
};

use crate::engines::health_monitor::{AggregateHealthStatus, EngineHealthMonitor};
use crate::engines::router::{EngineRouter, EngineRouterTrait};
use crate::engines::validators::validate_url;
use crate::utils::retry::RetryReason;
use std::sync::Arc;
use std::time::Duration;
use thiserror::Error;

/// Engine error types for EngineClient operations.
#[derive(Error, Debug)]
pub enum EngineError {
    /// Request failed with a specific error message
    #[error("Request failed: {0}")]
    RequestFailed(String),

    /// Request timed out
    #[error("Request timed out after {0:?}")]
    Timeout(Duration),

    /// All engines are unavailable
    #[error("All engines failed: {0}")]
    AllEnginesFailed(String),

    /// No engines available for the request
    #[error("No engines available")]
    NoEnginesAvailable,

    /// Invalid URL provided
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// SSRF protection triggered
    #[error("SSRF protection: {0}")]
    SsrfProtection(String),

    /// Browser/Playwright error
    #[error("Browser error: {0}")]
    BrowserError(String),

    /// 反爬虫检测命中（design.md §1.6/1.7，R-antibot-003）
    ///
    /// 携带 `antibot::classifier::classify` 给出的 `Detection::reason` 字符串。
    /// 可重试：`is_retryable()=true`，`retry_reason()=AntiBot`，
    /// 路由层据此切身份（UA/代理/stealth）+ 强制浏览器/FlareSolverr 引擎改派。
    #[error("Anti-bot detected: {0}")]
    AntiBotDetected(String),

    /// 引擎特性切换（T027，R-identity-002）
    ///
    /// 当引擎降级或特性切换（如 Chrome → HTTP、JS 渲染失败回退到静态抓取）时触发。
    /// 携带描述信息（如 "chrome_degraded_to_http"）。
    /// 可重试：`is_retryable()=true`，`retry_reason()=FeatureToggle`，
    /// 路由层据此换引擎重试，并按 `RetryDirective` 立即轮换 UA（attempt=0 特例）。
    #[error("Feature toggle: {0}")]
    FeatureToggle(String),

    /// Request expired (circuit breaker open)
    #[error("Request expired")]
    Expired,

    /// 引擎级 MRT 超时（架构审查 MEDIUM-2 修复，design.md §14 / T062）
    ///
    /// 区别于 `Timeout`（请求整体超时），此变体表示单引擎在 MRT（Maximum Response Time）
    /// 内未完成，router 触发瀑布式 fallback 切换到下一引擎。
    ///
    /// 可重试：`is_retryable()=true`，`retry_reason()=Transient`（同引擎可重试，
    /// 但 router 会优先切下一引擎而非重试同引擎）。
    #[error("Engine {engine} exceeded MRT of {mrt:?}")]
    EngineMrtExceeded {
        /// 引擎名称
        engine: String,
        /// 实际使用的 MRT（即 `effective_timeout = min(remaining, engine_mrt)`）
        mrt: Duration,
    },

    /// Other error
    #[error("Other error: {0}")]
    Other(String),

    /// Internal error
    #[error("Internal error: {0}")]
    Internal(String),
}

// From implementations for EngineError
impl From<String> for EngineError {
    fn from(msg: String) -> Self {
        EngineError::RequestFailed(msg)
    }
}

impl From<&str> for EngineError {
    fn from(msg: &str) -> Self {
        EngineError::RequestFailed(msg.to_string())
    }
}

impl From<anyhow::Error> for EngineError {
    fn from(err: anyhow::Error) -> Self {
        EngineError::Internal(err.to_string())
    }
}

impl EngineError {
    /// Check if the error is retryable.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::RequestFailed(_) => true,
            Self::Timeout(_) => true,
            Self::NoEnginesAvailable => false,
            Self::InvalidUrl(_) => false,
            Self::SsrfProtection(_) => false,
            Self::BrowserError(_) => true,
            Self::AntiBotDetected(_) => true,
            Self::FeatureToggle(_) => true,
            Self::Internal(_) => false,
            Self::AllEnginesFailed(_) => false,
            Self::Expired => false,
            Self::Other(_) => false,
            // 引擎级 MRT 超时：可重试（router 优先切下一引擎，而非重试同引擎）
            Self::EngineMrtExceeded { .. } => true,
        }
    }

    /// 将错误归类为重试原因（design.md §4，R-antibot-003 / R-identity-002）。
    ///
    /// 调用方应先查 [`is_retryable()`](Self::is_retryable)：
    /// 不可重试的错误虽返回 [`RetryReason::Transient`]，
    /// 但不会被重试系统消费，仅作占位以保证返回值完备。
    ///
    /// 映射：
    /// - `RequestFailed` / `Timeout` / `BrowserError` → `Transient`（同引擎可重试）
    /// - `AntiBotDetected` → `AntiBot`（需切身份 + 浏览器引擎改派）
    /// - `FeatureToggle` → `FeatureToggle`（需换引擎重试；T027 新增）
    /// - 其余不可重试变体 → `Transient`（占位）
    pub fn retry_reason(&self) -> RetryReason {
        match self {
            Self::AntiBotDetected(_) => RetryReason::AntiBot,
            Self::FeatureToggle(_) => RetryReason::FeatureToggle,
            Self::RequestFailed(_)
            | Self::Timeout(_)
            | Self::BrowserError(_)
            | Self::NoEnginesAvailable
            | Self::InvalidUrl(_)
            | Self::SsrfProtection(_)
            | Self::Internal(_)
            | Self::AllEnginesFailed(_)
            | Self::Expired
            | Self::Other(_)
            | Self::EngineMrtExceeded { .. } => RetryReason::Transient,
        }
    }
}

use async_trait::async_trait;

/// ScraperEngine trait - abstraction for different scraping engines
///
/// This trait defines the interface that all scraping engines must implement.
/// Each engine provides different capabilities (JS rendering, TLS fingerprinting, etc.)
/// and is scored based on how well it matches the request requirements.
#[async_trait]
pub trait ScraperEngine: Send + Sync {
    /// Perform a scraping request
    async fn scrape(
        &self,
        request: &InternalScrapeRequest,
    ) -> Result<InternalScrapeResponse, EngineError>;

    /// Calculate a support score for the given request
    ///
    /// Returns a score from 0-100 indicating how well this engine
    /// supports the request. Higher scores indicate better support.
    fn support_score(&self, request: &InternalScrapeRequest) -> u8;

    /// Get the engine name
    fn name(&self) -> &'static str;

    /// Check if the engine supports TLS fingerprinting
    ///
    /// Returns true if the engine can perform TLS fingerprinting
    /// for anti-fingerprinting purposes.
    fn supports_tls_fingerprint(&self) -> bool {
        false
    }

    /// 引擎级最大响应时间（MRT, Maximum Response Time）—— design.md §14 / T060。
    ///
    /// 用于 router 顺序 fallback 路径瀑布式超时：单引擎调用以
    /// `min(remaining_timeout, engine.max_response_time())` 包裹，
    /// 超 MRT 即切下一引擎（不切整体失败）。race 模式不受影响。
    ///
    /// # 默认实现
    ///
    /// 返回 30 秒（与 `EngineTimeoutSettings::default_timeout_seconds` 默认值一致）。
    /// 各引擎按类型覆写为更精确的 MRT：
    /// - HTTP fetch 引擎（reqwest）：5 秒（`fetch_seconds`）
    /// - CDP/浏览器引擎（playwright / flaresolverr_cdp / flaresolverr_full）：30 秒（`cdp_seconds`）
    /// - TLS 指纹引擎（flaresolverr_tls）：15 秒（`tls_seconds`）
    ///
    /// # 注入
    ///
    /// 引擎构造时应从 `EngineTimeoutSettings` 注入对应字段，避免硬编码
    /// （参考 `ReqwestEngine::new_with_timeout` 模式）。
    ///
    /// 架构审查 MEDIUM-1 修复：删除 `DEFAULT_ENGINE_MRT` 常量，默认实现直接返回
    /// `Duration::from_secs(30)`，避免与 `EngineTimeoutSettings::default_timeout_seconds`
    /// 形成隐式耦合（注释承诺"保持一致"但代码无引用关系）。
    fn max_response_time(&self) -> Duration {
        Duration::from_secs(30)
    }
}

/// Health status of the engine system.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum EngineHealthStatus {
    /// All engines are operational
    #[default]
    Healthy,
    /// Some engines are degraded or unavailable
    Degraded {
        /// List of engines that are unhealthy
        unhealthy_engines: Vec<String>,
        /// Message describing the degradation
        message: String,
    },
    /// No engines are available
    Unavailable {
        /// Message describing the unavailability
        message: String,
    },
}

/// Trait for EngineClient - enables dependency injection
#[async_trait]
pub trait EngineClientTrait: Send + Sync {
    /// Perform a scraping request
    async fn scrape(&self, request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError>;

    /// Perform health check on all registered engines
    async fn health_check(&self) -> EngineHealthStatus;

    /// Get the number of registered engines
    fn engine_count(&self) -> usize;

    /// Get list of registered engine names
    fn registered_engines(&self) -> Vec<String>;
}

/// Engine client - the single entry point for all scraping operations.
///
/// This struct encapsulates all internal implementation details:
/// - User-Agent rotation
/// - Circuit breaker state
/// - Engine selection algorithm
/// - Retry logic and backoff
/// - Connection pooling
///
/// Callers should use this struct for all scraping operations instead of
/// interacting with engines directly.
#[derive(Clone)]
pub struct EngineClient {
    /// Internal router for engine selection and request routing
    router: Arc<dyn EngineRouterTrait>,
    /// Internal health monitor for tracking engine health
    health_monitor: Arc<EngineHealthMonitor>,
}

impl EngineClient {
    /// Create a new EngineClient with default configuration.
    pub fn new() -> Self {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(EngineRouter::new(Vec::new()));
        Self::with_router(router)
    }

    /// Create an EngineClient with a custom router.
    pub fn with_router(router: Arc<dyn EngineRouterTrait>) -> Self {
        Self {
            router,
            health_monitor: Arc::new(EngineHealthMonitor::new(Vec::new())),
        }
    }

    /// Create an EngineClient with engines pre-registered.
    pub fn with_engines(engines: Vec<Arc<dyn ScraperEngine>>) -> Self {
        let router: Arc<dyn EngineRouterTrait> = Arc::new(EngineRouter::new(engines));
        let engines_for_health = Vec::new(); // Will need to get engines from router
        let health_monitor = Arc::new(EngineHealthMonitor::new(engines_for_health));
        Self {
            router,
            health_monitor,
        }
    }

    /// Perform a scraping request.
    ///
    /// This method automatically:
    /// - Validates the URL
    /// - Selects the optimal engine based on request requirements
    /// - Handles retries and circuit breaking
    /// - Rotates user agents
    /// - Returns a unified response
    ///
    /// # Arguments
    ///
    /// * `request` - The scrape request containing URL and options
    ///
    /// # Returns
    ///
    /// * `Ok(ScrapeResponse)` on success
    /// * `Err(EngineError)` on failure
    pub async fn scrape(&self, request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
        // Validate URL first
        if let Err(e) = validate_url(&request.url).await {
            return Err(EngineError::SsrfProtection(e.to_string()));
        }

        // Convert to internal request format
        let internal_request = request.to_internal();

        // Route to appropriate engine
        match self.router.route(&internal_request).await {
            Ok(response) => Ok(response.to_public(&request.url)),
            Err(e) => Err(convert_error(e)),
        }
    }

    /// Perform health check on all registered engines.
    ///
    /// # Returns
    ///
    /// * `EngineHealthStatus` indicating the health of all engines
    pub async fn health_check(&self) -> EngineHealthStatus {
        // Perform health check on all engines
        self.health_monitor.perform_all_health_checks().await;

        // Get aggregate status and convert to public type
        let status = self.health_monitor.get_aggregate_status().await;

        match status {
            AggregateHealthStatus::Healthy => EngineHealthStatus::Healthy,
            AggregateHealthStatus::Degraded(unhealthy_engines) => {
                let count = unhealthy_engines.len();
                EngineHealthStatus::Degraded {
                    unhealthy_engines,
                    message: format!("{} engines degraded", count),
                }
            }
            AggregateHealthStatus::Unavailable => EngineHealthStatus::Unavailable {
                message: "All engines unavailable".to_string(),
            },
        }
    }

    /// Get the number of registered engines.
    pub fn engine_count(&self) -> usize {
        self.router.registered_engines().len()
    }

    /// Get list of registered engine names.
    pub fn registered_engines(&self) -> Vec<String> {
        self.router.registered_engines()
    }
}

impl Default for EngineClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl EngineClientTrait for EngineClient {
    async fn scrape(&self, request: &ScrapeRequest) -> Result<ScrapeResponse, EngineError> {
        self.scrape(request).await
    }

    async fn health_check(&self) -> EngineHealthStatus {
        self.health_check().await
    }

    fn engine_count(&self) -> usize {
        self.engine_count()
    }

    fn registered_engines(&self) -> Vec<String> {
        self.registered_engines()
    }
}

/// Convert internal errors to public EngineError
fn convert_error(e: EngineError) -> EngineError {
    match e {
        EngineError::RequestFailed(msg) => EngineError::RequestFailed(msg),
        EngineError::Timeout(duration) => EngineError::Timeout(duration),
        EngineError::AllEnginesFailed(msg) => {
            if msg.contains("No suitable engines") {
                EngineError::NoEnginesAvailable
            } else {
                EngineError::RequestFailed(msg)
            }
        }
        EngineError::SsrfProtection(msg) => EngineError::SsrfProtection(msg),
        EngineError::BrowserError(msg) => EngineError::BrowserError(msg),
        EngineError::AntiBotDetected(msg) => EngineError::AntiBotDetected(msg),
        EngineError::FeatureToggle(msg) => EngineError::FeatureToggle(msg),
        EngineError::Expired => EngineError::Internal("Request expired".to_string()),
        EngineError::Other(msg) => EngineError::Internal(msg),
        EngineError::NoEnginesAvailable => EngineError::NoEnginesAvailable,
        EngineError::InvalidUrl(msg) => EngineError::InvalidUrl(msg),
        EngineError::Internal(msg) => EngineError::Internal(msg),
        EngineError::EngineMrtExceeded { engine, mrt } => {
            EngineError::EngineMrtExceeded { engine, mrt }
        }
    }
}


#[cfg(test)]
#[path = "tests/engine_client_test.rs"]
mod tests;
