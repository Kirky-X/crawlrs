// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! API Key 签发 handler（R-key-lifecycle-001 / T027-3）。
//!
//! ## 职责
//!
//! 接收 `POST /v1/admin/api-keys` 请求，调用 garrison `ApiKeyHandler::generate_with_namespace`
//! 签发新的 API Key（双段格式 `key_id.key_secret`），同时写入 crawlrs `api_keys` 表保留
//! `id`（api_key_id）/`team_id` 映射，供认证中间件反查。
//!
//! ## 设计
//!
//! - 明文 key 仅在响应中返回一次（CWE-916：garrison 不存明文 secret）
//! - garrison 管理哈希存储，crawlrs 的 `key_hash` 字段弃用（设为 `None`）
//! - crawlrs 的 `key` 字段存储 garrison `key_id`（公开标识，可安全记录到日志）
//! - 调用方必须持有 admin 权限（CWE-862 IDOR 防护）
//! - login_id = api_key_id 的 Uuid 字符串形式（design.md §5 约定，供中间件反向解析）
//!
//! ## Spec
//!
//! - R-key-lifecycle-001：签发路径改调 garrison ApiKeyHandler

use crate::common::time_utils;
use crate::domain::auth::ScopePermission;
use crate::infrastructure::auth::get_garrison_dao;
use crate::infrastructure::database::entities::api_key::{ActiveModel, Entity};
use crate::infrastructure::database::entities::team::Entity as TeamEntity;
use crate::presentation::handlers::response_builder::{error_response, ApiResponse};
use crate::presentation::middleware::auth_middleware::AuthState;
use axum::{
    extract::Extension,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use chrono::Utc;
use sea_orm::EntityTrait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// garrison namespace，所有 crawlrs 签发的 API Key 都归属此 namespace。
const GARRISON_NAMESPACE: &str = "crawlrs";

/// 默认过期时间（30 天，秒）。
const DEFAULT_EXPIRES_IN_SECS: i64 = 30 * 24 * 60 * 60;

/// 最大过期时间（1 年，秒）。CWE-208：拒绝异常长 TTL 防止长期未轮换 key。
const MAX_EXPIRES_IN_SECS: i64 = 365 * 24 * 60 * 60;

/// 请求 scopes 数量上限。CWE-208：拒绝超大 scopes 数组导致 garrison 调用耗时增加。
const MAX_SCOPES_LEN: usize = 10;

/// crawlrs scope 字符串常量（请求体接收的 scope 值）。
const SCOPE_READ: &str = "read";
const SCOPE_WRITE: &str = "write";
const SCOPE_ADMIN: &str = "admin";

/// garrison 权限串前缀（与 `auth_bridge.rs::PERM_*` 一致）。
const PERM_PREFIX: &str = "crawlrs:";

/// `POST /v1/admin/api-keys` 请求体。
#[derive(Debug, Clone, Deserialize)]
pub struct CreateApiKeyRequest {
    /// API Key 归属的 team_id（必填，不能为 nil UUID）。
    pub team_id: Uuid,
    /// 作用域列表（必填，非空；元素仅允许 `"read"`/`"write"`/`"admin"`）。
    pub scopes: Vec<String>,
    /// 过期时间（秒，可选；缺省 30 天；必须 > 0 且 <= 1 年）。
    pub expires_in_secs: Option<i64>,
}

/// `POST /v1/admin/api-keys` 响应体。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateApiKeyResponse {
    /// 明文 API Key（双段格式 `key_id.key_secret`，仅此一次返回）。
    pub api_key: String,
    /// crawlrs 内部 API Key ID（UUID，对应 `api_keys.id`）。
    pub api_key_id: Uuid,
    /// 归属 team_id。
    pub team_id: Uuid,
    /// 作用域列表（与请求一致）。
    pub scopes: Vec<String>,
    /// 过期时间（RFC3339）。
    pub expires_at: String,
}

