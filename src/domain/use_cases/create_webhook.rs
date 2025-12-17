// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::models::webhook::Webhook;
use crate::domain::repositories::task_repository::RepositoryError;
use crate::domain::repositories::webhook_repository::WebhookRepository;
use std::sync::Arc;
use uuid::Uuid;

pub struct CreateWebhookUseCase<R: WebhookRepository> {
    repo: Arc<R>,
}

impl<R: WebhookRepository> CreateWebhookUseCase<R> {
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    pub async fn execute(&self, team_id: Uuid, url: String) -> Result<Webhook, RepositoryError> {
        let webhook = Webhook::new(team_id, url);
        self.repo.create(&webhook).await?;
        Ok(webhook)
    }
}
