// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 认证共享类型（R-auth-engine-003 / Stage 3 重构）。
//!
//! ## 职责
//!
//! 抽出 `AuthError` 与 `AuthState` 解决 `auth_bridge` ↔ `auth_middleware` 之间的循环依赖
//! （决策 3：抽出 auth_types 模块）。两个模块都从此处导入共享类型，避免互相 `use`。
//!
//! ## AuthState DTO 化（决策 1 + 决策 2）
//!
//! `AuthState` 仅保留 4 个字段：`pool` / `team_id` / `api_key_id` / `scope`。
//! - `api_key_cache` 字段删除：原 `ApiKeyCache` 在 Stage 3 重写后仅缓存 `team_id`，
//!   改由 `auth_middleware::TEAM_ID_CACHE`（独立 LRU）承担，不再挂在 AuthState 上。
//! - `auth_rate_limiter` 字段删除：决策 2 要求仅依赖 garrison firewall，本地
//!   `AuthRateLimiter` 在 Stage 4 删除。
//! - `trusted_proxies` 字段删除：CWE-307 IP 限速改由 garrison firewall 承担，
//!   crawlrs 侧不再需要 trusted proxy 配置做 client IP 提取。
//!
//! ## Spec
//!
//! - R-auth-engine-003 / T015：DTO 化（移除 `auth_scope_service`/`api_key_cache`/
//!   `auth_rate_limiter`/`trusted_proxies` 字段）
//! - R-auth-engine-003 / T016：`AuthError::from_garrison` 错误映射

use crate::domain::auth::{ApiKeyScope, ScopePermission};
use dbnexus::DbPool;
use std::sync::Arc;
use uuid::Uuid;

/// 认证错误类型。
///
/// # garrison 错误映射（R-auth-engine-003 / T016）
///
/// `auth` feature 启用时，`AuthError::from_garrison(GarrisonError)` 复用
/// `GarrisonError::response_parts()` 获取 `(status, error_code, message)`，
/// 按 HTTP 状态码映射到对应变体：
///
/// | garrison 状态码 | error_code 示例 | AuthError 变体 |
/// |----------------|-----------------|----------------|
/// | 401 | NOT_LOGIN / INVALID_TOKEN / TOKEN_REVOKED / EXPIRED_TOKEN | `InvalidKey` |
/// | 403 NOT_PERMISSION / NOT_ROLE / FIREWALL_BLOCKED / SMS_CHANNEL_RECYCLED | `Forbidden` |
/// | 403 DISABLE_SERVICE | `InactiveKey` |
/// | 429 SMS_RATE_LIMIT_EXCEEDED | `RateLimited` |
/// | 500 DAO_ERROR / CONFIG_ERROR / INTERNAL_ERROR / SESSION_ERROR / ... | `InternalError` |
/// | 502 NETWORK_ERROR | `NetworkError` |
/// | 501 NOT_IMPLEMENTED | `NotImplemented` |
/// | 400 INVALID_PARAM / NOT_SAFE / SMS_VERIFY_MAX_ATTEMPTS / SMS_CODE_NOT_FOUND | `InvalidParam` |
#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error("Invalid or missing API key")]
    InvalidKey,
    #[error("API key is inactive")]
    InactiveKey,
    #[error("Missing required scope: {0}")]
    MissingScope(ScopePermission),
    #[error("Database error: {0}")]
    DatabaseError(#[from] sea_orm::DbErr),
    #[error("API key associated with nil team_id")]
    NilTeamId,
    #[error("API key has expired")]
    ExpiredKey,
    /// garrison 返回的 `login_id` 无法解析为 `Uuid`（design.md §5 约定 login_id = api_key_id 的 Uuid 字符串）。
    #[error("Invalid login_id from garrison: {0}")]
    InvalidLoginId(String),
    /// `api_key_id` 反查 crawlrs `api_keys` 表未命中（key 已被 garrison 吊销但 crawlrs 仍保留映射）。
    #[error("API key not found in crawlrs mapping: {0}")]
    KeyNotFound(Uuid),
    /// garrison 触发限速（429，对应 `SmsRateLimitExceeded`）。
    #[error("Rate limited by garrison")]
    RateLimited,
    /// garrison 拒绝授权（403，对应 `NotPermission` / `NotRole` / `FirewallBlocked` / `SmsChannelRecycled`）。
    #[error("Forbidden by garrison: {0}")]
    Forbidden(String),
    /// garrison 网络错误（502，对应 `Network`）。
    #[error("Garrison network error: {0}")]
    NetworkError(String),
    /// garrison 内部错误（500，对应 `Dao` / `Config` / `Internal` / `Session` / `Annotation` / `Context` / `OAuth2` / `InvalidStateTransition`）。
    #[error("Garrison internal error: {0}")]
    InternalError(String),
    /// garrison 未实现（501，对应 `NotImplemented`）。
    #[error("Garrison not implemented: {0}")]
    NotImplemented(String),
    /// garrison 参数无效（400，对应 `InvalidParam` / `NotSafe` / `SmsVerifyMaxAttempts` / `SmsCodeNotFound`）。
    #[error("Invalid param to garrison: {0}")]
    InvalidParam(String),
}