/// `POST /v1/admin/api-keys` handler。
///
/// 调用 garrison `ApiKeyHandler::generate_with_namespace` 签发新 API Key，
/// 同时在 crawlrs `api_keys` 表插入映射记录（`id`/`team_id`/`key`=`key_id`/`key_hash`=`None`）。
///
/// # 鉴权（单一防御点，CWE-862 IDOR 防护）
///
/// **本 handler 是 admin 权限校验的唯一防御点**。全局 `auth_middleware_inner`
/// 仅校验 API Key 有效性（garrison `check_api_key`），不强制 admin scope——
/// `scope_middleware` 虽存在但仅在测试 Router 中注册，未挂载到生产路由。
///
/// 调用方必须持有 `Admin` 权限（`auth_state.scope.has_permission(Admin)`），
/// 否则返回 403。后续若重构需保证此校验不被误删，否则即导致越权签发。
///
/// # 时序
///
/// `init_garrison_auth` 在 bootstrap 阶段注入 `GARRISON_DAO` 全局态，
/// 此 handler 在请求路径通过 `get_garrison_dao()` 读取（共享读锁，热路径无竞争）。
pub async fn create_api_key(
    Extension(auth_state): Extension<AuthState>,
    Json(req): Json<CreateApiKeyRequest>,
) -> Response {
    // 1. 鉴权：仅 admin 可签发新 key（CWE-862 IDOR 防护，单一防御点）
    if !auth_state.scope.has_permission(ScopePermission::Admin) {
        return error_response(
            StatusCode::FORBIDDEN,
            "Admin permission required to issue API keys",
        );
    }

    // 2. 输入校验
    if let Err(msg) = validate_request(&req) {
        return error_response(StatusCode::BAD_REQUEST, msg);
    }

    // 3. team_id 存在性校验（DB schema 未声明外键约束，必须 handler 主动校验避免孤儿记录）
    let pool_for_check = auth_state.pool.clone();
    if let Err(e) = check_team_exists(&pool_for_check, req.team_id).await {
        log::warn!(
            "team_id existence check failed (team_id={}): {}",
            req.team_id,
            e
        );
        return error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "Failed to validate team_id",
        );
    }

    // 4. 生成新 api_key_id（同时作为 garrison login_id，design.md §5 约定）
    let api_key_id = Uuid::new_v4();
    let expires_in_secs = req.expires_in_secs.unwrap_or(DEFAULT_EXPIRES_IN_SECS);
    let now = Utc::now();
    let expires_at = now + chrono::Duration::seconds(expires_in_secs);

    // 5. 映射 scopes 为 garrison 权限串（`"read"` → `"crawlrs:read"`，依此类推）
    let garrison_scopes = map_scopes_to_garrison_perms(&req.scopes);

    // 6. 获取 garrison DAO（必须已注入）
    let dao = match get_garrison_dao() {
        Some(d) => d,
        None => {
            log::error!(
                "garrison DAO not injected — call init_garrison_auth before serving requests"
            );
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Authentication service unavailable",
            );
        }
    };

    // 7. 调用 garrison 签发 API Key（明文 key 仅此一次可见）
    let handler = garrison::protocol::apikey::ApiKeyHandler::new(dao);
    let plaintext_key = match handler
        .generate_with_namespace(
            api_key_id.to_string(),
            GARRISON_NAMESPACE,
            garrison_scopes,
            expires_in_secs,
        )
        .await
    {
        Ok(k) => k,
        Err(e) => {
            log::error!(
                "garrison ApiKeyHandler::generate_with_namespace failed: api_key_id={}, err={}",
                api_key_id,
                e
            );
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Failed to issue API key");
        }
    };

    // 8. 严格解析 garrison 返回的 `key_id.key_secret` 提取 key_id（公开标识，可安全记录）
    //
    // 安全审查 [HIGH]：原 `unwrap_or_else(|| plaintext_key.clone())` 在 garrison 返回异常格式
    // 时会把明文 secret 当作 key_id 写入 DB（CWE-916 违规）。改为严格校验：split_once 失败
    // 返回 500 错误且不写入 DB，并记录原始错误供排查。
    let garrison_key_id = match plaintext_key.split_once('.') {
        Some((k_id, _)) => k_id.to_string(),
        None => {
            log::error!(
                "garrison returned malformed key (no '.' separator): api_key_id={} (key length={})",
                api_key_id,
                plaintext_key.len()
            );
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Authentication service returned malformed key",
            );
        }
    };

    // 9. 写入 crawlrs api_keys 表（id/team_id 映射，key_hash 弃用）
    match insert_api_key_mapping(&pool_for_check, api_key_id, req.team_id, garrison_key_id).await {
        Ok(()) => {
            let response = CreateApiKeyResponse {
                api_key: plaintext_key,
                api_key_id,
                team_id: req.team_id,
                scopes: req.scopes.clone(),
                expires_at: expires_at.to_rfc3339(),
            };
            // 安全审查 [MEDIUM]：响应含明文 API Key，必须禁用缓存（CWE-525）
            // Cache-Control: no-store（HTTP/1.1）+ Pragma: no-cache（HTTP/1.0 兼容）+ Expires: 0
            let mut resp =
                (StatusCode::CREATED, Json(ApiResponse::success(response))).into_response();
            resp.headers_mut()
                .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
            resp.headers_mut()
                .insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
            resp.headers_mut()
                .insert(header::EXPIRES, HeaderValue::from_static("0"));
            resp
        }
        Err(e) => {
            log::error!(
                "Failed to insert api_key mapping (api_key_id={}, team_id={}): {}",
                api_key_id,
                req.team_id,
                e
            );
            error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Failed to persist API key mapping",
            )
        }
    }
}

