// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::application::dto::webhook_request::{
    CreateWebhookRequest, WebhookListResponse, WebhookResponse,
};
use crate::domain::models::Webhook;
use crate::domain::repositories::webhook_repository::WebhookRepository;
use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::domain::use_cases::create_webhook::CreateWebhookUseCase;
use crate::engines::validators::validate_url;
use crate::presentation::errors::AppError;
use crate::presentation::handlers::response_builder::ApiResponse;
use crate::presentation::helpers::rate_limit_helper::check_rate_limit_as_app_error;
use crate::presentation::middleware::auth_middleware::AuthState;
use axum::{http::StatusCode, Extension, Json};
use std::sync::Arc;

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
        Ok(validated) => {
            log::debug!("Webhook URL passed SSRF validation url={} team_id={} resolved_ips={:?}", payload.url, team_id, validated.resolved_ips);
        }
        Err(e) => {
            log::warn!("SSRF attack attempt blocked via webhook URL url={} team_id={} api_key_id={} error={}", payload.url, team_id, auth_state.api_key_id, e);
            return Err(AppError::Validation(
                "Invalid webhook URL: potential security risk detected".to_string(),
            ));
        }
    }

    // 1. 检查限流
    check_rate_limit_as_app_error(rate_limiting_service.as_ref(), &api_key, "/v1/webhooks").await?;

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
