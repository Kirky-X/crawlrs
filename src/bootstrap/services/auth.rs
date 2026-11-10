// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! 认证与审计能力装配：garrison 鉴权、bootstrap admin key、任务队列与审计服务。

#[cfg(feature = "auth")]
mod bootstrap_admin_key;

#[cfg(all(test, feature = "auth"))]
mod auth_tests;

use std::sync::Arc;

use crate::bootstrap::infrastructure::InfrastructureComponents;
use crate::config::settings::Settings;
use crate::domain::services::audit_service::{AuditService, AuditServiceTrait};
use crate::infrastructure::database::repositories::audit_log_repo_impl::AuditLogRepositoryImpl;
use crate::queue::task_queue::{PostgresTaskQueue, TaskQueue};

#[cfg(feature = "auth")]
use bootstrap_admin_key::{
    announce_bootstrap_admin_key, bootstrap_admin_key_config, ensure_bootstrap_team,
    issue_bootstrap_admin_key, persist_bootstrap_api_key, resolve_bootstrap_team_id,
};

#[cfg(feature = "auth")]
use crate::bootstrap::error::BootstrapError;
#[cfg(feature = "auth")]
use crate::infrastructure::auth::{
    build_garrison_config, init_garrison_dao, set_audit_service, set_garrison_dao,
    CrawlrsGarrisonInterface,
};
#[cfg(feature = "auth")]
use dbnexus::DbPool;
#[cfg(feature = "auth")]
use garrison::prelude::{GarrisonDao, GarrisonInterface, GarrisonManager};
#[cfg(feature = "auth")]
use log::info;

/// 装配 garrison 认证鉴权与审计：任务队列 + 审计服务（含 garrison listener 全局态注入）。
pub(super) async fn init_auth_and_audit(
    #[cfg_attr(not(feature = "auth"), allow(unused_variables))] settings: &Settings,
    infrastructure: &InfrastructureComponents,
) -> (Arc<dyn TaskQueue>, Arc<dyn AuditServiceTrait>) {
    let repositories = &infrastructure.repositories;

    // 初始化 garrison 认证鉴权（auth-on 时 fail-fast）
    //
    // - 弱密钥 / 空密钥 → panic（强制运维提供强密钥，CWE-326）
    // - DAO oxcache 初始化失败 / GarrisonManager::init 失败 → panic
    //
    // 详见 [`init_garrison_auth`] 文档的「Fail-Fast 语义」章节。
    // 此调用无返回值——garrison 通过 GARRISON_MANAGER 全局单例暴露能力，
    // 后续中间件（重写 auth_middleware_inner）通过 GarrisonUtil 访问。
    //
    // pool 传 Arc<DbPool> 的 clone（Arc::clone 廉价 <10ns）。
    #[cfg(feature = "auth")]
    init_garrison_auth(settings, infrastructure.db.inner().clone())
        .await
        .expect(
            "garrison auth initialization failed (BootstrapError::GarrisonConfig/GarrisonDao/GarrisonManager) — \
                 check CRAWLRS__AUTH__JWT_SECRET env var (HS256 requires >=32 bytes)",
        );

    let queue: Arc<dyn TaskQueue> =
        Arc::new(PostgresTaskQueue::new(repositories.task_repo.clone()));

    let audit_repo = Arc::new(AuditLogRepositoryImpl::new(
        infrastructure.db.inner().clone(),
    ));
    let audit_service: Arc<dyn AuditServiceTrait> = Arc::new(AuditService::new(audit_repo));

    // 将 audit_service 注入 garrison listener 全局态。
    //
    // `set_audit_service` 通过 `parking_lot::RwLock<Option<Arc<…>>>` 暴露给
    // `CrawlrsAuditListener::on_event`，使 garrison 事件广播能桥接到 crawlrs
    // `audit_logs` 表。`RwLock`（而非 `OnceLock`）的理由：测试可通过
    // `reset_audit_service_for_test` 重置全局态，避免测试间污染。
    //
    // 时序：garrison init（在上方）通过 `inventory::submit!` 已注册 listener factory，
    // listener 实例在 `GarrisonManager::init` 时创建（无状态），但实际 `on_event`
    // 调用发生在第一个 HTTP 请求认证时（远晚于 bootstrap 完成），故此处注入时序安全。
    //
    // `set` 失败（已有实例）返回 Err，此处 panic——
    // 正常 bootstrap 路径只调用一次，二次调用表明初始化逻辑错误（如热重载未清理）。
    // `Arc<dyn AuditServiceTrait>` 未实现 Debug，故用 `is_err()` 而非 `expect` 处理。
    //
    // 注意：`init_services` 签名返回 `ServicesComponents` 而非
    // `Result<ServicesComponents, BootstrapError>`，故失败只能 panic。这是既有设计问题
    // （与上方 `init_garrison_auth().expect(...)` 一致），外科手术式修改不在此处重构，
    // 7 可考虑统一为 Result 返回类型。
    #[cfg(feature = "auth")]
    if set_audit_service(Arc::clone(&audit_service)).is_err() {
        panic!(
            "set_audit_service failed: audit_service already injected \
             (check for duplicate init_services calls)"
        );
    }

    (queue, audit_service)
}

