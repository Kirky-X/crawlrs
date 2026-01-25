// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::application::dto::webhook_request::{
    CreateWebhookRequest, WebhookListResponse, WebhookResponse,
};
use crate::domain::models::webhook::Webhook;
use crate::domain::repositories::webhook_repository::WebhookRepository;
use crate::domain::services::rate_limiting_service::{RateLimitResult, RateLimitingService};
use crate::domain::use_cases::create_webhook::CreateWebhookUseCase;
use crate::engines::validators::validate_url;
use crate::presentation::errors::AppError;
use crate::presentation::handlers::response_builder::ApiResponse;
use crate::presentation::middleware::auth_middleware::AuthState;
use axum::{http::StatusCode, Extension, Json};
use std::sync::Arc;
use tracing::error;

pub async fn create_webhook<R: WebhookRepository>(
    Extension(repo): Extension<Arc<R>>,
    Extension(rate_limiting_service): Extension<Arc<dyn RateLimitingService>>,
    Extension(auth_state): Extension<AuthState>,
    Json(payload): Json<CreateWebhookRequest>,
) -> Result<(StatusCode, Json<Webhook>), AppError> {
    let team_id = auth_state.team_id;
    let api_key = auth_state.api_key_id.to_string();

    // Validate webhook URL for SSRF protection
    match validate_url(&payload.url).await {
        Ok(_) => {}
        Err(e) => {
            tracing::warn!("Webhook URL validation failed for team {}: {}", team_id, e);
            return Err(AppError::Validation(
                "Invalid webhook URL: potential security risk detected".to_string(),
            ));
        }
    }

    // 1. 检查限流
    match rate_limiting_service
        .check_rate_limit(&api_key, "/v1/webhooks")
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

/// 列出团队的 Webhooks
pub async fn list_webhooks<R: WebhookRepository>(
    Extension(repo): Extension<Arc<R>>,
    Extension(auth_state): Extension<AuthState>,
) -> Result<Json<ApiResponse<WebhookListResponse>>, AppError> {
    let team_id = auth_state.team_id;
    let webhooks = repo.find_by_team_id(team_id).await?;
    let webhook_responses: Vec<WebhookResponse> = webhooks
        .into_iter()
        .map(|w| WebhookResponse {
            id: w.id,
            team_id: w.team_id,
            url: w.url,
            created_at: w.created_at,
            is_active: true,
            secret: None,
        })
        .collect();
    let total = webhook_responses.len();
    Ok(Json(ApiResponse::success(WebhookListResponse {
        webhooks: webhook_responses,
        total,
    })))
}