#[cfg(feature = "auth")]
impl AuthError {
    /// garrison 错误 → crawlrs `AuthError` 转换（R-auth-engine-003 / T016 方案 A）。
    ///
    /// 复用 `GarrisonError::response_parts()` 获取 `(status, error_code, message)`，
    /// 按 HTTP 状态码映射到对应 `AuthError` 变体。`message` 字段保留 garrison
    /// 通用错误消息（不泄露内部细节），通过 `log::error!` 记录完整错误。
    ///
    /// # 映射规则
    ///
    /// | status | error_code | AuthError 变体 |
    /// |--------|------------|---------------|
    /// | 401 | NOT_LOGIN / INVALID_TOKEN / TOKEN_REVOKED / EXPIRED_TOKEN / Exception(code=-1) | `InvalidKey` |
    /// | 403 | NOT_PERMISSION / NOT_ROLE / FIREWALL_BLOCKED / SMS_CHANNEL_RECYCLED / Exception(code=-2) | `Forbidden` |
    /// | 403 | DISABLE_SERVICE | `InactiveKey` |
    /// | 429 | SMS_RATE_LIMIT_EXCEEDED | `RateLimited` |
    /// | 500 | DAO_ERROR / CONFIG_ERROR / INTERNAL_ERROR / SESSION_ERROR / ANNOTATION_ERROR / CONTEXT_ERROR / OAUTH2_ERROR / INVALID_STATE_TRANSITION / Exception(其他) | `InternalError` |
    /// | 502 | NETWORK_ERROR | `NetworkError` |
    /// | 501 | NOT_IMPLEMENTED | `NotImplemented` |
    /// | 400 | INVALID_PARAM / NOT_SAFE / SMS_VERIFY_MAX_ATTEMPTS / SMS_CODE_NOT_FOUND | `InvalidParam` |
    /// | 其他 | — | `InternalError`（fail-safe，归为内部错误） |
    pub fn from_garrison(err: garrison::error::GarrisonError) -> Self {
        let (status, error_code, message, _ex_code) = err.response_parts();
        // MEDIUM-2 修复：4xx 降级为 warn（攻击者刷接口常见响应），5xx 保留 error（真正的内部错误）
        if status >= 500 {
            log::error!(
                "garrison internal error: status={}, error_code={}, message={}",
                status,
                error_code,
                message
            );
        } else {
            log::warn!(
                "garrison auth rejected: status={}, error_code={}, message={}",
                status,
                error_code,
                message
            );
        }
        match status {
            401 => AuthError::InvalidKey,
            403 => match error_code {
                "DISABLE_SERVICE" => AuthError::InactiveKey,
                _ => AuthError::Forbidden(error_code.to_string()),
            },
            429 => AuthError::RateLimited,
            502 => AuthError::NetworkError(error_code.to_string()),
            501 => AuthError::NotImplemented(error_code.to_string()),
            400 => AuthError::InvalidParam(error_code.to_string()),
            _ => AuthError::InternalError(error_code.to_string()),
        }
    }
}