/// 初始化 garrison 认证鉴权服务。
///
/// 装配流程：
/// 1. 调用 [`init_garrison_dao`] 获取 `Arc<dyn GarrisonDao>`（oxcache 内存存储，自管理实例）
/// 2. 调用 [`build_garrison_config`] 构造 `GarrisonConfig`（弱密钥拒绝，HS256 ≥32 字节）
/// 3. 构造 [`CrawlrsGarrisonInterface`] 并装为 `Arc<dyn GarrisonInterface>`
///    （注入 crawlrs 的 `DbPool` 用于查询 garrison RBAC 表）
/// 4. 调用 `GarrisonManager::init` 写入 `GARRISON_MANAGER` 全局单例
/// 5. 错误按故障层级映射为类型化变体
///    - 弱密钥 / 空密钥 → [`BootstrapError::GarrisonConfig`]（`#[from]` 自动转换）
///    - DAO oxcache 初始化失败 → [`BootstrapError::GarrisonDao`]
///    - `GarrisonManager::init` 失败 → [`BootstrapError::GarrisonManager`]
///
/// # Fail-Fast 语义
///
/// 此函数返回 `Result<(), BootstrapError>`，调用方（`init_auth_and_audit`）通过
/// `.expect()` 在失败时 panic，触发 bootstrap fail-fast：
/// - 弱密钥 / 空密钥 → 启动失败（强制运维提供强密钥）
/// - DAO oxcache 初始化失败 → 启动失败（内存资源不足）
/// - `GarrisonManager::init` 失败（配置非法等）→ 启动失败
///
/// 这是规则 12（失败必须显性化）的体现——不藏默认值背后。
///
/// # Arguments
///
/// * `settings` - 应用配置（含 `auth.jwt_secret`）
/// * `pool` - crawlrs 数据库连接池 `Arc<DbPool>`（注入到 `CrawlrsGarrisonInterface`
///   查询 RBAC 表；传 Arc 而非 DbPool）
///
/// # Returns
///
/// - `Ok(())` - garrison 初始化成功（`GARRISON_MANAGER` 单例已就绪，后续中间件通过
///   `GarrisonUtil` 访问）
/// - `Err(BootstrapError::GarrisonConfig)` - 弱密钥 / 空密钥 / 格式非法
/// - `Err(BootstrapError::GarrisonDao)` - DAO oxcache 初始化失败
/// - `Err(BootstrapError::GarrisonManager)` - `GarrisonManager::init` 失败
///
/// # Spec
///
/// - 构造 `CrawlrsGarrisonDao`/`build_garrison_config`/
///   `CrawlrsGarrisonInterface` 并调 `GarrisonManager::init`，失败按层级 map 为
///   `BootstrapError::GarrisonConfig` / `GarrisonDao` / `GarrisonManager`
#[cfg(feature = "auth")]
pub async fn init_garrison_auth(
    settings: &Settings,
    pool: Arc<DbPool>,
) -> Result<(), BootstrapError> {
    install_garrison(settings, &pool).await?;

    // 5. 决策 2：断言 garrison firewall 已启用（仅依赖 garrison 做 CWE-307 IP 限速）
    assert_bruteforce_firewall_compiled();

    // 6. 注入 bootstrap admin API key（若环境变量存在）。
    inject_bootstrap_admin_api_key(&pool).await?;

    info!("Garrison authentication manager initialized (token_style=jwt, HS256, firewall=enabled)");
    Ok(())
}

