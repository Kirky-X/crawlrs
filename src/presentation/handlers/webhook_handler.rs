// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use crate::domain::models::webhook::Webhook;
use crate::domain::repositories::webhook_repository::WebhookRepository;
use crate::domain::services::rate_limiting_service::{RateLimitResult, RateLimitingService};
use crate::domain::use_cases::create_webhook::CreateWebhookUseCase;
use crate::presentation::errors::AppError;
use axum::{http::StatusCode, Extension, Json};
use serde::Deserialize;
use std::sync::Arc;
use tracing::error;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct CreateWebhookPayload {
    pub url: String,
}

pub async fn create_webhook<R: WebhookRepository>(
    Extension(repo): Extension<Arc<R>>,
    Extension(rate_limiting_service): Extension<Arc<dyn RateLimitingService>>,
    Extension(team_id): Extension<Uuid>,
    Json(payload): Json<CreateWebhookPayload>,
) -> Result<(StatusCode, Json<Webhook>), AppError> {
    // 1. 检查限流
    match rate_limiting_service
        .check_rate_limit("default_api_key", "/v1/webhooks")
        .await
    {
        Ok(RateLimitResult::Denied { reason }) => {
            return Err(AppError::from(anyhow::anyhow!(
                "Rate limit exceeded: {}",
                reason
            )));
        }
        Ok(RateLimitResult::RetryAfter {
            retry_after_seconds,
        }) => {
            return Err(AppError::from(anyhow::anyhow!(
                "Rate limit exceeded, please retry after {} seconds",
                retry_after_seconds
            )));
        }
        Err(e) => {
            error!("Rate limiting service error: {}", e);
        }
        _ => {}
    }

    let use_case = CreateWebhookUseCase::new(repo);
    let webhook = use_case.execute(team_id, payload.url).await?;
    Ok((StatusCode::CREATED, Json(webhook)))
}
