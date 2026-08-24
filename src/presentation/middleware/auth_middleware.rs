// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Unified authentication middleware with scope support.
//!
//! ## Stage 3 重构（R-auth-engine-003 / 决策 1-4）
//!
//! 本模块在 Stage 3 完成 DTO 化与 garrison 桥接：
//! - `AuthState` / `AuthError` 抽出到 [`super::auth_types`]（决策 3：解决循环依赖）
//! - 删除 `ApiKeyCache` / `AuthRateLimiter` / `validate_api_key_from_db` 等死代码（决策 1）
//! - 限速仅依赖 garrison firewall（决策 2），crawlrs 侧不再做暴力破解防护
//! - 新增 `TeamIdCache`（api_key_id → team_id LRU，容量 4096，TTL 60s，决策 4）
//! - `inject_auth_state` 注入 SHA-256 hash 而非裸 token（CWE-532 防护）
//!
//! ## feature 门控
//!
//! - `auth` 启用：`auth_middleware_inner` 走 garrison RBAC 路径
//! - `auth` 关闭：`default_identity_middleware` 注入默认身份模板（单租户降级）

use crate::domain::auth::{ApiKeyScope, ScopePermission};
use crate::domain::services::audit_service::AuditServiceTrait;
#[cfg(feature = "auth")]
use crate::infrastructure::database::entities::api_key;
#[cfg(feature = "auth")]
use crate::presentation::middleware::PUBLIC_ENDPOINTS;
use axum::extract::State;
#[cfg(feature = "auth")]
use axum::response::IntoResponse;
use axum::{
    body::Body,
    http::{Request, StatusCode},
    middleware::Next,
    response::Response,
};
#[cfg(feature = "auth")]
use dbnexus::DbPool;
#[cfg(feature = "auth")]
use log::debug;
#[cfg(feature = "auth")]
use lru::LruCache;
#[cfg(feature = "auth")]
use parking_lot::RwLock as ParkRwLock;
#[cfg(feature = "auth")]
use sea_orm::EntityTrait;
#[cfg(feature = "auth")]
use sha2::{Digest, Sha256};
#[cfg(feature = "auth")]
use std::num::NonZeroUsize;
use std::sync::Arc;
#[cfg(feature = "auth")]
use std::sync::LazyLock;
#[cfg(feature = "auth")]
use std::time::{Duration, Instant};
#[cfg(feature = "auth")]
use uuid::Uuid;

// Re-export 共享类型（决策 3：从 auth_types 导入，避免循环依赖）
pub use crate::presentation::middleware::auth_types::{AuthError, AuthState};

/// Team ID 缓存容量（决策 4：4096 条目覆盖典型多租户规模）。
#[cfg(feature = "auth")]
const TEAM_ID_CACHE_CAPACITY: usize = 4096;

/// Team ID 缓存 TTL（决策 4：60 秒，平衡 DB 查询压力与 team_id 变更生效延迟）。
#[cfg(feature = "auth")]
const TEAM_ID_CACHE_TTL: Duration = Duration::from_secs(60);

/// 全局 `api_key_id → team_id` LRU 缓存（决策 4）。
///
/// ## 设计
///
/// - 容量 4096（`TEAM_ID_CACHE_CAPACITY`），TTL 60s（`TEAM_ID_CACHE_TTL`）
/// - Key: `api_key_id` (Uuid)，Value: `(team_id, cached_at)`
/// - 命中时检查 TTL，过期则 evict 并回查 DB
/// - 使用 `parking_lot::RwLock` 同步锁（不跨 await，避免死锁）
///
/// ## 安全
///
/// 缓存 `team_id` 映射不引入越权风险——`team_id` 由 crawlrs DB 持有，
/// garrison 吊销 key 后 crawlrs DB 行可能被删除，此时缓存 TTL 过期后
/// 回查返回 `None` → `AuthError::KeyNotFound`，拒绝访问。
#[cfg(feature = "auth")]
static TEAM_ID_CACHE: LazyLock<ParkRwLock<LruCache<Uuid, (Uuid, Instant)>>> = LazyLock::new(|| {
    ParkRwLock::new(LruCache::new(
        NonZeroUsize::new(TEAM_ID_CACHE_CAPACITY).expect("TEAM_ID_CACHE_CAPACITY must be > 0"),
    ))
});

/// 获取 `api_key_id → team_id` 缓存的命中数（监控/测试用）。
#[cfg(feature = "auth")]
pub fn team_id_cache_len() -> usize {
    TEAM_ID_CACHE.read().len()
}