/// 1-4. 构造并安装 garrison 全局单例（config → DAO → interface → manager）。
#[cfg(feature = "auth")]
async fn install_garrison(settings: &Settings, pool: &Arc<DbPool>) -> Result<(), BootstrapError> {
    // 1. 构造 GarrisonConfig（弱密钥 / 空密钥在此被拒绝）
    //
    // 顺序选择：先做便宜的同步 config 校验，再做异步 DAO 初始化——
    // 弱密钥场景下避免无谓的 oxcache 实例创建（简洁优先的执行层面体现）。
    // 重构：使用类型化 `BootstrapError::GarrisonConfig`（#[from] 自动转换）。
    let config = build_garrison_config(settings.auth.jwt_secret())?;
    let config = Arc::new(config);

    // 2. 初始化 DAO（garrison 内建 oxcache，自管理实例）
    // 重构：使用类型化 `BootstrapError::GarrisonDao`。
    let dao: Arc<dyn GarrisonDao> = init_garrison_dao()
        .await
        .map_err(|e| BootstrapError::GarrisonDao(format!("{e}")))?;

    // 注入 DAO 到全局态，供业务 handler（`api_key_handler`）通过
    // [`crate::infrastructure::auth::get_garrison_dao`] 读取。
    //
    // 时序：在 `GarrisonManager::init` 之前注入——`GarrisonManager::init` 接收 dao 的
    // 所有权（move），故此处先 `Arc::clone` 再传入 manager，全局态保留另一份 Arc 引用。
    //
    // 失败契约（显性化）：`set_garrison_dao` 返回 `Err(dao)` 表示已有实例被注入
    // （正常 bootstrap 路径只调用一次，二次调用表明初始化逻辑错误如热重载未清理），
    // 此处 panic 与 `set_audit_service` 一致——避免静默覆盖导致 dao 来源不确定。
    set_garrison_dao(Arc::clone(&dao)).map_err(|_| {
        BootstrapError::GarrisonDao(
            "set_garrison_dao failed: global DAO already injected \
             (check for duplicate init_garrison_auth calls)"
                .to_string(),
        )
    })?;

    // 3. 构造业务 Interface（注入 crawlrs DbPool 查询 RBAC 表）
    //
    // `(**pool).clone()`：deref &Arc<DbPool> → DbPool，再 clone（DbPool 内部为
    // Arc<DbPoolInner>，clone 廉价 <10ns）。`CrawlrsGarrisonInterface::new` 持有
    // owned DbPool，符合其既有签名（外科手术式修改，不改动代码）。
    let interface: Arc<dyn GarrisonInterface> =
        Arc::new(CrawlrsGarrisonInterface::new((**pool).clone()));

    // 4. 写入 garrison 全局单例（builder 模式，异步 build）
    // 重构：使用类型化 `BootstrapError::GarrisonManager`。
    GarrisonManager::builder()
        .dao(dao)
        .config(config)
        .interface(interface)
        .build()
        .await
        .map_err(|e| BootstrapError::GarrisonManager(format!("{e}")))?;

    Ok(())
}

