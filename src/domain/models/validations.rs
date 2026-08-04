// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Centralized domain validation functions.
//!
//! Each function is a pure, side-effect-free predicate or checker that
//! model methods delegate to. Keeping them in one module makes it easy
//! to audit, unit-test, and reuse validation logic across models.

use chrono::{DateTime, Utc};
use uuid::Uuid;

// ========== URL validation ==========

/// Validate that a URL is non-empty and starts with `http://` or `https://`.
///
/// # Arguments
///
/// * `url` - The URL string to validate.
///
/// # Errors
///
/// Returns a descriptive error string when the URL is invalid.
pub fn validate_url(url: &str) -> Result<(), String> {
    if url.is_empty() {
        return Err("URL cannot be empty".to_string());
    }
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err("URL must start with http:// or https://".to_string());
    }
    Ok(())
}

// ========== Team name validation ==========

/// Validate that a team name is non-empty and within length limits.
///
/// # Arguments
///
/// * `name` - The team name to validate.
///
/// # Errors
///
/// Returns a descriptive error string when the name is invalid.
pub fn validate_team_name(name: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err("Team name cannot be empty".to_string());
    }
    if name.len() > 255 {
        return Err("Team name must be 255 characters or fewer".to_string());
    }
    Ok(())
}

// ========== Task state checks ==========

/// Check whether a task can be retried based on its retry count and maximum.
///
/// # Arguments
///
/// * `retry_count` - Number of retries already attempted.
/// * `max_retries` - Maximum allowed retries.
pub fn can_task_retry(retry_count: i32, max_retries: i32) -> bool {
    retry_count < max_retries
}

/// Check whether a task has expired.
///
/// # Arguments
///
/// * `expires_at` - Optional expiration timestamp. `None` means no expiration.
pub fn is_task_expired(expires_at: Option<DateTime<Utc>>) -> bool {
    expires_at.is_some_and(|expires_at| Utc::now() > expires_at)
}

/// Check whether a task is currently locked.
///
/// A task is locked when it has a `lock_token` and the lock has not yet expired.
///
/// # Arguments
///
/// * `lock_token` - Optional lock token UUID.
/// * `lock_expires_at` - Optional lock expiration timestamp.
pub fn is_task_locked(lock_token: Option<Uuid>, lock_expires_at: Option<DateTime<Utc>>) -> bool {
    lock_token.is_some() && lock_expires_at.is_some_and(|expires_at| Utc::now() < expires_at)
}

// ========== Credits checks ==========

/// Check whether a credits balance is sufficient for a given amount.
///
/// # Arguments
///
/// * `balance` - Current balance.
/// * `amount` - Requested deduction amount.
pub fn has_sufficient_balance(balance: i64, amount: i64) -> bool {
    balance >= amount
}

// ========== Webhook retry check ==========

/// Check whether a webhook delivery can be retried.
///
/// # Arguments
///
/// * `attempt_count` - Number of delivery attempts already made.
/// * `max_retries` - Maximum allowed delivery attempts.
pub fn can_webhook_retry(attempt_count: i32, max_retries: i32) -> bool {
    attempt_count < max_retries
}

// ========== Crawl status checks ==========

