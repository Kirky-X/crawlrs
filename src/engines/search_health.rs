// Copyright (c) 2025 Kirky.X
//
// Licensed under MIT License
// See LICENSE file in the project root for full license information.

//! Search engine health check utilities
//! Provides health monitoring for search engines

use crate::engines::user_agent::get_rotated_user_agent;
use chrono::{DateTime, Utc};
use reqwest::Client;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tracing::{debug, warn};

/// Health status levels
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchEngineHealth {
    Healthy,
    Degraded,
    Unhealthy,
}

/// Per-domain health state to prevent cross-domain contamination
#[derive(Debug, Clone, Default)]
pub struct HealthState {
    pub consecutive_failures: u32,
    pub consecutive_successes: u32,
    pub last_check: DateTime<Utc>,
    pub total_requests: u32,
    pub total_failures: u32,
}

impl HealthState {
    pub fn record_success(&mut self) {
        self.consecutive_failures = 0;
        self.consecutive_successes += 1;
        self.total_requests += 1;
        self.last_check = Utc::now();
    }

    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
        self.consecutive_successes = 0;
        self.total_requests += 1;
        self.total_failures += 1;
        self.last_check = Utc::now();
    }

    pub fn calculate_status(&self, response_time: u64) -> SearchEngineHealth {
        if self.consecutive_failures >= 3 {
            SearchEngineHealth::Unhealthy
        } else if self.consecutive_failures > 0 || response_time > 3000 {
            SearchEngineHealth::Degraded
        } else {
            SearchEngineHealth::Healthy
        }
    }
}

/// Health check result with domain tracking
#[derive(Debug, Clone, Default)]
pub struct SearchHealthCheckResult {
    pub domain: String,
    pub engine_name: String,
    pub is_healthy: bool,
    pub status: SearchEngineHealth,
    pub response_time: u64,
    pub last_check: DateTime<Utc>,
    pub consecutive_failures: u32,
    pub consecutive_successes: u32,
    pub error_message: Option<String>,
}

/// Search engine health checker with per-domain state tracking
pub struct SearchHealthChecker {
    client: Client,
    domain_states: Arc<RwLock<HashMap<String, HealthState>>>,
}

impl Default for SearchHealthChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl SearchHealthChecker {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent(&get_rotated_user_agent())
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_else(|_| Client::new());