/// 校验请求体。
///
/// # 规则
///
/// - `team_id` 不能为 nil UUID（CWE-862：所有 key 必须归属真实 team）
/// - `scopes` 非空，元素仅允许 `"read"`/`"write"`/`"admin"`，数量 <= `MAX_SCOPES_LEN`
///   （CWE-208：拒绝超大 scopes 数组导致 garrison 调用耗时增加）
/// - `expires_in_secs`（若提供）必须 > 0 且 <= 1 年（CWE-208：拒绝异常 TTL）
fn validate_request(req: &CreateApiKeyRequest) -> Result<(), String> {
    if req.team_id == Uuid::nil() {
        return Err("team_id must not be nil UUID".to_string());
    }
    if req.scopes.is_empty() {
        return Err("scopes must not be empty".to_string());
    }
    if req.scopes.len() > MAX_SCOPES_LEN {
        return Err(format!(
            "scopes length must not exceed {} (got {})",
            MAX_SCOPES_LEN,
            req.scopes.len()
        ));
    }
    for scope in &req.scopes {
        match scope.as_str() {
            SCOPE_READ | SCOPE_WRITE | SCOPE_ADMIN => {}
            other => {
                return Err(format!(
                    "invalid scope: {} (allowed: read/write/admin)",
                    other
                ));
            }
        }
    }
    if let Some(expires_in) = req.expires_in_secs {
        if expires_in <= 0 {
            return Err("expires_in_secs must be positive".to_string());
        }
        if expires_in > MAX_EXPIRES_IN_SECS {
            return Err(format!(
                "expires_in_secs must not exceed {} seconds (1 year)",
                MAX_EXPIRES_IN_SECS
            ));
        }
    }
    Ok(())
}

/// 校验 `team_id` 在 `teams` 表中存在。
///
/// # 背景（安全审查 [MEDIUM]）
///
/// DB schema（`migrations/001_initial_schema.sql`）的 `api_keys.team_id` 列
/// **未声明** `FOREIGN KEY ... REFERENCES teams(id)` 约束，仅 `UUID NOT NULL`。
/// 故必须 handler 主动校验，否则会写入孤儿记录（破坏数据一致性）。
///
/// # 失败契约
///
/// - `Ok(())`：team 存在
/// - `Err(sea_orm::DbErr)`：DB 查询失败（连接错误等），由调用方映射为 500
/// - `team_id` 不存在：返回 `Err(DbErr::RecordNotFound("team not found"))`（CWE-862 IDOR 防护）
async fn check_team_exists(
    pool: &Arc<dbnexus::DbPool>,
    team_id: Uuid,
) -> Result<(), sea_orm::DbErr> {
    let session = pool
        .get_session("admin")
        .await
        .map_err(|e| sea_orm::DbErr::Custom(format!("db session: {}", e)))?;
    let conn = session
        .connection()
        .map_err(|e| sea_orm::DbErr::Custom(format!("db conn: {}", e)))?;
    let row = TeamEntity::find_by_id(team_id).one(conn).await?;
    if row.is_some() {
        Ok(())
    } else {
        // 使用 RecordNotFound 而非 Custom，便于上层根据错误类型映射状态码
        Err(sea_orm::DbErr::RecordNotFound(format!(
            "team not found: {}",
            team_id
        )))
    }
}

/// 映射 crawlrs scope 字符串为 garrison 权限串。
///
/// # 输入约定
///
/// 调用前必须经 `validate_request` 校验，本函数对未知 scope 走 `unreachable!`
/// （规则3：确定性逻辑，不交给运行时判断）。
fn map_scopes_to_garrison_perms(scopes: &[String]) -> Vec<String> {
    scopes
        .iter()
        .map(|s| match s.as_str() {
            SCOPE_READ => format!("{}{}", PERM_PREFIX, SCOPE_READ),
            SCOPE_WRITE => format!("{}{}", PERM_PREFIX, SCOPE_WRITE),
            SCOPE_ADMIN => format!("{}{}", PERM_PREFIX, SCOPE_ADMIN),
            _ => unreachable!("validate_request 已保证 scope 合法"),
        })
        .collect()
}

