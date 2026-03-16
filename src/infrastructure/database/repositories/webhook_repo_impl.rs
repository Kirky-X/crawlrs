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
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use std::sync::Arc;
use uuid::Uuid;

/// Webhook repository implementation
#[derive(Clone)]
pub struct WebhookRepoImpl {
    db: Arc<DatabaseConnection>,
}

impl WebhookRepoImpl {
    /// Create new webhook repository instance
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl WebhookRepository for WebhookRepoImpl {
    async fn create(&self, webhook: &Webhook) -> Result<Webhook, RepositoryError> {
        let entity = WebhookMapper::to_entity(webhook);
        let active_model = webhook::ActiveModel::from(entity);

        active_model
            .insert(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(webhook.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Webhook>, RepositoryError> {
        let entity = webhook::Entity::find_by_id(id)
            .one(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(entity.map(WebhookMapper::to_domain))
    }

    async fn find_by_team_id(&self, team_id: Uuid) -> Result<Vec<Webhook>, RepositoryError> {
        let entities = webhook::Entity::find()
            .filter(webhook::Column::TeamId.eq(team_id))
            .all(self.db.as_ref())
            .await
            .map_err(|e| RepositoryError::Database(e.into()))?;

        Ok(WebhookMapper::to_domain_list(entities))
    }
}
