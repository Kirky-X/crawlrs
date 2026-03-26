// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Webhook event repository implementation using Sea-ORM with Mapper

use crate::domain::models::WebhookEvent;
use crate::domain::repositories::task_repository::RepositoryError;
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crate::infrastructure::database::entities::webhook_event;
use crate::infrastructure::persistence::mappers::WebhookEventMapper;
use async_trait::async_trait;
use chrono::Utc;
use dbnexus::DbPool;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};
use std::sync::Arc;
use uuid::Uuid;

/// Webhook event repository implementation using Sea-ORM
#[derive(Clone)]
pub struct WebhookEventRepoImpl {
    /// Database pool
    pool: Arc<DbPool>,
}

impl WebhookEventRepoImpl {
    /// Create new webhook event repository instance
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WebhookEventRepository for WebhookEventRepoImpl {
    async fn create(&self, event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let entity = WebhookEventMapper::to_entity(event);
        let active_model = webhook_event::ActiveModel::from(entity);

        active_model
            .insert(session.connection().map_err(|e| RepositoryError::Database(e.into()))?)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(event.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<WebhookEvent>, RepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let entity = webhook_event::Entity::find_by_id(id)
            .one(session.connection().map_err(|e| RepositoryError::Database(e.into()))?)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(entity.map(WebhookEventMapper::to_domain))
    }

    async fn find_pending(&self, limit: u64) -> Result<Vec<WebhookEvent>, RepositoryError> {
        let now = Utc::now();

        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let conn = session.connection().map_err(|e| RepositoryError::Database(e.into()))?;

        // Find pending events
        let pending = webhook_event::Entity::find()
            .filter(webhook_event::Column::Status.eq(webhook_event::SeaWebhookStatus::Pending))
            .limit(limit)
            .all(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        // Also get failed events that are ready for retry
        let failed_retry = webhook_event::Entity::find()
            .filter(webhook_event::Column::Status.eq(webhook_event::SeaWebhookStatus::Failed))
            .filter(webhook_event::Column::NextRetryAt.lt(now))
            .limit(limit)
            .all(conn)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        let mut events = pending;
        events.extend(failed_retry);

        Ok(WebhookEventMapper::to_domain_list(events))
    }

    async fn update(&self, event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let entity = WebhookEventMapper::to_entity(event);
        let active_model = webhook_event::ActiveModel::from(entity);

        active_model
            .update(session.connection().map_err(|e| RepositoryError::Database(e.into()))?)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(event.clone())
    }

    async fn find_by_team_id_paginated(
        &self,
        team_id: Uuid,
        limit: u32,
        offset: u32,
    ) -> Result<Vec<WebhookEvent>, RepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let entities = webhook_event::Entity::find()
            .filter(webhook_event::Column::TeamId.eq(team_id))
            .order_by(webhook_event::Column::CreatedAt, sea_orm::Order::Desc)
            .limit(limit as u64)
            .offset(offset as u64)
            .all(session.connection().map_err(|e| RepositoryError::Database(e.into()))?)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(WebhookEventMapper::to_domain_list(entities))
    }

    async fn count_by_team_id(&self, team_id: Uuid) -> Result<u64, RepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let count = webhook_event::Entity::find()
            .filter(webhook_event::Column::TeamId.eq(team_id))
            .count(session.connection().map_err(|e| RepositoryError::Database(e.into()))?)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(count)
    }
}
