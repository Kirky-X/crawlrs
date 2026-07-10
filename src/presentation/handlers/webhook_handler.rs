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
            log::debug!(
                "Webhook URL passed SSRF validation url={} team_id={} resolved_ips={:?}",
                payload.url,
                team_id,
                validated.resolved_ips
            );
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use uuid::Uuid;

    // ========== CreateWebhookRequest tests ==========

    #[test]
    fn test_create_webhook_request_valid() {
        let json = r#"{"url":"https://example.com/webhook"}"#;
        let req: CreateWebhookRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.url, "https://example.com/webhook");
    }

    #[test]
    fn test_create_webhook_request_rejects_unknown_fields() {
        let json = r#"{"url":"https://example.com","extra":"field"}"#;
        let result: Result<CreateWebhookRequest, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_webhook_request_serialization() {
        let req = CreateWebhookRequest {
            url: "https://example.com/hook".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["url"], "https://example.com/hook");
    }

    #[test]
    fn test_create_webhook_request_round_trip() {
        let original = CreateWebhookRequest {
            url: "https://my.webhook.site/abc123".to_string(),
        };
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: CreateWebhookRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.url, original.url);
    }

    // ========== Webhook to WebhookResponse mapping ==========

    #[test]
    fn test_webhook_to_response_mapping() {
        let webhook_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let webhook = Webhook {
            id: webhook_id,
            team_id,
            url: "https://example.com/hook".to_string(),
            created_at: Utc::now(),
        };
        let response = WebhookResponse {
            id: webhook.id,
            team_id: webhook.team_id,
            url: webhook.url.clone(),
            created_at: webhook.created_at,
            is_active: true,
            secret: None,
        };
        assert_eq!(response.id, webhook_id);
        assert_eq!(response.team_id, team_id);
        assert_eq!(response.url, "https://example.com/hook");
        assert!(response.is_active);
        assert!(response.secret.is_none());
    }

    #[test]
    fn test_webhook_response_serialization() {
        let response = WebhookResponse {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            url: "https://example.com/hook".to_string(),
            created_at: Utc::now(),
            is_active: true,
            secret: Some("secret123".to_string()),
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["url"], "https://example.com/hook");
        assert_eq!(parsed["is_active"], true);
        assert_eq!(parsed["secret"], "secret123");
    }

    #[test]
    fn test_webhook_response_secret_none_serialized() {
        let response = WebhookResponse {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            url: "https://example.com/hook".to_string(),
            created_at: Utc::now(),
            is_active: false,
            secret: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["is_active"], false);
        assert!(parsed["secret"].is_null());
    }

    // ========== WebhookListResponse serialization ==========

    #[test]
    fn test_webhook_list_response_empty() {
        let response = WebhookListResponse {
            webhooks: vec![],
            total: 0,
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["total"], 0);
        assert_eq!(parsed["webhooks"], serde_json::Value::Array(vec![]));
    }

    #[test]
    fn test_webhook_list_response_with_items() {
        let webhook1 = WebhookResponse {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            url: "https://hook1.example.com".to_string(),
            created_at: Utc::now(),
            is_active: true,
            secret: None,
        };
        let webhook2 = WebhookResponse {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            url: "https://hook2.example.com".to_string(),
            created_at: Utc::now(),
            is_active: false,
            secret: None,
        };
        let response = WebhookListResponse {
            webhooks: vec![webhook1, webhook2],
            total: 2,
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["total"], 2);
        assert_eq!(parsed["webhooks"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["webhooks"][0]["url"], "https://hook1.example.com");
        assert_eq!(parsed["webhooks"][1]["url"], "https://hook2.example.com");
    }

    #[test]
    fn test_webhook_list_response_total_matches_count() {
        let webhooks: Vec<WebhookResponse> = (0..5)
            .map(|_| WebhookResponse {
                id: Uuid::new_v4(),
                team_id: Uuid::new_v4(),
                url: "https://example.com".to_string(),
                created_at: Utc::now(),
                is_active: true,
                secret: None,
            })
            .collect();
        let count = webhooks.len();
        let response = WebhookListResponse {
            webhooks,
            total: count,
        };
        assert_eq!(response.total, 5);
        assert_eq!(response.webhooks.len(), 5);
    }

    // ========== Webhook model construction ==========

    #[test]
    fn test_webhook_new_constructor() {
        let id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let webhook = Webhook::new(id, team_id, "https://example.com/hook".to_string());
        assert_eq!(webhook.id, id);
        assert_eq!(webhook.team_id, team_id);
        assert_eq!(webhook.url, "https://example.com/hook");
        assert!(webhook.created_at <= Utc::now());
    }
}