/// 写入 `api_keys` 表（`id`/`team_id`/`key`=`garrison_key_id`/`key_hash`=`None`）。
///
/// # 失败契约（规则12 显性化）
///
/// 返回 `Err(sea_orm::DbErr)` 由调用方映射为 500，记录到 `log::error!`。
/// 常见失败场景：unique 冲突（极小概率，UUID 碰撞）/ DB 连接失败 / NOT NULL 约束。
///
/// # 弃用字段访问（T028）
///
/// `key_hash` 字段已标注 `#[deprecated]`（garrison 自管哈希），但此处仍需
/// 显式写入 `None`（保留列约束），故加 `#[allow(deprecated)]`。待全量重签
/// 完成后随 `api_keys.key_hash` 列一并移除。
#[allow(deprecated)]
async fn insert_api_key_mapping(
    pool: &Arc<dbnexus::DbPool>,
    api_key_id: Uuid,
    team_id: Uuid,
    garrison_key_id: String,
) -> Result<(), sea_orm::DbErr> {
    let session = pool
        .get_session("admin")
        .await
        .map_err(|e| sea_orm::DbErr::Custom(format!("db session: {}", e)))?;
    let conn = session
        .connection()
        .map_err(|e| sea_orm::DbErr::Custom(format!("db conn: {}", e)))?;
    let now = time_utils::to_db_datetime(Utc::now());
    let active = ActiveModel {
        id: sea_orm::ActiveValue::Set(api_key_id),
        team_id: sea_orm::ActiveValue::Set(team_id),
        key: sea_orm::ActiveValue::Set(garrison_key_id),
        // R-key-lifecycle-003：key_hash 弃用（garrison 自管 sha256(secret_hash)）
        key_hash: sea_orm::ActiveValue::Set(None),
        created_at: sea_orm::ActiveValue::Set(now),
        updated_at: sea_orm::ActiveValue::Set(None),
    };
    Entity::insert(active).exec(conn).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_helpers::{create_test_db_pool, skip_if_no_test_db};
    use crate::domain::auth::ApiKeyScope;
    use crate::infrastructure::auth::garrison_dao::{
        init_garrison_dao, reset_garrison_dao_for_test, set_garrison_dao,
    };
    use crate::infrastructure::database::entities::api_key::Entity as ApiKeyEntity;
    use crate::presentation::handlers::response_builder::ApiResponse;
    use axum::response::IntoResponse;
    use sea_orm::EntityTrait;

    // ========== 测试辅助 ==========

    /// 构造一个 admin scope 的 AuthState（pool 指向测试 DB）。
    fn make_admin_auth_state() -> AuthState {
        let pool = create_test_db_pool();
        AuthState::new(
            pool,
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::full_access(),
        )
    }

    /// 构造一个非 admin scope 的 AuthState（read-only）。
    fn make_read_only_auth_state() -> AuthState {
        let pool = create_test_db_pool();
        AuthState::new(
            pool,
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::read_only(),
        )
    }

    /// 注入 garrison DAO 全局态（必须先调 `reset_garrison_dao_for_test()` 清理先前状态）。
    ///
    /// 安全审查 [LOW]：原 `let _ = ...` 丢弃 `Result` 掩盖了「DAO 已注入」错误。
    /// 改为 panic 显性化失败——`Arc<dyn GarrisonDao>` 不实现 `Debug`，无法用 `.expect()`，
    /// 故用 `if let Err(_) = ... { panic!(...) }` 模式。
    async fn inject_test_garrison_dao() {
        let dao = init_garrison_dao()
            .await
            .expect("init_garrison_dao must succeed in test");
        if set_garrison_dao(dao).is_err() {
            panic!(
                "set_garrison_dao failed: DAO already injected \
                 (call reset_garrison_dao_for_test() before inject)"
            );
        }
    }

    /// 在测试 DB 中创建 team 记录，满足 `check_team_exists` 校验。
    ///
    /// T034 修复：原测试未创建 team 导致 `check_team_exists` 返回 `RecordNotFound`，
    /// handler 映射为 500 错误。此函数在调用 `create_api_key` 前预置 team。
    ///
    /// # Panics
    ///
    /// DB 写入失败时 panic（测试环境异常，不应静默）。
    async fn seed_team_in_db(pool: &Arc<dbnexus::DbPool>, team_id: Uuid) {
        use crate::infrastructure::database::entities::team::ActiveModel as TeamActiveModel;
        use sea_orm::ActiveValue;

        let session = pool
            .get_session("admin")
            .await
            .expect("get_session failed in seed_team_in_db");
        let conn = session
            .connection()
            .expect("connection failed in seed_team_in_db");
        let now = time_utils::to_db_datetime(Utc::now());
        let active = TeamActiveModel {
            id: ActiveValue::Set(team_id),
            name: ActiveValue::Set(format!("test-team-{}", team_id)),
            allowed_countries: ActiveValue::Set(None),
            blocked_countries: ActiveValue::Set(None),
            ip_whitelist: ActiveValue::Set(None),
            domain_blacklist: ActiveValue::Set(None),
            enable_geo_restrictions: ActiveValue::Set(false),
            created_at: ActiveValue::Set(now),
            updated_at: ActiveValue::Set(now),
        };
        TeamEntity::insert(active)
            .exec(conn)
            .await
            .expect("Failed to seed team in DB");
    }

    // ========== validate_request 测试 ==========

    #[test]
    fn test_validate_request_happy_path_minimal() {
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec!["read".to_string()],
            expires_in_secs: Some(3600),
        };
        assert!(validate_request(&req).is_ok());
    }

    #[test]
    fn test_validate_request_happy_path_all_scopes() {
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec!["read".to_string(), "write".to_string(), "admin".to_string()],
            expires_in_secs: Some(86400),
        };
        assert!(validate_request(&req).is_ok());
    }

    #[test]
    fn test_validate_request_default_expires_ok() {
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec!["admin".to_string()],
            expires_in_secs: None,
        };
        assert!(validate_request(&req).is_ok());
    }

    #[test]
    fn test_validate_request_max_expires_ok() {
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec!["read".to_string()],
            expires_in_secs: Some(MAX_EXPIRES_IN_SECS),
        };
        assert!(validate_request(&req).is_ok());
    }

    #[test]
    fn test_validate_request_nil_team_id_rejected() {
        let req = CreateApiKeyRequest {
            team_id: Uuid::nil(),
            scopes: vec!["read".to_string()],
            expires_in_secs: None,
        };
        let err = validate_request(&req).unwrap_err();
        assert!(err.contains("team_id"), "err: {}", err);
    }

    #[test]
    fn test_validate_request_empty_scopes_rejected() {
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec![],
            expires_in_secs: None,
        };
        let err = validate_request(&req).unwrap_err();
        assert!(err.contains("scopes"), "err: {}", err);
    }

    #[test]
    fn test_validate_request_invalid_scope_rejected() {
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec!["superuser".to_string()],
            expires_in_secs: None,
        };
        let err = validate_request(&req).unwrap_err();
        assert!(err.contains("invalid scope"), "err: {}", err);
        assert!(err.contains("superuser"), "err: {}", err);
    }

    #[test]
    fn test_validate_request_mix_valid_invalid_scope_rejected() {
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec!["read".to_string(), "delete".to_string()],
            expires_in_secs: None,
        };
        let err = validate_request(&req).unwrap_err();
        assert!(err.contains("delete"), "err: {}", err);
    }

    #[test]
    fn test_validate_request_zero_expires_rejected() {
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec!["read".to_string()],
            expires_in_secs: Some(0),
        };
        let err = validate_request(&req).unwrap_err();
        assert!(err.contains("positive"), "err: {}", err);
    }

    #[test]
    fn test_validate_request_negative_expires_rejected() {
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec!["read".to_string()],
            expires_in_secs: Some(-1),
        };
        let err = validate_request(&req).unwrap_err();
        assert!(err.contains("positive"), "err: {}", err);
    }

    #[test]
    fn test_validate_request_over_max_expires_rejected() {
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec!["read".to_string()],
            expires_in_secs: Some(MAX_EXPIRES_IN_SECS + 1),
        };
        let err = validate_request(&req).unwrap_err();
        assert!(err.contains("1 year"), "err: {}", err);
    }

    /// 安全审查 [MEDIUM] 补测：scopes 数量超过 MAX_SCOPES_LEN 时必须被拒绝。
    #[test]
    fn test_validate_request_scopes_too_long_rejected() {
        // 构造 11 个 scope（超过 MAX_SCOPES_LEN=10）
        let oversized_scopes: Vec<String> =
            std::iter::repeat_n("read".to_string(), MAX_SCOPES_LEN + 1).collect();
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: oversized_scopes,
            expires_in_secs: None,
        };
        let err = validate_request(&req).unwrap_err();
        assert!(
            err.contains("scopes length must not exceed"),
            "must reject oversized scopes, err: {}",
            err
        );
        assert!(err.contains(&MAX_SCOPES_LEN.to_string()));
    }

    /// 边界值测试：正好 MAX_SCOPES_LEN 个 scope 应通过校验。
    #[test]
    fn test_validate_request_scopes_at_limit_ok() {
        let scopes: Vec<String> = std::iter::repeat_n("read".to_string(), MAX_SCOPES_LEN).collect();
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes,
            expires_in_secs: None,
        };
        assert!(
            validate_request(&req).is_ok(),
            "MAX_SCOPES_LEN boundary must pass"
        );
    }

    #[test]
    fn test_validate_request_empty_string_scope_rejected() {
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec!["".to_string()],
            expires_in_secs: None,
        };
        let err = validate_request(&req).unwrap_err();
        assert!(err.contains("invalid scope"), "err: {}", err);
    }

    /// 安全审查 [HIGH] 补测：garrison 返回不含 '.' 的 malformed key 时，
    /// create_api_key 必须返回 500 且不写 DB（不将明文 secret 当 key_id 持久化）。
    ///
    /// 此测试为纯逻辑验证：通过 mock 替换 garrison ApiKeyHandler 不可行（garrison 类型不公开 mock trait），
    /// 故仅验证 split_once 解析逻辑的边界行为，确保 fallback 路径不会写入明文。
    #[test]
    fn test_garrison_key_parsing_rejects_no_separator() {
        // 模拟 garrison 返回的不含 '.' 的字符串
        let malformed_key = "no_separator_in_this_key_string";
        // 验证 split_once 解析失败
        assert!(malformed_key.split_once('.').is_none());
        // 修复后的 handler 在此场景返回 500 且不写 DB（已在 create_api_key 步骤 8 中实现）

        // 验证正常 garrison key（含 '.'）能正确解析
        let normal_key = "abc123key_id.xyz456secret";
        let parsed = normal_key.split_once('.').map(|(k_id, _)| k_id.to_string());
        assert_eq!(parsed.as_deref(), Some("abc123key_id"));
    }

    #[test]
    fn test_validate_request_case_sensitive_scope_rejected() {
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec!["Read".to_string()],
            expires_in_secs: None,
        };
        let err = validate_request(&req).unwrap_err();
        assert!(
            err.contains("Read") || err.contains("invalid scope"),
            "err: {}",
            err
        );
    }

    #[test]
    fn test_validate_request_duplicate_scopes_ok() {
        // 重复 scope 不算错误（garrison 端去重，crawlrs 不强制唯一）
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec!["read".to_string(), "read".to_string()],
            expires_in_secs: None,
        };
        assert!(validate_request(&req).is_ok());
    }

    // ========== map_scopes_to_garrison_perms 测试 ==========

    #[test]
    fn test_map_scopes_read_only() {
        let perms = map_scopes_to_garrison_perms(&["read".to_string()]);
        assert_eq!(perms, vec!["crawlrs:read".to_string()]);
    }

    #[test]
    fn test_map_scopes_write_only() {
        let perms = map_scopes_to_garrison_perms(&["write".to_string()]);
        assert_eq!(perms, vec!["crawlrs:write".to_string()]);
    }

    #[test]
    fn test_map_scopes_admin_only() {
        let perms = map_scopes_to_garrison_perms(&["admin".to_string()]);
        assert_eq!(perms, vec!["crawlrs:admin".to_string()]);
    }

    #[test]
    fn test_map_scopes_all_three() {
        let perms = map_scopes_to_garrison_perms(&[
            "read".to_string(),
            "write".to_string(),
            "admin".to_string(),
        ]);
        assert_eq!(
            perms,
            vec![
                "crawlrs:read".to_string(),
                "crawlrs:write".to_string(),
                "crawlrs:admin".to_string(),
            ]
        );
    }

    #[test]
    fn test_map_scopes_empty_input_returns_empty() {
        let perms = map_scopes_to_garrison_perms(&[]);
        assert!(perms.is_empty());
    }

    // ========== create_api_key 鉴权失败测试（不需要 garrison DAO / DB） ==========

    #[tokio::test]
    async fn test_create_api_key_non_admin_returns_403() {
        if skip_if_no_test_db() {
            return;
        }
        let auth_state = make_read_only_auth_state();
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec!["read".to_string()],
            expires_in_secs: None,
        };

        let response = create_api_key(Extension(auth_state), Json(req))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_create_api_key_denied_scope_returns_403() {
        if skip_if_no_test_db() {
            return;
        }
        let auth_state = AuthState::new(
            create_test_db_pool(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::denied(),
        );
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec!["read".to_string()],
            expires_in_secs: None,
        };

        let response = create_api_key(Extension(auth_state), Json(req))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_create_api_key_nil_team_id_returns_400() {
        if skip_if_no_test_db() {
            return;
        }
        let auth_state = make_admin_auth_state();
        let req = CreateApiKeyRequest {
            team_id: Uuid::nil(),
            scopes: vec!["read".to_string()],
            expires_in_secs: None,
        };

        let response = create_api_key(Extension(auth_state), Json(req))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_api_key_empty_scopes_returns_400() {
        if skip_if_no_test_db() {
            return;
        }
        let auth_state = make_admin_auth_state();
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec![],
            expires_in_secs: None,
        };

        let response = create_api_key(Extension(auth_state), Json(req))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_api_key_invalid_scope_returns_400() {
        if skip_if_no_test_db() {
            return;
        }
        let auth_state = make_admin_auth_state();
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec!["superuser".to_string()],
            expires_in_secs: None,
        };

        let response = create_api_key(Extension(auth_state), Json(req))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_api_key_zero_expires_returns_400() {
        if skip_if_no_test_db() {
            return;
        }
        let auth_state = make_admin_auth_state();
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec!["read".to_string()],
            expires_in_secs: Some(0),
        };

        let response = create_api_key(Extension(auth_state), Json(req))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_api_key_negative_expires_returns_400() {
        if skip_if_no_test_db() {
            return;
        }
        let auth_state = make_admin_auth_state();
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec!["read".to_string()],
            expires_in_secs: Some(-100),
        };

        let response = create_api_key(Extension(auth_state), Json(req))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_create_api_key_over_max_expires_returns_400() {
        if skip_if_no_test_db() {
            return;
        }
        let auth_state = make_admin_auth_state();
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec!["read".to_string()],
            expires_in_secs: Some(MAX_EXPIRES_IN_SECS + 1),
        };

        let response = create_api_key(Extension(auth_state), Json(req))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    // ========== create_api_key garrison DAO 未注入测试 ==========

    #[tokio::test]
    async fn test_create_api_key_dao_not_injected_returns_500() {
        // 不注入 DAO，验证返回 500（而非 panic）
        // 持有 TEST_MUTEX 到测试结束，避免其他测试注入 DAO 干扰本测试的"DAO 未注入"场景
        let _guard = crate::infrastructure::auth::garrison_dao::test_mutex()
            .lock()
            .await;
        reset_garrison_dao_for_test();

        if skip_if_no_test_db() {
            return;
        }
        let auth_state = make_admin_auth_state();
        let req = CreateApiKeyRequest {
            team_id: Uuid::new_v4(),
            scopes: vec!["read".to_string()],
            expires_in_secs: None,
        };

        let response = create_api_key(Extension(auth_state), Json(req))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    // ========== create_api_key 端到端测试（需要真实 DB + garrison DAO） ==========
    //
    // 以下测试需要 TEST_DATABASE_URL/DATABASE_URL 与 garrison sync_mode 多线程 runtime，
    // 跳过条件：无测试 DB。运行时使用 `multi_thread` flavor 驱动 garrison block_in_place。

    /// T028：`key_hash` 字段已弃用（garrison 自管哈希），此测试验证新签发 key
    /// 的 `key_hash` 为 `None`，故需访问 deprecated 字段，加 `#[allow(deprecated)]`。
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    #[allow(deprecated)]
    async fn test_create_api_key_happy_path_admin_creates_key_for_any_team() {
        if skip_if_no_test_db() {
            return;
        }
        let _guard = crate::infrastructure::auth::garrison_dao::test_mutex()
            .lock()
            .await;
        reset_garrison_dao_for_test();
        inject_test_garrison_dao().await;

        if skip_if_no_test_db() {
            return;
        }
        let auth_state = make_admin_auth_state();
        let target_team_id = Uuid::new_v4();
        // T034 修复：handler 步骤 3 的 `check_team_exists` 要求 team 存在，
        // 必须在调用 create_api_key 前预置 team 记录，否则返回 500。
        seed_team_in_db(&auth_state.pool, target_team_id).await;

        let req = CreateApiKeyRequest {
            team_id: target_team_id,
            scopes: vec!["read".to_string(), "write".to_string()],
            expires_in_secs: Some(3600),
        };

        let response = create_api_key(Extension(auth_state.clone()), Json(req))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::CREATED);

        // 解析响应体
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed: ApiResponse<CreateApiKeyResponse> = serde_json::from_slice(&body).unwrap();
        assert!(parsed.success);
        let data = parsed.data.unwrap();

        // 验证响应字段
        assert!(!data.api_key.is_empty(), "api_key must be non-empty");
        assert!(
            data.api_key.contains('.'),
            "api_key must be dual-segment format"
        );
        assert_eq!(data.team_id, target_team_id);
        assert_eq!(data.scopes, vec!["read".to_string(), "write".to_string()]);
        assert!(!data.expires_at.is_empty());

        // 验证 api_key_id 在 crawlrs api_keys 表中存在（且 team_id 匹配）
        let session = auth_state.pool.get_session("admin").await.unwrap();
        let conn = session.connection().unwrap();
        let row = ApiKeyEntity::find_by_id(data.api_key_id)
            .one(conn)
            .await
            .unwrap()
            .expect("api_key mapping must exist in DB after create");
        assert_eq!(row.team_id, target_team_id);
        assert_eq!(row.key, data.api_key.split_once('.').unwrap().0);
        assert!(row.key_hash.is_none(), "key_hash must be None (deprecated)");

        reset_garrison_dao_for_test();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_create_api_key_default_expires_used_when_none() {
        if skip_if_no_test_db() {
            return;
        }
        let _guard = crate::infrastructure::auth::garrison_dao::test_mutex()
            .lock()
            .await;
        reset_garrison_dao_for_test();
        inject_test_garrison_dao().await;

        if skip_if_no_test_db() {
            return;
        }
        let auth_state = make_admin_auth_state();
        let target_team_id = Uuid::new_v4();
        seed_team_in_db(&auth_state.pool, target_team_id).await;
        let req = CreateApiKeyRequest {
            team_id: target_team_id,
            scopes: vec!["read".to_string()],
            expires_in_secs: None, // 测试默认值
        };

        let response = create_api_key(Extension(auth_state), Json(req))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::CREATED);

        reset_garrison_dao_for_test();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_create_api_key_admin_scope_single() {
        if skip_if_no_test_db() {
            return;
        }
        let _guard = crate::infrastructure::auth::garrison_dao::test_mutex()
            .lock()
            .await;
        reset_garrison_dao_for_test();
        inject_test_garrison_dao().await;

        if skip_if_no_test_db() {
            return;
        }
        let auth_state = make_admin_auth_state();
        let target_team_id = Uuid::new_v4();
        seed_team_in_db(&auth_state.pool, target_team_id).await;
        let req = CreateApiKeyRequest {
            team_id: target_team_id,
            scopes: vec!["admin".to_string()],
            expires_in_secs: Some(86400),
        };

        let response = create_api_key(Extension(auth_state), Json(req))
            .await
            .into_response();

        assert_eq!(response.status(), StatusCode::CREATED);

        reset_garrison_dao_for_test();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_create_api_key_returns_distinct_keys_per_call() {
        if skip_if_no_test_db() {
            return;
        }
        let _guard = crate::infrastructure::auth::garrison_dao::test_mutex()
            .lock()
            .await;
        reset_garrison_dao_for_test();
        inject_test_garrison_dao().await;

        if skip_if_no_test_db() {
            return;
        }
        let auth_state = make_admin_auth_state();
        let target_team_id = Uuid::new_v4();
        seed_team_in_db(&auth_state.pool, target_team_id).await;
        let req = CreateApiKeyRequest {
            team_id: target_team_id,
            scopes: vec!["read".to_string()],
            expires_in_secs: Some(3600),
        };

        let resp1 = create_api_key(Extension(auth_state.clone()), Json(req.clone()))
            .await
            .into_response();
        let resp2 = create_api_key(Extension(auth_state.clone()), Json(req))
            .await
            .into_response();

        assert_eq!(resp1.status(), StatusCode::CREATED);
        assert_eq!(resp2.status(), StatusCode::CREATED);

        // 解析两个响应，验证 api_key 不同
        let body1 = axum::body::to_bytes(resp1.into_body(), usize::MAX)
            .await
            .unwrap();
        let body2 = axum::body::to_bytes(resp2.into_body(), usize::MAX)
            .await
            .unwrap();
        let parsed1: ApiResponse<CreateApiKeyResponse> = serde_json::from_slice(&body1).unwrap();
        let parsed2: ApiResponse<CreateApiKeyResponse> = serde_json::from_slice(&body2).unwrap();
        assert_ne!(
            parsed1.data.unwrap().api_key,
            parsed2.data.unwrap().api_key,
            "consecutive generate calls must return distinct keys"
        );

        reset_garrison_dao_for_test();
    }

    // ========== 常量值测试（防止意外修改） ==========

    #[test]
    fn test_garrison_namespace_value() {
        assert_eq!(GARRISON_NAMESPACE, "crawlrs");
    }

    #[test]
    fn test_default_expires_in_secs_value() {
        assert_eq!(DEFAULT_EXPIRES_IN_SECS, 30 * 24 * 60 * 60);
    }

    #[test]
    fn test_max_expires_in_secs_value() {
        assert_eq!(MAX_EXPIRES_IN_SECS, 365 * 24 * 60 * 60);
    }

    #[test]
    fn test_scope_constants() {
        assert_eq!(SCOPE_READ, "read");
        assert_eq!(SCOPE_WRITE, "write");
        assert_eq!(SCOPE_ADMIN, "admin");
    }

    // ========== DTO 序列化测试 ==========

    #[test]
    fn test_create_api_key_request_deserialize_minimal() {
        let json = r#"{"team_id":"550e8400-e29b-41d4-a716-446655440000","scopes":["read"]}"#;
        let req: CreateApiKeyRequest = serde_json::from_str(json).unwrap();
        assert_eq!(
            req.team_id.to_string(),
            "550e8400-e29b-41d4-a716-446655440000"
        );
        assert_eq!(req.scopes, vec!["read".to_string()]);
        assert!(req.expires_in_secs.is_none());
    }

    #[test]
    fn test_create_api_key_request_deserialize_full() {
        let json = r#"{
            "team_id":"550e8400-e29b-41d4-a716-446655440000",
            "scopes":["read","write","admin"],
            "expires_in_secs":7200
        }"#;
        let req: CreateApiKeyRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.scopes.len(), 3);
        assert_eq!(req.expires_in_secs, Some(7200));
    }

    #[test]
    fn test_create_api_key_request_missing_team_id_fails() {
        let json = r#"{"scopes":["read"]}"#;
        let result: Result<CreateApiKeyRequest, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_api_key_request_missing_scopes_fails() {
        let json = r#"{"team_id":"550e8400-e29b-41d4-a716-446655440000"}"#;
        let result: Result<CreateApiKeyRequest, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_api_key_request_invalid_team_id_fails() {
        let json = r#"{"team_id":"not-a-uuid","scopes":["read"]}"#;
        let result: Result<CreateApiKeyRequest, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_api_key_response_serialize() {
        let resp = CreateApiKeyResponse {
            api_key: "abc123.def456".to_string(),
            api_key_id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            scopes: vec!["read".to_string()],
            expires_at: "2026-08-25T12:00:00Z".to_string(),
        };
        let json = serde_json::to_string(&resp).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["api_key"], "abc123.def456");
        assert!(parsed["api_key_id"].is_string());
        assert!(parsed["team_id"].is_string());
        assert_eq!(parsed["scopes"][0], "read");
        assert_eq!(parsed["expires_at"], "2026-08-25T12:00:00Z");
    }
}
