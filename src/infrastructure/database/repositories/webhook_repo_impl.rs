// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Webhook repository implementation using Sea-ORM with Mapper

use crate::domain::models::Webhook;
use crate::domain::repositories::task_repository::RepositoryError;
use crate::domain::repositories::webhook_repository::WebhookRepository;
use crate::infrastructure::database::entities::webhook;
use crate::infrastructure::persistence::mappers::WebhookMapper;
use async_trait::async_trait;
use dbnexus::DbPool;
use sea_orm::{ActiveModelTrait, ColumnTrait, EntityTrait, QueryFilter};
use std::sync::Arc;
use uuid::Uuid;

/// Webhook repository implementation
#[derive(Clone)]
pub struct WebhookRepoImpl {
    pool: Arc<DbPool>,
}

impl WebhookRepoImpl {
    /// Create new webhook repository instance
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl WebhookRepository for WebhookRepoImpl {
    async fn create(&self, webhook: &Webhook) -> Result<Webhook, RepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let entity = WebhookMapper::to_entity(webhook);
        let active_model = webhook::ActiveModel::from(entity);

        active_model
            .insert(session.connection().map_err(|e| RepositoryError::Database(e.into()))?)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(webhook.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Webhook>, RepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let entity = webhook::Entity::find_by_id(id)
            .one(session.connection().map_err(|e| RepositoryError::Database(e.into()))?)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(entity.map(WebhookMapper::to_domain))
    }

    async fn find_by_team_id(&self, team_id: Uuid) -> Result<Vec<Webhook>, RepositoryError> {
        let session = self.pool.get_session("admin").await
            .map_err(|e| RepositoryError::Database(e.into()))?;
        
        let entities = webhook::Entity::find()
            .filter(webhook::Column::TeamId.eq(team_id))
            .all(session.connection().map_err(|e| RepositoryError::Database(e.into()))?)
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(WebhookMapper::to_domain_list(entities))
    }
}