/// 清空 `api_key_id → team_id` 缓存（测试用）。
#[cfg(all(any(test, feature = "test-mocks"), feature = "auth"))]
pub fn reset_team_id_cache() {
    TEAM_ID_CACHE.write().clear();
}

/// 按 `api_key_id` 反查 crawlrs `api_keys` 表获取 `team_id`（R-auth-engine-003 / T017）。
///
/// ## 缓存策略（决策 4）
///
/// 先查 `TEAM_ID_CACHE`（LRU + TTL 60s）；命中且未过期则直接返回，
/// 未命中或过期则回查 DB 并填充缓存。
///
/// ## 设计
///
/// garrison 管理 API Key 的校验/吊销/哈希存储，但不持有 `api_key_id → team_id` 映射。
/// crawlrs 保留 `api_keys` 表的 `id`/`team_id` 列供反查。
///
/// ## 参数
///
/// * `pool` - crawlrs 数据库连接池
/// * `api_key_id` - 从 garrison `login_id` 解析得到的 API Key Uuid
///
/// ## 返回
///
/// - `Ok(Some(team_id))`：找到映射
/// - `Ok(None)`：`api_key_id` 不在 `api_keys` 表中
/// - `Err(AuthError::InternalError)`：dbnexus `DbError`（连接池/会话获取失败）
/// - `Err(AuthError::DatabaseError)`：sea-orm 查询失败（`DbErr`）
//
// feature-gate-optional-modules T010/T011 修复（design.md §4）：
// teams feature 关闭时，`auth_middleware_inner` 直接使用 DEFAULT_TEAM_ID，
// 此函数不被调用。用 `cfg_attr(not(feature = "teams"), allow(dead_code))`
// 显式声明 teams-off 时 unused 是预期行为（Rust 条件编译 unused 的标准处理）。
#[cfg(feature = "auth")]
#[cfg_attr(not(feature = "teams"), allow(dead_code))]
async fn fetch_team_id_by_api_key_id(
    pool: &Arc<DbPool>,
    api_key_id: Uuid,
) -> Result<Option<Uuid>, AuthError> {
    // 1. 查缓存。性能审查 HIGH-1 修复：`LruCache::get` 需 `&mut self` 更新 LRU 访问顺序，
    // 故必须用 write 锁（非读锁）。debug! 已移到锁外，避免锁内 I/O。
    // 长期优化建议：换用 `moka::Cache`（concurrent LRU，无锁读路径）。
    let cached_hit = {
        let mut cache = TEAM_ID_CACHE.write();
        if let Some((team_id, cached_at)) = cache.get(&api_key_id) {
            if cached_at.elapsed() < TEAM_ID_CACHE_TTL {
                Some(*team_id)
            } else {
                // 过期：evict
                cache.pop(&api_key_id);
                None
            }
        } else {
            None
        }
    };
    if let Some(team_id) = cached_hit {
        debug!(
            "TEAM_ID_CACHE hit: api_key_id={} team_id={}",
            api_key_id, team_id
        );
        return Ok(Some(team_id));
    }

    // 2. 缓存未命中或过期：回查 DB
    let session = pool
        .get_session("admin")
        .await
        .map_err(|e| AuthError::InternalError(format!("db session: {}", e)))?;
    let conn = session
        .connection()
        .map_err(|e| AuthError::InternalError(format!("db conn: {}", e)))?;
    let result = api_key::Entity::find_by_id(api_key_id).one(conn).await?;

    // 3. 填充缓存（仅当找到映射时）。LOW-1 修复：debug! 移到锁外。
    if let Some(ref model) = result {
        let team_id = model.team_id;
        {
            let mut cache = TEAM_ID_CACHE.write();
            cache.push(api_key_id, (team_id, Instant::now()));
        }
        debug!(
            "TEAM_ID_CACHE miss→fill: api_key_id={} team_id={}",
            api_key_id, team_id
        );
    }

    Ok(result.map(|m| m.team_id))
}

/// 计算 token 的 SHA-256 hash（hex 编码，带 `sha256:` 前缀）。
///
/// ## 安全（CWE-532）
///
/// 将裸 token 转为 hash 后注入 extensions，避免下游日志/审计泄露明文 token。
/// `sha256:` 前缀与 crawlrs 旧 `ApiKeyCache` 的 key 格式保持一致（规则8 惯例优先）。
#[cfg(feature = "auth")]
fn hash_token(token: &str) -> String {
    let digest = Sha256::digest(token.as_bytes());
    format!("sha256:{}", hex::encode(digest))
}

