// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! CrawlrsAuditListener — 桥接 garrison 事件到 crawlrs [`AuditServiceTrait`]。
//!
//! ## 设计背景（R-audit-firewall-001 / T024）
//!
//! garrison 内建 `AuditLogListener` 被 `#[cfg(feature = "db-sqlite")]` 门控，
//! 与 crawlrs PostgreSQL 后端不兼容。按用户决策（方案 A：自写 listener 桥接），
//! 本模块实现 [`GarrisonListener`] trait，将 [`GarrisonEvent`] 转换为
//! [`AuditLogEntry`] 并调用 [`AuditServiceTrait::log`] 持久化到 crawlrs 现有
//! `audit_logs` 表（PostgreSQL）。
//!
//! ## 时序与依赖注入
//!
//! `GarrisonManager::init` 通过 `inventory::iter` 收集监听器，发生在
//! [`init_garrison_auth`]（bootstrap 早期）。但 [`AuditServiceTrait`] 实例
//! 在 [`init_services`] 中创建（bootstrap 后期），时序上晚于 garrison 初始化。
//!
//! 解法：使用 [`parking_lot::RwLock`]`<Option<Arc<…>>>` 全局态持有
//! `Arc<dyn AuditServiceTrait>` 引用——
//! - 监听器在 `on_event` 中通过 `read()` 读取 audit_service（懒读）
//! - [`init_services`] 创建 audit_service 后调用 [`set_audit_service`] 注入
//! - 在注入前若 garrison 广播事件，`on_event` 仅 `log::warn!` 不中断
//! - 测试可通过 [`reset_audit_service_for_test`] 重置全局态（解决测试污染）
//!
//! ## 背压与可观察性
//!
//! `on_event` 通过 [`tokio::task::JoinSet`] 管理 inflight audit write task，
//! 同时用 [`tokio::sync::Semaphore`]（容量 `AUDIT_INFLIGHT_LIMIT`）限制并发数，
//! 避免 DB 异常时 task 雪崩 OOM。Semaphore 满时 drop 事件 + warn。
//! [`wait_audit_tasks`] 供 shutdown 时优雅等待 inflight task 完成。
//!
//! ## 失败契约（规则12 显性化）
//!
//! 审计日志是 best-effort persistence，存在以下失败场景：
//! - **audit_service 未注入**：bootstrap 完成前的事件被 drop + warn
//! - **背压满**：inflight task 数超过 `AUDIT_INFLIGHT_LIMIT` 时 drop + warn
//! - **DB 写入失败**：`audit_service.log()` 返回 Err 时 warn + metrics counter
//! - **进程崩溃**：inflight task 中的事件可能丢失（`panic = "abort"` 下立即终止）
//!
//! 设计权衡：监听器失败不传播（与 garrison `AuditLogListener` 契约一致），
//! 不阻塞 garrison 主流程。多租户审计完整性通过 `metadata.garrison_tenant_id`
//! 记录原始 i64 tenant_id 供追溯（不强行转换为 crawlrs team_id Uuid，避免语义错位）。
//!
//! ## 安全防护
//!
//! - **token 脱敏（CWE-532）**：[`GarrisonEvent`] 携带的 token 不写入
//!   [`AuditLogEntry`]，仅在 metadata 中记录前 8 字符前缀（与 garrison
//!   `mask_audit_token` 一致）。短 token（< 16 字符）直接脱敏为 `***…`，
//!   避免完整原文落入 metadata
//! - **失败不传播**：[`on_event`] 失败仅 `log::warn!`，返回 `Ok(())`
//!   （与 garrison `AuditLogListener` 行为一致：监听器失败不中断主流程）
//! - **异步写入**：通过 [`JoinSet`] 异步执行避免阻塞 garrison 主流程
//! - **字段长度限制（CWE-400 防 DoS）**：所有 metadata 字段均截断到上限：
//!   - `User-Agent`：`MAX_USER_AGENT_LEN`（256 字符）
//!   - `denial_reason`：`MAX_DENIAL_REASON_LEN`（256 字符）
//!   - `login_id`：`MAX_LOGIN_ID_LEN`（128 字符）
//!   - `permission`：`MAX_PERMISSION_LEN`（128 字符，安全审查 M-1 修复）
//!   - `role`：`MAX_ROLE_LEN`（128 字符，安全审查 M-2 修复）
//!   - `device`：`MAX_DEVICE_LEN`（256 字符，安全审查 M-3 修复）
//!   - `user_id`：`MAX_USER_ID_LEN`（128 字符，安全审查 M-4 修复）
//!   - `provider`：`MAX_PROVIDER_LEN`（64 字符，安全审查 L-1 修复）
//!   - `request_ip_raw`：`MAX_RAW_IP_LEN`（64 字符，IP 解析失败时记录原始字符串）
//! - **审计完整性（CWE-778）**：
//!   - login_id 始终写入 metadata（即使 api_key_id 解析失败，原始 login_id 仍保留）
//!   - IP 解析失败时记录原始字符串到 `metadata.request_ip_raw`（安全审查 L-2 修复），
//!     避免恶意 X-Forwarded-For 构造非 IP 字符串绕过 IP 记录
//!   - 多租户审计通过 `metadata.garrison_tenant_id` 记录原始 i64 tenant_id
//!
//! ## Spec
//!
//! - R-audit-firewall-001：注册 garrison `listener` 审计监听器，订阅
//!   Login/Logout/PermissionCheck/Kickout 事件，经 crawlrs `AuditServiceTrait` 输出

use crate::domain::auth::{AuditDecision, AuditLogEntry};
use crate::domain::services::audit_log_builder::AuditLogBuilder;
use crate::domain::services::audit_service::AuditServiceTrait;
use async_trait::async_trait;
use garrison::context::tenant::current_tenant_id_strict;
use garrison::error::GarrisonResult;
use garrison::listener::{GarrisonEvent, GarrisonListener, RequestContext};
use parking_lot::RwLock;
use std::net::IpAddr;
use std::sync::{Arc, LazyLock, Mutex as StdMutex};
use std::time::Duration;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;

#[cfg(test)]
use tokio::sync::Mutex;

/// inflight audit write task 上限。
///
/// 超过此值时 drop 新事件 + warn（背压，避免 DB 异常时 task 雪崩 OOM）。
/// 64 个并发 INSERT 足以支撑 crawlrs 认证 QPS（典型 < 100 QPS）。
const AUDIT_INFLIGHT_LIMIT: usize = 64;

/// token 脱敏的最小长度阈值。短于此长度的 token 直接脱敏为 `***…`，
/// 避免短 API key 完整泄露到 audit_logs metadata（CWE-532）。
const MASK_TOKEN_MIN_LEN: usize = 16;

/// token 脱敏保留的前缀字符数。
const MASK_TOKEN_PREFIX_LEN: usize = 8;

/// User-Agent 字段最大长度（截断防 DoS，CWE-400）。
///
/// 256 字符覆盖正常浏览器 UA（典型 < 200 字符），超长截断避免 audit_logs 表膨胀。
const MAX_USER_AGENT_LEN: usize = 256;

/// denial_reason 字段最大长度（截断防 DoS）。
///
/// garrison reason 通常 < 100 字符（如 "brute_force: 5 failures"），
/// 256 字符上限足够，超长截断避免 audit_logs 表膨胀。
const MAX_DENIAL_REASON_LEN: usize = 256;

/// login_id metadata 字段最大长度（截断防 DoS）。
const MAX_LOGIN_ID_LEN: usize = 128;

/// permission metadata 字段最大长度（截断防 DoS，安全审查 M-1 修复）。
///
/// permission 标识符如 `crawlrs:admin`/`crawlrs:read` 通常 < 32 字符，
/// 128 字符上限足够，超长截断避免 audit_logs 表膨胀。
const MAX_PERMISSION_LEN: usize = 128;

/// role metadata 字段最大长度（截断防 DoS，安全审查 M-2 修复）。
///
/// role 标识符如 `admin`/`user`/`team_lead` 通常 < 32 字符，
/// 128 字符上限足够，超长截断避免 audit_logs 表膨胀。
const MAX_ROLE_LEN: usize = 128;

/// device metadata 字段最大长度（截断防 DoS，安全审查 M-3 修复）。
///
/// device 标识符可能由客户端提供（设备指纹/User-Agent 摘要），
/// 256 字符上限与 User-Agent 一致，超长截断避免 audit_logs 表膨胀。
const MAX_DEVICE_LEN: usize = 256;

/// user_id metadata 字段最大长度（截断防 DoS，安全审查 M-4 修复）。
///
/// 社交登录 provider 返回的 user_id 通常为数字 ID 或短字符串，
/// 128 字符上限足够，超长截断避免 audit_logs 表膨胀。
const MAX_USER_ID_LEN: usize = 128;

/// provider metadata 字段最大长度（截断防 DoS，安全审查 L-1 修复）。
///
/// social provider 标识符如 `github`/`google`/`microsoft` 通常 < 16 字符，
/// 64 字符上限足够，超长截断避免 audit_logs 表膨胀。
const MAX_PROVIDER_LEN: usize = 64;