        Self {
            client,
            domain_states: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn extract_domain(&self, url: &str) -> String {
        url::Url::parse(url)
            .map(|u| u.host_str().unwrap_or("unknown").to_string())
            .unwrap_or_else(|_| "unknown".to_string())
    }

    pub async fn check_health(&self, engine_name: &str, test_url: &str) -> SearchHealthCheckResult {
        let domain = self.extract_domain(test_url);
        let start_time = std::time::Instant::now();

        match self.client.get(test_url).send().await {
            Ok(response) => {
                let response_time = start_time.elapsed().as_millis() as u64;
                let is_success =
                    response.status().is_success() && response.content_length().unwrap_or(0) > 100;

                let mut states = self.domain_states.write().await;
                let state = states
                    .entry(domain.clone())
                    .or_insert_with(HealthState::default);

                if is_success {
                    state.record_success();
                    let status = state.calculate_status(response_time);

                    debug!(
                        "{} health check passed for {} ({}ms)",
                        engine_name, domain, response_time
                    );

                    SearchHealthCheckResult {
                        domain,
                        engine_name: engine_name.to_string(),
                        is_healthy: true,
                        status,
                        response_time,
                        last_check: state.last_check,
                        consecutive_failures: state.consecutive_failures,
                        consecutive_successes: state.consecutive_successes,
                        error_message: None,
                    }
                } else {
                    state.record_failure();
                    let status = state.calculate_status(response_time);

                    warn!(
                        "{} health check failed for {}: status {}",
                        engine_name,
                        domain,
                        response.status()
                    );

                    SearchHealthCheckResult {
                        domain,
                        engine_name: engine_name.to_string(),
                        is_healthy: false,
                        status,
                        response_time,
                        last_check: state.last_check,
                        consecutive_failures: state.consecutive_failures,
                        consecutive_successes: state.consecutive_successes,
                        error_message: Some(format!("HTTP {}", response.status())),
                    }
                }
            }
            Err(e) => {
                let response_time = start_time.elapsed().as_millis() as u64;

                let mut states = self.domain_states.write().await;
                let state = states
                    .entry(domain.clone())
                    .or_insert_with(HealthState::default);

                state.record_failure();
                let status = state.calculate_status(response_time);

                warn!("{} health check error for {}: {}", engine_name, domain, e);

                SearchHealthCheckResult {
                    domain,
                    engine_name: engine_name.to_string(),
                    is_healthy: false,
                    status,
                    response_time,
                    last_check: state.last_check,
                    consecutive_failures: state.consecutive_failures,
                    consecutive_successes: state.consecutive_successes,
                    error_message: Some(e.to_string()),
                }
            }
        }
    }

    pub async fn get_domain_health(&self, domain: &str) -> Option<SearchHealthCheckResult> {
        let states = self.domain_states.read().await;
        states.get(domain).map(|state| SearchHealthCheckResult {
            domain: domain.to_string(),
            engine_name: String::new(),
            is_healthy: state.consecutive_failures < 3,
            status: state.calculate_status(0),
            response_time: 0,
            last_check: state.last_check,
            consecutive_failures: state.consecutive_failures,
            consecutive_successes: state.consecutive_successes,
            error_message: None,
        })
    }

    pub async fn get_aggregate_health(&self) -> SearchEngineHealth {
        let states = self.domain_states.read().await;

        if states.is_empty() {
            return SearchEngineHealth::Healthy;
        }

        let mut unhealthy_count = 0;
        let mut degraded_count = 0;

        for state in states.values() {
            let status = state.calculate_status(0);
            match status {
                SearchEngineHealth::Unhealthy => unhealthy_count += 1,
                SearchEngineHealth::Degraded => degraded_count += 1,
                SearchEngineHealth::Healthy => {}
            }
        }

        if unhealthy_count > 0 {
            SearchEngineHealth::Unhealthy
        } else if degraded_count > 0 {
            SearchEngineHealth::Degraded
        } else {
            SearchEngineHealth::Healthy
        }
    }

    pub async fn reset_domain(&self, domain: &str) {
        let mut states = self.domain_states.write().await;
        if let Some(state) = states.get_mut(domain) {
            *state = HealthState::default();
        }
    }

    pub async fn reset_all(&self) {
        let mut states = self.domain_states.write().await;
        states.clear();
    }
}

/// URLs for health checking different search engines
pub fn get_health_check_url(engine: &str) -> String {
    match engine.to_lowercase().as_str() {
        "google" => "https://www.google.com/search?q=health+check&num=1".to_string(),
        "bing" => "https://www.bing.com/search?q=health+check&num=1".to_string(),
        "baidu" => "https://www.baidu.com/s?wd=health+check&num=1".to_string(),
        "sogou" => "https://www.sogou.com/web?query=health+check&num=1".to_string(),
        _ => format!("https://{}.com/search?q=health+check&num=1", engine),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_health_check_urls() {
        assert!(get_health_check_url("google").contains("google.com"));
        assert!(get_health_check_url("bing").contains("bing.com"));
        assert!(get_health_check_url("baidu").contains("baidu.com"));
        assert!(get_health_check_url("sogou").contains("sogou.com"));
    }

    #[test]
    fn test_health_checker_creation() {
        let checker = SearchHealthChecker::new();
        assert!(true);
    }

    #[tokio::test]
    async fn test_health_checker_domain_isolation() {
        let checker = SearchHealthChecker::new();

        {
            let mut states = checker.domain_states.write().await;
            states.insert("domain-a.com".to_string(), {
                let mut state = HealthState::default();
                state.record_failure();
                state.record_failure();
                state
            });
        }

        let health_b = checker.get_domain_health("domain-b.com").await;
        assert!(health_b.is_none());

        let health_a = checker.get_domain_health("domain-a.com").await;
        assert!(health_a.is_some());
        assert_eq!(health_a.unwrap().consecutive_failures, 2);
    }

    #[tokio::test]
    async fn test_reset_all() {
        let checker = SearchHealthChecker::new();

        {
            let mut states = checker.domain_states.write().await;
            states.insert("test.com".to_string(), HealthState::default());
        }

        checker.reset_all().await;
        assert_eq!(
            checker.get_aggregate_health().await,
            SearchEngineHealth::Healthy
        );
    }

    #[tokio::test]
    async fn test_aggregate_health() {
        let checker = SearchHealthChecker::new();

        assert_eq!(
            checker.get_aggregate_health().await,
            SearchEngineHealth::Healthy
        );

        {
            let mut states = checker.domain_states.write().await;
            states.insert("healthy.com".to_string(), {
                let mut s = HealthState::default();
                s.record_success();
                s
            });
            states.insert("unhealthy.com".to_string(), {
                let mut s = HealthState::default();
                s.record_failure();
                s.record_failure();
                s.record_failure();
                s
            });
        }

        assert_eq!(
            checker.get_aggregate_health().await,
            SearchEngineHealth::Unhealthy
        );
    }
}
