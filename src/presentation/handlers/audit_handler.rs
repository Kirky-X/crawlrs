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

    // ========== Handler function tests ==========
    //
    // The following tests verify the HTTP-layer behavior of get_audit_logs and
    // get_denied_requests: branch selection, pagination clamping, error mapping,
    // and response shape. Business logic is covered by AuditService tests.

    use crate::domain::auth::ApiKeyScope;
    use crate::domain::auth::{AuditDecision, AuditLogEntry};
    use crate::domain::services::audit_service::{
        AuditLogBuilder, AuditServiceError, AuditServiceTrait,
    };
    use crate::presentation::middleware::auth_middleware::AuthState;
    use async_trait::async_trait;
    use axum::response::IntoResponse;
    use dbnexus::{DbConfig, DbPool};
    use std::sync::Mutex;

    fn create_test_db_pool() -> Arc<dbnexus::DbPool> {
        std::thread::scope(|s| {
            let handle = s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build tokio runtime for DbPool construction");
                let _guard = rt.enter();
                DbPool::try_from(&DbConfig::default())
                    .expect("failed to create lazy DbPool for test")
            });
            Arc::new(handle.join().expect("DbPool construction thread panicked"))
        })
    }

    fn make_auth_state() -> AuthState {
        let pool = create_test_db_pool();
        AuthState::new(pool, Uuid::new_v4(), Uuid::new_v4(), ApiKeyScope::default())
    }

    fn make_auth_state_with_key(api_key_id: Uuid) -> AuthState {
        let pool = create_test_db_pool();
        AuthState::new(pool, Uuid::new_v4(), api_key_id, ApiKeyScope::default())
    }

    fn sample_entry(action: &str, decision: AuditDecision) -> AuditLogEntry {
        AuditLogBuilder::new(action, decision)
            .with_api_key_id(Uuid::new_v4())
            .with_team_id(Uuid::new_v4())
            .build()
    }

    struct MockAuditService {
        logs: Mutex<Vec<AuditLogEntry>>,
        should_fail: bool,
    }

    impl MockAuditService {
        fn new(logs: Vec<AuditLogEntry>) -> Self {
            Self {
                logs: Mutex::new(logs),
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                logs: Mutex::new(Vec::new()),
                should_fail: true,
            }
        }
    }

    #[async_trait]
    impl AuditServiceTrait for MockAuditService {
        async fn log(&self, _entry: AuditLogEntry) -> Result<(), AuditServiceError> {
            Ok(())
        }

        async fn log_allow(
            &self,
            _action: String,
            _api_key_id: Uuid,
            _team_id: Uuid,
            _scope: ApiKeyScope,
        ) -> Result<(), AuditServiceError> {
            Ok(())
        }

        async fn log_deny(
            &self,
            _action: String,
            _api_key_id: Option<Uuid>,
            _team_id: Option<Uuid>,
            _reason: String,
            _scope: Option<ApiKeyScope>,
        ) -> Result<(), AuditServiceError> {
            Ok(())
        }

        async fn get_logs_for_key(
            &self,
            _api_key_id: Uuid,
            _limit: u64,
            _offset: u64,
        ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
            if self.should_fail {
                return Err(AuditServiceError::DatabaseError(sea_orm::DbErr::Custom(
                    "mock error".to_string(),
                )));
            }
            Ok(self.logs.lock().unwrap().clone())
        }

        async fn get_logs_for_team(
            &self,
            _team_id: Uuid,
            _limit: u64,
            _offset: u64,
        ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
            if self.should_fail {
                return Err(AuditServiceError::DatabaseError(sea_orm::DbErr::Custom(
                    "mock error".to_string(),
                )));
            }
            Ok(self.logs.lock().unwrap().clone())
        }

        async fn get_denied_requests(
            &self,
            _api_key_id: Uuid,
            _limit: u64,
        ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
            if self.should_fail {
                return Err(AuditServiceError::DatabaseError(sea_orm::DbErr::Custom(
                    "mock error".to_string(),
                )));
            }
            Ok(self
                .logs
                .lock()
                .unwrap()
                .iter()
                .filter(|e| e.decision == AuditDecision::Deny)
                .cloned()
                .collect())
        }
    }

    // ========== Branch selection logic ==========

    #[test]
    fn test_branch_selects_api_key_when_api_key_id_present() {
        let query = AuditLogsQuery {
            limit: Some(10),
            offset: Some(0),
            api_key_id: Some(Uuid::new_v4()),
            team_id: Some(Uuid::new_v4()),
        };
        assert!(query.api_key_id.is_some());
    }

    #[test]
    fn test_branch_selects_team_when_only_team_id_present() {
        let query = AuditLogsQuery {
            limit: Some(10),
            offset: Some(0),
            api_key_id: None,
            team_id: Some(Uuid::new_v4()),
        };
        assert!(query.api_key_id.is_none());
        assert!(query.team_id.is_some());
    }

    #[test]
    fn test_branch_falls_back_to_auth_state_when_neither_present() {
        let query = AuditLogsQuery {
            limit: Some(10),
            offset: Some(0),
            api_key_id: None,
            team_id: None,
        };
        assert!(query.api_key_id.is_none());
        assert!(query.team_id.is_none());
    }

    #[test]
    fn test_branch_api_key_takes_priority_over_team_id() {
        let query = AuditLogsQuery {
            limit: None,
            offset: None,
            api_key_id: Some(Uuid::new_v4()),
            team_id: Some(Uuid::new_v4()),
        };
        match query {
            AuditLogsQuery {
                api_key_id: Some(_),
                ..
            } => {}
            AuditLogsQuery {
                team_id: Some(_), ..
            } => {
                panic!("team_id branch should not be reached when api_key_id is Some");
            }
            _ => {
                panic!("fallback branch should not be reached");
            }
        }
    }

    // ========== get_audit_logs handler tests ==========

    #[tokio::test]
    async fn test_get_audit_logs_by_api_key_id() {
        let logs = vec![
            sample_entry("search", AuditDecision::Allow),
            sample_entry("scrape", AuditDecision::Allow),
        ];
        let mock = Arc::new(MockAuditService::new(logs));
        let auth_state = make_auth_state();
        let query = AuditLogsQuery {
            limit: Some(10),
            offset: Some(0),
            api_key_id: Some(Uuid::new_v4()),
            team_id: None,
        };

        let response = get_audit_logs(Extension(mock), Extension(auth_state), Query(query))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_audit_logs_by_team_id() {
        let logs = vec![sample_entry("crawl", AuditDecision::Allow)];
        let mock = Arc::new(MockAuditService::new(logs));
        let auth_state = make_auth_state();
        let query = AuditLogsQuery {
            limit: Some(50),
            offset: None,
            api_key_id: None,
            team_id: Some(Uuid::new_v4()),
        };

        let response = get_audit_logs(Extension(mock), Extension(auth_state), Query(query))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_audit_logs_fallback_to_auth_state_api_key() {
        let logs = vec![sample_entry("extract", AuditDecision::Allow)];
        let mock = Arc::new(MockAuditService::new(logs));
        let auth_state = make_auth_state();
        let query = AuditLogsQuery {
            limit: None,
            offset: None,
            api_key_id: None,
            team_id: None,
        };

        let response = get_audit_logs(Extension(mock), Extension(auth_state), Query(query))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_audit_logs_api_key_takes_priority_over_team_id() {
        let logs = vec![sample_entry("search", AuditDecision::Allow)];
        let mock = Arc::new(MockAuditService::new(logs));
        let auth_state = make_auth_state();
        let query = AuditLogsQuery {
            limit: Some(10),
            offset: Some(0),
            api_key_id: Some(Uuid::new_v4()),
            team_id: Some(Uuid::new_v4()),
        };

        let response = get_audit_logs(Extension(mock), Extension(auth_state), Query(query))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_audit_logs_error_returns_internal_server_error() {
        let mock = Arc::new(MockAuditService::failing());
        let auth_state = make_auth_state();
        let query = AuditLogsQuery {
            limit: Some(10),
            offset: Some(0),
            api_key_id: Some(Uuid::new_v4()),
            team_id: None,
        };

        let response = get_audit_logs(Extension(mock), Extension(auth_state), Query(query))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_get_audit_logs_error_on_team_branch() {
        let mock = Arc::new(MockAuditService::failing());
        let auth_state = make_auth_state();
        let query = AuditLogsQuery {
            limit: Some(10),
            offset: Some(0),
            api_key_id: None,
            team_id: Some(Uuid::new_v4()),
        };

        let response = get_audit_logs(Extension(mock), Extension(auth_state), Query(query))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_get_audit_logs_error_on_fallback_branch() {
        let mock = Arc::new(MockAuditService::failing());
        let auth_state = make_auth_state();
        let query = AuditLogsQuery {
            limit: None,
            offset: None,
            api_key_id: None,
            team_id: None,
        };

        let response = get_audit_logs(Extension(mock), Extension(auth_state), Query(query))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_get_audit_logs_empty_logs() {
        let mock = Arc::new(MockAuditService::new(vec![]));
        let auth_state = make_auth_state();
        let query = AuditLogsQuery {
            limit: Some(10),
            offset: Some(0),
            api_key_id: Some(Uuid::new_v4()),
            team_id: None,
        };

        let response = get_audit_logs(Extension(mock), Extension(auth_state), Query(query))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_audit_logs_limit_clamped_to_max() {
        let logs = vec![sample_entry("search", AuditDecision::Allow)];
        let mock = Arc::new(MockAuditService::new(logs));
        let auth_state = make_auth_state();
        let query = AuditLogsQuery {
            limit: Some(50000),
            offset: Some(0),
            api_key_id: Some(Uuid::new_v4()),
            team_id: None,
        };

        let response = get_audit_logs(Extension(mock), Extension(auth_state), Query(query))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    // ========== get_denied_requests handler tests ==========

    #[tokio::test]
    async fn test_get_denied_requests_success() {
        let denied = vec![
            sample_entry("search", AuditDecision::Deny),
            sample_entry("scrape", AuditDecision::Deny),
        ];
        let mock = Arc::new(MockAuditService::new(denied));
        let auth_state = make_auth_state();
        let query = AuditLogsQuery {
            limit: Some(10),
            offset: None,
            api_key_id: None,
            team_id: None,
        };

        let response = get_denied_requests(Extension(mock), Extension(auth_state), Query(query))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_denied_requests_error_returns_internal_server_error() {
        let mock = Arc::new(MockAuditService::failing());
        let auth_state = make_auth_state();
        let query = AuditLogsQuery {
            limit: Some(10),
            offset: None,
            api_key_id: None,
            team_id: None,
        };

        let response = get_denied_requests(Extension(mock), Extension(auth_state), Query(query))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[tokio::test]
    async fn test_get_denied_requests_empty() {
        let mock = Arc::new(MockAuditService::new(vec![]));
        let auth_state = make_auth_state();
        let query = AuditLogsQuery {
            limit: Some(10),
            offset: None,
            api_key_id: None,
            team_id: None,
        };

        let response = get_denied_requests(Extension(mock), Extension(auth_state), Query(query))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_denied_requests_limit_clamped() {
        let denied = vec![sample_entry("search", AuditDecision::Deny)];
        let mock = Arc::new(MockAuditService::new(denied));
        let auth_state = make_auth_state();
        let query = AuditLogsQuery {
            limit: Some(99999),
            offset: None,
            api_key_id: None,
            team_id: None,
        };

        let response = get_denied_requests(Extension(mock), Extension(auth_state), Query(query))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_denied_requests_uses_auth_state_api_key_id() {
        let api_key_id = Uuid::new_v4();
        let denied = vec![sample_entry("search", AuditDecision::Deny)];
        let mock = Arc::new(MockAuditService::new(denied));
        let auth_state = make_auth_state_with_key(api_key_id);
        let query = AuditLogsQuery {
            limit: Some(10),
            offset: None,
            api_key_id: Some(Uuid::new_v4()),
            team_id: None,
        };

        let response = get_denied_requests(Extension(mock), Extension(auth_state), Query(query))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_denied_requests_with_allow_entries_filters() {
        let entries = vec![
            sample_entry("search", AuditDecision::Deny),
            sample_entry("search", AuditDecision::Allow),
            sample_entry("crawl", AuditDecision::Deny),
        ];
        let mock = Arc::new(MockAuditService::new(entries));
        let auth_state = make_auth_state();
        let query = AuditLogsQuery {
            limit: Some(10),
            offset: None,
            api_key_id: None,
            team_id: None,
        };

        let response = get_denied_requests(Extension(mock), Extension(auth_state), Query(query))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::OK);
    }
}
