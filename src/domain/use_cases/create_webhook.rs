// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::models::Webhook;
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
        let now = chrono::Utc::now().naive_utc();
        let webhook = Webhook {
            id: Uuid::new_v4(),
            team_id,
            url,
            created_at: now,
        };
        self.repo.create(&webhook).await?;
        Ok(webhook)
    }
}
