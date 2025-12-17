// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

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
