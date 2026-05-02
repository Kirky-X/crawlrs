// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Rate limit checking helper
//!
//! Provides a unified rate limit check function used across handlers.
//! Eliminates code duplication in crawl, scrape, search, and webhook handlers.

use crate::domain::services::rate_limiting_service::{RateLimitResult, RateLimitingService};
use crate::presentation::errors::AppError;
use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use tracing::error;

/// Check rate limit for an API key and endpoint.
///
/// This helper consolidates the rate limit check logic that was repeated
/// across multiple handlers (crawl, scrape, search, webhook).
///
/// # Arguments
///
/// * `service` - The rate limiting service (implements RateLimitingService trait)
/// * `api_key` - The API key to check
/// * `endpoint` - The endpoint path being accessed
///
/// # Returns
///
/// * `Ok(())` - Rate limit check passed
/// * `Err(Response)` - Rate limit exceeded, with appropriate error response
pub async fn check_rate_limit<T: RateLimitingService + ?Sized>(
    service: &T,
    api_key: &str,
    endpoint: &str,
) -> Result<(), Response> {
    match service.check_rate_limit(api_key, endpoint).await {
        Ok(RateLimitResult::Denied { reason }) => Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "success": false,
                "error": format!("Rate limit exceeded: {}", reason)
            })),
        )
            .into_response()),
        Ok(RateLimitResult::RetryAfter {
            retry_after_seconds,
        }) => Err((
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({
                "success": false,
                "error": "Rate limit exceeded, please retry later",
                "retry_after_seconds": retry_after_seconds
            })),
        )
            .into_response()),
        Err(e) => {
            error!("Rate limiting service error: {}", e);
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Check rate limit for an API key and endpoint, returning AppError.
///
/// This variant is for handlers that return `Result<T, AppError>`.
///
/// # Arguments
///
/// * `service` - The rate limiting service (implements RateLimitingService trait)
/// * `api_key` - The API key to check
/// * `endpoint` - The endpoint path being accessed
///
/// # Returns
///
/// * `Ok(())` - Rate limit check passed
/// * `Err(AppError::RateLimited)` - Rate limit exceeded
pub async fn check_rate_limit_as_app_error<T: RateLimitingService + ?Sized>(
    service: &T,
    api_key: &str,
    endpoint: &str,
) -> Result<(), AppError> {
    match service.check_rate_limit(api_key, endpoint).await {
        Ok(RateLimitResult::Denied { reason }) => Err(AppError::RateLimited(format!(
            "Rate limit exceeded: {}",
            reason
        ))),
        Ok(RateLimitResult::RetryAfter {
            retry_after_seconds,
        }) => Err(AppError::RateLimited(format!(
            "Rate limit exceeded, please retry after {} seconds",
            retry_after_seconds
        ))),
        Err(e) => {
            error!("Rate limiting service error: {}", e);
            Ok(())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::services::rate_limiting_service::{
        BacklogService, ConcurrencyConfig, ConcurrencyControlService, ConcurrencyResult,
        QuotaService, RateLimitConfig, RateLimitResult, RateLimitService, RateLimitingError,
    };
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use uuid::Uuid;

    struct MockRateLimitingService {
        call_count: Arc<AtomicUsize>,
        result: RateLimitResult,
    }

    #[async_trait]
    impl RateLimitService for MockRateLimitingService {
        async fn check_rate_limit(
            &self,
            _api_key: &str,
            _endpoint: &str,
        ) -> Result<RateLimitResult, RateLimitingError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            Ok(self.result.clone())
        }

        async fn get_team_rate_limit_config(
            &self,
            _team_id: Uuid,
        ) -> Result<RateLimitConfig, RateLimitingError> {
            Ok(RateLimitConfig::default())
        }

        async fn update_team_rate_limit_config(
            &self,
            _team_id: Uuid,
            _config: RateLimitConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn cleanup_expired_rate_limits(&self) -> Result<u64, RateLimitingError> {
            Ok(0)
        }
    }

    #[async_trait]
    impl ConcurrencyControlService for MockRateLimitingService {
        async fn check_team_concurrency(
            &self,
            _team_id: Uuid,
            _task_id: Uuid,
        ) -> Result<ConcurrencyResult, RateLimitingError> {
            Ok(ConcurrencyResult::Allowed)
        }

        async fn release_team_concurrency_slot(
            &self,
            _team_id: Uuid,
            _task_id: Uuid,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn get_team_current_concurrency(
            &self,
            _team_id: Uuid,
        ) -> Result<u32, RateLimitingError> {
            Ok(0)
        }

        async fn get_team_concurrency_config(
            &self,
            _team_id: Uuid,
        ) -> Result<ConcurrencyConfig, RateLimitingError> {
            Ok(ConcurrencyConfig::default())
        }

        async fn update_team_concurrency_config(
            &self,
            _team_id: Uuid,
            _config: ConcurrencyConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
    }

    #[async_trait]
    impl BacklogService for MockRateLimitingService {
        async fn process_backlog_tasks(&self, _team_id: Uuid) -> Result<u32, RateLimitingError> {
            Ok(0)
        }
    }

    #[async_trait]
    impl QuotaService for MockRateLimitingService {
        async fn check_and_deduct_quota(
            &self,
            _team_id: Uuid,
            _amount: i64,
            _transaction_type: crate::domain::models::CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }

        async fn get_quota_balance(&self, _team_id: Uuid) -> Result<i64, RateLimitingError> {
            Ok(1000)
        }
    }

    // 实现组合 trait（向后兼容）
    #[async_trait]
    impl RateLimitingService for MockRateLimitingService {}

    #[tokio::test]
    async fn test_allowed_passed() {
        let call_count = Arc::new(AtomicUsize::new(0));
        let service = MockRateLimitingService {
            call_count: call_count.clone(),
            result: RateLimitResult::Allowed,
        };
        let result = check_rate_limit(&service, "test-key", "/v1/test").await;
        assert!(result.is_ok());
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_denied_returns_429() {
        let service = MockRateLimitingService {
            call_count: Arc::new(AtomicUsize::new(0)),
            result: RateLimitResult::Denied {
                reason: "Too many requests".to_string(),
            },
        };
        let result = check_rate_limit(&service, "test-key", "/v1/test").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_retry_after_returns_429_with_seconds() {
        let service = MockRateLimitingService {
            call_count: Arc::new(AtomicUsize::new(0)),
            result: RateLimitResult::RetryAfter {
                retry_after_seconds: 30,
            },
        };
        let result = check_rate_limit(&service, "test-key", "/v1/test").await;
        assert!(result.is_err());
    }
}
