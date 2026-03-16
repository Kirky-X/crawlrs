// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::common::constants::server_config;
use crate::domain::services::audit_service::AuditServiceTrait;
use crate::presentation::handlers::response_builder::{error_response, ApiResponse};
use crate::presentation::middleware::auth_middleware::AuthState;
use axum::{
    extract::{Extension, Query},
    http::StatusCode,
    response::IntoResponse,
};
use serde::Deserialize;
use std::sync::Arc;
use uuid::Uuid;

#[derive(Deserialize)]
pub struct AuditLogsQuery {
    pub limit: Option<u64>,
    pub offset: Option<u64>,
    pub api_key_id: Option<Uuid>,
    pub team_id: Option<Uuid>,
}

/// 审计日志响应数据传输对象
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AuditLogsResponseDto<T> {
    /// 日志列表
    pub logs: Vec<T>,
}

/// 拒绝请求响应数据传输对象
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DeniedRequestsResponseDto<T> {
    /// 拒绝请求列表
    pub denied_requests: Vec<T>,
}

pub async fn get_audit_logs(
    Extension(audit_service): Extension<Arc<dyn AuditServiceTrait>>,
    Extension(auth_state): Extension<AuthState>,
    Query(query): Query<AuditLogsQuery>,
) -> impl IntoResponse {
    let limit = query
        .limit
        .unwrap_or(server_config::DEFAULT_PAGE_LIMIT as u64)
        .min(server_config::MAX_PAGE_LIMIT as u64);
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
                Ok(logs) => {
                    let response = AuditLogsResponseDto { logs };
                    (StatusCode::OK, Json(ApiResponse::success(response))).into_response()
                }
                Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
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
                Ok(logs) => {
                    let response = AuditLogsResponseDto { logs };
                    (StatusCode::OK, Json(ApiResponse::success(response))).into_response()
                }
                Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            }
        }
        _ => {
            match audit_service
                .get_logs_for_key(auth_state.api_key_id, limit, offset)
                .await
            {
                Ok(logs) => {
                    let response = AuditLogsResponseDto { logs };
                    (StatusCode::OK, Json(ApiResponse::success(response))).into_response()
                }
                Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
            }
        }
    }
}

pub async fn get_denied_requests(
    Extension(audit_service): Extension<Arc<dyn AuditServiceTrait>>,
    Extension(auth_state): Extension<AuthState>,
    Query(query): Query<AuditLogsQuery>,
) -> impl IntoResponse {
    let limit = query
        .limit
        .unwrap_or(server_config::DEFAULT_PAGE_LIMIT as u64)
        .min(server_config::MAX_PAGE_LIMIT as u64);

    match audit_service
        .get_denied_requests(auth_state.api_key_id, limit)
        .await
    {
        Ok(logs) => {
            let response = DeniedRequestsResponseDto {
                denied_requests: logs,
            };
            (StatusCode::OK, Json(ApiResponse::success(response))).into_response()
        }
        Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()),
    }
}
