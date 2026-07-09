// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Webhook domain model - pure domain entity without ORM annotations
//!
//! This module contains the pure domain model for Webhook,
//! following Domain-Driven Design principles.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Webhook domain model
///
/// Represents a webhook endpoint for delivering event notifications.
/// This is a pure domain model without any ORM annotations.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Webhook {
    /// Unique identifier
    pub id: Uuid,
    /// Team ID for multi-tenancy
    pub team_id: Uuid,
    /// Webhook endpoint URL
    pub url: String,
    /// When the webhook was created
    pub created_at: DateTime<Utc>,
}

impl Webhook {
    /// Create a new webhook
    pub fn new(id: Uuid, team_id: Uuid, url: String) -> Self {
        Self {
            id,
            team_id,
            url,
            created_at: Utc::now(),
        }
    }

    /// Validate the webhook URL
    pub fn validate_url(&self) -> Result<(), WebhookError> {
        // Basic URL validation
        if self.url.is_empty() {
            return Err(WebhookError::InvalidUrl("URL cannot be empty".to_string()));
        }

        // Check for valid URL scheme
        if !self.url.starts_with("http://") && !self.url.starts_with("https://") {
            return Err(WebhookError::InvalidUrl(
                "URL must start with http:// or https://".to_string(),
            ));
        }

        Ok(())
    }
}

/// Webhook event domain model
///
/// Represents a single webhook delivery attempt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WebhookEvent {
    /// Unique identifier
    pub id: Uuid,
    /// Team ID for multi-tenancy
    pub team_id: Uuid,
    /// Webhook ID this event belongs to
    pub webhook_id: Uuid,
    /// Type of event
    pub event_type: WebhookEventType,
    /// Event payload as JSON
    pub payload: serde_json::Value,
    /// Webhook URL at the time of delivery
    pub webhook_url: String,
    /// Current delivery status
    pub status: WebhookStatus,
    /// Number of delivery attempts
    pub attempt_count: i32,
    /// Maximum retry attempts
    pub max_retries: i32,
    /// HTTP response status code (if delivered)
    pub response_status: Option<i32>,
    /// Response body (if any)
    pub response_body: Option<String>,
    /// Error message (if failed)
    pub error_message: Option<String>,
    /// When to retry next (if pending retry)
    pub next_retry_at: Option<DateTime<Utc>>,
    /// When the event was created
    pub created_at: DateTime<Utc>,
    /// When the event was last updated
    pub updated_at: DateTime<Utc>,
    /// When the event was delivered (if successful)
    pub delivered_at: Option<DateTime<Utc>>,
}

impl WebhookEvent {
    /// Create a new webhook event
    pub fn new(
        id: Uuid,
        team_id: Uuid,
        webhook_id: Uuid,
        event_type: WebhookEventType,
        payload: serde_json::Value,
        webhook_url: String,
    ) -> Self {
        let now = Utc::now();
        Self {
            id,
            team_id,
            webhook_id,
            event_type,
            payload,
            webhook_url,
            status: WebhookStatus::Pending,
            attempt_count: 0,
            max_retries: 5,
            response_status: None,
            response_body: None,
            error_message: None,
            next_retry_at: None,
            created_at: now,
            updated_at: now,
            delivered_at: None,
        }
    }

    /// Create a webhook event with all fields (for mappers)
    #[allow(clippy::too_many_arguments)]
    pub fn with_all_fields(
        id: Uuid,
        team_id: Uuid,
        webhook_id: Uuid,
        event_type: WebhookEventType,
        payload: serde_json::Value,
        webhook_url: String,
        status: WebhookStatus,
        attempt_count: i32,
        max_retries: i32,
        response_status: Option<i32>,
        response_body: Option<String>,
        error_message: Option<String>,
        next_retry_at: Option<DateTime<Utc>>,
        created_at: DateTime<Utc>,
        updated_at: DateTime<Utc>,
        delivered_at: Option<DateTime<Utc>>,
    ) -> Self {
        Self {
            id,
            team_id,
            webhook_id,
            event_type,
            payload,
            webhook_url,
            status,
            attempt_count,
            max_retries,
            response_status,
            response_body,
            error_message,
            next_retry_at,
            created_at,
            updated_at,
            delivered_at,
        }
    }

    /// Check if the event can be retried
    pub fn can_retry(&self) -> bool {
        self.attempt_count < self.max_retries && self.status != WebhookStatus::Delivered
    }

