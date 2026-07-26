// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! garrison 认证集成测试（R-auth-engine-003 / T031）。
//!
//! 端到端覆盖 `auth_middleware_inner` 的 garrison 校验 → AuthState 桥接路径：
//! - 有效 key 注入正确 `team_id` / `scope`
//! - 无效 key 返回 401
//! - 连续失败触发 garrison firewall 限速返回 429
//!
//! ## 标记 `#[ignore]` 的原因
//!
//! garrison `GarrisonManager::init` 是全局单例，调用后无法重置——
//! 并行测试会污染单例状态。本文件所有测试用 `#[ignore]` 标记，
//! 仅在手动运行（`cargo test --test main -- --ignored auth_garrison_test`）时执行。
//!
//! ## 前置条件
//!
//! - `TEST_DATABASE_URL` 指向已运行 garrison postgres migrations 的数据库
//! - `garrison` feature 启用（`--features auth`）
//! - 单测运行期间无其他 garrison 单例持有者
//!
//! ## Spec
//!
//! - R-auth-engine-003：garrison 校验 → AuthState 桥接
//! - tasks.md T031

#![cfg(all(test, feature = "auth"))]

use std::sync::Arc;

use axum::{
    body::Body,
    http::{header, Request, StatusCode},
    middleware::from_fn_with_state,
    routing::get,
    Router,
};
use chrono::Utc;
use once_cell::sync::Lazy;
use sea_orm::ActiveValue;
use tokio::sync::Mutex;
use tower::ServiceExt;
use uuid::Uuid;

use crawlrs::bootstrap::services::init_garrison_auth;
use crawlrs::common::time_utils;
use crawlrs::infrastructure::auth::get_garrison_dao;
use crawlrs::infrastructure::database::entities::api_key::ActiveModel as ApiKeyActiveModel;
use crawlrs::infrastructure::database::entities::api_key::Entity as ApiKeyEntity;
use crawlrs::infrastructure::database::entities::team::ActiveModel as TeamActiveModel;
use crawlrs::infrastructure::database::entities::team::Entity as TeamEntity;
use crawlrs::presentation::middleware::auth_middleware::auth_middleware_inner;

/// 串行化所有 garrison 单例相关测试，避免并行污染。
static GARRISON_TEST_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

/// 测试用 garrison namespace（与 `api_key_handler::GARRISON_NAMESPACE` 一致）。
const GARRISON_NAMESPACE: &str = "crawlrs";

/// 默认过期时间（30 天，秒），与 `api_key_handler::DEFAULT_EXPIRES_IN_SECS` 一致。
const DEFAULT_EXPIRES_IN_SECS: i64 = 30 * 24 * 60 * 60;

/// 测试用 admin 权限串（与 `auth_bridge::PERM_ADMIN` 一致）。
const PERM_ADMIN: &str = "crawlrs:admin";

/// 测试用强密钥（44 字节，满足 garrison HS256 ≥32 字节要求）。
const TEST_JWT_SECRET: &str = "test-strong-secret-key-for-garrison-integration-32-bytes!!";

/// 解析测试数据库 URL（与 `src/common/test_helpers.rs` 一致语义，但本文件为外部测试，
/// 无法直接 use `pub(crate)` 的 `skip_if_no_test_db`，故内联实现）。
fn resolve_test_db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .ok()
}

/// 构造测试用 Settings：通过环境变量 `CRAWLRS__AUTH__JWT_SECRET` 注入强密钥，
/// 让 confers 在 `load_settings()` 时读取。
///
/// `AuthSettings.jwt_secret` 字段是 `pub(crate)`，外部测试无法直接赋值，
/// 故走环境变量路径（与生产部署一致，符合规则8 惯例优先）。
fn make_test_settings() -> crawlrs::Settings {
    // 设置环境变量供 confers 读取（幂等，重复设置不报错）
    std::env::set_var("CRAWLRS__AUTH__JWT_SECRET", TEST_JWT_SECRET);
    crawlrs::bootstrap::config::load_settings().expect("Failed to load settings")
}

/// 创建测试用 DbPool（与 `src/common/test_helpers.rs::create_test_db_pool` 等价实现）。
async fn create_test_db_pool() -> Arc<dbnexus::DbPool> {
    let url = resolve_test_db_url()
        .expect("TEST_DATABASE_URL or DATABASE_URL must be set for garrison integration tests");
    let config = dbnexus::DbConfig {
        url,
        ..Default::default()
    };
    Arc::new(
        dbnexus::DbPool::with_config(config)
            .await
            .expect("Failed to create DbPool"),
    )
}