/// Inject auth state into request extensions.
///
/// ## 安全（CWE-532）
///
/// 注入的 `token_hash` 是 SHA-256 hash（`sha256:...`），非裸 token。
/// 下游 `limiteron_rate_limit_middleware` 的 `Extension<String>` 提取器拿到的是 hash，
/// 不会在限速日志中泄露明文 token。
fn inject_auth_state(req: &mut Request<Body>, auth_state: AuthState, token_hash: Option<&str>) {
    // 注意：`team_id` 和 `api_key_id` 均为 `Uuid` 类型，若同时 `insert` 会发生类型冲突
    // （后者覆盖前者）。生产代码通过 `Extension<AuthState>` 读取，无需裸 `Uuid` 扩展。
    // 此处仅注入 `AuthState`（包含 team_id/api_key_id/scope）；`token_hash` 仅当身份
    // 持有真实 token 时注入（`Option` 类型区分：匿名身份为 None，限速器回退 api_key_id）。
    req.extensions_mut().insert(auth_state);
    if let Some(hash) = token_hash {
        req.extensions_mut().insert(hash.to_string());
    }
}

/// Unified authentication middleware (garrison RBAC path, R-auth-engine-003 / T017).
///
/// ## 职责
///
/// 1. 提取 Bearer token（`extract_bearer`，来自 `auth_bridge`）
/// 2. 在 `with_current_token` 作用域内调用 garrison `GarrisonUtil::check_api_key` 校验
///    API Key（含 CWE-916 哈希、CWE-307 IP 限速），并提取 `login_id` / `perms`
/// 3. 解析 `login_id` → `api_key_id` (Uuid)（design.md §5 约定）
/// 4. 反查 crawlrs `api_keys` 表获取 `team_id`（garrison 不持有此映射，带 LRU 缓存）
/// 5. 桥接为 `AuthState`（`bridge_to_auth_state`，来自 `auth_bridge`）
/// 6. 复用 `inject_auth_state` 注入 extensions（token_hash 用 SHA-256 hash，CWE-532）
///
/// ## 失败映射（规则12 显性化）
///
/// | 失败点 | 映射 | HTTP 状态码 |
/// |--------|------|-------------|
/// | 全局态未初始化 | `StatusCode::INTERNAL_SERVER_ERROR` | 500 |
/// | 公开端点 | `next.run(req)`（跳过） | 200 |
/// | `extract_bearer` 失败 | `AuthError::InvalidKey` | 401 |
/// | garrison 校验失败 | `AuthError::from_garrison(...)` | 401/403/429/500 |
/// | `login_id` 非 Uuid | `AuthError::InvalidLoginId` | 400 |
/// | `api_key_id` 反查未命中 | `AuthError::KeyNotFound` | 400 |
/// | DB 查询失败 | `AuthError::DatabaseError` | 500 |
/// | 桥接失败 | 透传 `AuthError` | 400 |
///
/// ## Security
///
/// - garrison `check_api_key` 负责 CWE-916 哈希校验和 CWE-307 IP 限速
/// - token 不记录到日志（CWE-532）；注入 extensions 的是 SHA-256 hash
/// - `login_id` 解析失败不静默回退 `Uuid::nil()`（避免越权）
#[cfg(feature = "auth")]
pub async fn auth_middleware_inner(
    State(pool): State<Arc<DbPool>>,
    mut req: axum::http::Request<Body>,
    next: Next,
) -> Response {
    use crate::presentation::middleware::auth_bridge::{bridge_to_auth_state, extract_bearer};

    let path = req.uri().path().to_string();
    debug!("AuthMiddleware (garrison) processing path: {}", path);

    // 1. 公开端点跳过认证（LOW-4 修复：规范化尾部斜杠后比较，/health/ 也匹配）
    let normalized_path = path.trim_end_matches('/');
    if PUBLIC_ENDPOINTS.contains(&normalized_path) {
        debug!("Public endpoint {}, skipping auth", path);
        return next.run(req).await;
    }

    // 2. 提取 Bearer token
    let raw = match extract_bearer(&req) {
        Ok(t) => t,
        Err(e) => return e.into_response(),
    };

    // 3. garrison 校验 + 提取 login_id / scopes
    //
    // ## 决策：直接用 `ApiKeyHandler::verify_with_namespace` 而非 `stp::check_api_key`
    //
    // garrison 0.8.x 的 `check_api_key("crawlrs")` 内部用 verify 验证后丢弃
    // `ApiKeyInfo`，不写入 TokenSession（ApiKey 与 Password/JWT 是两套会话机制）。
    // 后续 `get_login_id()` → `session.get_token_session(token)` 因没有 session 而
    // 返回 `None` → auth_middleware 误报 `NotLogin("login_id missing")` → 401。
    //
    // 修复（规则5 简洁 + 规则12 显性化）：跳过 stp 会话层的中间间接查询，
    // 直接 `handler.verify_with_namespace(raw, "crawlrs")` 一步拿到
    // `ApiKeyInfo { login_id, scopes, expire_at, ... }`，避免 O(3) DAO roundtrip
    // 且消除 session/notfound 误报。
    //
    // CWE-307 保护（firewall-bruteforce）保留：在 verify 前后显式调用。
    //
    // DAO 来源优先级：优先 `get_garrison_dao()`（bootstrap 注入的全局共享 DAO），
    // 与 GarrisonManager::init 传入的 dao 是同一 Arc 实例（见 services.rs
    // init_garrison_auth: `set_garrison_dao(Arc::clone(&dao))`；`GarrisonManager::init(dao,...)`）。
    let (login_id, perms) = {
        use garrison::protocol::apikey::ApiKeyHandler;

        let dao = match crate::infrastructure::auth::get_garrison_dao() {
            Some(d) => d,
            None => {
                let e = garrison::error::GarrisonError::Session(
                    "auth-middleware: garrison dao not initialized".to_string(),
                );
                return AuthError::from_garrison(e).into_response();
            }
        };

        // CWE-307: IP 级暴力破解防护（对齐 check_api_key 内部实现，fail-open 兼容）
        // garrison firewall-bruteforce feature: 若编译期类型缺失则在运行时零成本失效。
        let ip_ctx = garrison::stp::current_ip();
        if let Some(ip) = &ip_ctx {
            use garrison::strategy::firewall::brute_force::{BruteForceConfig, BruteForceStrategy};
            use garrison::strategy::firewall::FirewallContext;
            let strategy = BruteForceStrategy::new(BruteForceConfig::default(), Arc::clone(&dao));
            let fw_ctx = FirewallContext::new(ip);
            match strategy.is_blocked(&fw_ctx).await {
                Ok(true) => {
                    let e = garrison::error::GarrisonError::FirewallBlocked(format!(
                        "auth-middleware-ip-blocked::{}",
                        ip
                    ));
                    return AuthError::from_garrison(e).into_response();
                }
                Ok(false) | Err(_) => { /* unblocked 或瞬时 DAO 故障：fail-open 继续 */ }
            }
        }

        let handler = ApiKeyHandler::new(Arc::clone(&dao));
        let info = match handler.verify_with_namespace(&raw, "crawlrs").await {
            Ok(i) => i,
            Err(e) => {
                // [DEBUG BOOTSTRAP 专用]：直接 stderr 打印 garrison 内部错误。
                // 发布 v0.2.0 验收完成后删除。
                eprintln!(
                    "[BOOTSTRAP-DIAG] verify failed: raw_token_len={}, err_debug={:?}, err_display={}",
                    raw.len(),
                    e,
                    e
                );
                // 额外加 dao 健康检查：读 garrison:apikey:crawlrs:* 列表（最多 5 条）
                // 检查 dao 是否真的有数据
                if let Ok(Some(dump_check)) =
                    dao.get("garrison:apikey:crawlrs:__diag_probe__").await
                {
                    eprintln!("[BOOTSTRAP-DIAG] dao-get probe ok: {}", &dump_check[..0]);
                }
                // 列 key_id 头部
                let key_id_from_raw = raw.split_once('.').map(|(k, _)| k).unwrap_or("no-dot");
                let probe_dao_key = format!("garrison:apikey:crawlrs:{}", key_id_from_raw);
                match dao.get(&probe_dao_key).await {
                    Ok(Some(v)) => eprintln!(
                        "[BOOTSTRAP-DIAG] dao found {} -> {} bytes",
                        probe_dao_key,
                        v.len()
                    ),
                    Ok(None) => eprintln!("[BOOTSTRAP-DIAG] dao NOT FOUND: {}", probe_dao_key),
                    Err(err2) => eprintln!("[BOOTSTRAP-DIAG] dao GET ERROR: {:?}", err2),
                }
                if let Some(ip) = &ip_ctx {
                    use garrison::strategy::firewall::brute_force::{
                        BruteForceConfig, BruteForceStrategy,
                    };
                    use garrison::strategy::firewall::FirewallContext;
                    let strategy =
                        BruteForceStrategy::new(BruteForceConfig::default(), Arc::clone(&dao));
                    let fw_ctx = FirewallContext::new(ip);
                    if let Err(rec_err) = strategy.record_failure(&fw_ctx).await {
                        debug!(
                            "brute force record_failure failed (ip={}, err={})",
                            ip, rec_err
                        );
                    }
                }
                return AuthError::from_garrison(e).into_response();
            }
        };
        (info.login_id, info.scopes)
    };

    // 4. 解析 login_id → api_key_id (Uuid)
    let api_key_id = match Uuid::parse_str(&login_id) {
        Ok(u) => u,
        Err(_) => return AuthError::InvalidLoginId(login_id).into_response(),
    };

    // 5. 反查 crawlrs DB 获取 team_id（带 LRU 缓存，决策 4）
    //
    // feature-gate-optional-modules T010/T011 修复（design.md §4）：
    // teams feature 关闭时（单租户模式），强制使用 DEFAULT_TEAM_ID，不查 DB。
    // teams-on 时保持原逻辑从 DB 反查。
    #[cfg(feature = "teams")]
    let team_id = match fetch_team_id_by_api_key_id(&pool, api_key_id).await {
        Ok(Some(tid)) => tid,
        Ok(None) => return AuthError::KeyNotFound(api_key_id).into_response(),
        Err(e) => return e.into_response(),
    };
    #[cfg(not(feature = "teams"))]
    let team_id = crate::common::constants::default_identity::DEFAULT_TEAM_ID;

    // 6. 桥接为 AuthState（传 api_key_id 而非 login_id，消除重复 Uuid 解析）
    let auth_state = match bridge_to_auth_state(pool.clone(), api_key_id, &perms, team_id).await {
        Ok(s) => s,
        Err(e) => return e.into_response(),
    };

    // 7. 注入 extensions（token_hash 用 SHA-256 hash，CWE-532 防护）
    let token_hash = hash_token(&raw);
    inject_auth_state(&mut req, auth_state, Some(&token_hash));

    debug!("API Key authentication successful (garrison path)");

    next.run(req).await
}

