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
use crate::domain::repositories::webhook_repository::WebhookRepository;
use crate::domain::use_cases::create_webhook::CreateWebhookUseCase;
use crate::presentation::errors::AppError;
use axum::{http::StatusCode, Extension, Json};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct CreateWebhookPayload {
    pub url: String,
    pub team_id: Uuid,
}

pub async fn create_webhook<R: WebhookRepository>(
    Extension(repo): Extension<Arc<R>>,
    Json(payload): Json<CreateWebhookPayload>,
) -> Result<(StatusCode, Json<Webhook>), AppError> {
    let use_case = CreateWebhookUseCase::new(repo);
    let webhook = use_case.execute(payload.team_id, payload.url).await?;
    Ok((StatusCode::CREATED, Json(webhook)))
}