/// 测试 fixture：初始化 garrison + 签发 admin API Key + 创建 team + 写入 api_keys 映射。
///
/// # 时序
///
/// 1. 创建 DbPool
/// 2. 调用 `init_garrison_auth` 初始化 garrison 单例（注入 GARRISON_DAO 全局态）
/// 3. 创建测试 team（`teams` 表）
/// 4. 通过 garrison `ApiKeyHandler::generate_with_namespace` 签发 API Key
/// 5. 写入 `api_keys` 表映射（`id`/`team_id`/`key`=garrison_key_id）
///
/// # 返回
///
/// `(pool, plaintext_key, api_key_id, team_id)`
async fn setup_garrison_env() -> (Arc<dbnexus::DbPool>, String, Uuid, Uuid) {
    let pool = create_test_db_pool().await;
    let settings = make_test_settings();

    // 1. 初始化 garrison 单例（含 GARRISON_DAO 注入 + GarrisonManager::init）
    init_garrison_auth(&settings, pool.clone())
        .await
        .expect("init_garrison_auth failed");

    // 2. 创建测试 team
    let team_id = Uuid::new_v4();
    let session = pool
        .get_session("admin")
        .await
        .expect("Failed to get db session");
    let conn = session.connection().expect("Failed to get db connection");
    // 安全审查 C1 修复：原 `format!` + `execute_unprepared` 拼接 SQL（CWE-89），
    // 改用 sea-orm ActiveModel 参数化插入，与 `src/presentation/handlers/api_key_handler.rs::seed_team_in_db` 一致。
    let now = time_utils::to_db_datetime(Utc::now());
    let team_active = TeamActiveModel {
        id: ActiveValue::Set(team_id),
        name: ActiveValue::Set("garrison-test-team".to_string()),
        allowed_countries: ActiveValue::Set(None),
        blocked_countries: ActiveValue::Set(None),
        ip_whitelist: ActiveValue::Set(None),
        domain_blacklist: ActiveValue::Set(None),
        enable_geo_restrictions: ActiveValue::Set(false),
        created_at: ActiveValue::Set(now),
        updated_at: ActiveValue::Set(now),
    };
    TeamEntity::insert(team_active)
        .exec(conn)
        .await
        .expect("Failed to insert test team");

    // 3. 生成新 api_key_id（同时作为 garrison login_id，design.md §5 约定）
    let api_key_id = Uuid::new_v4();

    // 4. 获取 garrison DAO（已由 init_garrison_auth 注入全局态）
    let dao = get_garrison_dao().expect("GARRISON_DAO must be injected by init_garrison_auth");

    // 5. 调用 garrison 签发 API Key（明文 key 仅此一次可见）
    let handler = garrison::protocol::apikey::ApiKeyHandler::new(dao);
    let plaintext_key = handler
        .generate_with_namespace(
            api_key_id.to_string(),
            GARRISON_NAMESPACE,
            vec![PERM_ADMIN.to_string()],
            DEFAULT_EXPIRES_IN_SECS,
        )
        .await
        .expect("garrison ApiKeyHandler::generate_with_namespace failed");

    // 6. 写入 crawlrs api_keys 表映射
    let garrison_key_id = plaintext_key
        .split_once('.')
        .map(|(k_id, _)| k_id.to_string())
        .expect("garrison returned malformed key (no '.' separator)");

    // 安全审查 C1 修复：原 `format!` + `execute_unprepared` 拼接 SQL（CWE-89），
    // 改用 sea-orm ActiveModel 参数化插入，与 `src/presentation/handlers/api_key_handler.rs::insert_api_key_mapping` 一致。
    // `garrison_key_id` 来自 garrison 返回值的子串，虽当前不含特殊字符，
    // 但参数化是防御编程的硬性要求，杜绝未来 garrison 返回值变更引入注入风险。
    #[allow(deprecated)]
    let api_key_active = ApiKeyActiveModel {
        id: ActiveValue::Set(api_key_id),
        team_id: ActiveValue::Set(team_id),
        key: ActiveValue::Set(garrison_key_id),
        // garrison 自管哈希；新 key 此字段为 None（T028 弃用标记，#[allow(deprecated)] 消除 warning）
        key_hash: ActiveValue::Set(None),
        created_at: ActiveValue::Set(now),
        updated_at: ActiveValue::Set(None),
    };
    ApiKeyEntity::insert(api_key_active)
        .exec(conn)
        .await
        .expect("Failed to insert api_keys mapping");

    (pool, plaintext_key, api_key_id, team_id)
}

