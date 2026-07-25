// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Garrison Principal ↔ crawlrs AuthState 桥接（R-auth-engine-003 / R-authz-rbac-002）。
//!
//! ## 职责
//!
//! 将 garrison 校验返回的 `Principal` 桥接为 crawlrs 既有 `AuthState`，
//! 保持 handler 层 19 个 `Extension<AuthState>` 提取点零改动（换内核保外壳）。
//!
//! - [`map_perms_to_scope`]：garrison 权限串 → crawlrs `ApiKeyScope`（确定性查找表，规则3）
//! - [`bridge_to_auth_state`]：login_id/perms/team_id → AuthState（T014）
//! - [`extract_bearer`]：从 axum Request 提取 Bearer token（T016）
//!
//! ## feature 门控
//!
//! 本模块整体 `#[cfg(feature = "auth")]`——auth-off 时 garrison 不编译，
//! 走 feature-gate 的 `default_identity_middleware` 路径，不需要桥接。
//!
//! ## Spec
//!
//! - R-authz-rbac-002：`map_perms_to_scope` 实现 garrison 权限串到 ApiKeyScope 的映射
//! - R-auth-engine-003：`bridge_to_auth_state` 实现分解参数到 AuthState 的桥接

use crate::domain::auth::ApiKeyScope;
use crate::presentation::middleware::auth_middleware::{AuthError, AuthState};
use axum::body::Body;
use axum::http::{header, Request};
use dbnexus::DbPool;
use std::sync::Arc;
use uuid::Uuid;

/// garrison 权限串：read。
const PERM_READ: &str = "crawlrs:read";
/// garrison 权限串：write。
const PERM_WRITE: &str = "crawlrs:write";
/// garrison 权限串：admin（蕴含 read + write）。
const PERM_ADMIN: &str = "crawlrs:admin";

/// 默认搜索配额（与 `ApiKeyScope::default()` 一致）。
const DEFAULT_SEARCH_LIMIT: u32 = 100;
/// 默认抓取配额（与 `ApiKeyScope::default()` 一致）。
const DEFAULT_SCRAPE_LIMIT: u32 = 50;

/// `Authorization: Bearer <token>` scheme 前缀（RFC 7235，大小写敏感）。
const BEARER_PREFIX: &str = "Bearer ";

/// 将 garrison 权限串列表映射为 crawlrs `ApiKeyScope`（R-authz-rbac-002 / T012-T013）。
///
/// ## 权限蕴含规则（确定性查找表，规则3）
///
/// | garrison 权限串 | read | write | admin |
/// |----------------|------|-------|-------|
/// | `crawlrs:read`  | ✓    |       |       |
/// | `crawlrs:write` |      | ✓     |       |
/// | `crawlrs:admin` | ✓    | ✓     | ✓     |
///
/// `admin` 蕴含 `read` + `write`（角色层级，garrison role_hierarchy 预计算）。
/// `write` **不**蕴含 `read`（最小权限原则）。
///
/// ## 配额
///
/// `search_limit` / `scrape_limit` 使用默认值（100/50）。
/// 后续 T014 `bridge_to_auth_state` 会从 principal 扩展属性读取配额覆盖。
///
/// ## 参数
///
/// * `perms` - garrison `GarrisonInterface::get_permission_list` 返回的权限串切片
///
/// ## 返回
///
/// 返回对应的 `ApiKeyScope`。空切片或无匹配权限 → `denied()`（全 false）。
///
/// ## 示例
///
/// ```
/// use crawlrs::presentation::middleware::auth_bridge::map_perms_to_scope;
///
/// let admin_scope = map_perms_to_scope(&["crawlrs:admin".to_string()]);
/// assert!(admin_scope.read && admin_scope.write && admin_scope.admin);
///
/// let read_scope = map_perms_to_scope(&["crawlrs:read".to_string()]);
/// assert!(read_scope.read && !read_scope.write && !read_scope.admin);
/// ```
pub fn map_perms_to_scope(perms: &[String]) -> ApiKeyScope {
    // 确定性查找表：遍历一次权限串，按匹配设置对应标志。
    // 不用 HashSet——N 通常 <5（garrison role_hierarchy 预计算后权限很少），
    // 线性扫描 + 早期返回比 HashSet 分配更高效（规则5 简洁优先）。
    let mut read = false;
    let mut write = false;
    let mut admin = false;

    for perm in perms {
        match perm.as_str() {
            PERM_ADMIN => {
                // admin 蕴含 read + write
                read = true;
                write = true;
                admin = true;
            }
            PERM_READ => read = true,
            PERM_WRITE => write = true,
            // 未知权限串忽略（garrison 可能返回其他 namespace 的权限，如其他应用）
            _ => {}
        }
    }

    ApiKeyScope::with_custom_limits(
        read,
        write,
        admin,
        DEFAULT_SEARCH_LIMIT,
        DEFAULT_SCRAPE_LIMIT,
    )
}

