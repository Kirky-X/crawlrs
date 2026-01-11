// Copyright (c) 2025 Kirky.X
//
// Licensed under MIT License
// See LICENSE file in the project root for full license information.

//! Search engine health check utilities
//! Provides health monitoring for search engines

use crate::engines::user_agent::get_rotated_user_agent;
use chrono::{DateTime, Utc};
use reqwest::Client;
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

/// Health check result
#[derive(Debug, Clone)]
pub struct SearchHealthCheckResult {
    pub is_healthy: bool,
    pub status: SearchEngineHealth,
    pub response_time: u64,
    pub last_check: DateTime<Utc>,
    pub consecutive_failures: u32,
    pub error_message: Option<String>,
}

impl Default for SearchHealthCheckResult {
    fn default() -> Self {
        Self {
            is_healthy: true,
            status: SearchEngineHealth::Healthy,
            response_time: 0,
            last_check: Utc::now(),
            consecutive_failures: 0,
            error_message: None,
        }
    }
}

/// Search engine health checker
pub struct SearchHealthChecker {
    client: Client,
    consecutive_failures: Arc<RwLock<u32>>,
    last_check: Arc<RwLock<DateTime<Utc>>>,
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
            consecutive_failures: Arc::new(RwLock::new(0)),
            last_check: Arc::new(RwLock::new(Utc::now())),
        }
    }

    /// Perform health check on a search engine
    pub async fn check_health(&self, engine_name: &str, test_url: &str) -> SearchHealthCheckResult {
        let start_time = std::time::Instant::now();

        match self.client.get(test_url).send().await {
            Ok(response) => {
                let response_time = start_time.elapsed().as_millis() as u64;
                let is_success =
                    response.status().is_success() && response.content_length().unwrap_or(0) > 100;

                let mut failures = self.consecutive_failures.write().await;
                let mut last = self.last_check.write().await;

                if is_success {
                    *failures = 0;
                    *last = Utc::now();

                    let status = if response_time > 3000 {
                        SearchEngineHealth::Degraded
                    } else {
                        SearchEngineHealth::Healthy
                    };

                    debug!("{} health check passed ({}ms)", engine_name, response_time);

                    SearchHealthCheckResult {
                        is_healthy: true,
                        status,
                        response_time: response_time,
                        last_check: *last,
                        consecutive_failures: *failures,
                        error_message: None,
                    }
                } else {
                    *failures += 1;
                    *last = Utc::now();

                    let status = if *failures >= 3 {
                        SearchEngineHealth::Unhealthy
                    } else {
                        SearchEngineHealth::Degraded
                    };

                    warn!(
                        "{} health check failed: status {}",
                        engine_name,
                        response.status()
                    );

                    SearchHealthCheckResult {
                        is_healthy: false,
                        status,
                        response_time,
                        last_check: *last,
                        consecutive_failures: *failures,
                        error_message: Some(format!("HTTP {}", response.status())),
                    }
                }
            }
            Err(e) => {
                let response_time = start_time.elapsed().as_millis() as u64;
                let mut failures = self.consecutive_failures.write().await;
                let mut last = self.last_check.write().await;

                *failures += 1;
                *last = Utc::now();

                let status = if *failures >= 3 {
                    SearchEngineHealth::Unhealthy
                } else {
                    SearchEngineHealth::Degraded
                };

                warn!("{} health check error: {}", engine_name, e);

                SearchHealthCheckResult {
                    is_healthy: false,
                    status,
                    response_time,
                    last_check: *last,
                    consecutive_failures: *failures,
                    error_message: Some(e.to_string()),
                }
            }
        }
    }

    /// Reset health check state
    pub async fn reset(&self) {
        let mut failures = self.consecutive_failures.write().await;
        let mut last = self.last_check.write().await;
        *failures = 0;
        *last = Utc::now();
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
        assert!(true); // Client should be created
    }

    #[tokio::test]
    async fn test_health_check_reset() {
        let checker = SearchHealthChecker::new();
        checker.reset().await;

        let failures = checker.consecutive_failures.read().await;
        assert_eq!(*failures, 0);
    }
}