/// 原始 IP 字符串 metadata 字段最大长度（IP 解析失败时记录，安全审查 L-2 修复）。
///
/// IPv6 最长 45 字符（`ffff:ffff:ffff:ffff:ffff:ffff:255.255.255.255`），
/// 留余量到 64 字符，恶意构造超长值时截断。
const MAX_RAW_IP_LEN: usize = 64;

/// 全局 [`AuditServiceTrait`] 引用，由 [`set_audit_service`] 注入，由 [`CrawlrsAuditListener::on_event`] 读取。
///
/// 使用 [`parking_lot::RwLock`]`<Option<…>>` 而非 [`std::sync::OnceLock`] 的理由：
/// - 测试可通过 [`reset_audit_service_for_test`] 重置全局态，避免测试间污染
/// - `read()` 是共享读锁，热路径无竞争（bootstrap 后只读不变）
/// - `set_audit_service` 在已有实例时返回 `Err`，避免静默覆盖
static AUDIT_SERVICE: RwLock<Option<Arc<dyn AuditServiceTrait>>> = RwLock::new(None);

/// 全局 inflight task 计数信号量，用于背压控制。
///
/// `on_event` 在 spawn 前 `try_acquire`，task 完成后 `drop(permit)`。
/// 满时 drop 事件 + warn，避免 DB 异常时 task 雪崩。
static AUDIT_SEMAPHORE: LazyLock<Semaphore> =
    LazyLock::new(|| Semaphore::new(AUDIT_INFLIGHT_LIMIT));

/// 全局 inflight audit task 集合，用于 shutdown 时优雅等待。
///
/// `on_event` 通过 `JoinSet::spawn` 创建 task（由 JoinSet 持有 JoinHandle），
/// [`wait_audit_tasks`] 供 shutdown 时 `join_all().await` 等待完成。
///
/// 使用 `std::sync::Mutex` 而非 `tokio::sync::Mutex` 的理由：
/// - `spawn` 是同步方法，不跨 await 持有锁
/// - `wait_audit_tasks` 中 `std::mem::take` 取出 JoinSet 后立即释放锁，再 await
static AUDIT_TASKS: LazyLock<StdMutex<JoinSet<()>>> =
    LazyLock::new(|| StdMutex::new(JoinSet::new()));

/// 测试串行化锁，避免 `AUDIT_SERVICE` 全局态在并行测试中竞态。
///
/// 涉及 `set_audit_service`/`reset_audit_service_for_test` 的测试通过此锁串行化。
/// `#[tokio::test]` 默认并行执行，全局 `RwLock` 会污染——串行化是必要折衷。
///
/// T034 修复：从 `std::sync::Mutex` 改为 `tokio::sync::Mutex`（经 `OnceLock` 延迟初始化）。
/// 原因：`std::sync::MutexGuard` 跨 await 持有会阻塞 runtime 线程，导致并行测试死锁。
/// `tokio::sync::MutexGuard` 是 async-aware，安全跨 await 持有。
/// 与 `garrison_dao::TEST_MUTEX` 同一模式（OnceLock + tokio::sync::Mutex）。
#[cfg(test)]
static TEST_MUTEX: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();

/// 注入 crawlrs [`AuditServiceTrait`] 实例，供 [`CrawlrsAuditListener`] 后续读取。
///
/// # 调用时序
///
/// 在 [`crate::bootstrap::services::init_services`] 中创建 `audit_service` 之后调用，
/// 早于 garrison 第一次广播事件（实际首次广播发生在第一个 HTTP 请求认证时，
/// 远晚于 bootstrap 完成）。
///
/// # 参数
///
/// - `service`: `Arc<dyn AuditServiceTrait>` 实例（来自 `AuditService::new(repo)`）
///
/// # 返回
///
/// - `Ok(())` — 注入成功（先前为 None）
/// - `Err(service)` — 已有实例被注入（返回传入的 service 让调用方处理）
///
/// # Spec
///
/// - R-audit-firewall-001
pub fn set_audit_service(
    service: Arc<dyn AuditServiceTrait>,
) -> Result<(), Arc<dyn AuditServiceTrait>> {
    let mut guard = AUDIT_SERVICE.write();
    if guard.is_some() {
        return Err(service);
    }
    *guard = Some(service);
    Ok(())
}

/// 重置全局 [`AUDIT_SERVICE`]（仅测试用）。
///
/// 单测在 setup/teardown 中调用以避免测试间全局态污染。
/// 生产代码禁止调用——会导致 audit_service 丢失，事件被静默 drop。
///
/// T034 修复：暴露为 `pub(crate)` 以供 `test_helpers::reset_garrison_global_state_for_test`
/// 统一调用——所有调用 `init_services` 的测试都需 reset AUDIT_SERVICE 全局态。
#[cfg(test)]
pub(crate) fn reset_audit_service_for_test() {
    *AUDIT_SERVICE.write() = None;
}

/// 测试专用：获取 [`TEST_MUTEX`] 的引用（用于跨模块串行化）。
///
/// T034 修复：与 `garrison_dao::test_mutex` 同一模式，供 `test_helpers`
/// 统一获取锁，避免调用 `init_services` 的测试间全局态竞态。
#[cfg(test)]
pub(crate) fn test_mutex() -> &'static Mutex<()> {
    TEST_MUTEX.get_or_init(|| Mutex::new(()))
}

/// 读取全局 [`AuditServiceTrait`] 引用（clone `Arc`）。
///
/// 供 [`CrawlrsAuditListener::on_event`] 在热路径调用——`read()` 是共享读锁，
/// 不阻塞其他读者。返回 `Option<Arc<…>>`，未注入时为 `None`。
fn get_audit_service() -> Option<Arc<dyn AuditServiceTrait>> {
    AUDIT_SERVICE.read().clone()
}

/// 等待所有 inflight audit task 完成（shutdown 时调用）。
///
/// 取出全局 [`JoinSet`] 并 `join_all().await`，超时后返回（不阻塞 shutdown）。
/// 取出后新事件会创建新的空 JoinSet（shutdown 期间不再有新事件，安全）。
///
/// # 参数
///
/// - `timeout`: 最大等待时间（建议 5-10s，平衡 shutdown 速度与审计完整性）
///
/// # 行为
///
/// - 取出 JoinSet（`std::mem::take`），立即释放 `Mutex` 锁
/// - `join_all().await` 等待所有 inflight task 完成
/// - 超时后返回未完成的 task 被 abort（JoinSet drop 时自动 abort）
///
/// # Race Condition 说明（架构审查 M3）
///
/// `std::mem::take` 取出 JoinSet 后到 `join_all` 完成期间，若仍有 `on_event`
/// 调用（理论不应有，但实际可能因 shutdown 期间 garrison 还在广播事件）：
/// - 新事件会 spawn 到新创建的空 JoinSet 中（`LazyLock` 初始化的 Mutex 内默认值）
/// - 这些新 task **不会被当前 `wait_audit_tasks` 等待**——它们会在进程退出时
///   被 `JoinSet::drop` 自动 abort（数据丢失）
///
/// 设计权衡：
/// - shutdown 期间新事件丢失是 best-effort 审计契约的合理折衷（与 garrison
///   `AuditLogListener` 行为一致：监听器失败不阻塞主流程）
/// - 不增加 "shutting down" atomic flag 的理由：
///   1. shutdown 是边界场景，新增 flag 增加复杂度但收益有限
///   2. 进程退出时 `panic = "abort"` 会立即终止所有 task，flag 无法保证持久化
///   3. 真正的 shutdown 顺序保证应由调用方（`init_services` 之后的 shutdown
///      hook）控制，先停止 HTTP listener 再 `wait_audit_tasks`
///
/// 如需更严格的 shutdown 顺序保证，调用方应在 `wait_audit_tasks` 前先关闭
/// HTTP listener（停止新事件来源），再调用本函数等待 inflight task。
///
/// # Spec
///
/// - R-audit-firewall-001：shutdown 时优雅等待 inflight audit task
pub async fn wait_audit_tasks(timeout: Duration) {
    let join_set = std::mem::take(&mut *AUDIT_TASKS.lock().expect("AUDIT_TASKS poisoned"));
    let _ = tokio::time::timeout(timeout, join_set.join_all()).await;
    // join_set drop 时未完成的 task 被 abort
}

/// crawlrs 自有的 garrison 事件监听器。
///
/// 实现 [`GarrisonListener`] trait，将 [`GarrisonEvent`] 转换为 [`AuditLogEntry`]
/// 并调用 [`AuditServiceTrait::log`] 持久化到 crawlrs `audit_logs` 表。
///
/// # 设计决策（用户方案 A）
///
/// 不复用 garrison `AuditLogListener`（`db-sqlite` feature，与 PostgreSQL 不兼容），
/// 而是自实现 listener 桥接到 crawlrs 现有 [`AuditServiceTrait`]。理由：
/// - crawlrs 已有 PostgreSQL-backed `audit_logs` 表 + `AuditLogRepository` 实例
/// - 复用现有审计查询 API（`audit_handler` 的 `GET /audit/logs`）
/// - 避免双写 SQLite + PostgreSQL 导致审计数据分裂
pub struct CrawlrsAuditListener;