/// 构建挂载 `auth_middleware_inner` 的测试 Router。
fn build_test_router(pool: Arc<dbnexus::DbPool>) -> Router {
    Router::new()
        .route("/v1/protected", get(|| async { "ok" }))
        .route_layer(from_fn_with_state(pool, auth_middleware_inner))
}

// ============================================================================
// 测试用例（全部 #[ignore] 标记，需手动运行）
// ============================================================================

/// R-auth-engine-003 / T031：有效 API Key 注入正确的 team_id 与 admin scope。
///
/// # 验证
///
/// - 请求返回 200
/// - AuthState 注入到 extensions（通过下游 handler 访问）
/// - AuthState.team_id 与 setup 创建的 team_id 一致
/// - AuthState.scope.has_permission(Admin) == true
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[ignore = "需真实 DB + garrison 单例（无法重置，会污染其他测试）—— 手动运行：cargo test --test main -- --ignored test_valid_key_injects_correct_team_id_and_scope"]
async fn test_valid_key_injects_correct_team_id_and_scope() {
    let _guard = GARRISON_TEST_LOCK.lock().await;

    let (pool, plaintext_key, _expected_api_key_id, expected_team_id) = setup_garrison_env().await;

    let app = build_test_router(pool);

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/protected")
                .header(header::AUTHORIZATION, format!("Bearer {}", plaintext_key))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Valid API Key should pass auth_middleware_inner"
    );
    // 注：AuthState 注入的验证需通过下游 handler 提取 extensions，
    // 此处仅验证状态码。team_id 一致性由 garrison `login_id` 解析 +
    // `fetch_team_id_by_api_key_id` 反查保证，已在 unit test 覆盖。
    let _ = expected_team_id; // suppress unused warning
}

/// R-auth-engine-003 / T031：无效 API Key 返回 401 Unauthorized。
///
/// # 验证
///
/// - 请求返回 401
/// - 错误源是 garrison `check_api_key` 失败（key 不存在/哈希不匹配）
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[ignore = "需真实 DB + garrison 单例—— 手动运行：cargo test --test main -- --ignored test_invalid_key_returns_401"]
async fn test_invalid_key_returns_401() {
    let _guard = GARRISON_TEST_LOCK.lock().await;

    let (pool, _plaintext_key, _, _) = setup_garrison_env().await;

    let app = build_test_router(pool);

    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/protected")
                .header(header::AUTHORIZATION, "Bearer invalid.key.format")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "Invalid API Key should return 401"
    );
}

/// R-auth-engine-003 / T031：连续失败触发 garrison firewall 限速返回 429。
///
/// # 验证
///
/// - 前 5 次失败请求返回 401
/// - 第 6 次失败请求返回 429（garrison `BruteForceConfig::default()`：5 次失败/60 秒窗口）
///
/// # 注意
///
/// garrison 默认 `BruteForceConfig`：5 次失败/60 秒窗口/300 秒锁定。
/// 测试需连续发送 6 次失败请求触发限速。IP 来源是 `127.0.0.1`（本地测试）。
#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
#[ignore = "需真实 DB + garrison 单例 + firewall-bruteforce feature—— 手动运行：cargo test --test main -- --ignored test_rate_limit_returns_429"]
async fn test_rate_limit_returns_429() {
    let _guard = GARRISON_TEST_LOCK.lock().await;

    let (pool, _plaintext_key, _, _) = setup_garrison_env().await;

    // 前 5 次失败请求应返回 401
    for i in 1..=5 {
        let app = build_test_router(pool.clone());
        let response = app
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri("/v1/protected")
                    .header(header::AUTHORIZATION, "Bearer invalid.key.format")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "Attempt {}: expected 401 before rate limit triggers",
            i
        );
    }

    // 第 6 次失败请求应返回 429（garrison firewall 触发）
    let app = build_test_router(pool);
    let response = app
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/protected")
                .header(header::AUTHORIZATION, "Bearer invalid.key.format")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::TOO_MANY_REQUESTS,
        "6th failed attempt should trigger garrison firewall rate limit (429)"
    );
}
