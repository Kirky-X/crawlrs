// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::models::webhook::Webhook;
use crate::domain::repositories::task_repository::RepositoryError;
use crate::domain::repositories::webhook_repository::WebhookRepository;
use crate::infrastructure::database::entities::webhook;
use async_trait::async_trait;
use sea_orm::*;
use std::sync::Arc;
use uuid::Uuid;

/// Webhook仓库实现
#[derive(Clone)]
pub struct WebhookRepoImpl {
    db: Arc<DatabaseConnection>,
}

impl WebhookRepoImpl {
    /// 创建新的Webhook仓库实现
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl WebhookRepository for WebhookRepoImpl {
    async fn create(&self, webhook: &Webhook) -> Result<Webhook, RepositoryError> {
        let model = webhook::ActiveModel {
            id: Set(webhook.id),
            team_id: Set(webhook.team_id),
            url: Set(webhook.url.clone()),
            created_at: Set(webhook.created_at.into()),
        };

        model.insert(self.db.as_ref()).await?;
        Ok(webhook.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<Webhook>, RepositoryError> {
        let model = webhook::Entity::find_by_id(id)
            .one(self.db.as_ref())
            .await?;

        Ok(model.map(Into::into))
    }
}

impl From<webhook::Model> for Webhook {
    fn from(model: webhook::Model) -> Self {
        Self {
            id: model.id,
            team_id: model.team_id,
            url: model.url,
            created_at: model.created_at.into(),
        }
    }
}
