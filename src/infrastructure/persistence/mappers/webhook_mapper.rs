// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Webhook Mapper - converts between Webhook domain model and database entity

use crate::common::time_utils::{from_db_datetime, from_db_datetime_opt, to_db_datetime, to_db_datetime_opt};
use crate::domain::models::{Webhook, WebhookEvent, WebhookEventType, WebhookStatus};
use crate::infrastructure::database::entities::{webhook, webhook_event};
use uuid::Uuid;

/// Mapper for converting between Webhook domain model and database entity
pub struct WebhookMapper;

impl WebhookMapper {
    /// Convert database entity to domain model
    pub fn to_domain(entity: webhook::Model) -> Webhook {
        Webhook {
            id: entity.id,
            team_id: entity.team_id,
            url: entity.url,
            created_at: from_db_datetime(entity.created_at),
        }
    }

    /// Convert domain model to database entity
    pub fn to_entity(domain: &Webhook) -> webhook::Model {
        webhook::Model {
            id: domain.id,
            team_id: domain.team_id,
            url: domain.url.clone(),
            created_at: to_db_datetime(domain.created_at),
        }
    }

    /// Convert multiple entities to domain models
    pub fn to_domain_list(entities: Vec<webhook::Model>) -> Vec<Webhook> {
        entities.into_iter().map(Self::to_domain).collect()
    }
}

/// Mapper for converting between WebhookEvent domain model and database entity
pub struct WebhookEventMapper;

impl WebhookEventMapper {
    /// Convert database entity to domain model
    pub fn to_domain(entity: webhook_event::Model) -> WebhookEvent {
        WebhookEvent::with_all_fields(
            entity.id,
            entity.team_id,
            entity.webhook_id.unwrap_or(Uuid::nil()),
            Self::parse_event_type(&entity.event_type),
            entity.payload,
            entity.webhook_url,
            Self::parse_status_from_enum(&entity.status),
            entity.attempt_count,
            entity.max_retries,
            entity.response_status,
            entity.response_body,
            entity.error_message,
            from_db_datetime_opt(entity.next_retry_at),
            from_db_datetime(entity.created_at),
            from_db_datetime(entity.updated_at),
            from_db_datetime_opt(entity.delivered_at),
        )
    }

    /// Convert domain model to database entity
    pub fn to_entity(domain: &WebhookEvent) -> webhook_event::Model {
        webhook_event::Model {
            id: domain.id,
            team_id: domain.team_id,
            webhook_id: Some(domain.webhook_id),
            event_type: domain.event_type.to_string(),
            payload: domain.payload.clone(),
            webhook_url: domain.webhook_url.clone(),
            status: Self::status_to_enum(&domain.status),
            attempt_count: domain.attempt_count,
            max_retries: domain.max_retries,
            response_status: domain.response_status,
            response_body: domain.response_body.clone(),
            error_message: domain.error_message.clone(),
            next_retry_at: to_db_datetime_opt(domain.next_retry_at),
            created_at: to_db_datetime(domain.created_at),
            updated_at: to_db_datetime(domain.updated_at),
            delivered_at: to_db_datetime_opt(domain.delivered_at),
        }
    }

    /// Convert multiple entities to domain models
    pub fn to_domain_list(entities: Vec<webhook_event::Model>) -> Vec<WebhookEvent> {
        entities.into_iter().map(Self::to_domain).collect()
    }

    /// Parse event type from string
    fn parse_event_type(s: &str) -> WebhookEventType {
        s.parse().unwrap_or(WebhookEventType::Custom(s.to_string()))
    }

    /// Parse status from enum
    fn parse_status_from_enum(status: &webhook_event::SeaWebhookStatus) -> WebhookStatus {
        match status {
            webhook_event::SeaWebhookStatus::Pending => WebhookStatus::Pending,
            webhook_event::SeaWebhookStatus::Delivered => WebhookStatus::Delivered,
            webhook_event::SeaWebhookStatus::Failed => WebhookStatus::Failed,
            webhook_event::SeaWebhookStatus::Dead => WebhookStatus::Dead,
        }
    }

    /// Convert status to enum
    fn status_to_enum(status: &WebhookStatus) -> webhook_event::SeaWebhookStatus {
        match status {
            WebhookStatus::Pending => webhook_event::SeaWebhookStatus::Pending,
            WebhookStatus::Delivered => webhook_event::SeaWebhookStatus::Delivered,
            WebhookStatus::Failed => webhook_event::SeaWebhookStatus::Failed,
            WebhookStatus::Dead => webhook_event::SeaWebhookStatus::Dead,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    #[test]
    fn test_webhook_mapper_roundtrip() {
        let now = Utc::now();
        let domain = Webhook {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            url: "https://example.com/webhook".to_string(),
            created_at: now,
        };

        let entity = WebhookMapper::to_entity(&domain);
        let back_to_domain = WebhookMapper::to_domain(entity);

        assert_eq!(domain.id, back_to_domain.id);
        assert_eq!(domain.url, back_to_domain.url);
    }

    #[test]
    fn test_webhook_event_mapper_roundtrip() {
        let now = Utc::now();
        let domain = WebhookEvent::with_all_fields(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            WebhookEventType::CrawlCompleted,
            serde_json::json!({"test": "data"}),
            "https://example.com/webhook".to_string(),
            WebhookStatus::Pending,
            0,
            5,
            None,
            None,
            None,
            None,
            now,
            now,
            None,
        );

        let entity = WebhookEventMapper::to_entity(&domain);
        let back_to_domain = WebhookEventMapper::to_domain(entity);

        assert_eq!(domain.id, back_to_domain.id);
        assert_eq!(domain.event_type, back_to_domain.event_type);
        assert_eq!(domain.status, back_to_domain.status);
    }
}