impl CrawlrsAuditListener {
    /// 创建监听器实例。
    ///
    /// 监听器自身无状态——`audit_service` 通过 [`RwLock`] 全局注入，
    /// 避免 `inventory::submit!` factory 函数需要构造时拿到 `Arc<dyn AuditServiceTrait>`
    /// 的时序难题（garrison init 时 audit_service 尚未创建）。
    pub fn new() -> Self {
        Self
    }
}

impl Default for CrawlrsAuditListener {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl GarrisonListener for CrawlrsAuditListener {
    /// 事件处理：转换 + 异步写入，失败不传播。
    ///
    /// # 行为
    ///
    /// 1. 调用 [`event_to_audit_entry`] 将 [`GarrisonEvent`] 转换为 [`AuditLogEntry`]
    /// 2. 通过 [`get_audit_service`] 读取全局 audit_service
    ///    - 未注入：`log::warn!` 后返回 `Ok(())`（不阻塞 garrison 主流程）
    /// 3. 通过 [`Semaphore::try_acquire`] 获取 inflight permit（背压）
    ///    - 满时 drop 事件 + warn + metrics counter（避免 DB 异常时 task 雪崩 OOM）
    /// 4. 通过 [`JoinSet::spawn`] 异步调用 `audit_service.log(entry)`
    ///    - permit 随 task move，task 完成后自动释放
    ///    - 同步等待会阻塞 garrison 事件广播（影响认证延迟）
    ///    - 失败仅 `log::warn!` + metrics counter，不传播错误（监听器契约）
    ///    - shutdown 时可通过 [`wait_audit_tasks`] 优雅等待 inflight task
    ///
    /// # 背压
    ///
    /// `AUDIT_SEMAPHORE`（容量 `AUDIT_INFLIGHT_LIMIT`=64）限制并发 INSERT 数。
    /// 超过上限时 drop 事件 + warn，避免 DB 异常时 task 无限堆积导致 OOM
    /// （性能审查 HIGH-1 修复）。
    ///
    /// # 失败契约
    ///
    /// 审计日志是 best-effort persistence（见模块文档「失败契约」章节）。
    /// 监听器失败不传播（与 garrison `AuditLogListener` 契约一致）。
    ///
    /// # Spec
    ///
    /// - R-audit-firewall-001：订阅 Login/Logout/PermissionCheck/Kickout 事件
    ///   （实际订阅所有 [`GarrisonEvent`] 变体，spec 列举的为关键子集）
    async fn on_event(&self, event: &GarrisonEvent) -> GarrisonResult<()> {
        let entry = match event_to_audit_entry(event) {
            Ok(e) => e,
            Err(e) => {
                log::warn!(
                    "crawlrs audit listener: failed to convert garrison event to audit entry: {}",
                    e
                );
                return Ok(());
            }
        };

        let service = match get_audit_service() {
            Some(s) => s,
            None => {
                log::warn!(
                    "crawlrs audit listener: audit_service not yet injected, \
                     event dropped (this is expected before bootstrap completes)"
                );
                return Ok(());
            }
        };

        // 背压：获取 inflight permit，满时 drop 事件（避免 DB 异常时 task 雪崩）
        let permit = match AUDIT_SEMAPHORE.try_acquire() {
            Ok(p) => p,
            Err(_) => {
                log::warn!(
                    "crawlrs audit listener: inflight limit ({}) reached, \
                     event dropped (DB may be slow or down)",
                    AUDIT_INFLIGHT_LIMIT
                );
                #[cfg(feature = "metrics")]
                metrics::counter!("crawlrs_audit_log_dropped_total").increment(1);
                return Ok(());
            }
        };

        // 通过 JoinSet 管理 inflight task，shutdown 时可优雅等待
        // audit_service.log() 内部走 sea-orm PostgreSQL INSERT，可能耗时 1-5ms
        // permit 随 task move，task 完成后自动释放（drop permit → semaphore 计数 +1）
        let mut guard = AUDIT_TASKS.lock().expect("AUDIT_TASKS poisoned");
        guard.spawn(async move {
            let _permit = permit; // 持有 permit 直到 log 完成
            if let Err(e) = service.log(entry).await {
                log::warn!(
                    "crawlrs audit listener: failed to persist audit log entry: {}",
                    e
                );
                #[cfg(feature = "metrics")]
                metrics::counter!("crawlrs_audit_log_failures_total").increment(1);
            }
        });

        Ok(())
    }
}

/// 将 [`GarrisonEvent`] 转换为 [`AuditLogEntry`]。
///
/// # 转换规则
///
/// | GarrisonEvent | AuditDecision | requested_action |
/// |---------------|---------------|------------------|
/// | Login | Allow | "auth.login" |
/// | Logout | Allow | "auth.logout" |
/// | LoginFailure | Deny | "auth.login" |
/// | Kickout | Deny | "auth.kickout" |
/// | PermissionCheck | Allow | "auth.permission_check" |
/// | RoleCheck | Allow | "auth.role_check" |
/// | TokenExpired | Deny | "auth.token_expired" |
/// | TokenRefresh | Allow | "auth.token_refresh" |
/// | TokenRotate | Allow | "auth.token_rotate" |
/// | RevokeToken | Allow | "auth.revoke_token" |
/// | SessionTimeout | Deny | "auth.session_timeout" |
/// | AccountLocked | Deny | "auth.account_locked" |
/// | FirewallBlock | Deny | "auth.firewall_block" |
/// | Replaced | Deny | "auth.replaced" |
/// | SocialLogin | Allow | "auth.social_login" |
/// | TenantSwitch | Allow | "auth.tenant_switch" |
/// | DeviceBlock | Deny | "auth.device_block" |
/// | DeviceUnblock | Allow | "auth.device_unblock" |
/// | ConfigReload | Allow | "auth.config_reload" |
/// | TempCredentialConsumed | Allow | "auth.temp_credential_consumed" |
///
/// 注：`AnomalousLoginDetected` 由 garrison `anomalous-detector-dual` feature 门控，
/// crawlrs 未启用故不编译——文档不列出该变体以保持与代码一致。
///
/// # token 脱敏
///
/// [`GarrisonEvent`] 携带的 `token`/`old_token`/`new_token`/`old_key`/`new_key` 字段
/// 不写入 [`AuditLogEntry`] 任何字段，仅在 `metadata` 中记录前 8 字符前缀
/// （CWE-532 防御，与 garrison `mask_audit_token` 一致）。
///
/// # api_key_id 解析与 login_id 保留
///
/// `login_id` 字段尝试解析为 [`uuid::Uuid`] 填入 `api_key_id`，失败时 `api_key_id=None`。
/// **无论解析是否成功，原始 `login_id` 字符串都写入 `metadata.login_id`**（HIGH-1 修复），
/// 确保非 UUID 主体（如 `"user-123"`、邮箱）的认证事件可追溯（CWE-778）。
/// garrison 签发 API Key 时以 crawlrs api_key uuid 作 login_id，故正常路径可解析。
///
/// # denial_reason 长度限制
///
/// `denial_reason` 截断到 `MAX_DENIAL_REASON_LEN`（256 字符），防 DoS（CWE-400）。
///
/// # 返回
///
/// - `Ok(AuditLogEntry)` — 转换成功
/// - `Err(GarrisonError)` — 内部错误（理论上不可达，所有变体都被覆盖）
fn event_to_audit_entry(event: &GarrisonEvent) -> GarrisonResult<AuditLogEntry> {
    let (action, decision, denial_reason, login_id): (
        &str,
        AuditDecision,
        Option<String>,
        Option<&str>,
    ) = match event {
        GarrisonEvent::Login { login_id, .. } => (
            "auth.login",
            AuditDecision::Allow,
            None,
            Some(login_id.as_str()),
        ),
        GarrisonEvent::Logout { login_id, .. } => (
            "auth.logout",
            AuditDecision::Allow,
            None,
            Some(login_id.as_str()),
        ),
        GarrisonEvent::LoginFailure {
            login_id, reason, ..
        } => (
            "auth.login",
            AuditDecision::Deny,
            Some(truncate_string(reason, MAX_DENIAL_REASON_LEN)),
            Some(login_id.as_str()),
        ),
        GarrisonEvent::Kickout {
            login_id, reason, ..
        } => (
            "auth.kickout",
            AuditDecision::Deny,
            Some(truncate_string(reason, MAX_DENIAL_REASON_LEN)),
            Some(login_id.as_str()),
        ),
        GarrisonEvent::PermissionCheck {
            login_id,
            permission,
            ..
        } => {
            let entry = build_entry(
                "auth.permission_check",
                AuditDecision::Allow,
                None,
                Some(login_id),
                event,
            )
            .with_metadata(
                "permission",
                serde_json::Value::String(truncate_string(permission, MAX_PERMISSION_LEN)),
            );
            return Ok(entry.build());
        }
        GarrisonEvent::RoleCheck { login_id, role, .. } => {
            let entry = build_entry(
                "auth.role_check",
                AuditDecision::Allow,
                None,
                Some(login_id),
                event,
            )
            .with_metadata(
                "role",
                serde_json::Value::String(truncate_string(role, MAX_ROLE_LEN)),
            );
            return Ok(entry.build());
        }
        GarrisonEvent::TokenExpired { .. } => {
            ("auth.token_expired", AuditDecision::Deny, None, None)
        }
        GarrisonEvent::TokenRefresh {
            login_id,
            old_token,
            new_token,
            ..
        } => {
            let entry = build_entry(
                "auth.token_refresh",
                AuditDecision::Allow,
                None,
                Some(login_id),
                event,
            )
            .with_metadata(
                "old_token_prefix",
                serde_json::Value::String(mask_token(old_token)),
            )
            .with_metadata(
                "new_token_prefix",
                serde_json::Value::String(mask_token(new_token)),
            );
            return Ok(entry.build());
        }
        GarrisonEvent::RevokeToken { token, .. } => {
            let entry = build_entry("auth.revoke_token", AuditDecision::Allow, None, None, event)
                .with_metadata("token_prefix", serde_json::Value::String(mask_token(token)));
            return Ok(entry.build());
        }
        GarrisonEvent::SessionTimeout { login_id, .. } => (
            "auth.session_timeout",
            AuditDecision::Deny,
            None,
            Some(login_id.as_str()),
        ),
        GarrisonEvent::AccountLocked {
            login_id, reason, ..
        } => (
            "auth.account_locked",
            AuditDecision::Deny,
            Some(truncate_string(reason, MAX_DENIAL_REASON_LEN)),
            Some(login_id.as_str()),
        ),
        GarrisonEvent::FirewallBlock {
            login_id, reason, ..
        } => (
            "auth.firewall_block",
            AuditDecision::Deny,
            Some(truncate_string(reason, MAX_DENIAL_REASON_LEN)),
            Some(login_id.as_str()),
        ),
        GarrisonEvent::TokenRotate {
            old_key, new_key, ..
        } => {
            let entry = build_entry("auth.token_rotate", AuditDecision::Allow, None, None, event)
                .with_metadata(
                    "old_key_prefix",
                    serde_json::Value::String(mask_token(old_key)),
                )
                .with_metadata(
                    "new_key_prefix",
                    serde_json::Value::String(mask_token(new_key)),
                );
            return Ok(entry.build());
        }
        GarrisonEvent::TempCredentialConsumed { key, value, .. } => {
            // MEDIUM-1 (CWE-532): 凭据 value 不落审计，仅记长度（防下游未消费时重放）
            let value_len = value.len();
            let entry = build_entry(
                "auth.temp_credential_consumed",
                AuditDecision::Allow,
                None,
                None,
                event,
            )
            .with_metadata("key_prefix", serde_json::Value::String(mask_token(key)))
            .with_metadata("value_len", serde_json::Value::Number(value_len.into()));
            return Ok(entry.build());
        }
        GarrisonEvent::SocialLogin {
            provider,
            user_id,
            login_id,
            ..
        } => {
            let entry = build_entry(
                "auth.social_login",
                AuditDecision::Allow,
                None,
                login_id.as_deref(),
                event,
            )
            .with_metadata(
                "provider",
                serde_json::Value::String(truncate_string(provider, MAX_PROVIDER_LEN)),
            )
            .with_metadata(
                "user_id",
                serde_json::Value::String(truncate_string(user_id, MAX_USER_ID_LEN)),
            );
            return Ok(entry.build());
        }
        GarrisonEvent::TenantSwitch {
            login_id,
            from_tenant,
            to_tenant,
            ..
        } => {
            let entry = build_entry(
                "auth.tenant_switch",
                AuditDecision::Allow,
                None,
                Some(login_id),
                event,
            )
            .with_metadata(
                "from_tenant",
                serde_json::Value::Number((*from_tenant).into()),
            )
            .with_metadata("to_tenant", serde_json::Value::Number((*to_tenant).into()));
            return Ok(entry.build());
        }
        GarrisonEvent::DeviceBlock {
            login_id, device, ..
        } => {
            let entry = build_entry(
                "auth.device_block",
                AuditDecision::Deny,
                Some("device_blocked".to_string()),
                Some(login_id),
                event,
            )
            .with_metadata(
                "device",
                serde_json::Value::String(truncate_string(device, MAX_DEVICE_LEN)),
            );
            return Ok(entry.build());
        }
        GarrisonEvent::DeviceUnblock {
            login_id, device, ..
        } => {
            let entry = build_entry(
                "auth.device_unblock",
                AuditDecision::Allow,
                None,
                Some(login_id),
                event,
            )
            .with_metadata(
                "device",
                serde_json::Value::String(truncate_string(device, MAX_DEVICE_LEN)),
            );
            return Ok(entry.build());
        }
        GarrisonEvent::ConfigReload { config_version, .. } => {
            let entry = build_entry(
                "auth.config_reload",
                AuditDecision::Allow,
                None,
                None,
                event,
            )
            .with_metadata(
                "config_version",
                serde_json::Value::Number((*config_version).into()),
            );
            return Ok(entry.build());
        }
        GarrisonEvent::Replaced {
            login_id, reason, ..
        } => (
            "auth.replaced",
            AuditDecision::Deny,
            Some(truncate_string(reason, MAX_DENIAL_REASON_LEN)),
            Some(login_id.as_str()),
        ),
    };

    // 通用路径（无特殊 metadata 处理的变体）
    let entry = build_entry(action, decision, denial_reason, login_id, event);
    Ok(entry.build())
}

/// 构造 [`AuditLogBuilder`]，注入请求上下文（IP/User-Agent）、api_key_id 与 login_id metadata。
///
/// # 字段填充策略
///
/// - **api_key_id**：`login_id` 尝试解析为 [`uuid::Uuid`]，成功填入，失败 None
/// - **metadata.login_id**：原始 `login_id` 字符串（截断到 `MAX_LOGIN_ID_LEN`，HIGH-1 修复）
///   - 无论 `api_key_id` 是否解析成功，都写入 `metadata.login_id` 保留主体标识
///   - 非 UUID 主体（如 `"user-123"`、邮箱）的认证事件可追溯（CWE-778）
/// - **metadata.garrison_tenant_id**：从 garrison task-local tenant context 读取（HIGH-2 修复）
///   - garrison `tenant_id` 是 `i64`，crawlrs `team_id` 是 `Uuid`，不强行转换避免语义错位
///   - 仅记录原始 `i64` 值供多租户审计追溯
/// - **User-Agent**：截断到 `MAX_USER_AGENT_LEN`（256 字符，CWE-400 防 DoS）
/// - **IP**：`IpAddr::parse` 验证格式，过滤非法字符串
fn build_entry(
    action: &str,
    decision: AuditDecision,
    denial_reason: Option<String>,
    login_id: Option<&str>,
    event: &GarrisonEvent,
) -> AuditLogBuilder {
    let api_key_id = login_id.and_then(|id| uuid::Uuid::parse_str(id).ok());
    let mut builder =
        AuditLogBuilder::new(action.to_string(), decision).maybe_with_api_key_id(api_key_id);

    // HIGH-1 修复：无论 api_key_id 是否解析成功，原始 login_id 都写入 metadata
    // 非 UUID 主体（社交登录、LoginFailure 等）的认证事件可追溯（CWE-778）
    if let Some(raw) = login_id {
        let truncated = truncate_string(raw, MAX_LOGIN_ID_LEN);
        builder = builder.with_metadata("login_id", serde_json::Value::String(truncated));
    }

    // HIGH-2 修复：从 garrison task-local 读取 tenant_id，记录到 metadata 供多租户追溯
    // 用 strict 版本（无上下文返回 None），不 fail-closed——审计日志不应因缺 tenant context 而失败
    if let Some(tenant_id) = current_tenant_id_strict() {
        builder = builder.with_metadata(
            "garrison_tenant_id",
            serde_json::Value::Number(tenant_id.into()),
        );
    }

    if let Some(reason) = denial_reason {
        builder = builder.with_denial_reason(reason);
    }

    // 从 request_context 提取 IP 和 User-Agent
    if let Some(ctx) = extract_request_context(event) {
        if let Some(ip_str) = &ctx.ip {
            match ip_str.parse::<IpAddr>() {
                Ok(ip) => {
                    builder = builder.with_ip_address(ip);
                }
                Err(_) => {
                    // L-2 修复（CWE-778）：IP 解析失败时记录原始字符串到 metadata，
                    // 供安全分析追溯（如恶意 X-Forwarded-For 构造非 IP 字符串）
                    // 截断到 MAX_RAW_IP_LEN 防止 audit_logs 表膨胀
                    let truncated = truncate_string(ip_str, MAX_RAW_IP_LEN);
                    builder = builder
                        .with_metadata("request_ip_raw", serde_json::Value::String(truncated));
                }
            }
        }
        if let Some(ua) = &ctx.user_agent {
            // MEDIUM-2 修复：截断 User-Agent 防 DoS（CWE-400）
            let truncated = truncate_string(ua, MAX_USER_AGENT_LEN);
            builder = builder.with_user_agent(truncated);
        }
    }

    builder
}

/// 截断字符串到最大字符数（按 `char_indices` 计字符，避免 UTF-8 边界切割）。
///
/// 超长字符串追加 `…` 表示被截断（仅当实际截断时）。
///
/// # 性能（性能审查 MEDIUM-1/LOW-1 修复）
///
/// 单次遍历：用 `char_indices().nth(max_chars)` 同时判断长度并定位字节边界，
/// 避免 `chars().count()` + `char_indices().nth()` 双遍历。用 `String::with_capacity`
/// + `push_str` + `push` 替代 `format!`，避免格式化解析开销。
fn truncate_string(s: &str, max_chars: usize) -> String {
    match s.char_indices().nth(max_chars) {
        None => s.to_string(),
        Some((boundary, _)) => {
            let mut result = String::with_capacity(boundary + 3);
            result.push_str(&s[..boundary]);
            result.push('\u{2026}');
            result
        }
    }
}

/// 从 [`GarrisonEvent`] 提取 [`RequestContext`] 引用。
///
/// 遍历所有变体，返回 `Option<&RequestContext>`。
/// `None` 表示事件未携带请求上下文（向后兼容）。
fn extract_request_context(event: &GarrisonEvent) -> Option<&RequestContext> {
    match event {
        GarrisonEvent::Login {
            request_context, ..
        }
        | GarrisonEvent::Logout {
            request_context, ..
        }
        | GarrisonEvent::Kickout {
            request_context, ..
        }
        | GarrisonEvent::PermissionCheck {
            request_context, ..
        }
        | GarrisonEvent::RoleCheck {
            request_context, ..
        }
        | GarrisonEvent::TokenExpired {
            request_context, ..
        }
        | GarrisonEvent::LoginFailure {
            request_context, ..
        }
        | GarrisonEvent::TokenRefresh {
            request_context, ..
        }
        | GarrisonEvent::RevokeToken {
            request_context, ..
        }
        | GarrisonEvent::SessionTimeout {
            request_context, ..
        }
        | GarrisonEvent::AccountLocked {
            request_context, ..
        }
        | GarrisonEvent::FirewallBlock {
            request_context, ..
        }
        | GarrisonEvent::TokenRotate {
            request_context, ..
        }
        | GarrisonEvent::TempCredentialConsumed {
            request_context, ..
        }
        | GarrisonEvent::SocialLogin {
            request_context, ..
        }
        | GarrisonEvent::TenantSwitch {
            request_context, ..
        }
        | GarrisonEvent::DeviceBlock {
            request_context, ..
        }
        | GarrisonEvent::DeviceUnblock {
            request_context, ..
        }
        | GarrisonEvent::ConfigReload {
            request_context, ..
        }
        | GarrisonEvent::Replaced {
            request_context, ..
        } => request_context.as_ref(),
    }
}

/// token 脱敏：取前 8 字符 + "…"（CWE-532，对齐 garrison `mask_audit_token`）。
///
/// 短 token（< [`MASK_TOKEN_MIN_LEN`] 字符）直接脱敏为 `***…`，避免完整原文
/// 落入 audit_logs metadata（安全审查 MEDIUM-1 修复）。
///
/// live session token 不得原样落审计——攻击者获得 audit_logs 只读权限
/// （SQL 注入副产品/备份泄漏/replica）即可在 exp 内重放冒充会话。
///
/// # 性能（性能审查 HIGH-1/HIGH-2/L-5 修复）
///
/// 单次遍历 + 提前 break：
/// - 用 `char_indices` 提供的 `char` 直接计算 `len_utf8`，避免重新切片+重新创建
///   chars 迭代器（HIGH-1）
/// - 字符数达到 `MASK_TOKEN_MIN_LEN` 后立即 break，避免遍历剩余字符（HIGH-2）
/// - `prefix_byte_end` 必然在 `char_count == MASK_TOKEN_PREFIX_LEN` 时被设置
///   （因 `MASK_TOKEN_PREFIX_LEN=8 < MASK_TOKEN_MIN_LEN=16`），故删除死代码
///   fallback 分支（安全 L-5 / 架构 L1）
fn mask_token(token: &str) -> String {
    let mut char_count = 0usize;
    let mut prefix_byte_end = 0usize;

    for (i, c) in token.char_indices() {
        char_count += 1;
        if char_count == MASK_TOKEN_PREFIX_LEN {
            prefix_byte_end = i + c.len_utf8();
        }
        // 字符数达到 MIN_LEN 后即可判定非短 token，提前 break 避免遍历剩余字符
        if char_count >= MASK_TOKEN_MIN_LEN {
            break;
        }
    }

    if char_count < MASK_TOKEN_MIN_LEN {
        // 短 token 直接脱敏，避免完整泄露
        return "***\u{2026}".to_string();
    }

    // 此时 prefix_byte_end 必然已设置（char_count >= 16 > 8 = PREFIX_LEN）
    format!("{}\u{2026}", &token[..prefix_byte_end])
}

// ============================================================================
// 编译期监听器注册（inventory）
// ============================================================================

/// `inventory` factory 函数：返回 `Arc<dyn GarrisonListener>` 实例。
///
/// `inventory::submit!` 在编译期注册此 factory，`GarrisonManager::init` 时
/// 通过 `inventory::iter::<GarrisonListenerEntry>()` 收集并调用。
fn crawlrs_audit_listener_factory() -> Arc<dyn GarrisonListener> {
    Arc::new(CrawlrsAuditListener::new())
}

// 编译期注册监听器（仅当 garrison `listener` feature 启用时）
inventory::submit! {
    garrison::listener::GarrisonListenerEntry {
        factory: crawlrs_audit_listener_factory,
    }
}

// ============================================================================
// 测试
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::auth::AuditDecision;
    use crate::domain::services::audit_service::AuditServiceError;
    use uuid::Uuid;

