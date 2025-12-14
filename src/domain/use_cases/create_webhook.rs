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