    /// Record a delivery attempt
    pub fn record_attempt(
        &mut self,
        success: bool,
        response_status: Option<i32>,
        error: Option<String>,
    ) {
        self.attempt_count += 1;
        self.updated_at = Utc::now();

        if success {
            self.status = WebhookStatus::Delivered;
            self.delivered_at = Some(Utc::now());
            self.response_status = response_status;
        } else {
            self.error_message = error;
            self.response_status = response_status;

            if self.attempt_count >= self.max_retries {
                self.status = WebhookStatus::Dead;
            } else {
                self.status = WebhookStatus::Failed;
                // Schedule next retry with exponential backoff
                let delay = chrono::Duration::seconds(2_i64.pow(self.attempt_count as u32));
                self.next_retry_at = Some(Utc::now() + delay);
            }
        }
    }

    /// Mark as delivered
    pub fn mark_delivered(&mut self, response_status: i32, response_body: Option<String>) {
        self.status = WebhookStatus::Delivered;
        self.response_status = Some(response_status);
        self.response_body = response_body;
        self.delivered_at = Some(Utc::now());
        self.updated_at = Utc::now();
    }

    /// Mark as failed
    pub fn mark_failed(&mut self, error_message: String) {
        self.status = if self.attempt_count >= self.max_retries {
            WebhookStatus::Dead
        } else {
            WebhookStatus::Failed
        };
        self.error_message = Some(error_message);
        self.updated_at = Utc::now();
    }
}

/// Webhook event type enumeration
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WebhookEventType {
    /// Crawl completed successfully
    CrawlCompleted,
    /// Crawl failed
    CrawlFailed,
    /// Scrape completed successfully
    ScrapeCompleted,
    /// Scrape failed
    ScrapeFailed,
    /// Custom event type
    Custom(String),
}

impl std::fmt::Display for WebhookEventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebhookEventType::CrawlCompleted => write!(f, "crawl.completed"),
            WebhookEventType::CrawlFailed => write!(f, "crawl.failed"),
            WebhookEventType::ScrapeCompleted => write!(f, "scrape.completed"),
            WebhookEventType::ScrapeFailed => write!(f, "scrape.failed"),
            WebhookEventType::Custom(s) => write!(f, "{}", s),
        }
    }
}

impl std::str::FromStr for WebhookEventType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "crawl.completed" => Ok(WebhookEventType::CrawlCompleted),
            "crawl.failed" => Ok(WebhookEventType::CrawlFailed),
            "scrape.completed" => Ok(WebhookEventType::ScrapeCompleted),
            "scrape.failed" => Ok(WebhookEventType::ScrapeFailed),
            s => Ok(WebhookEventType::Custom(s.to_string())),
        }
    }
}

/// Webhook status enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum WebhookStatus {
    /// Waiting to be delivered
    #[default]
    Pending,
    /// Successfully delivered
    Delivered,
    /// Delivery failed, will retry
    Failed,
    /// Delivery failed permanently
    Dead,
}

impl std::fmt::Display for WebhookStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WebhookStatus::Pending => write!(f, "pending"),
            WebhookStatus::Delivered => write!(f, "delivered"),
            WebhookStatus::Failed => write!(f, "failed"),
            WebhookStatus::Dead => write!(f, "dead"),
        }
    }
}

impl std::str::FromStr for WebhookStatus {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "pending" => Ok(WebhookStatus::Pending),
            "delivered" => Ok(WebhookStatus::Delivered),
            "failed" => Ok(WebhookStatus::Failed),
            "dead" => Ok(WebhookStatus::Dead),
            _ => Err(format!("Invalid webhook status: {}", s)),
        }
    }
}

/// Webhook domain errors
#[derive(Debug, thiserror::Error)]
pub enum WebhookError {
    /// Invalid URL
    #[error("Invalid URL: {0}")]
    InvalidUrl(String),

    /// Delivery failed
    #[error("Delivery failed: {0}")]
    DeliveryFailed(String),

    /// Database error
    #[error("Database error: {0}")]
    DatabaseError(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    // ========== Webhook tests ==========

    #[test]
    fn test_webhook_new_sets_fields_and_timestamp() {
        let id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let url = "https://example.com/webhook".to_string();

        let before = Utc::now();
        let webhook = Webhook::new(id, team_id, url.clone());
        let after = Utc::now();

        assert_eq!(webhook.id, id, "id should match");
        assert_eq!(webhook.team_id, team_id, "team_id should match");
        assert_eq!(webhook.url, url, "url should match");
        assert!(
            webhook.created_at >= before && webhook.created_at <= after,
            "created_at should be set to now"
        );
    }

    #[test]
    fn test_webhook_validate_url_https_passes() {
        let webhook = Webhook::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "https://example.com/webhook".to_string(),
        );
        assert!(
            webhook.validate_url().is_ok(),
            "https URL should be valid"
        );
    }