/// 从 axum `Request` 提取 Bearer token（R-auth-engine-003 / T016）。
///
/// ## 提取规则
///
/// 1. 读 `Authorization` header；缺失 → `AuthError::InvalidKey`
/// 2. header 值非可见 ASCII → `AuthError::InvalidKey`
/// 3. 不以 `Bearer ` 前缀开头 → `AuthError::InvalidKey`（RFC 7235：scheme 大小写敏感）
/// 4. 截取前缀后的 token；空 token → `AuthError::InvalidKey`
///
/// ## 安全
///
/// - scheme 大小写敏感：RFC 7235 规定 `Bearer` 是规范形式，故 `bearer` / `BEARER` 拒绝。
///   crawlrs 旧实现也是大小写敏感的（`auth_middleware.rs::extract_bearer_token`），
///   本桥接保持一致性（规则8 惯例优先于新颖）。
/// - 不记录 token 内容到日志（避免 CWE-532 凭据泄露）。
///
/// ## 参数
///
/// * `req` - axum `Request<Body>`
///
/// ## 返回
///
/// - `Ok(token)`：提取的 Bearer token 字符串（owned）
/// - `Err(AuthError::InvalidKey)`：header 缺失 / 非 Bearer scheme / 空 token
///
/// ## 示例
///
/// ```ignore
/// use crawlrs::presentation::middleware::auth_bridge::extract_bearer;
///
/// let mut req = axum::http::Request::builder()
///     .header("authorization", "Bearer ak_test_123")
///     .body(axum::body::Body::empty())
///     .unwrap();
/// let token = extract_bearer(&req).unwrap();
/// assert_eq!(token, "ak_test_123");
/// ```
pub fn extract_bearer(req: &Request<Body>) -> Result<String, AuthError> {
    let auth_header = req
        .headers()
        .get(header::AUTHORIZATION)
        .ok_or(AuthError::InvalidKey)?;

    let value = auth_header.to_str().map_err(|_| AuthError::InvalidKey)?;

    // 检查 `Bearer ` 前缀（大小写敏感，RFC 7235）
    if !value.starts_with(BEARER_PREFIX) {
        return Err(AuthError::InvalidKey);
    }

    let token = &value[BEARER_PREFIX.len()..];
    if token.is_empty() {
        return Err(AuthError::InvalidKey);
    }

    Ok(token.to_string())
}

