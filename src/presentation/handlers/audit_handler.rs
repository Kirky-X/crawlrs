// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::services::audit_service::AuditService;
use crate::presentation::middleware::auth_middleware::AuthState;
use axum::{
    extract::{Extension, Query},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct AuditLogsQuery {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub api_key_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
}

pub async fn get_audit_logs(
    Extension(audit_service): Extension<Arc<AuditService>>,
    Extension(auth_state): Extension<AuthState>,
    Query(query): Query<AuditLogsQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(100).min(1000);
    let offset = query.offset.unwrap_or(0);

    match query {
        AuditLogsQuery {
            api_key_id: Some(api_key_id),
            team_id: _,
            ..
        } => {
            match audit_service
                .get_logs_for_key(api_key_id, limit, offset)
                .await
            {
                Ok(logs) => (StatusCode::OK, Json(json!({ "logs": logs }))),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                ),
            }
        }
        AuditLogsQuery {
            team_id: Some(team_id),
            api_key_id: _,
            ..
        } => {
            match audit_service
                .get_logs_for_team(team_id, limit, offset)
                .await
            {
                Ok(logs) => (StatusCode::OK, Json(json!({ "logs": logs }))),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                ),
            }
        }
        _ => {
            match audit_service
                .get_logs_for_key(auth_state.api_key_id, limit, offset)
                .await
            {
                Ok(logs) => (StatusCode::OK, Json(json!({ "logs": logs }))),
                Err(e) => (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({ "error": e.to_string() })),
                ),
            }
        }
    }
}

pub async fn get_denied_requests(
    Extension(audit_service): Extension<Arc<AuditService>>,
    Extension(auth_state): Extension<AuthState>,
    Query(query): Query<AuditLogsQuery>,
) -> impl IntoResponse {
    let limit = query.limit.unwrap_or(100).min(1000);

    match audit_service
        .get_denied_requests(auth_state.api_key_id, limit)
        .await
    {
        Ok(logs) => (StatusCode::OK, Json(json!({ "denied_requests": logs }))),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": e.to_string() })),
        ),
    }
}
