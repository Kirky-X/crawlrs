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
    Json,
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

#[cfg(test)]
mod tests {
    use super::*;

    // ========== AuditLogsQuery deserialization tests ==========

    #[test]
    fn test_audit_logs_query_empty() {
        let query = serde_urlencoded::from_str::<AuditLogsQuery>("").unwrap();
        assert!(query.limit.is_none());
        assert!(query.offset.is_none());
        assert!(query.api_key_id.is_none());
        assert!(query.team_id.is_none());
    }

    #[test]
    fn test_audit_logs_query_with_limit_and_offset() {
        let query_str = "limit=50&offset=100";
        let query = serde_urlencoded::from_str::<AuditLogsQuery>(query_str).unwrap();
        assert_eq!(query.limit, Some(50));
        assert_eq!(query.offset, Some(100));
    }

    #[test]
    fn test_audit_logs_query_with_api_key_id() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let query_str = format!("api_key_id={}", uuid_str);
        let query = serde_urlencoded::from_str::<AuditLogsQuery>(&query_str).unwrap();
        assert!(query.api_key_id.is_some());
        assert_eq!(query.api_key_id.unwrap().to_string(), uuid_str);
    }

    #[test]
    fn test_audit_logs_query_with_team_id() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let query_str = format!("team_id={}", uuid_str);
        let query = serde_urlencoded::from_str::<AuditLogsQuery>(&query_str).unwrap();
        assert!(query.team_id.is_some());
    }

    #[test]
    fn test_audit_logs_query_with_all_params() {
        let uuid_str = "550e8400-e29b-41d4-a716-446655440000";
        let query_str = format!(
            "limit=10&offset=20&api_key_id={}&team_id={}",
            uuid_str, uuid_str
        );
        let query = serde_urlencoded::from_str::<AuditLogsQuery>(&query_str).unwrap();
        assert_eq!(query.limit, Some(10));
        assert_eq!(query.offset, Some(20));
        assert!(query.api_key_id.is_some());
        assert!(query.team_id.is_some());
    }

    #[test]
    fn test_audit_logs_query_invalid_uuid_fails() {
        let query_str = "api_key_id=not-a-uuid";
        let result = serde_urlencoded::from_str::<AuditLogsQuery>(query_str);
        assert!(result.is_err());
    }

    #[test]
    fn test_audit_logs_query_invalid_limit_fails() {
        let query_str = "limit=abc";
        let result = serde_urlencoded::from_str::<AuditLogsQuery>(query_str);
        assert!(result.is_err());
    }

    // ========== AuditLogsResponseDto serialization ==========

    #[test]
    fn test_audit_logs_response_dto_empty() {
        let dto: AuditLogsResponseDto<serde_json::Value> = AuditLogsResponseDto { logs: vec![] };
        let json = serde_json::to_string(&dto).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["logs"], serde_json::Value::Array(vec![]));
    }

    #[test]
    fn test_audit_logs_response_dto_with_string_logs() {
        let dto: AuditLogsResponseDto<&str> = AuditLogsResponseDto {
            logs: vec!["log entry 1", "log entry 2"],
        };
        let json = serde_json::to_string(&dto).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["logs"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["logs"][0], "log entry 1");
        assert_eq!(parsed["logs"][1], "log entry 2");
    }

    #[test]
    fn test_audit_logs_response_dto_with_json_logs() {
        let logs = vec![
            serde_json::json!({"action": "read", "resource": "task"}),
            serde_json::json!({"action": "write", "resource": "webhook"}),
        ];
        let dto = AuditLogsResponseDto { logs };
        let json = serde_json::to_string(&dto).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["logs"][0]["action"], "read");
        assert_eq!(parsed["logs"][1]["action"], "write");
    }

    // ========== DeniedRequestsResponseDto serialization ==========

    #[test]
    fn test_denied_requests_response_dto_empty() {
        let dto: DeniedRequestsResponseDto<serde_json::Value> = DeniedRequestsResponseDto {
            denied_requests: vec![],
        };
        let json = serde_json::to_string(&dto).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["denied_requests"], serde_json::Value::Array(vec![]));
    }

    #[test]
    fn test_denied_requests_response_dto_with_entries() {
        let dto = DeniedRequestsResponseDto {
            denied_requests: vec![
                serde_json::json!({"reason": "rate limited", "ip": "1.2.3.4"}),
                serde_json::json!({"reason": "forbidden", "ip": "5.6.7.8"}),
            ],
        };
        let json = serde_json::to_string(&dto).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["denied_requests"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["denied_requests"][0]["reason"], "rate limited");
        assert_eq!(parsed["denied_requests"][1]["reason"], "forbidden");
    }

    // ========== server_config constants ==========

    #[test]
    fn test_default_page_limit_value() {
        assert_eq!(server_config::DEFAULT_PAGE_LIMIT, 100);
    }

    #[test]
    fn test_max_page_limit_value() {
        assert_eq!(server_config::MAX_PAGE_LIMIT, 1000);
    }

    #[test]
    fn test_default_page_limit_less_than_max() {
        assert!(server_config::DEFAULT_PAGE_LIMIT < server_config::MAX_PAGE_LIMIT);
    }
}