    #[test]
    fn test_webhook_validate_url_http_passes() {
        let webhook = Webhook::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "http://example.com/webhook".to_string(),
        );
        assert!(
            webhook.validate_url().is_ok(),
            "http URL should be valid"
        );
    }

    #[test]
    fn test_webhook_validate_url_empty_fails() {
        let webhook = Webhook::new(Uuid::new_v4(), Uuid::new_v4(), String::new());
        let err = webhook
            .validate_url()
            .expect_err("empty URL should be invalid");
        match err {
            WebhookError::InvalidUrl(msg) => {
                assert!(
                    msg.contains("empty"),
                    "error message should mention empty: {}",
                    msg
                );
            }
            other => panic!("expected InvalidUrl, got {:?}", other),
        }
    }

    #[test]
    fn test_webhook_validate_url_invalid_scheme_fails() {
        let webhook = Webhook::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "ftp://example.com/webhook".to_string(),
        );
        let err = webhook
            .validate_url()
            .expect_err("ftp URL should be invalid");
        match err {
            WebhookError::InvalidUrl(msg) => {
                assert!(
                    msg.contains("http://") && msg.contains("https://"),
                    "error message should mention valid schemes: {}",
                    msg
                );
            }
            other => panic!("expected InvalidUrl, got {:?}", other),
        }
    }

    // ========== WebhookEvent::new tests ==========

    #[test]
    fn test_webhook_event_new_defaults() {
        let id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let webhook_id = Uuid::new_v4();
        let payload = serde_json::json!({"event": "test"});
        let url = "https://example.com/hook".to_string();

        let before = Utc::now();
        let event = WebhookEvent::new(
            id,
            team_id,
            webhook_id,
            WebhookEventType::CrawlCompleted,
            payload.clone(),
            url.clone(),
        );
        let after = Utc::now();

        assert_eq!(event.id, id);
        assert_eq!(event.team_id, team_id);
        assert_eq!(event.webhook_id, webhook_id);
        assert_eq!(event.event_type, WebhookEventType::CrawlCompleted);
        assert_eq!(event.payload, payload);
        assert_eq!(event.webhook_url, url);
        assert_eq!(event.status, WebhookStatus::Pending, "new event should be Pending");
        assert_eq!(event.attempt_count, 0, "new event should have 0 attempts");
        assert_eq!(event.max_retries, 5, "default max_retries should be 5");
        assert!(event.response_status.is_none());
        assert!(event.response_body.is_none());
        assert!(event.error_message.is_none());
        assert!(event.next_retry_at.is_none());
        assert!(event.delivered_at.is_none());
        assert!(
            event.created_at >= before && event.created_at <= after,
            "created_at should be now"
        );
        assert_eq!(
            event.created_at, event.updated_at,
            "created_at and updated_at should match for new event"
        );
    }

    // ========== WebhookEvent::with_all_fields tests ==========

    #[test]
    fn test_webhook_event_with_all_fields_sets_everything() {
        let id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let webhook_id = Uuid::new_v4();
        let payload = serde_json::json!({"k": "v"});
        let url = "https://example.com/h".to_string();
        let created = Utc::now();
        let updated = created + chrono::Duration::seconds(10);
        let delivered = updated + chrono::Duration::seconds(5);
        let next_retry = updated + chrono::Duration::seconds(30);

        let event = WebhookEvent::with_all_fields(
            id,
            team_id,
            webhook_id,
            WebhookEventType::ScrapeFailed,
            payload.clone(),
            url.clone(),
            WebhookStatus::Failed,
            3,
            5,
            Some(500),
            Some("err body".to_string()),
            Some("boom".to_string()),
            Some(next_retry),
            created,
            updated,
            Some(delivered),
        );

        assert_eq!(event.id, id);
        assert_eq!(event.team_id, team_id);
        assert_eq!(event.webhook_id, webhook_id);
        assert_eq!(event.event_type, WebhookEventType::ScrapeFailed);
        assert_eq!(event.payload, payload);
        assert_eq!(event.webhook_url, url);
        assert_eq!(event.status, WebhookStatus::Failed);
        assert_eq!(event.attempt_count, 3);
        assert_eq!(event.max_retries, 5);
        assert_eq!(event.response_status, Some(500));
        assert_eq!(event.response_body, Some("err body".to_string()));
        assert_eq!(event.error_message, Some("boom".to_string()));
        assert_eq!(event.next_retry_at, Some(next_retry));
        assert_eq!(event.created_at, created);
        assert_eq!(event.updated_at, updated);
        assert_eq!(event.delivered_at, Some(delivered));
    }

    // ========== WebhookEvent::can_retry tests ==========

    #[test]
    fn test_can_retry_true_when_attempts_below_max_and_not_delivered() {
        let mut event = make_event();
        event.attempt_count = 2;
        event.max_retries = 5;
        event.status = WebhookStatus::Failed;
        assert!(
            event.can_retry(),
            "should be retryable when attempts < max and not delivered"
        );
    }

    #[test]
    fn test_can_retry_false_when_delivered() {
        let mut event = make_event();
        event.attempt_count = 1;
        event.max_retries = 5;
        event.status = WebhookStatus::Delivered;
        assert!(
            !event.can_retry(),
            "delivered event should not be retryable"
        );
    }

    #[test]
    fn test_can_retry_false_when_attempts_reach_max() {
        let mut event = make_event();
        event.attempt_count = 5;
        event.max_retries = 5;
        event.status = WebhookStatus::Failed;
        assert!(
            !event.can_retry(),
            "event at max retries should not be retryable"
        );
    }

    // ========== WebhookEvent::record_attempt tests ==========

    #[test]
    fn test_record_attempt_success_marks_delivered() {
        let mut event = make_event();
        let before = Utc::now();
        event.record_attempt(true, Some(200), None);

        assert_eq!(event.attempt_count, 1, "attempt_count should increment");
        assert_eq!(event.status, WebhookStatus::Delivered, "status should be Delivered");
        assert_eq!(event.response_status, Some(200));
        assert!(
            event.delivered_at.is_some(),
            "delivered_at should be set on success"
        );
        assert!(event.delivered_at.expect("delivered_at set") >= before);
        assert!(event.updated_at >= before, "updated_at should advance");
    }

    #[test]
    fn test_record_attempt_failure_with_retries_left_sets_failed_and_schedule() {
        let mut event = make_event();
        event.max_retries = 5;
        let before = Utc::now();

        event.record_attempt(false, Some(500), Some("server error".to_string()));

        assert_eq!(event.attempt_count, 1);
        assert_eq!(event.status, WebhookStatus::Failed, "should be Failed not Dead");
        assert_eq!(event.response_status, Some(500));
        assert_eq!(event.error_message, Some("server error".to_string()));
        let next = event
            .next_retry_at
            .expect("next_retry_at should be scheduled on failure with retries left");
        // Exponential backoff: 2^1 = 2 seconds
        assert!(
            next >= before + chrono::Duration::seconds(2),
            "next_retry_at should be ~2s in the future"
        );
    }

    #[test]
    fn test_record_attempt_failure_at_max_sets_dead() {
        let mut event = make_event();
        event.attempt_count = 4;
        event.max_retries = 5;

        event.record_attempt(false, None, Some("boom".to_string()));

        assert_eq!(event.attempt_count, 5);
        assert_eq!(event.status, WebhookStatus::Dead, "should be Dead when max reached");
        assert!(
            event.next_retry_at.is_none(),
            "next_retry_at should not be set when dead"
        );
        assert_eq!(event.error_message, Some("boom".to_string()));
    }

    // ========== WebhookEvent::mark_delivered tests ==========

    #[test]
    fn test_mark_delivered_sets_status_and_response() {
        let mut event = make_event();
        event.status = WebhookStatus::Failed;
        let before = Utc::now();

        event.mark_delivered(200, Some("ok body".to_string()));

        assert_eq!(event.status, WebhookStatus::Delivered);
        assert_eq!(event.response_status, Some(200));
        assert_eq!(event.response_body, Some("ok body".to_string()));
        assert!(event.delivered_at.is_some());
        assert!(event.updated_at >= before);
    }

    #[test]
    fn test_mark_delivered_without_body() {
        let mut event = make_event();
        event.mark_delivered(204, None);
        assert_eq!(event.status, WebhookStatus::Delivered);
        assert_eq!(event.response_status, Some(204));
        assert!(event.response_body.is_none());
    }

    // ========== WebhookEvent::mark_failed tests ==========

    #[test]
    fn test_mark_failed_with_retries_left_sets_failed() {
        let mut event = make_event();
        event.attempt_count = 1;
        event.max_retries = 5;

        event.mark_failed("network error".to_string());

        assert_eq!(event.status, WebhookStatus::Failed, "should be Failed when retries remain");
        assert_eq!(event.error_message, Some("network error".to_string()));
    }

    #[test]
    fn test_mark_failed_at_max_sets_dead() {
        let mut event = make_event();
        event.attempt_count = 5;
        event.max_retries = 5;

        event.mark_failed("exhausted".to_string());

        assert_eq!(event.status, WebhookStatus::Dead, "should be Dead at max retries");
        assert_eq!(event.error_message, Some("exhausted".to_string()));
    }

    // ========== WebhookEventType Display / FromStr tests ==========

    #[test]
    fn test_webhook_event_type_display_all_variants() {
        assert_eq!(WebhookEventType::CrawlCompleted.to_string(), "crawl.completed");
        assert_eq!(WebhookEventType::CrawlFailed.to_string(), "crawl.failed");
        assert_eq!(WebhookEventType::ScrapeCompleted.to_string(), "scrape.completed");
        assert_eq!(WebhookEventType::ScrapeFailed.to_string(), "scrape.failed");
        assert_eq!(
            WebhookEventType::Custom("custom.event".to_string()).to_string(),
            "custom.event"
        );
    }

    #[test]
    fn test_webhook_event_type_from_str_known_variants() {
        assert_eq!(
            WebhookEventType::from_str("crawl.completed").expect("valid"),
            WebhookEventType::CrawlCompleted
        );
        assert_eq!(
            WebhookEventType::from_str("crawl.failed").expect("valid"),
            WebhookEventType::CrawlFailed
        );
        assert_eq!(
            WebhookEventType::from_str("scrape.completed").expect("valid"),
            WebhookEventType::ScrapeCompleted
        );
        assert_eq!(
            WebhookEventType::from_str("scrape.failed").expect("valid"),
            WebhookEventType::ScrapeFailed
        );
    }

    #[test]
    fn test_webhook_event_type_from_str_unknown_becomes_custom() {
        let result = WebhookEventType::from_str("my.custom.event").expect("custom variant");
        assert_eq!(result, WebhookEventType::Custom("my.custom.event".to_string()));
    }

    #[test]
    fn test_webhook_event_type_roundtrip_serde() {
        let variants = vec![
            WebhookEventType::CrawlCompleted,
            WebhookEventType::CrawlFailed,
            WebhookEventType::ScrapeCompleted,
            WebhookEventType::ScrapeFailed,
            WebhookEventType::Custom("x".to_string()),
        ];
        for v in variants {
            let json = serde_json::to_string(&v).expect("serialize");
            let back: WebhookEventType =
                serde_json::from_str(&json).expect("deserialize");
            assert_eq!(v, back, "roundtrip should preserve variant: {}", json);
        }
    }

    // ========== WebhookStatus Display / FromStr tests ==========

    #[test]
    fn test_webhook_status_display_all_variants() {
        assert_eq!(WebhookStatus::Pending.to_string(), "pending");
        assert_eq!(WebhookStatus::Delivered.to_string(), "delivered");
        assert_eq!(WebhookStatus::Failed.to_string(), "failed");
        assert_eq!(WebhookStatus::Dead.to_string(), "dead");
    }

    #[test]
    fn test_webhook_status_from_str_valid() {
        assert_eq!(
            WebhookStatus::from_str("pending").expect("valid"),
            WebhookStatus::Pending
        );
        assert_eq!(
            WebhookStatus::from_str("delivered").expect("valid"),
            WebhookStatus::Delivered
        );
        assert_eq!(
            WebhookStatus::from_str("failed").expect("valid"),
            WebhookStatus::Failed
        );
        assert_eq!(
            WebhookStatus::from_str("dead").expect("valid"),
            WebhookStatus::Dead
        );
    }

    #[test]
    fn test_webhook_status_from_str_invalid_returns_error() {
        let err = WebhookStatus::from_str("unknown")
            .expect_err("invalid status should error");
        assert!(
            err.contains("Invalid webhook status"),
            "error should describe invalid status: {}",
            err
        );
    }

    #[test]
    fn test_webhook_status_default_is_pending() {
        assert_eq!(WebhookStatus::default(), WebhookStatus::Pending);
    }

    // ========== WebhookError tests ==========

    #[test]
    fn test_webhook_error_display_messages() {
        let invalid = WebhookError::InvalidUrl("bad url".to_string());
        assert!(invalid.to_string().contains("Invalid URL"));
        assert!(invalid.to_string().contains("bad url"));

        let delivery = WebhookError::DeliveryFailed("timeout".to_string());
        assert!(delivery.to_string().contains("Delivery failed"));

        let db = WebhookError::DatabaseError("conn lost".to_string());
        assert!(db.to_string().contains("Database error"));
    }

    // ========== Helper ==========

    fn make_event() -> WebhookEvent {
        WebhookEvent::new(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            WebhookEventType::CrawlCompleted,
            serde_json::json!({}),
            "https://example.com/hook".to_string(),
        )
    }
}