impl axum::response::IntoResponse for AuthError {
    /// 将 `AuthError` 转换为 HTTP 响应（R-auth-engine-003 / T016）。
    ///
    /// # 状态码映射
    ///
    /// | AuthError 变体 | HTTP 状态码 |
    ///|---------------|-------------|
    /// | `InvalidKey` / `ExpiredKey` / `NilTeamId` | 401 Unauthorized |
    /// | `InactiveKey` / `MissingScope` / `Forbidden` | 403 Forbidden |
    /// | `RateLimited` | 429 Too Many Requests |
    /// | `InvalidLoginId` / `KeyNotFound` / `InvalidParam` | 400 Bad Request |
    /// | `DatabaseError` / `InternalError` / `NetworkError` / `NotImplemented` | 500 Internal Server Error |
    ///
    /// # 安全（CWE-209 信息泄露防护，MEDIUM-2 修复）
    ///
    /// 不向客户端透传 garrison `error_code` / `error_message` / 内部 Uuid / DbErr 等敏感信息，
    /// 避免攻击者推断后端架构（"使用 garrison"、"DB 层错误"等）。仅返回状态码对应的通用消息。
    /// 详细错误已通过 `from_garrison` 的 `log::warn!` / `log::error!` 入日志。
    fn into_response(self) -> axum::response::Response {
        use axum::http::StatusCode;
        let status = match &self {
            AuthError::InvalidKey | AuthError::ExpiredKey | AuthError::NilTeamId => {
                StatusCode::UNAUTHORIZED
            }
            AuthError::InactiveKey | AuthError::MissingScope(_) | AuthError::Forbidden(_) => {
                StatusCode::FORBIDDEN
            }
            AuthError::RateLimited => StatusCode::TOO_MANY_REQUESTS,
            AuthError::InvalidLoginId(_)
            | AuthError::KeyNotFound(_)
            | AuthError::InvalidParam(_) => StatusCode::BAD_REQUEST,
            AuthError::DatabaseError(_)
            | AuthError::InternalError(_)
            | AuthError::NetworkError(_)
            | AuthError::NotImplemented(_) => StatusCode::INTERNAL_SERVER_ERROR,
        };
        // 仅返回状态码对应的 canonical reason，不暴露内部细节（CWE-209 防护）
        (status, status.canonical_reason().unwrap_or("Error")).into_response()
    }
}

/// 注入到请求 extensions 的认证状态（DTO，R-auth-engine-003 / T015）。
///
/// # 字段
///
/// 仅保留 4 个字段（Stage 3 DTO 化决策）：
/// - `pool`：crawlrs 数据库连接池（下游 handler 可能需要）
/// - `team_id`：API Key 所属团队
/// - `api_key_id`：API Key ID（审计日志、特性开关）
/// - `scope`：API Key 权限范围（garrison RBAC → `map_perms_to_scope` 填充）
///
/// # Security
///
/// scope 由 garrison RBAC 经 `map_perms_to_scope` 确定，不再从 crawlrs DB 加载，
/// 避免 crawlrs 侧权限修改滞后导致越权（garrison 是权限单一来源）。
#[derive(Clone)]
pub struct AuthState {
    /// Database pool for additional queries
    pub pool: Arc<DbPool>,
    /// Team ID associated with the API key
    pub team_id: Uuid,
    /// API Key ID for audit logging and feature flags
    pub api_key_id: Uuid,
    /// Scope permissions for the API key
    pub scope: ApiKeyScope,
}