/// 5. 决策 2：断言 garrison firewall 已启用（仅依赖 garrison 做 CWE-307 IP 限速）。
///
/// crawlrs 已删除本地 `AuthRateLimiter`，暴力破解防护完全依赖 garrison 的
/// `firewall` + `firewall-bruteforce` features。若 features 未启用，garrison
/// 不会执行 IP 级限速，导致 CWE-307 防护缺失。
///
/// 此处通过引用 `BruteForceStrategy` 类型做编译期断言——若 garrison 未启用
/// `firewall-bruteforce` feature，该类型不存在，编译失败（fail-fast）。
/// 运行时无需额外检查（garrison 在 `check_api_key` 内部自动调用 firewall）。
///
/// 注：Cargo feature 是统一效应，此处断言仅防止"误关闭 features"配置错误，
/// 不防恶意依赖（见 auth_middleware.rs `reset_global_auth_state` 注释）。
///
/// ## 阈值配置限制
///
/// garrison 0.8.1 在 `check_api_key` 内部硬编码 `BruteForceConfig::default()`
/// （5 次失败 / 60s 窗口 / 300s 锁定），不通过 `GarrisonConfig` 暴露自定义入口。
/// 当前使用 garrison 默认值；若需自定义阈值，需等 garrison 后续版本暴露配置接口
/// 或在 crawlrs 侧实现自定义 `check_api_key` wrapper（暂不实施简洁优先）。
#[cfg(feature = "auth")]
fn assert_bruteforce_firewall_compiled() {
    // garrison 顶层 re-export（lib.rs:621，gated on `firewall-bruteforce` feature）
    use garrison::BruteForceStrategy as _FwAssert;
    // 引用类型做编译期检查（无运行时开销）
    let _ = std::marker::PhantomData::<_FwAssert>;
}

/// 6. 注入 bootstrap admin API key（若环境变量存在）。
///
/// ## 设计背景
///
/// garrison DAO 默认是内存实现（`GarrisonDaoOxcache`），进程间不共享数据。
/// 外部工具签发的 API Key 写入工具进程的
/// 独立 DAO 实例，server 进程无法读取 → 所有认证请求均 401。
///
/// 标准解法：server 启动时读取环境变量 `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY`，
/// 若存在则直接用 garrison `ApiKeyHandler::generate_with_namespace` 签发到
/// server 自己的 DAO（内存共享），同时写入 crawlrs `api_keys`/`teams` 表保留
/// `api_key_id → team_id` 映射（auth_middleware 反查依赖）。
///
/// ## 安全（CWE-532 / CWE-798）
///
/// - 明文 key 仅通过环境变量注入一次，从不写入日志或配置文件；
/// - garrison 侧仅存 `sha256(key_secret)`（CWE-916 与 generate_internal 一致）；
/// - 环境变量不存在或为空时完全跳过此分支（不影响部署未启用 bootstrap 的场景）。
///
/// ## 格式
///
/// `CRAWLRS__BOOTSTRAP_ADMIN_API_KEY` = `<team_id_uuid>`（可选）；
///   不传 team_id 时自动创建 `"bootstrap-admin-team"` team。
/// 内部签发双段格式：`key_id.key_secret`（`ApiKeyHandler::generate_with_namespace`），
/// key_id / key_secret 各 32 hex，TTL 30 天，scope `crawlrs:admin`。
#[cfg(feature = "auth")]
async fn inject_bootstrap_admin_api_key(pool: &Arc<DbPool>) -> Result<(), BootstrapError> {
    let Some(cfg_val) = bootstrap_admin_key_config() else {
        return Ok(());
    };

    let team_id =
        resolve_bootstrap_team_id(&cfg_val).unwrap_or_else(crate::domain::id::Id::generate);
    let api_key_id = crate::domain::id::Id::generate();

    let now = crate::common::time_utils::to_db_datetime(chrono::Utc::now());
    let session = pool.get_session("admin").await.map_err(|e| {
        BootstrapError::GarrisonManager(format!("bootstrap_admin_key: db session: {e}"))
    })?;
    let conn = session.connection().map_err(|e| {
        BootstrapError::GarrisonManager(format!("bootstrap_admin_key: db conn: {e}"))
    })?;

    ensure_bootstrap_team(conn, team_id, now).await?;
    let key = issue_bootstrap_admin_key(api_key_id).await?;
    persist_bootstrap_api_key(conn, api_key_id, team_id, &key.key_id, now).await?;
    announce_bootstrap_admin_key(&key.plaintext, team_id, api_key_id);

    Ok(())
}