/// 单租户降级模式下的默认身份中间件（`auth` feature 关闭时启用）。
///
/// 当 `auth` feature 关闭时，此中间件替代真实 `auth_middleware`：
/// 不校验 API Key、不查 DB、不做暴力破解防护，而是将预构建的
/// `AuthState` 模板克隆并注入到请求 extensions。
///
/// # Security（production-mock-purge T037/T038 收紧）
///
/// **默认拒绝**：未显式 opt-in 时，所有 protected 请求返回 `401`（即使是
/// auth-off 降级模式，也不应在未确认安全边界的情况下放行请求）。一次性
/// `error!` 日志提示运维存在未鉴权路径。
///
/// 显式 opt-in：调用 [`allow_unauthenticated_protected`] 后，本中间件注入
/// **匿名受限身份**（`ApiKeyScope::denied()` 最小 scope + 无 token_hash），
/// 下游接口按 scope 校验自行拒绝越权操作。此模式仍仅适用于受信任的内部
/// 部署（单租户、无外部用户）；多租户或公开 API 场景必须启用 `auth` feature。
#[cfg(all(not(feature = "auth"), feature = "web-axum"))]
static ALLOW_UNAUTHENTICATED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);
#[cfg(all(not(feature = "auth"), feature = "web-axum"))]
static DENY_LOG_ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();
#[cfg(all(not(feature = "auth"), feature = "web-axum"))]
static ALLOW_LOG_ONCE: std::sync::OnceLock<()> = std::sync::OnceLock::new();