impl AuthState {
    /// Create a new AuthState with required fields.
    pub fn new(pool: Arc<DbPool>, team_id: Uuid, api_key_id: Uuid, scope: ApiKeyScope) -> Self {
        Self {
            pool,
            team_id,
            api_key_id,
            scope,
        }
    }
}

impl std::fmt::Debug for AuthState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // LOW-3 修复：显式占位 pool 字段（避免维护者不知道还有 pool 字段）。
        // 不输出 pool 内部细节（避免泄露连接串）。
        f.debug_struct("AuthState")
            .field("team_id", &self.team_id)
            .field("api_key_id", &self.api_key_id)
            .field("scope", &self.scope)
            .field("pool", &"<DbPool>")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::response::IntoResponse;

    /// AuthState::new 应正确填充所有字段。
    #[test]
    fn test_auth_state_new_populates_fields() {
        let pool = crate::common::test_helpers::create_test_db_pool();
        let team_id = Uuid::new_v4();
        let api_key_id = Uuid::new_v4();
        let scope = ApiKeyScope::full_access();

        let state = AuthState::new(pool.clone(), team_id, api_key_id, scope.clone());

        assert_eq!(state.team_id, team_id);
        assert_eq!(state.api_key_id, api_key_id);
        assert_eq!(state.scope, scope);
    }

    /// AuthState 应可 Clone（请求 extensions 注入需要）。
    #[test]
    fn test_auth_state_is_clone() {
        let pool = crate::common::test_helpers::create_test_db_pool();
        let state = AuthState::new(pool, Uuid::new_v4(), Uuid::new_v4(), ApiKeyScope::default());
        let cloned = state.clone();
        assert_eq!(state.team_id, cloned.team_id);
        assert_eq!(state.api_key_id, cloned.api_key_id);
    }

    /// AuthState Debug 输出不应包含 pool 内部细节（避免泄露连接串）。
    /// LOW-3 修复后：pool 字段显式占位为 "<DbPool>"，但不应包含连接串/数据库 URL 等内部细节。
    #[test]
    fn test_auth_state_debug_does_not_leak_pool_internals() {
        let pool = crate::common::test_helpers::create_test_db_pool();
        let state = AuthState::new(pool, Uuid::new_v4(), Uuid::new_v4(), ApiKeyScope::default());
        let debug = format!("{:?}", state);
        assert!(debug.contains("AuthState"));
        assert!(debug.contains("team_id"));
        // pool 字段显式占位为 "<DbPool>"，但不应包含数据库 URL / 连接串等内部细节
        assert!(debug.contains("pool") || debug.contains("DbPool"));
        assert!(!debug.contains("postgres"));
        assert!(!debug.contains("postgresql://"));
    }

    /// AuthError::InvalidKey 应转换为 401。
    #[test]
    fn test_auth_error_invalid_key_is_unauthorized() {
        use axum::http::StatusCode;
        let response = AuthError::InvalidKey.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    /// AuthError::Forbidden 应转换为 403。
    #[test]
    fn test_auth_error_forbidden_is_forbidden() {
        use axum::http::StatusCode;
        let response = AuthError::Forbidden("NOT_PERMISSION".to_string()).into_response();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    /// AuthError::RateLimited 应转换为 429。
    #[test]
    fn test_auth_error_rate_limited_is_too_many_requests() {
        use axum::http::StatusCode;
        let response = AuthError::RateLimited.into_response();
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    /// AuthError::InvalidLoginId 应转换为 400。
    #[test]
    fn test_auth_error_invalid_login_id_is_bad_request() {
        use axum::http::StatusCode;
        let response = AuthError::InvalidLoginId("not-a-uuid".to_string()).into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    /// AuthError::InternalError 应转换为 500。
    #[test]
    fn test_auth_error_internal_error_is_server_error() {
        use axum::http::StatusCode;
        let response = AuthError::InternalError("DAO_ERROR".to_string()).into_response();
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
