// Copyright 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use crate::domain::models::webhook::{WebhookEvent, WebhookEventType, WebhookStatus};
use crate::domain::repositories::task_repository::RepositoryError;
use crate::domain::repositories::webhook_event_repository::WebhookEventRepository;
use crate::infrastructure::database::entities::webhook_event::{self, SeaWebhookStatus};
use async_trait::async_trait;
use chrono::Utc;
use sea_orm::*;
use std::sync::Arc;
use uuid::Uuid;

/// Webhook事件仓库实现
#[derive(Clone)]
pub struct WebhookEventRepoImpl {
    db: Arc<DatabaseConnection>,
}

impl WebhookEventRepoImpl {
    /// 创建新的Webhook事件仓库实现
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }
}

impl From<WebhookStatus> for SeaWebhookStatus {
    fn from(status: WebhookStatus) -> Self {
        match status {
            WebhookStatus::Pending => SeaWebhookStatus::Pending,
            WebhookStatus::Delivered => SeaWebhookStatus::Delivered,
            WebhookStatus::Failed => SeaWebhookStatus::Failed,
            WebhookStatus::Dead => SeaWebhookStatus::Dead,
        }
    }
}

impl From<SeaWebhookStatus> for WebhookStatus {
    fn from(status: SeaWebhookStatus) -> Self {
        match status {
            SeaWebhookStatus::Pending => WebhookStatus::Pending,
            SeaWebhookStatus::Delivered => WebhookStatus::Delivered,
            SeaWebhookStatus::Failed => WebhookStatus::Failed,
            SeaWebhookStatus::Dead => WebhookStatus::Dead,
        }
    }
}

#[async_trait]
impl WebhookEventRepository for WebhookEventRepoImpl {
    async fn create(&self, event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError> {
        let active_model = webhook_event::ActiveModel {
            id: Set(event.id),
            team_id: Set(event.team_id),
            webhook_id: Set(Some(event.webhook_id)),
            event_type: Set(event.event_type.to_string()),
            payload: Set(event.payload.clone()),
            webhook_url: Set(event.webhook_url.clone()),
            status: Set(event.status.into()),
            attempt_count: Set(event.attempt_count),
            max_retries: Set(event.max_retries),
            next_retry_at: Set(event.next_retry_at.map(Into::into)),
            created_at: Set(event.created_at.into()),
            delivered_at: Set(event.delivered_at.map(Into::into)),
            ..Default::default()
        };

        webhook_event::Entity::insert(active_model)
            .exec(self.db.as_ref())
            .await?;

        Ok(event.clone())
    }

    async fn find_by_id(&self, id: Uuid) -> Result<Option<WebhookEvent>, RepositoryError> {
        let model = webhook_event::Entity::find_by_id(id)
            .one(self.db.as_ref())
            .await?;

        Ok(model.map(Into::into))
    }

    async fn find_pending(&self, limit: u64) -> Result<Vec<WebhookEvent>, RepositoryError> {
        let now = Utc::now();

        let models = webhook_event::Entity::find()
            .filter(
                Condition::any()
                    .add(webhook_event::Column::Status.eq(SeaWebhookStatus::Pending))
                    .add(
                        Condition::all()
                            .add(webhook_event::Column::Status.eq(SeaWebhookStatus::Failed))
                            .add(webhook_event::Column::NextRetryAt.lte(now)),
                    ),
            )
            .order_by_asc(webhook_event::Column::CreatedAt)
            .limit(limit)
            .all(self.db.as_ref())
            .await?;

        let events = models.into_iter().map(Into::into).collect();

        Ok(events)
    }

    async fn update(&self, event: &WebhookEvent) -> Result<WebhookEvent, RepositoryError> {
        let mut active: webhook_event::ActiveModel = webhook_event::Entity::find_by_id(event.id)
            .one(self.db.as_ref())
            .await?
            .ok_or(RepositoryError::NotFound)?
            .into();

        active.status = Set(event.status.into());
        active.attempt_count = Set(event.attempt_count);
        active.next_retry_at = Set(event.next_retry_at.map(Into::into));
        active.delivered_at = Set(event.delivered_at.map(Into::into));
        active.response_status = Set(event.response_status.map(|s| s as i16));

        let updated_model = active.update(self.db.as_ref()).await?;

        Ok(updated_model.into())
    }
}

impl From<webhook_event::Model> for WebhookEvent {
    fn from(model: webhook_event::Model) -> Self {
        Self {
            id: model.id,
            team_id: model.team_id,
            webhook_id: model.webhook_id.unwrap_or_default(),
            event_type: WebhookEventType::Custom(model.event_type), // Simplified mapping
            status: model.status.into(),
            payload: model.payload,
            webhook_url: model.webhook_url,
            response_status: model.response_status.map(|s| s as i32),
            response_body: None, // Not stored in DB
            error_message: None, // Not stored in DB
            attempt_count: model.attempt_count,
            max_retries: model.max_retries,
            next_retry_at: model.next_retry_at.map(Into::into),
            created_at: model.created_at.into(),
            updated_at: Utc::now(), // Not stored in DB, use current time
            delivered_at: model.delivered_at.map(Into::into),
        }
    }
}

impl From<WebhookEvent> for webhook_event::ActiveModel {
    fn from(event: WebhookEvent) -> Self {
        Self {
            id: Set(event.id),
            team_id: Set(event.team_id),
            webhook_id: Set(Some(event.webhook_id)),
            event_type: Set(event.event_type.to_string()),
            status: Set(event.status.into()),
            payload: Set(event.payload),
            webhook_url: Set(event.webhook_url),
            response_status: Set(event.response_status.map(|s| s as i16)),
            attempt_count: Set(event.attempt_count),
            max_retries: Set(event.max_retries),
            next_retry_at: Set(event.next_retry_at.map(Into::into)),
            created_at: Set(event.created_at.into()),
            delivered_at: Set(event.delivered_at.map(Into::into)),
        }
    }
}