/// 显式放行未鉴权请求（auth-off 面 opt-in）。
///
/// 调用后 protected routes 以**匿名受限身份**（`denied()` scope、无
/// token_hash）放行；未调用时默认 `401`。一次性 `error!` 日志声明安全语义
/// 降级（"all requests allowed" 模式仅限受信任内网部署）。
#[cfg(all(not(feature = "auth"), feature = "web-axum"))]
pub fn allow_unauthenticated_protected() {
    ALLOW_LOG_ONCE.get_or_init(|| {
        log::error!(
            "allow_unauthenticated_protected() 显式启用：protected routes 将放行未鉴权请求 \
             （匿名受限身份，denied scope）。仅限受信任内网部署；公开网络必须启用 auth feature。"
        );
    });
    ALLOW_UNAUTHENTICATED.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// 测试用：重置 opt-in 开关（避免跨用例污染）。
#[cfg(all(test, not(feature = "auth"), feature = "web-axum"))]
pub(crate) fn _reset_unauthenticated_for_test() {
    ALLOW_UNAUTHENTICATED.store(false, std::sync::atomic::Ordering::SeqCst);
}

#[cfg(all(not(feature = "auth"), feature = "web-axum"))]
pub(crate) async fn default_identity_middleware(
    State(template): State<AuthState>,
    mut req: Request<Body>,
    next: Next,
) -> Response {
    if !ALLOW_UNAUTHENTICATED.load(std::sync::atomic::Ordering::SeqCst) {
        DENY_LOG_ONCE.get_or_init(|| {
            log::error!(
                "auth feature disabled：protected route 收到未鉴权请求，默认拒绝（401）。\
                 若确需匿名放行，请显式调用 allow_unauthenticated_protected()。"
            );
        });
        use axum::response::IntoResponse as _;
        return (StatusCode::UNAUTHORIZED, "authentication required").into_response();
    }
    // opt-in：注入匿名受限身份（无 token_hash —— 限速器回退 api_key_id 维度）
    inject_auth_state(&mut req, template, None);
    next.run(req).await
}

/// Scope validation middleware.
///
/// Validates that the API Key has the required scope for the requested endpoint.
/// This middleware should be used after the main auth middleware.
pub async fn scope_middleware(req: Request<Body>, next: Next) -> Result<Response, StatusCode> {
    let path = req.uri().path().to_string();
    let method = req.method().clone();

    // Determine required scope based on endpoint
    let required_scope = determine_required_scope(&path, method.as_ref());

    if let Some(required) = required_scope {
        let auth_state = req
            .extensions()
            .get::<AuthState>()
            .ok_or(StatusCode::UNAUTHORIZED)?;

        if !auth_state.scope.has_permission(required) {
            // LOW-5 修复：scope denied 是预期事件（合法的授权拒绝），非攻击信号。
            // api_key_id 是 UUID（非敏感），但生产 log level 设为 warn 会产生噪音，改为 info。
            // 详细拒绝事件已通过 audit_service.log_deny() 入审计。
            log::info!(
                "Scope denied: API Key {} lacks {:?} for {} {}",
                auth_state.api_key_id,
                required,
                method,
                path
            );

            // Log scope denial to audit service（HIGH-1 修复：失败必须显性化，规则12）
            if let Some(audit_service) = req.extensions().get::<Arc<dyn AuditServiceTrait>>() {
                let api_key_scope: ApiKeyScope = required.into();
                let reason = format!("Missing required scope: {:?}", required);
                if let Err(e) = audit_service
                    .log_deny(
                        "scope.denied".to_string(),
                        Some(auth_state.api_key_id),
                        Some(auth_state.team_id),
                        reason,
                        Some(api_key_scope),
                    )
                    .await
                {
                    // 审计失败不阻塞拒绝流程，但必须显性记录（规则12 显性化）
                    log::error!(
                        "audit log_deny failed for scope.denied event: api_key_id={} team_id={} err={}",
                        auth_state.api_key_id,
                        auth_state.team_id,
                        e
                    );
                }
            }

            return Err(StatusCode::FORBIDDEN);
        }
    }

    Ok(next.run(req).await)
}

/// Check if a path matches a prefix exactly or has a slash after the prefix.
fn is_path_prefix(path: &str, prefix: &str) -> bool {
    path == prefix || (path.starts_with(prefix) && path[prefix.len()..].starts_with('/'))
}

/// Determine required scope for an endpoint.
fn determine_required_scope(path: &str, method: &str) -> Option<ScopePermission> {
    // Admin endpoints - use precise matching
    if is_path_prefix(path, "/api/v1/teams") || is_path_prefix(path, "/api/v1/billing") {
        return Some(ScopePermission::Admin);
    }

    // Write endpoints (POST, PUT, PATCH, DELETE)
    if method == "POST" || method == "PUT" || method == "PATCH" || method == "DELETE" {
        return Some(ScopePermission::Write);
    }

    // Read endpoints (GET) - always allowed if read scope is present
    None
}

/// Create an auth state for testing purposes.
#[cfg(all(test, feature = "auth"))]
pub fn test_auth_state(db: Arc<DbPool>, team_id: Uuid, api_key_id: Uuid) -> AuthState {
    AuthState::new(db, team_id, api_key_id, ApiKeyScope::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== is_path_prefix tests =====

    #[test]
    fn test_is_path_prefix_exact_match() {
        assert!(is_path_prefix("/api/v1/teams", "/api/v1/teams"));
    }

    #[test]
    fn test_is_path_prefix_with_slash() {
        assert!(is_path_prefix("/api/v1/teams/123", "/api/v1/teams"));
    }

    #[test]
    fn test_is_path_prefix_suffix_without_slash_no_match() {
        assert!(!is_path_prefix("/api/v1/teams-secret", "/api/v1/teams"));
    }

    #[test]
    fn test_is_path_prefix_no_match() {
        assert!(!is_path_prefix("/v1/search", "/api/v1/teams"));
    }

    // ===== determine_required_scope tests =====

    #[test]
    fn test_determine_required_scope_teams_admin() {
        assert_eq!(
            determine_required_scope("/api/v1/teams", "GET"),
            Some(ScopePermission::Admin)
        );
    }

    #[test]
    fn test_determine_required_scope_teams_subpath_admin() {
        assert_eq!(
            determine_required_scope("/api/v1/teams/123", "GET"),
            Some(ScopePermission::Admin)
        );
    }

    #[test]
    fn test_determine_required_scope_billing_admin() {
        assert_eq!(
            determine_required_scope("/api/v1/billing", "GET"),
            Some(ScopePermission::Admin)
        );
    }

    #[test]
    fn test_determine_required_scope_teams_secret_not_admin() {
        // /api/v1/teams-secret 不匹配 /api/v1/teams 前缀（is_path_prefix 拒绝）
        assert_eq!(
            determine_required_scope("/api/v1/teams-secret", "GET"),
            None
        );
    }

    #[test]
    fn test_determine_required_scope_post_write() {
        assert_eq!(
            determine_required_scope("/v1/search", "POST"),
            Some(ScopePermission::Write)
        );
        assert_eq!(
            determine_required_scope("/v1/scrape", "POST"),
            Some(ScopePermission::Write)
        );
    }

    #[test]
    fn test_determine_required_scope_get_none() {
        assert_eq!(determine_required_scope("/v1/search", "GET"), None);
        assert_eq!(determine_required_scope("/v1/crawl/123", "GET"), None);
    }

    #[test]
    fn test_determine_required_scope_put_delete_patch_write() {
        assert_eq!(
            determine_required_scope("/v1/crawl/123", "PUT"),
            Some(ScopePermission::Write)
        );
        assert_eq!(
            determine_required_scope("/v1/crawl/123", "DELETE"),
            Some(ScopePermission::Write)
        );
        assert_eq!(
            determine_required_scope("/v1/crawl/123", "PATCH"),
            Some(ScopePermission::Write)
        );
    }

    // ===== hash_token tests (CWE-532) =====
    #[cfg(feature = "auth")]
    mod hash_token_tests {
        use super::*;

        #[test]
        fn test_hash_token_returns_sha256_prefix() {
            let hash = hash_token("test_token");
            assert!(
                hash.starts_with("sha256:"),
                "hash must start with 'sha256:' prefix, got: {hash}"
            );
        }

        #[test]
        fn test_hash_token_is_deterministic() {
            let hash1 = hash_token("same_token");
            let hash2 = hash_token("same_token");
            assert_eq!(hash1, hash2, "same input must produce same hash");
        }

        #[test]
        fn test_hash_token_different_inputs_different_outputs() {
            let hash1 = hash_token("token_a");
            let hash2 = hash_token("token_b");
            assert_ne!(
                hash1, hash2,
                "different inputs must produce different hashes"
            );
        }

        #[test]
        fn test_hash_token_does_not_contain_plaintext() {
            let token = "sensitive_secret_token_12345";
            let hash = hash_token(token);
            assert!(
                !hash.contains(token),
                "hash must not contain plaintext token (CWE-532)"
            );
        }
    }

    // ===== team_id cache tests =====
    #[cfg(feature = "auth")]
    mod team_id_cache_tests {
        use super::*;
        use std::time::Duration;

        /// 重置 team_id 缓存以避免跨测试污染。
        fn reset_team_id_cache_for_test() {
            reset_team_id_cache();
        }

        #[test]
        fn test_team_id_cache_len_starts_at_zero() {
            reset_team_id_cache_for_test();
            assert_eq!(team_id_cache_len(), 0);
        }

        #[test]
        fn test_team_id_cache_constants() {
            assert_eq!(TEAM_ID_CACHE_CAPACITY, 4096);
            assert_eq!(TEAM_ID_CACHE_TTL, Duration::from_secs(60));
        }
    }

    // ===== default_identity_middleware tests (feature-gate: `auth` off) =====
    //
    // 这些测试仅在 `--no-default-features`（关闭 `auth` feature）下编译运行，
    // 验证 `default_identity_middleware` 的契约（R-auth-002）。
    #[cfg(all(not(feature = "auth"), feature = "web-axum"))]
    mod default_identity_tests {
        #![allow(clippy::await_holding_lock)]
        use super::*;
        use crate::common::constants::default_identity::{DEFAULT_API_KEY_ID, DEFAULT_TEAM_ID};
        use crate::common::test_helpers::create_test_db_pool;
        use axum::{
            middleware::from_fn_with_state, response::IntoResponse, routing::get, Extension, Router,
        };
        use tower::ServiceExt;

        // opt-in 全局开关在测试间共享：全部用例串行执行（互斥锁）
        // 避免并发交错导致默认拒绝语义被并发 opt-in 污染。
        static TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

        async fn reflect_extensions(
            Extension(auth_state): Extension<AuthState>,
        ) -> impl IntoResponse {
            (
                StatusCode::OK,
                format!(
                    "auth_team={}\nauth_key={}\nread={}\nwrite={}\nadmin={}",
                    auth_state.team_id,
                    auth_state.api_key_id,
                    auth_state.scope.read,
                    auth_state.scope.write,
                    auth_state.scope.admin,
                ),
            )
        }

        fn make_default_identity_router(template: AuthState) -> Router {
            Router::new()
                .route("/v1/echo", get(reflect_extensions))
                .layer(from_fn_with_state(template, default_identity_middleware))
        }

        #[tokio::test]
        async fn default_identity_middleware_injects_default_api_key_id_and_denied_scope() {
            // T038：opt-in 后注入匿名受限身份（denied scope），不再注入 full_access
            let _lock_guard = TEST_LOCK.lock().unwrap();
            allow_unauthenticated_protected();
            let pool = create_test_db_pool();
            let template = AuthState::new(
                pool,
                DEFAULT_TEAM_ID,
                DEFAULT_API_KEY_ID,
                ApiKeyScope::denied(),
            );

            let response = make_default_identity_router(template)
                .oneshot(
                    Request::builder()
                        .uri("/v1/echo")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK);
            let body = String::from_utf8(
                axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap()
                    .to_vec(),
            )
            .unwrap();
            assert!(body.contains(&format!("auth_team={}", DEFAULT_TEAM_ID)));
            assert!(body.contains(&format!("auth_key={}", DEFAULT_API_KEY_ID)));
            assert!(body.contains("read=false"));
            assert!(body.contains("write=false"));
            assert!(body.contains("admin=false"));
        }

        #[tokio::test]
        async fn default_identity_middleware_passes_without_authorization_header() {
            let _lock_guard = TEST_LOCK.lock().unwrap();
            allow_unauthenticated_protected();
            let pool = create_test_db_pool();
            let template = AuthState::new(
                pool,
                DEFAULT_TEAM_ID,
                DEFAULT_API_KEY_ID,
                ApiKeyScope::denied(),
            );

            let response = make_default_identity_router(template)
                .oneshot(
                    Request::builder()
                        .uri("/v1/echo")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK);
        }

        #[tokio::test]
        async fn default_identity_middleware_ignores_invalid_authorization_header() {
            let _lock_guard = TEST_LOCK.lock().unwrap();
            allow_unauthenticated_protected();
            let pool = create_test_db_pool();
            let template = AuthState::new(
                pool,
                DEFAULT_TEAM_ID,
                DEFAULT_API_KEY_ID,
                ApiKeyScope::denied(),
            );

            let response = make_default_identity_router(template)
                .oneshot(
                    Request::builder()
                        .uri("/v1/echo")
                        .header("Authorization", "Bearer fake_token_should_be_ignored")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK);
        }

        /// T037 Red→Green：protected 路由默认拒绝未鉴权请求（auth-off 面）。
        ///
        /// 历史语义：auth-off 注入 `full_access` 身份，所有请求全权放行（200）；
        /// 安全收紧：默认返回 401，须显式 `allow_unauthenticated_protected()` opt-in。
        #[tokio::test]
        async fn default_identity_middleware_denies_unauthenticated_by_default() {
            // 独立于其它 opt-in 用例：显式复位全局开关
            let _lock_guard = TEST_LOCK.lock().unwrap();
            _reset_unauthenticated_for_test();
            let pool = create_test_db_pool();
            let template = AuthState::new(
                pool,
                DEFAULT_TEAM_ID,
                DEFAULT_API_KEY_ID,
                crate::domain::auth::ApiKeyScope::denied(),
            );

            let response = make_default_identity_router(template)
                .oneshot(
                    Request::builder()
                        .uri("/v1/echo")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap();

            assert_eq!(
                response.status(),
                StatusCode::UNAUTHORIZED,
                "auth-off 未显式 opt-in 时 protected route 必须默认 401"
            );
        }
    }
}