/// 将 garrison 校验返回的分解参数桥接为 crawlrs `AuthState`（R-auth-engine-003 / T014）。
///
/// ## 桥接逻辑
///
/// 1. `login_id` → `api_key_id` (Uuid)：design.md §5 约定签发 API Key 时
///    `login_id = api_key_id` 的 Uuid 字符串形式；解析失败 → `AuthError::InvalidLoginId`
/// 2. `perms` → `scope`：调用 [`map_perms_to_scope`] 映射为 `ApiKeyScope`
/// 3. 构造 `AuthState::new(pool, team_id, api_key_id, scope)`——不携带 cache/rate_limiter/
///    trusted_proxies（这些字段将在 Stage 4 T019/T021 删除，DTO 化后 `AuthState`
///    仅有 `pool/team_id/api_key_id/scope` 四字段）
///
/// ## 参数
///
/// * `pool` - crawlrs 数据库连接池（`Arc<DbPool>`，`DbPool` 内部 `Arc`，clone 廉价）
/// * `login_id` - garrison `GarrisonUtil::get_login_id` 返回的 login_id 字符串
/// * `perms` - garrison `GarrisonUtil::get_permission_list` 返回的权限串列表
/// * `team_id` - 由中间件反查 crawlrs `api_keys` 表获取的 team_id（design.md §3 步骤 4）
///
/// ## 返回
///
/// - `Ok(AuthState)`：成功构造的 AuthState，可注入到请求 extensions
/// - `Err(AuthError::InvalidLoginId)`：`login_id` 无法解析为 Uuid
///
/// ## 失败显性化（规则12）
///
/// `login_id` 解析失败时返回 `AuthError::InvalidLoginId`，不静默回退到 `Uuid::nil()`
/// （避免安全敏感场景下生成"匿名" AuthState，导致越权风险）。
///
/// ## 示例
///
/// ```ignore
/// use crawlrs::presentation::middleware::auth_bridge::bridge_to_auth_state;
/// use std::sync::Arc;
/// use dbnexus::DbPool;
/// use uuid::Uuid;
///
/// # async fn demo(pool: Arc<DbPool>) {
/// let login_id = Uuid::new_v4().to_string();
/// let perms = vec!["crawlrs:admin".to_string()];
/// let team_id = Uuid::new_v4();
/// let auth_state = bridge_to_auth_state(pool, login_id, perms, team_id).await.unwrap();
/// assert_eq!(auth_state.team_id, team_id);
/// # }
/// ```
pub async fn bridge_to_auth_state(
    pool: Arc<DbPool>,
    login_id: String,
    perms: Vec<String>,
    team_id: Uuid,
) -> Result<AuthState, AuthError> {
    // 1. 解析 login_id → api_key_id (Uuid)
    let api_key_id =
        Uuid::parse_str(&login_id).map_err(|_| AuthError::InvalidLoginId(login_id.clone()))?;

    // 2. 权限映射（确定性查找表，规则3）
    let scope = map_perms_to_scope(&perms);

    // 3. 构造 AuthState（DTO 化后仅 4 字段，cache/rate_limiter/trusted_proxies 由 Stage 4 删除）
    Ok(AuthState::new(pool, team_id, api_key_id, scope))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::auth::ScopePermission;

    // ========== map_perms_to_scope 测试（R-authz-rbac-002 / T012）==========

    /// R-authz-rbac-002：admin 权限 → read/write/admin 全 true
    #[test]
    fn test_map_perms_admin_grants_all() {
        let perms = vec!["crawlrs:admin".to_string()];
        let scope = map_perms_to_scope(&perms);
        assert!(scope.read, "admin should grant read");
        assert!(scope.write, "admin should grant write");
        assert!(scope.admin, "admin should grant admin");
    }

    /// R-authz-rbac-002：read 权限 → 仅 read
    #[test]
    fn test_map_perms_read_grants_only_read() {
        let perms = vec!["crawlrs:read".to_string()];
        let scope = map_perms_to_scope(&perms);
        assert!(scope.read, "read should grant read");
        assert!(!scope.write, "read should NOT grant write");
        assert!(!scope.admin, "read should NOT grant admin");
    }

    /// R-authz-rbac-002：write 权限 → 仅 write（不蕴含 read，最小权限原则）
    #[test]
    fn test_map_perms_write_grants_only_write() {
        let perms = vec!["crawlrs:write".to_string()];
        let scope = map_perms_to_scope(&perms);
        assert!(!scope.read, "write should NOT grant read (least privilege)");
        assert!(scope.write, "write should grant write");
        assert!(!scope.admin, "write should NOT grant admin");
    }

    /// R-authz-rbac-002：read + write → read + write（admin 不被授予）
    #[test]
    fn test_map_perms_read_and_write_grants_read_write() {
        let perms = vec!["crawlrs:read".to_string(), "crawlrs:write".to_string()];
        let scope = map_perms_to_scope(&perms);
        assert!(scope.read, "read+write should grant read");
        assert!(scope.write, "read+write should grant write");
        assert!(!scope.admin, "read+write should NOT grant admin");
    }

    /// R-authz-rbac-002：空权限 → denied（全 false）
    #[test]
    fn test_map_perms_empty_denies_all() {
        let perms: Vec<String> = vec![];
        let scope = map_perms_to_scope(&perms);
        assert!(!scope.read, "empty perms should deny read");
        assert!(!scope.write, "empty perms should deny write");
        assert!(!scope.admin, "empty perms should deny admin");
    }

    /// R-authz-rbac-002：未知权限串 → denied（全 false，忽略其他 namespace）
    #[test]
    fn test_map_perms_unknown_perm_denies_all() {
        let perms = vec!["otherapp:read".to_string(), "foo:admin".to_string()];
        let scope = map_perms_to_scope(&perms);
        assert!(!scope.read, "unknown perm should deny read");
        assert!(!scope.write, "unknown perm should deny write");
        assert!(!scope.admin, "unknown perm should deny admin");
    }

    /// R-authz-rbac-002：admin + read → admin 蕴含 read，去重后全 true
    #[test]
    fn test_map_perms_admin_and_read_dedup() {
        let perms = vec!["crawlrs:admin".to_string(), "crawlrs:read".to_string()];
        let scope = map_perms_to_scope(&perms);
        assert!(scope.read, "admin+read should grant read");
        assert!(scope.write, "admin+read should grant write");
        assert!(scope.admin, "admin+read should grant admin");
    }

    /// R-authz-rbac-002：大小写敏感（"Crawlrs:admin" ≠ "crawlrs:admin"）
    #[test]
    fn test_map_perms_case_sensitive() {
        let perms = vec!["Crawlrs:admin".to_string(), "CRAWLRS:READ".to_string()];
        let scope = map_perms_to_scope(&perms);
        assert!(!scope.read, "wrong case should deny read");
        assert!(!scope.write, "wrong case should deny write");
        assert!(!scope.admin, "wrong case should deny admin");
    }

    /// R-authz-rbac-002：search_limit/scrape_limit 使用默认值（100/50）
    #[test]
    fn test_map_perms_default_limits() {
        let perms = vec!["crawlrs:admin".to_string()];
        let scope = map_perms_to_scope(&perms);
        assert_eq!(
            scope.search_limit, 100,
            "search_limit should be default 100"
        );
        assert_eq!(scope.scrape_limit, 50, "scrape_limit should be default 50");
    }

    /// R-authz-rbac-002：混合权限（read + 未知 + admin）→ admin 蕴含全 true
    #[test]
    fn test_map_perms_mixed_with_unknown() {
        let perms = vec![
            "crawlrs:read".to_string(),
            "otherapp:write".to_string(),
            "crawlrs:admin".to_string(),
        ];
        let scope = map_perms_to_scope(&perms);
        assert!(scope.read, "mixed with admin should grant read");
        assert!(scope.write, "mixed with admin should grant write");
        assert!(scope.admin, "mixed with admin should grant admin");
    }

    /// R-authz-rbac-002：常量一致性验证
    #[test]
    fn test_perm_constants() {
        assert_eq!(PERM_READ, "crawlrs:read");
        assert_eq!(PERM_WRITE, "crawlrs:write");
        assert_eq!(PERM_ADMIN, "crawlrs:admin");
        assert_eq!(DEFAULT_SEARCH_LIMIT, 100);
        assert_eq!(DEFAULT_SCRAPE_LIMIT, 50);
        assert_eq!(BEARER_PREFIX, "Bearer ");
    }

    // ========== extract_bearer 测试（R-auth-engine-003 / T016）==========

    /// 构造测试用 Request（含指定 Authorization header）
    fn make_request(auth_header: Option<&str>) -> Request<Body> {
        let mut req = axum::http::Request::builder()
            .uri("/")
            .body(Body::empty())
            .unwrap();
        if let Some(value) = auth_header {
            req.headers_mut()
                .insert(header::AUTHORIZATION, value.parse().unwrap());
        }
        req
    }

    /// R-auth-engine-003：合法 Bearer token 提取成功
    #[test]
    fn test_extract_bearer_valid_token() {
        let req = make_request(Some("Bearer ak_test_12345"));
        let result = extract_bearer(&req);
        assert!(result.is_ok(), "valid Bearer should be Ok");
        assert_eq!(result.unwrap(), "ak_test_12345");
    }

    /// R-auth-engine-003：Authorization header 缺失 → InvalidKey
    #[test]
    fn test_extract_bearer_missing_header() {
        let req = make_request(None);
        let result = extract_bearer(&req);
        assert!(result.is_err(), "missing header should be Err");
        match result.unwrap_err() {
            AuthError::InvalidKey => {}
            other => panic!("expected InvalidKey, got {:?}", other),
        }
    }

    /// R-auth-engine-003：非 Bearer scheme → InvalidKey
    #[test]
    fn test_extract_bearer_wrong_scheme() {
        let req = make_request(Some("Basic abc123"));
        let result = extract_bearer(&req);
        assert!(result.is_err(), "non-Bearer scheme should be Err");
        match result.unwrap_err() {
            AuthError::InvalidKey => {}
            other => panic!("expected InvalidKey, got {:?}", other),
        }
    }

    /// R-auth-engine-003：Bearer 后 token 为空 → InvalidKey
    #[test]
    fn test_extract_bearer_empty_token() {
        let req = make_request(Some("Bearer "));
        let result = extract_bearer(&req);
        assert!(result.is_err(), "empty token should be Err");
        match result.unwrap_err() {
            AuthError::InvalidKey => {}
            other => panic!("expected InvalidKey, got {:?}", other),
        }
    }

    /// R-auth-engine-003：scheme 大小写敏感（`bearer` ≠ `Bearer`）
    #[test]
    fn test_extract_bearer_case_sensitive_scheme() {
        let req = make_request(Some("bearer ak_test"));
        let result = extract_bearer(&req);
        assert!(result.is_err(), "lowercase 'bearer' should be Err");
        match result.unwrap_err() {
            AuthError::InvalidKey => {}
            other => panic!("expected InvalidKey, got {:?}", other),
        }
    }

    /// R-auth-engine-003：header 值含非 ASCII 字节 → InvalidKey
    #[test]
    fn test_extract_bearer_non_ascii_header() {
        let mut req = axum::http::Request::builder()
            .uri("/")
            .body(Body::empty())
            .unwrap();
        // 构造一个非 ASCII header 值（0x80 字节）
        let non_ascii_value = axum::http::HeaderValue::from_bytes(b"Bearer \x80abc").unwrap();
        req.headers_mut()
            .insert(header::AUTHORIZATION, non_ascii_value);
        let result = extract_bearer(&req);
        assert!(result.is_err(), "non-ASCII header should be Err");
        match result.unwrap_err() {
            AuthError::InvalidKey => {}
            other => panic!("expected InvalidKey, got {:?}", other),
        }
    }

    /// R-auth-engine-003：含特殊字符但合法的 token（含 `=` / `.`）
    #[test]
    fn test_extract_bearer_token_with_special_chars() {
        let req = make_request(Some("Bearer ak.test_key=123"));
        let result = extract_bearer(&req);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "ak.test_key=123");
    }

    // ========== bridge_to_auth_state 测试（R-auth-engine-003 / T014）==========

    /// 构造测试用 `Arc<DbPool>`（跳过 `TEST_DATABASE_URL` 缺失场景）。
    ///
    /// `DbPool` 在 dbnexus 0.4 无 Default 实现，必须连真实 PostgreSQL。
    /// `bridge_to_auth_state` 内部只调用 `AuthState::new(pool, ...)` 把 pool
    /// 存进 `Arc<DbPool>` 字段，不执行任何 DB 操作，故任何 `Arc<DbPool>` 即可。
    async fn make_test_pool() -> Option<Arc<DbPool>> {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return None;
        }
        Some(crate::common::test_helpers::create_test_db_pool())
    }

    /// R-auth-engine-003：合法 Uuid login_id + admin perms → 全 true scope + 正确字段
    #[tokio::test]
    async fn test_bridge_to_auth_state_admin_perms() {
        let pool = match make_test_pool().await {
            Some(p) => p,
            None => {
                eprintln!(
                    "[skip] TEST_DATABASE_URL not set — test requires real DbPool for AuthState::new"
                );
                return;
            }
        };
        let api_key_id = Uuid::new_v4();
        let login_id = api_key_id.to_string();
        let perms = vec!["crawlrs:admin".to_string()];
        let team_id = Uuid::new_v4();

        let auth_state = bridge_to_auth_state(pool, login_id, perms, team_id)
            .await
            .expect("admin bridge should succeed");

        assert_eq!(auth_state.api_key_id, api_key_id);
        assert_eq!(auth_state.team_id, team_id);
        assert!(auth_state.scope.read);
        assert!(auth_state.scope.write);
        assert!(auth_state.scope.admin);
        assert!(auth_state.scope.has_permission(ScopePermission::Admin));
    }

    /// R-auth-engine-003：合法 Uuid login_id + read perms → 仅 read scope
    #[tokio::test]
    async fn test_bridge_to_auth_state_read_perms() {
        let pool = match make_test_pool().await {
            Some(p) => p,
            None => {
                eprintln!(
                    "[skip] TEST_DATABASE_URL not set — test requires real DbPool for AuthState::new"
                );
                return;
            }
        };
        let api_key_id = Uuid::new_v4();
        let login_id = api_key_id.to_string();
        let perms = vec!["crawlrs:read".to_string()];
        let team_id = Uuid::new_v4();

        let auth_state = bridge_to_auth_state(pool, login_id, perms, team_id)
            .await
            .expect("read bridge should succeed");

        assert_eq!(auth_state.api_key_id, api_key_id);
        assert_eq!(auth_state.team_id, team_id);
        assert!(auth_state.scope.read);
        assert!(!auth_state.scope.write);
        assert!(!auth_state.scope.admin);
    }

    /// R-auth-engine-003：非 Uuid login_id → `AuthError::InvalidLoginId`（规则12 失败显性化）
    #[tokio::test]
    async fn test_bridge_to_auth_state_invalid_login_id() {
        let pool = match make_test_pool().await {
            Some(p) => p,
            None => {
                eprintln!(
                    "[skip] TEST_DATABASE_URL not set — test requires real DbPool for AuthState::new"
                );
                return;
            }
        };
        let login_id = "not-a-uuid".to_string();
        let perms = vec!["crawlrs:admin".to_string()];
        let team_id = Uuid::new_v4();

        let result = bridge_to_auth_state(pool, login_id.clone(), perms, team_id).await;

        assert!(result.is_err(), "invalid login_id should be Err");
        match result.unwrap_err() {
            AuthError::InvalidLoginId(returned_id) => {
                assert_eq!(
                    returned_id, login_id,
                    "returned login_id should match input"
                );
            }
            other => panic!("expected InvalidLoginId, got {:?}", other),
        }
    }

    /// R-auth-engine-003：空 perms → denied scope（全 false），但 AuthState 构造成功
    #[tokio::test]
    async fn test_bridge_to_auth_state_empty_perms_denied_scope() {
        let pool = match make_test_pool().await {
            Some(p) => p,
            None => {
                eprintln!(
                    "[skip] TEST_DATABASE_URL not set — test requires real DbPool for AuthState::new"
                );
                return;
            }
        };
        let api_key_id = Uuid::new_v4();
        let login_id = api_key_id.to_string();
        let perms: Vec<String> = vec![];
        let team_id = Uuid::new_v4();

        let auth_state = bridge_to_auth_state(pool, login_id, perms, team_id)
            .await
            .expect("empty perms should still bridge successfully");

        // 空 perms → denied scope（全 false），但 AuthState 字段正确填充
        assert_eq!(auth_state.api_key_id, api_key_id);
        assert_eq!(auth_state.team_id, team_id);
        assert!(!auth_state.scope.read);
        assert!(!auth_state.scope.write);
        assert!(!auth_state.scope.admin);
    }

    /// R-auth-engine-003：未知 perms → denied scope（与空 perms 等价）
    #[tokio::test]
    async fn test_bridge_to_auth_state_unknown_perms_denied_scope() {
        let pool = match make_test_pool().await {
            Some(p) => p,
            None => {
                eprintln!(
                    "[skip] TEST_DATABASE_URL not set — test requires real DbPool for AuthState::new"
                );
                return;
            }
        };
        let api_key_id = Uuid::new_v4();
        let login_id = api_key_id.to_string();
        let perms = vec!["otherapp:admin".to_string(), "foo:read".to_string()];
        let team_id = Uuid::new_v4();

        let auth_state = bridge_to_auth_state(pool, login_id, perms, team_id)
            .await
            .expect("unknown perms should still bridge successfully");

        assert!(!auth_state.scope.read);
        assert!(!auth_state.scope.write);
        assert!(!auth_state.scope.admin);
    }

    /// R-auth-engine-003：Uuid 大小写（`ABCD...` vs `abcd...`）均能解析（Uuid 不区分大小写）
    #[tokio::test]
    async fn test_bridge_to_auth_state_uppercase_uuid_login_id() {
        let pool = match make_test_pool().await {
            Some(p) => p,
            None => {
                eprintln!(
                    "[skip] TEST_DATABASE_URL not set — test requires real DbPool for AuthState::new"
                );
                return;
            }
        };
        let api_key_id = Uuid::new_v4();
        // Uuid::to_string 输出小写，但 Uuid::parse_str 接受大写
        let login_id = api_key_id.to_string().to_uppercase();
        let perms = vec!["crawlrs:read".to_string()];
        let team_id = Uuid::new_v4();

        let auth_state = bridge_to_auth_state(pool, login_id, perms, team_id)
            .await
            .expect("uppercase Uuid login_id should bridge successfully");

        assert_eq!(auth_state.api_key_id, api_key_id);
    }

    // ========== AuthError::from_garrison 测试（R-auth-engine-003 / T016）==========

    /// R-auth-engine-003：401 错误 → InvalidKey
    #[test]
    fn test_from_garrison_401_to_invalid_key() {
        let cases = vec![
            garrison::error::GarrisonError::NotLogin("test".to_string()),
            garrison::error::GarrisonError::InvalidToken("test".to_string()),
            garrison::error::GarrisonError::TokenRevoked("test".to_string()),
            garrison::error::GarrisonError::ExpiredToken("test".to_string()),
        ];
        for err in cases {
            let auth_err = AuthError::from_garrison(err);
            match auth_err {
                AuthError::InvalidKey => {}
                other => panic!("expected InvalidKey, got {:?}", other),
            }
        }
    }

    /// R-auth-engine-003：403 DISABLE_SERVICE → InactiveKey
    #[test]
    fn test_from_garrison_403_disable_service_to_inactive_key() {
        let err = garrison::error::GarrisonError::DisableService {
            service: "default".to_string(),
            until: None,
        };
        let auth_err = AuthError::from_garrison(err);
        match auth_err {
            AuthError::InactiveKey => {}
            other => panic!("expected InactiveKey, got {:?}", other),
        }
    }

    /// R-auth-engine-003：403 其他（NOT_PERMISSION / NOT_ROLE / FIREWALL_BLOCKED / SMS_CHANNEL_RECYCLED）→ Forbidden
    #[test]
    fn test_from_garrison_403_others_to_forbidden() {
        let cases = vec![
            garrison::error::GarrisonError::NotPermission("test".to_string()),
            garrison::error::GarrisonError::NotRole("test".to_string()),
            garrison::error::GarrisonError::FirewallBlocked("bruteforce".to_string()),
            garrison::error::GarrisonError::SmsChannelRecycled,
        ];
        for err in cases {
            let auth_err = AuthError::from_garrison(err);
            match auth_err {
                AuthError::Forbidden(_) => {}
                other => panic!("expected Forbidden, got {:?}", other),
            }
        }
    }

    /// R-auth-engine-003：429 SMS_RATE_LIMIT_EXCEEDED → RateLimited
    #[test]
    fn test_from_garrison_429_to_rate_limited() {
        let err = garrison::error::GarrisonError::SmsRateLimitExceeded {
            window: "hourly".to_string(),
        };
        let auth_err = AuthError::from_garrison(err);
        match auth_err {
            AuthError::RateLimited => {}
            other => panic!("expected RateLimited, got {:?}", other),
        }
    }

    /// R-auth-engine-003：502 NETWORK_ERROR → NetworkError
    #[test]
    fn test_from_garrison_502_to_network_error() {
        let err = garrison::error::GarrisonError::Network("test".to_string());
        let auth_err = AuthError::from_garrison(err);
        match auth_err {
            AuthError::NetworkError(_) => {}
            other => panic!("expected NetworkError, got {:?}", other),
        }
    }

    /// R-auth-engine-003：501 NOT_IMPLEMENTED → NotImplemented
    #[test]
    fn test_from_garrison_501_to_not_implemented() {
        let err = garrison::error::GarrisonError::NotImplemented("test".to_string());
        let auth_err = AuthError::from_garrison(err);
        match auth_err {
            AuthError::NotImplemented(_) => {}
            other => panic!("expected NotImplemented, got {:?}", other),
        }
    }

    /// R-auth-engine-003：400（INVALID_PARAM / NOT_SAFE / SMS_VERIFY_MAX_ATTEMPTS / SMS_CODE_NOT_FOUND）→ InvalidParam
    #[test]
    fn test_from_garrison_400_to_invalid_param() {
        let cases = vec![
            garrison::error::GarrisonError::InvalidParam("test".to_string()),
            garrison::error::GarrisonError::NotSafe {
                reason: "MFA_TOTP_REQUIRED".to_string(),
            },
            garrison::error::GarrisonError::SmsVerifyMaxAttempts,
            garrison::error::GarrisonError::SmsCodeNotFound,
        ];
        for err in cases {
            let auth_err = AuthError::from_garrison(err);
            match auth_err {
                AuthError::InvalidParam(_) => {}
                other => panic!("expected InvalidParam, got {:?}", other),
            }
        }
    }

    /// R-auth-engine-003：500（DAO_ERROR / CONFIG_ERROR / INTERNAL_ERROR / SESSION_ERROR / ANNOTATION_ERROR / CONTEXT_ERROR / OAUTH2_ERROR / INVALID_STATE_TRANSITION）→ InternalError
    #[test]
    fn test_from_garrison_500_to_internal_error() {
        let cases = vec![
            garrison::error::GarrisonError::Dao("test".to_string()),
            garrison::error::GarrisonError::Config("test".to_string()),
            garrison::error::GarrisonError::Internal("test".to_string()),
            garrison::error::GarrisonError::Session("test".to_string()),
            garrison::error::GarrisonError::Annotation("test".to_string()),
            garrison::error::GarrisonError::Context("test".to_string()),
            garrison::error::GarrisonError::OAuth2("test".to_string()),
            garrison::error::GarrisonError::InvalidStateTransition {
                from: "A".to_string(),
                to: "B".to_string(),
            },
        ];
        for err in cases {
            let auth_err = AuthError::from_garrison(err);
            match auth_err {
                AuthError::InternalError(_) => {}
                other => panic!("expected InternalError, got {:?}", other),
            }
        }
    }

    /// R-auth-engine-003：Exception(code=-1) → InvalidKey（401）
    #[test]
    fn test_from_garrison_exception_code_neg1_to_invalid_key() {
        let err = garrison::error::GarrisonError::Exception(
            garrison::exception::GarrisonException::new(-1, "test"),
        );
        let auth_err = AuthError::from_garrison(err);
        match auth_err {
            AuthError::InvalidKey => {}
            other => panic!("expected InvalidKey, got {:?}", other),
        }
    }

    /// R-auth-engine-003：Exception(code=-2) → Forbidden（403）
    #[test]
    fn test_from_garrison_exception_code_neg2_to_forbidden() {
        let err = garrison::error::GarrisonError::Exception(
            garrison::exception::GarrisonException::new(-2, "test"),
        );
        let auth_err = AuthError::from_garrison(err);
        match auth_err {
            AuthError::Forbidden(_) => {}
            other => panic!("expected Forbidden, got {:?}", other),
        }
    }

    /// R-auth-engine-003：Exception(其他 code) → InternalError（500，fail-safe）
    #[test]
    fn test_from_garrison_exception_other_code_to_internal_error() {
        let err = garrison::error::GarrisonError::Exception(
            garrison::exception::GarrisonException::new(100, "test"),
        );
        let auth_err = AuthError::from_garrison(err);
        match auth_err {
            AuthError::InternalError(_) => {}
            other => panic!("expected InternalError, got {:?}", other),
        }
    }
}