    /// 获取测试串行化锁 guard，避免 `AUDIT_SERVICE` 全局态在并行测试中竞态。
    ///
    /// `#[tokio::test]` 默认并行执行，涉及 `set_audit_service`/`reset_audit_service_for_test`
    /// 的测试必须串行——guard 在 setup 阶段持有，await 前 drop（spawn task 持有 Arc clone，
    /// 全局态改变不影响已 spawn 的 task）。
    ///
    /// T034 修复：改为 async，使用 `tokio::sync::Mutex` 避免跨 await 持有 `std::sync::MutexGuard`
    /// 阻塞 runtime 线程。调用方需 `.await`。
    async fn test_lock() -> tokio::sync::MutexGuard<'static, ()> {
        test_mutex().lock().await
    }

    // ============================================================
    // mask_token 测试
    // ============================================================

    #[test]
    fn test_mask_token_truncates_to_8_chars_with_ellipsis() {
        let token = "abcdefghijklmnopqrstuvwxyz1234567890";
        let masked = mask_token(token);
        assert_eq!(masked, "abcdefgh\u{2026}");
        assert!(masked.len() < token.len());
    }

    #[test]
    fn test_mask_token_short_token_below_threshold_is_masked() {
        // 短 token（< 16 字符）应直接脱敏为 ***…，避免完整泄露
        let token = "short";
        let masked = mask_token(token);
        assert_eq!(masked, "***\u{2026}");
    }

    #[test]
    fn test_mask_token_empty_token_returns_masked_ellipsis() {
        let masked = mask_token("");
        assert_eq!(masked, "***\u{2026}");
    }

    #[test]
    fn test_mask_token_token_at_min_length_keeps_8_prefix() {
        // 16 字符 token 刚好等于阈值，应保留前 8 字符前缀
        let token = "0123456789abcdef";
        let masked = mask_token(token);
        assert_eq!(masked, "01234567\u{2026}");
    }

    #[test]
    fn test_mask_token_token_just_below_min_length_is_masked() {
        // 15 字符 token（< 16 阈值）应直接脱敏
        let token = "0123456789abcde";
        let masked = mask_token(token);
        assert_eq!(masked, "***\u{2026}");
    }

    // ============================================================
    // event_to_audit_entry 测试（覆盖关键变体）
    // ============================================================

    #[test]
    fn test_login_event_converts_to_allow_entry() {
        let login_id = Uuid::new_v4().to_string();
        let event = GarrisonEvent::Login {
            login_id: login_id.clone(),
            token: "tok-1234567890".to_string(),
            device: None,
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        assert_eq!(entry.requested_action, "auth.login");
        assert_eq!(entry.decision, AuditDecision::Allow);
        assert!(entry.denial_reason.is_none());
        assert_eq!(entry.api_key_id, Uuid::parse_str(&login_id).ok());
    }

    #[test]
    fn test_login_failure_event_converts_to_deny_entry_with_reason() {
        let event = GarrisonEvent::LoginFailure {
            login_id: "user-123".to_string(),
            reason: "invalid_credentials".to_string(),
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        assert_eq!(entry.requested_action, "auth.login");
        assert_eq!(entry.decision, AuditDecision::Deny);
        assert_eq!(entry.denial_reason.as_deref(), Some("invalid_credentials"));
        // login_id "user-123" 无法解析为 Uuid，api_key_id 应为 None（M-2 语义）
        assert!(entry.api_key_id.is_none());
    }

    #[test]
    fn test_kickout_event_converts_to_deny_entry() {
        let event = GarrisonEvent::Kickout {
            login_id: "user-123".to_string(),
            token: "tok".to_string(),
            reason: "admin_kickout".to_string(),
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        assert_eq!(entry.requested_action, "auth.kickout");
        assert_eq!(entry.decision, AuditDecision::Deny);
        assert_eq!(entry.denial_reason.as_deref(), Some("admin_kickout"));
    }

    #[test]
    fn test_logout_event_converts_to_allow_entry() {
        let event = GarrisonEvent::Logout {
            login_id: "user-123".to_string(),
            token: "tok".to_string(),
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        assert_eq!(entry.requested_action, "auth.logout");
        assert_eq!(entry.decision, AuditDecision::Allow);
    }

    #[test]
    fn test_permission_check_event_includes_permission_metadata() {
        let event = GarrisonEvent::PermissionCheck {
            login_id: "user-123".to_string(),
            permission: "crawlrs:admin".to_string(),
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        assert_eq!(entry.requested_action, "auth.permission_check");
        assert_eq!(entry.decision, AuditDecision::Allow);
        // metadata 应包含 permission 字段
        if let serde_json::Value::Object(map) = &entry.metadata {
            assert_eq!(
                map.get("permission").and_then(|v| v.as_str()),
                Some("crawlrs:admin")
            );
        } else {
            panic!("metadata should be a JSON object");
        }
    }

    #[test]
    fn test_role_check_event_includes_role_metadata() {
        let event = GarrisonEvent::RoleCheck {
            login_id: "user-123".to_string(),
            role: "admin".to_string(),
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        assert_eq!(entry.requested_action, "auth.role_check");
        if let serde_json::Value::Object(map) = &entry.metadata {
            assert_eq!(map.get("role").and_then(|v| v.as_str()), Some("admin"));
        } else {
            panic!("metadata should be a JSON object");
        }
    }

    #[test]
    fn test_token_refresh_event_masks_tokens_in_metadata() {
        let event = GarrisonEvent::TokenRefresh {
            login_id: "user-123".to_string(),
            old_token: "old-token-1234567890".to_string(),
            new_token: "new-token-1234567890".to_string(),
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        assert_eq!(entry.requested_action, "auth.token_refresh");
        assert_eq!(entry.decision, AuditDecision::Allow);
        if let serde_json::Value::Object(map) = &entry.metadata {
            let old_prefix = map
                .get("old_token_prefix")
                .and_then(|v| v.as_str())
                .expect("old_token_prefix should exist");
            let new_prefix = map
                .get("new_token_prefix")
                .and_then(|v| v.as_str())
                .expect("new_token_prefix should exist");
            // 仅前 8 字符 + 省略号
            assert!(old_prefix.starts_with("old-toke"));
            assert!(old_prefix.ends_with('\u{2026}'));
            assert!(new_prefix.starts_with("new-toke"));
            assert!(new_prefix.ends_with('\u{2026}'));
            // 原文不应出现在 metadata 中
            assert!(!format!("{map:?}").contains("old-token-1234567890"));
            assert!(!format!("{map:?}").contains("new-token-1234567890"));
        } else {
            panic!("metadata should be a JSON object");
        }
    }

    #[test]
    fn test_revoke_token_event_masks_token_in_metadata() {
        let event = GarrisonEvent::RevokeToken {
            token: "secret-token-1234567890".to_string(),
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        assert_eq!(entry.requested_action, "auth.revoke_token");
        if let serde_json::Value::Object(map) = &entry.metadata {
            let prefix = map
                .get("token_prefix")
                .and_then(|v| v.as_str())
                .expect("token_prefix should exist");
            assert!(prefix.starts_with("secret-t"));
            assert!(prefix.ends_with('\u{2026}'));
            // 原文不应出现
            assert!(!format!("{map:?}").contains("secret-token-1234567890"));
        } else {
            panic!("metadata should be a JSON object");
        }
    }

    #[test]
    fn test_token_rotate_event_masks_keys_in_metadata() {
        let event = GarrisonEvent::TokenRotate {
            old_key: "old-key-1234567890".to_string(),
            new_key: "new-key-1234567890".to_string(),
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        assert_eq!(entry.requested_action, "auth.token_rotate");
        if let serde_json::Value::Object(map) = &entry.metadata {
            assert!(map
                .get("old_key_prefix")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .starts_with("old-key-"));
            assert!(map
                .get("new_key_prefix")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .starts_with("new-key-"));
            // 原文不应出现
            assert!(!format!("{map:?}").contains("old-key-1234567890"));
            assert!(!format!("{map:?}").contains("new-key-1234567890"));
        } else {
            panic!("metadata should be a JSON object");
        }
    }

    #[test]
    fn test_account_locked_event_converts_to_deny_entry() {
        let event = GarrisonEvent::AccountLocked {
            login_id: "user-123".to_string(),
            reason: "brute_force: 5 failures in 1h".to_string(),
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        assert_eq!(entry.requested_action, "auth.account_locked");
        assert_eq!(entry.decision, AuditDecision::Deny);
        assert_eq!(
            entry.denial_reason.as_deref(),
            Some("brute_force: 5 failures in 1h")
        );
    }

    #[test]
    fn test_firewall_block_event_converts_to_deny_entry() {
        let event = GarrisonEvent::FirewallBlock {
            login_id: "user-123".to_string(),
            reason: "ip_blacklist".to_string(),
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        assert_eq!(entry.requested_action, "auth.firewall_block");
        assert_eq!(entry.decision, AuditDecision::Deny);
        assert_eq!(entry.denial_reason.as_deref(), Some("ip_blacklist"));
    }

    #[test]
    fn test_session_timeout_event_converts_to_deny_entry() {
        let event = GarrisonEvent::SessionTimeout {
            login_id: "user-123".to_string(),
            token: "tok".to_string(),
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        assert_eq!(entry.requested_action, "auth.session_timeout");
        assert_eq!(entry.decision, AuditDecision::Deny);
    }

    #[test]
    fn test_temp_credential_consumed_does_not_log_value() {
        let event = GarrisonEvent::TempCredentialConsumed {
            key: "temp-key-1234567890".to_string(),
            value: "secret-value".to_string(),
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        assert_eq!(entry.requested_action, "auth.temp_credential_consumed");
        if let serde_json::Value::Object(map) = &entry.metadata {
            // value_len 字段应存在，但 value 原文不应出现
            assert!(map.get("value_len").is_some());
            assert!(!format!("{map:?}").contains("secret-value"));
            // key 应被截断
            let key_prefix = map
                .get("key_prefix")
                .and_then(|v| v.as_str())
                .expect("key_prefix should exist");
            assert!(key_prefix.ends_with('\u{2026}'));
        } else {
            panic!("metadata should be a JSON object");
        }
    }

    #[test]
    fn test_request_context_propagates_ip_and_user_agent() {
        let event = GarrisonEvent::Login {
            login_id: "user-123".to_string(),
            token: "tok".to_string(),
            device: None,
            request_context: Some(RequestContext {
                ip: Some("192.168.1.1".to_string()),
                user_agent: Some("Mozilla/5.0".to_string()),
            }),
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        assert_eq!(
            entry.ip_address().map(|ip| ip.to_string()).as_deref(),
            Some("192.168.1.1")
        );
        assert_eq!(entry.user_agent(), Some("Mozilla/5.0"));
    }

    #[test]
    fn test_request_context_none_yields_none_ip_and_user_agent() {
        let event = GarrisonEvent::Login {
            login_id: "user-123".to_string(),
            token: "tok".to_string(),
            device: None,
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        assert!(entry.ip_address().is_none());
        assert!(entry.user_agent().is_none());
    }

    #[test]
    fn test_login_id_parses_to_uuid_when_valid() {
        let api_key_id = Uuid::new_v4();
        let event = GarrisonEvent::Login {
            login_id: api_key_id.to_string(),
            token: "tok".to_string(),
            device: None,
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        assert_eq!(entry.api_key_id, Some(api_key_id));
    }

    #[test]
    fn test_login_id_unparseable_yields_none_api_key_id() {
        let event = GarrisonEvent::Login {
            login_id: "not-a-uuid".to_string(),
            token: "tok".to_string(),
            device: None,
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        assert!(entry.api_key_id.is_none());
    }

    // ============================================================
    // metadata 字段截断测试（安全审查 M-1~M-4/L-1 修复验证）
    // ============================================================

    #[test]
    fn test_permission_check_event_truncates_long_permission_metadata() {
        // 构造 200 字符超长 permission，应截断到 MAX_PERMISSION_LEN=128 字符 + "…"
        let long_permission = "crawlrs:admin_".to_string() + &"x".repeat(200);
        let event = GarrisonEvent::PermissionCheck {
            login_id: "user-123".to_string(),
            permission: long_permission.clone(),
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        if let serde_json::Value::Object(map) = &entry.metadata {
            let permission_value = map
                .get("permission")
                .and_then(|v| v.as_str())
                .expect("permission metadata should exist");
            // 应截断到 128 字符 + "…"（共 129 字符，UTF-8 字节长度 130）
            assert_eq!(permission_value.chars().count(), MAX_PERMISSION_LEN + 1);
            assert!(permission_value.ends_with('\u{2026}'));
            assert!(permission_value.starts_with("crawlrs:admin_"));
        } else {
            panic!("metadata should be a JSON object");
        }
    }

    #[test]
    fn test_role_check_event_truncates_long_role_metadata() {
        let long_role = "admin_".to_string() + &"x".repeat(200);
        let event = GarrisonEvent::RoleCheck {
            login_id: "user-123".to_string(),
            role: long_role,
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        if let serde_json::Value::Object(map) = &entry.metadata {
            let role_value = map
                .get("role")
                .and_then(|v| v.as_str())
                .expect("role metadata should exist");
            assert_eq!(role_value.chars().count(), MAX_ROLE_LEN + 1);
            assert!(role_value.ends_with('\u{2026}'));
        } else {
            panic!("metadata should be a JSON object");
        }
    }

    #[test]
    fn test_device_block_event_truncates_long_device_metadata() {
        let long_device = "device-".to_string() + &"x".repeat(300);
        let event = GarrisonEvent::DeviceBlock {
            login_id: "user-123".to_string(),
            device: long_device,
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        if let serde_json::Value::Object(map) = &entry.metadata {
            let device_value = map
                .get("device")
                .and_then(|v| v.as_str())
                .expect("device metadata should exist");
            assert_eq!(device_value.chars().count(), MAX_DEVICE_LEN + 1);
            assert!(device_value.ends_with('\u{2026}'));
        } else {
            panic!("metadata should be a JSON object");
        }
    }

    #[test]
    fn test_social_login_event_truncates_long_user_id_and_provider_metadata() {
        let long_user_id = "uid-".to_string() + &"x".repeat(200);
        let long_provider = "provider-".to_string() + &"x".repeat(100);
        let event = GarrisonEvent::SocialLogin {
            provider: long_provider,
            user_id: long_user_id,
            login_id: Some("user-123".to_string()),
            request_context: None,
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        if let serde_json::Value::Object(map) = &entry.metadata {
            let user_id_value = map
                .get("user_id")
                .and_then(|v| v.as_str())
                .expect("user_id metadata should exist");
            assert_eq!(user_id_value.chars().count(), MAX_USER_ID_LEN + 1);
            assert!(user_id_value.ends_with('\u{2026}'));

            let provider_value = map
                .get("provider")
                .and_then(|v| v.as_str())
                .expect("provider metadata should exist");
            assert_eq!(provider_value.chars().count(), MAX_PROVIDER_LEN + 1);
            assert!(provider_value.ends_with('\u{2026}'));
        } else {
            panic!("metadata should be a JSON object");
        }
    }

    // ============================================================
    // IP 解析失败记录原始字符串测试（安全审查 L-2 修复验证）
    // ============================================================

    #[test]
    fn test_request_context_invalid_ip_records_raw_string_to_metadata() {
        // 恶意构造的非法 IP 字符串，应记录到 metadata.request_ip_raw 供安全分析
        let malicious_ip = "<script>alert(1)</script>".to_string();
        let event = GarrisonEvent::Login {
            login_id: "user-123".to_string(),
            token: "tok".to_string(),
            device: None,
            request_context: Some(RequestContext {
                ip: Some(malicious_ip.clone()),
                user_agent: None,
            }),
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        // 解析失败：ip_address 字段应为 None
        assert!(
            entry.ip_address().is_none(),
            "invalid IP should not be set to ip_address field"
        );
        // metadata 应记录 request_ip_raw 字段（截断后）
        if let serde_json::Value::Object(map) = &entry.metadata {
            let raw_ip = map
                .get("request_ip_raw")
                .and_then(|v| v.as_str())
                .expect("request_ip_raw metadata should exist for unparseable IP");
            assert_eq!(raw_ip, malicious_ip);
        } else {
            panic!("metadata should be a JSON object");
        }
    }

    #[test]
    fn test_request_context_valid_ip_does_not_record_raw_metadata() {
        // 合法 IP 应只设置 ip_address 字段，不写入 metadata.request_ip_raw
        let event = GarrisonEvent::Login {
            login_id: "user-123".to_string(),
            token: "tok".to_string(),
            device: None,
            request_context: Some(RequestContext {
                ip: Some("192.168.1.1".to_string()),
                user_agent: None,
            }),
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        assert_eq!(
            entry.ip_address().map(|ip| ip.to_string()).as_deref(),
            Some("192.168.1.1")
        );
        if let serde_json::Value::Object(map) = &entry.metadata {
            assert!(
                map.get("request_ip_raw").is_none(),
                "valid IP should not be recorded to request_ip_raw metadata"
            );
        }
    }

    #[test]
    fn test_request_context_long_invalid_ip_truncated_in_metadata() {
        // 超长非法 IP 字符串应截断到 MAX_RAW_IP_LEN 防止 audit_logs 表膨胀
        let long_invalid_ip = "x".repeat(200);
        let event = GarrisonEvent::Login {
            login_id: "user-123".to_string(),
            token: "tok".to_string(),
            device: None,
            request_context: Some(RequestContext {
                ip: Some(long_invalid_ip),
                user_agent: None,
            }),
        };
        let entry = event_to_audit_entry(&event).expect("conversion should succeed");
        if let serde_json::Value::Object(map) = &entry.metadata {
            let raw_ip = map
                .get("request_ip_raw")
                .and_then(|v| v.as_str())
                .expect("request_ip_raw should exist");
            assert_eq!(raw_ip.chars().count(), MAX_RAW_IP_LEN + 1);
            assert!(raw_ip.ends_with('\u{2026}'));
        }
    }

    // ============================================================
    // truncate_string UTF-8 边界测试（性能审查 MEDIUM-1 修复验证）
    // ============================================================

    #[test]
    fn test_truncate_string_short_string_returned_as_is() {
        let s = "hello";
        let result = truncate_string(s, 10);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_truncate_string_exact_length_returned_as_is() {
        let s = "hello";
        let result = truncate_string(s, 5);
        assert_eq!(result, "hello");
    }

    #[test]
    fn test_truncate_string_long_string_truncated_with_ellipsis() {
        let s = "hello world";
        let result = truncate_string(s, 5);
        assert_eq!(result, "hello\u{2026}");
    }

    #[test]
    fn test_truncate_string_utf8_multibyte_boundary_preserved() {
        // 中文字符占 3 字节，截断到 2 个字符应保留 "你好" + "…"
        let s = "你好世界hello";
        let result = truncate_string(s, 2);
        assert_eq!(result, "你好\u{2026}");
        // 字节边界不应被切割（不应产生 panic 或乱码）
        assert!(result.chars().count() == 3); // 2 个中文字符 + 1 个省略号
    }

    #[test]
    fn test_truncate_string_empty_string_returned_as_is() {
        let result = truncate_string("", 10);
        assert_eq!(result, "");
    }

    #[test]
    fn test_truncate_string_zero_max_chars_always_truncates_to_ellipsis() {
        // max_chars=0 时，nth(0) = Some((0, first_char))，触发截断
        let result = truncate_string("hello", 0);
        assert_eq!(result, "\u{2026}");
    }

    // ============================================================
    // Mock AuditServiceTrait — 捕获 log() 调用用于验证
    // ============================================================

    /// 测试用 mock audit service，捕获 `log()` 调用以验证 on_event 行为。
    ///
    /// 通过 `Mutex<Vec<AuditLogEntry>>` 共享状态捕获——`tokio::spawn` 异步写入后，
    /// 测试主线程通过 `entries()` 读取并断言。
    struct CapturingAuditService {
        entries: std::sync::Mutex<Vec<AuditLogEntry>>,
        fail_on_log: bool,
        // 确定性同步：log() 完成后 notify_one，测试端 notified().await 替代 500ms 轮询，
        // 消除并行测试下 spawn task 未及时调度导致的 flaky 失败。
        notify: Arc<tokio::sync::Notify>,
    }

    impl CapturingAuditService {
        fn new() -> Self {
            Self {
                entries: std::sync::Mutex::new(Vec::new()),
                fail_on_log: false,
                notify: Arc::new(tokio::sync::Notify::new()),
            }
        }

        fn failing() -> Self {
            Self {
                entries: std::sync::Mutex::new(Vec::new()),
                fail_on_log: true,
                notify: Arc::new(tokio::sync::Notify::new()),
            }
        }

        fn entries(&self) -> Vec<AuditLogEntry> {
            self.entries.lock().unwrap().clone()
        }

        fn entry_count(&self) -> usize {
            self.entries.lock().unwrap().len()
        }

        /// 返回 `Notify` 引用，测试端 `notify.notified().await` 确定性等待 `log()` 完成。
        fn notify(&self) -> &tokio::sync::Notify {
            &self.notify
        }
    }

    #[async_trait]
    impl AuditServiceTrait for CapturingAuditService {
        async fn log(&self, entry: AuditLogEntry) -> Result<(), AuditServiceError> {
            if self.fail_on_log {
                self.notify.notify_one();
                return Err(AuditServiceError::RepositoryError(
                    crate::domain::repositories::audit_log_repository::AuditRepositoryError::DatabaseError(
                        sea_orm::DbErr::Custom("mock failure".to_string()).into(),
                    ),
                ));
            }
            self.entries.lock().unwrap().push(entry);
            self.notify.notify_one();
            Ok(())
        }

        async fn log_allow(
            &self,
            _action: String,
            _api_key_id: Uuid,
            _team_id: Uuid,
            _scope: crate::domain::auth::ApiKeyScope,
        ) -> Result<(), AuditServiceError> {
            Ok(())
        }

        async fn log_deny(
            &self,
            _action: String,
            _api_key_id: Option<Uuid>,
            _team_id: Option<Uuid>,
            _reason: String,
            _scope: Option<crate::domain::auth::ApiKeyScope>,
        ) -> Result<(), AuditServiceError> {
            Ok(())
        }

        async fn get_logs_for_key(
            &self,
            _api_key_id: Uuid,
            _limit: u64,
            _offset: u64,
        ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
            Ok(self.entries())
        }

        async fn get_logs_for_team(
            &self,
            _team_id: Uuid,
            _limit: u64,
            _offset: u64,
        ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
            Ok(self.entries())
        }

        async fn get_denied_requests(
            &self,
            _api_key_id: Uuid,
            _limit: u64,
        ) -> Result<Vec<AuditLogEntry>, AuditServiceError> {
            Ok(self.entries())
        }

        async fn cleanup_old_logs(
            &self,
            _retention_days: i64,
            _policy: &crate::domain::retention_policy::RetentionBatchPolicy,
        ) -> Result<u64, AuditServiceError> {
            Ok(0)
        }
    }

    // ============================================================
    // set_audit_service 测试
    // ============================================================

    #[tokio::test]
    async fn test_set_audit_service_succeeds_when_none() {
        let _guard = test_lock().await;
        // setup: 确保全局态干净
        reset_audit_service_for_test();
        let mock = Arc::new(CapturingAuditService::new()) as Arc<dyn AuditServiceTrait>;
        let result = set_audit_service(mock);
        assert!(result.is_ok(), "set_audit_service should succeed when None");
        // teardown: 清理全局态供后续测试
        reset_audit_service_for_test();
    }

    #[tokio::test]
    async fn test_set_audit_service_returns_err_when_already_set() {
        let _guard = test_lock().await;
        // setup: 先注入一个实例
        reset_audit_service_for_test();
        let mock1 = Arc::new(CapturingAuditService::new()) as Arc<dyn AuditServiceTrait>;
        match set_audit_service(mock1) {
            Ok(()) => {}
            Err(_) => panic!("setup: first set_audit_service should succeed"),
        }

        // 第二次注入应返回 Err，且返回传入的 service
        let mock2 = Arc::new(CapturingAuditService::new()) as Arc<dyn AuditServiceTrait>;
        let result = set_audit_service(mock2.clone());
        assert!(
            result.is_err(),
            "second set_audit_service should return Err when already set"
        );
        // 返回的应该是传入的 mock2（让调用方处理）
        let returned = result.unwrap_err();
        assert!(
            Arc::ptr_eq(&returned, &mock2),
            "returned service should be the one passed in"
        );

        // teardown
        reset_audit_service_for_test();
    }

    // ============================================================
    // CrawlrsAuditListener::on_event 测试（真实集成路径）
    // ============================================================

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_on_event_persists_entry_via_audit_service() {
        // 串行化：全程持有 test_lock，防止并行测试在 setup 与 on_event 之间
        // reset AUDIT_SERVICE，导致 on_event 的 get_audit_service() 返回 None 或其他 mock。
        // 先检查后 notified().await 模式处理 notify_one 早于 notified() 注册的竞态。
        let _guard = test_lock().await;
        reset_audit_service_for_test();
        let mock = Arc::new(CapturingAuditService::new());
        match set_audit_service(mock.clone() as Arc<dyn AuditServiceTrait>) {
            Ok(()) => {}
            Err(_) => panic!("setup: set_audit_service should succeed"),
        }

        let listener = CrawlrsAuditListener::new();
        let api_key_id = Uuid::new_v4();
        let event = GarrisonEvent::Login {
            login_id: api_key_id.to_string(),
            token: "tok_0123456789abcdef".to_string(), // >= 16 chars
            device: None,
            request_context: None,
        };

        // act
        let result = listener.on_event(&event).await;

        // assert: on_event 不传播错误
        assert!(result.is_ok(), "on_event should never propagate errors");

        // 确定性等待 spawn task 完成：notify.notified().await 替代 500ms 轮询，
        // 消除并行测试下 spawn task 未及时调度导致的 flaky 失败。
        // 安全超时 5s 作为兜底（避免 mock 实现错误时无限挂起）。
        let notify_fut = mock.notify().notified();
        tokio::pin!(notify_fut);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            // 先检查是否已写入（可能 spawn task 在 await 前已完成）
            if mock.entry_count() > 0 {
                break;
            }
            if std::time::Instant::now() >= deadline {
                panic!(
                    "timed out waiting for audit log entry: entry_count={}",
                    mock.entry_count()
                );
            }
            // 竞态：notify_one 在 notified() 注册前调用会丢失，故用 timeout 兜底
            tokio::select! {
                _ = &mut notify_fut => {}
                _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
                    panic!(
                        "timed out waiting for audit log entry: entry_count={}",
                        mock.entry_count()
                    );
                }
            }
        }

        // assert: entry 被持久化，且字段正确
        let entries = mock.entries();
        assert_eq!(
            entries.len(),
            1,
            "exactly one entry should be persisted via log()"
        );
        let entry = &entries[0];
        assert_eq!(entry.requested_action, "auth.login");
        assert_eq!(entry.decision, AuditDecision::Allow);
        assert_eq!(entry.api_key_id, Some(api_key_id));

        // teardown: 仍持有 _guard，直接 reset
        reset_audit_service_for_test();
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_on_event_returns_ok_when_audit_service_not_injected() {
        // setup: 持有串行化锁，确保全局态为 None
        {
            let _guard = test_lock().await;
            reset_audit_service_for_test();
        }

        let listener = CrawlrsAuditListener::new();
        let event = GarrisonEvent::Login {
            login_id: "user-123".to_string(),
            token: "tok_0123456789abcdef".to_string(),
            device: None,
            request_context: None,
        };

        // act: audit_service 未注入，应返回 Ok 且 warn（不阻塞）
        let result = listener.on_event(&event).await;

        // assert: 不传播错误，事件被静默 drop
        assert!(
            result.is_ok(),
            "on_event should return Ok when service not injected"
        );

        // teardown: 全局态已为 None，无需重置
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn test_on_event_returns_ok_when_log_fails() {
        // setup: 持有串行化锁，注入失败型 mock
        {
            let _guard = test_lock().await;
            reset_audit_service_for_test();
            let mock = Arc::new(CapturingAuditService::failing());
            match set_audit_service(mock.clone() as Arc<dyn AuditServiceTrait>) {
                Ok(()) => {}
                Err(_) => panic!("setup: set_audit_service should succeed"),
            }
        }; // guard drop

        let listener = CrawlrsAuditListener::new();
        let event = GarrisonEvent::Login {
            login_id: Uuid::new_v4().to_string(),
            token: "tok_0123456789abcdef".to_string(),
            device: None,
            request_context: None,
        };

        // act: log() 返回 Err，on_event 应吞掉错误返回 Ok
        let result = listener.on_event(&event).await;

        // assert: 不传播错误（监听器契约）
        assert!(
            result.is_ok(),
            "on_event should swallow log() errors and return Ok"
        );

        // teardown
        {
            let _guard = test_lock().await;
            reset_audit_service_for_test();
        }
    }

    // ============================================================
    // factory 函数测试
    // ============================================================

    #[test]
    fn test_crawlrs_audit_listener_factory_returns_arc() {
        let listener: Arc<dyn GarrisonListener> = crawlrs_audit_listener_factory();
        // 验证 factory 返回有效实例
        let _ = listener;
    }

    #[test]
    fn test_crawlrs_audit_listener_new_returns_instance() {
        let listener = CrawlrsAuditListener::new();
        let _ = listener;
    }

    #[test]
    fn test_crawlrs_audit_listener_default_returns_instance() {
        // 通过 trait 调用 Default::default()，验证 Default impl 存在且返回实例
        // （clippy::default_constructed_unit_structs 允许 trait 调用形式）
        let listener: CrawlrsAuditListener = Default::default();
        let _ = listener;
    }
}
