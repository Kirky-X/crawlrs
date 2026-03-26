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
    pub fn record_attempt(&mut self, success: bool, response_status: Option<i32>, error: Option<String>) {
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