/// Check whether a crawl is in a finished state (completed, failed, or cancelled).
///
/// # Arguments
///
/// * `status` - The crawl status string to check.
pub fn is_crawl_finished(status: &str) -> bool {
    matches!(status, "completed" | "failed" | "cancelled")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    // ========== validate_url ==========

    #[test]
    fn test_validate_url_valid_http() {
        assert!(validate_url("http://example.com").is_ok());
    }

    #[test]
    fn test_validate_url_valid_https() {
        assert!(validate_url("https://example.com/path?q=1").is_ok());
    }

    #[test]
    fn test_validate_url_empty() {
        assert!(validate_url("").is_err());
        assert_eq!(validate_url("").unwrap_err(), "URL cannot be empty");
    }

    #[test]
    fn test_validate_url_no_scheme() {
        assert!(validate_url("example.com").is_err());
        assert!(validate_url("example.com")
            .unwrap_err()
            .contains("http:// or https://"));
    }

    #[test]
    fn test_validate_url_ftp_scheme_rejected() {
        assert!(validate_url("ftp://example.com").is_err());
    }

    // ========== validate_team_name ==========

    #[test]
    fn test_validate_team_name_valid() {
        assert!(validate_team_name("My Team").is_ok());
    }

    #[test]
    fn test_validate_team_name_empty() {
        assert!(validate_team_name("").is_err());
        assert_eq!(
            validate_team_name("").unwrap_err(),
            "Team name cannot be empty"
        );
    }

    #[test]
    fn test_validate_team_name_too_long() {
        let long_name = "a".repeat(256);
        assert!(validate_team_name(&long_name).is_err());
    }

    #[test]
    fn test_validate_team_name_at_max_length() {
        let max_name = "a".repeat(255);
        assert!(validate_team_name(&max_name).is_ok());
    }

    // ========== can_task_retry ==========

    #[test]
    fn test_can_task_retry_under_limit() {
        assert!(can_task_retry(0, 3));
        assert!(can_task_retry(2, 3));
    }

    #[test]
    fn test_can_task_retry_at_limit() {
        assert!(!can_task_retry(3, 3));
    }

    #[test]
    fn test_can_task_retry_over_limit() {
        assert!(!can_task_retry(5, 3));
    }

    #[test]
    fn test_can_task_retry_zero_max() {
        assert!(!can_task_retry(0, 0));
    }

    // ========== is_task_expired ==========

    #[test]
    fn test_is_task_expired_none() {
        assert!(!is_task_expired(None));
    }

    #[test]
    fn test_is_task_expired_future() {
        let future = Some(Utc::now() + Duration::hours(1));
        assert!(!is_task_expired(future));
    }

    #[test]
    fn test_is_task_expired_past() {
        let past = Some(Utc::now() - Duration::hours(1));
        assert!(is_task_expired(past));
    }

    // ========== is_task_locked ==========

    #[test]
    fn test_is_task_locked_no_token() {
        assert!(!is_task_locked(None, None));
        assert!(!is_task_locked(None, Some(Utc::now() + Duration::hours(1))));
    }

    #[test]
    fn test_is_task_locked_with_valid_token() {
        let token = Some(Uuid::new_v4());
        let expires = Some(Utc::now() + Duration::hours(1));
        assert!(is_task_locked(token, expires));
    }

    #[test]
    fn test_is_task_locked_with_expired_token() {
        let token = Some(Uuid::new_v4());
        let expires = Some(Utc::now() - Duration::hours(1));
        assert!(!is_task_locked(token, expires));
    }

    // ========== has_sufficient_balance ==========

    #[test]
    fn test_has_sufficient_balance_enough() {
        assert!(has_sufficient_balance(100, 50));
        assert!(has_sufficient_balance(100, 100));
    }

    #[test]
    fn test_has_sufficient_balance_insufficient() {
        assert!(!has_sufficient_balance(50, 100));
    }

    #[test]
    fn test_has_sufficient_balance_zero() {
        assert!(has_sufficient_balance(0, 0));
        assert!(!has_sufficient_balance(0, 1));
    }

    // ========== can_webhook_retry ==========

    #[test]
    fn test_can_webhook_retry_under_limit() {
        assert!(can_webhook_retry(0, 3));
        assert!(can_webhook_retry(2, 3));
    }

    #[test]
    fn test_can_webhook_retry_at_limit() {
        assert!(!can_webhook_retry(3, 3));
    }

    // ========== is_crawl_finished ==========

    #[test]
    fn test_is_crawl_finished_completed() {
        assert!(is_crawl_finished("completed"));
    }

    #[test]
    fn test_is_crawl_finished_failed() {
        assert!(is_crawl_finished("failed"));
    }

    #[test]
    fn test_is_crawl_finished_cancelled() {
        assert!(is_crawl_finished("cancelled"));
    }

    #[test]
    fn test_is_crawl_finished_running() {
        assert!(!is_crawl_finished("running"));
        assert!(!is_crawl_finished("pending"));
        assert!(!is_crawl_finished("queued"));
    }
}
