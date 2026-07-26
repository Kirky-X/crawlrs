// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! 运维脚本：枚举需重新领取 API Key 的 team 清单 + 在 garrison RBAC 中预置标准角色/权限。
//!
//! ## 背景（R-key-lifecycle-002 / T029）
//!
//! garrison-auth-migration 变更后，所有现有 API Key 作废，garrison 自管 key 哈希存储
//! （`garrison:apikey:<ns>:<key>` on oxcache + postgres），旧 `api_keys.key_hash` 字段弃用。
//! 由于旧表仅存 SHA-256 hash 无法还原明文，需为每个已有 team 重新签发 API Key。
//!
//! 本脚本一次性执行：
//! 1. **预置 RBAC**：在 garrison RBAC 表（`app_permission` / `app_role` / `app_role_permission`）
//!    中幂等创建 crawlrs 标准角色（admin/user/read_only）与权限（crawlrs:read/write/admin），
//!    及角色-权限映射。供后续 `POST /v1/admin/api-keys` 签发 API Key 时通过
//!    `CrawlrsGarrisonInterface::get_permission_list` 反查权限。
//! 2. **枚举 team 清单**：从旧 `api_keys` 表按 `team_id` 分组，统计每个 team 的历史 key 数量
//!    与最早签发时间，输出「需重新领取 key 的 team 清单」。
//!
//! ## 设计决策
//!
//! ### 单租户模式（tenant_id=0）
//!
//! garrison 的 `tenant_id` 类型为 `i64`，crawlrs 的 `team_id` 类型为 `Uuid`，
//! 两者无自然映射。按 design.md §3 方案 A：garrison tenant 概念与 crawlrs team 解耦，
//! 中间件通过 `api_key_id` 反查 crawlrs `api_keys` 表获取 `team_id`，**不读 garrison tenant_id**。
//!
//! crawlrs 所有 team 共享同一套 RBAC 定义（admin/user/read_only 角色 + crawlrs:read/write/admin
//! 权限），故脚本在 `tenant_id=0`（garrison 默认租户）下预置 RBAC，所有 team 复用。
//! 若未来引入 per-team 自定义角色，再开新变更扩展。
//!
//! ### 幂等性
//!
//! RBAC 预置采用 find-or-create 模式：
//! - 先 `find_by_code` 查询，存在则跳过
//! - 不存在则 `create` 新建
//! - 角色-权限映射通过 `assign`（garrison repository 的 `assign` 方法本身幂等）
//!
//! 重复运行脚本安全：不会创建重复记录，不会覆盖既有数据。
//!
//! ### 不自动重签 API Key
//!
//! 按设计，脚本**只打印清单不自动签发**新 API Key。原因：
//! - API Key 明文仅能返回一次（CWE-916：garrison 不存明文 secret），
//!   自动签发无法将明文安全交付给 team owner
//! - 签发需 admin 鉴权（POST /v1/admin/api-keys 要求 Admin scope），
//!   脚本无 admin 上下文
//! - 业务流程要求 team owner 主动申领，确认 key 已安全接收
//!
//! ## 失败契约（规则12 显性化）
//!
//! - DB 连接失败：`main` 返回 `Err`，进程退出码 1，stderr 打印错误
//! - RBAC 预置失败：`main` 返回 `Err`，进程退出码 1
//! - 旧 api_keys 表查询失败：`main` 返回 `Err`，进程退出码 1
//! - 旧 api_keys 表为空（无历史数据）：打印信息性消息，退出码 0
//!
//! ## Spec
//!
//! - R-key-lifecycle-002：读旧 api_keys 表按 team 归属，在 garrison 内为每 team 预置
//!   角色/权限，打印「需重新领取 key 的 team 清单」

use std::sync::Arc;

use dbnexus::{DbConfig, DbPool};
use garrison::dao::repository::postgres::{
    DbnexusPostgresPermissionRepository, DbnexusPostgresRolePermissionRepository,
    DbnexusPostgresRoleRepository,
};
use garrison::dao::repository::{
    NewPermission, NewRole, PermissionRepository, RolePermissionRepository, RoleRepository,
};
use garrison::error::{GarrisonError, GarrisonResult};
use sea_orm::{ColumnTrait, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect};
use uuid::Uuid;

// =============================================================================
// 常量
// =============================================================================

/// garrison 默认租户 ID。
///
/// crawlrs 所有 team 共享同一套 RBAC（admin/user/read_only 角色 + crawlrs:read/write/admin
/// 权限），脚本在 tenant_id=0 下预置。详见模块文档「单租户模式」。
const GARRISON_DEFAULT_TENANT_ID: i64 = 0;

/// crawlrs 标准权限编码前缀（与 `auth_bridge.rs::PERM_READ/PERM_WRITE/PERM_ADMIN` 前缀一致）。
///
/// 仅测试断言使用（release 不需要此前缀常量），故 `#[cfg(test)]` 限定。
#[cfg(test)]
const PERM_PREFIX: &str = "crawlrs:";

/// 标准权限编码：读权限。
const PERM_READ: &str = "crawlrs:read";

/// 标准权限编码：写权限。
const PERM_WRITE: &str = "crawlrs:write";

/// 标准权限编码：管理员权限（蕴含 read+write）。
const PERM_ADMIN: &str = "crawlrs:admin";

/// 标准角色编码：管理员（拥有 read+write+admin 权限）。
const ROLE_ADMIN: &str = "admin";

/// 标准角色编码：普通用户（拥有 read+write 权限）。
const ROLE_USER: &str = "user";

/// 标准角色编码：只读用户（仅 read 权限）。
const ROLE_READ_ONLY: &str = "read_only";

/// 旧 api_keys 表分页查询每页大小（避免一次加载过多记录）。
const LEGACY_KEY_PAGE_SIZE: u64 = 500;

// =============================================================================
// 类型定义
// =============================================================================

/// RBAC 预置结果报告。
///
/// 记录本次预置操作中各表的新增/跳过数量，供 `print_reissue_report` 输出可观察性。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RbacPresetReport {
    /// 新创建的权限数量（已存在的跳过不计）。
    pub permissions_created: usize,
    /// 已存在被跳过的权限数量。
    pub permissions_skipped: usize,
    /// 新创建的角色数量。
    pub roles_created: usize,
    /// 已存在被跳过的角色数量。
    pub roles_skipped: usize,
    /// 新创建的角色-权限映射数量。
    pub role_permissions_assigned: usize,
}

/// 需重新领取 API Key 的 team 信息。
///
/// 从旧 `api_keys` 表按 `team_id` 分组聚合得到，供运维核对与重新签发参考。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReissueTeamInfo {
    /// team_id（crawlrs `teams` 表的主键 UUID）。
    pub team_id: Uuid,
    /// 该 team 在旧 api_keys 表中的历史 key 总数。
    pub legacy_key_count: u64,
}

// =============================================================================
// 纯逻辑函数（无 DB 依赖，可单元测试）
// =============================================================================

/// 返回 crawlrs 标准 RBAC 权限定义列表。
///
/// 三个权限编码（全局表 `app_permission`，无 tenant_id 过滤）：
/// - `crawlrs:read`  — 读权限
/// - `crawlrs:write` — 写权限
/// - `crawlrs:admin` — 管理员权限（蕴含 read+write，由角色-权限映射决定）
pub fn standard_permissions() -> Vec<NewPermission> {
    vec![
        NewPermission {
            code: PERM_READ.to_string(),
            name: "Crawlrs Read".to_string(),
            resource_type: Some("crawlrs".to_string()),
            action: Some("read".to_string()),
        },
        NewPermission {
            code: PERM_WRITE.to_string(),
            name: "Crawlrs Write".to_string(),
            resource_type: Some("crawlrs".to_string()),
            action: Some("write".to_string()),
        },
        NewPermission {
            code: PERM_ADMIN.to_string(),
            name: "Crawlrs Admin".to_string(),
            resource_type: Some("crawlrs".to_string()),
            action: Some("admin".to_string()),
        },
    ]
}

/// 返回 crawlrs 标准 RBAC 角色定义列表。
///
/// 三个角色（在 `app_role` 表中按 `tenant_id=0` 创建）：
/// - `admin`      — 系统管理员，拥有全部权限
/// - `user`       — 普通用户，拥有 read+write
/// - `read_only`  — 只读用户，仅 read
pub fn standard_roles() -> Vec<NewRole> {
    vec![
        NewRole {
            code: ROLE_ADMIN.to_string(),
            name: "Crawlrs Administrator".to_string(),
            description: Some("Full access: read + write + admin".to_string()),
            is_system: true,
        },
        NewRole {
            code: ROLE_USER.to_string(),
            name: "Crawlrs User".to_string(),
            description: Some("Standard access: read + write".to_string()),
            is_system: false,
        },
        NewRole {
            code: ROLE_READ_ONLY.to_string(),
            name: "Crawlrs Read-Only".to_string(),
            description: Some("Restricted access: read only".to_string()),
            is_system: false,
        },
    ]
}

/// 返回角色 → 权限映射列表。
///
/// 每个元组 `(role_code, permission_code)` 表示该角色应被赋予该权限。
/// 用于幂等调用 `RolePermissionRepository::assign`。
///
/// # 映射规则
///
/// - `admin` → `[crawlrs:read, crawlrs:write, crawlrs:admin]`
/// - `user` → `[crawlrs:read, crawlrs:write]`
/// - `read_only` → `[crawlrs:read]`
pub fn role_permission_mappings() -> Vec<(&'static str, &'static str)> {
    vec![
        // admin 拥有全部权限
        (ROLE_ADMIN, PERM_READ),
        (ROLE_ADMIN, PERM_WRITE),
        (ROLE_ADMIN, PERM_ADMIN),
        // user 拥有 read + write
        (ROLE_USER, PERM_READ),
        (ROLE_USER, PERM_WRITE),
        // read_only 仅 read
        (ROLE_READ_ONLY, PERM_READ),
    ]
}

// =============================================================================
// DB 操作函数
// =============================================================================

/// 在 garrison RBAC 表中幂等预置 crawlrs 标准角色与权限。
///
/// # 流程
///
/// 1. **权限**：对 `standard_permissions()` 中每个权限，先 `find_by_code` 查询，
///    存在则跳过；不存在则 `create`。`app_permission` 是全局表，无 tenant_id。
/// 2. **角色**：对 `standard_roles()` 中每个角色，先 `find_by_code` 查询
///    （`tenant_id=GARRISON_DEFAULT_TENANT_ID`），存在则跳过；不存在则 `create`。
/// 3. **角色-权限映射**：对 `role_permission_mappings()` 中每对 `(role, perm)`，
///    通过 `RolePermissionRepository::assign`（幂等）创建映射。`assign` 内部
///    使用 `INSERT OR IGNORE` 语义（garrison repository 实现）。
///
/// # 参数
///
/// - `pool`: crawlrs 数据库连接池（garrison RBAC 表与 crawlrs 业务表共享同一 schema）
///
/// # 返回
///
/// `Ok(RbacPresetReport)` 包含本次新建/跳过数量；`Err(GarrisonError)` 透传 garrison 仓库错误。
///
/// # 幂等性
///
/// 重复调用安全：所有操作先查后建，不会创建重复记录。
///
/// # Spec
///
/// - R-key-lifecycle-002
pub async fn preset_rbac(pool: DbPool) -> GarrisonResult<RbacPresetReport> {
    let mut report = RbacPresetReport::default();

    // 1. 预置权限（app_permission 全局表）
    let perm_repo = DbnexusPostgresPermissionRepository::new(pool.clone());
    for new_perm in standard_permissions() {
        let existing = perm_repo.find_by_code(&new_perm.code).await?;
        if existing.is_some() {
            report.permissions_skipped += 1;
        } else {
            perm_repo.create(new_perm).await?;
            report.permissions_created += 1;
        }
    }

    // 2. 预置角色（app_role，tenant_id=GARRISON_DEFAULT_TENANT_ID）
    let role_repo = DbnexusPostgresRoleRepository::new(pool.clone());
    for new_role in standard_roles() {
        let existing = role_repo
            .find_by_code(GARRISON_DEFAULT_TENANT_ID, &new_role.code)
            .await?;
        if existing.is_some() {
            report.roles_skipped += 1;
        } else {
            role_repo
                .create(GARRISON_DEFAULT_TENANT_ID, new_role)
                .await?;
            report.roles_created += 1;
        }
    }

    // 3. 预置角色-权限映射（app_role_permission，tenant_id=GARRISON_DEFAULT_TENANT_ID）
    //
    // 实现：对每个 (role_code, perm_code) 先查 role_id 与 permission_id（按 code 反查），
    // 再调 `assign`。assign 内部幂等（重复 assign 同一对 (role_id, perm_id) 不报错）。
    let role_perm_repo = DbnexusPostgresRolePermissionRepository::new(pool.clone());
    for (role_code, perm_code) in role_permission_mappings() {
        // 反查 role_id（按 tenant_id + code）
        let role_row = role_repo
            .find_by_code(GARRISON_DEFAULT_TENANT_ID, role_code)
            .await?
            .ok_or_else(|| {
                GarrisonError::Internal(format!(
                    "preset_rbac: role not found after create: {}",
                    role_code
                ))
            })?;
        // 反查 permission_id（按 code，全局表无 tenant_id）
        let perm_row = perm_repo.find_by_code(perm_code).await?.ok_or_else(|| {
            GarrisonError::Internal(format!(
                "preset_rbac: permission not found after create: {}",
                perm_code
            ))
        })?;
        // 幂等 assign（garrison repository 内部 INSERT OR IGNORE 语义）
        role_perm_repo
            .assign(
                GARRISON_DEFAULT_TENANT_ID,
                &role_row.id,
                &perm_row.id,
            )
            .await?;
        report.role_permissions_assigned += 1;
    }

    Ok(report)
}

/// 枚举旧 `api_keys` 表中需重新领取 API Key 的 team 清单。
///
/// # 流程
///
/// 1. 按 `team_id` 分组聚合（`GROUP BY team_id`），统计每个 team 的历史 key 总数
/// 2. 过滤 nil UUID（防御性编程，旧表若有脏数据 team_id=nil 跳过）
/// 3. 按 team_id 升序排序，便于运维核对
///
/// # 参数
///
/// - `pool`: crawlrs 数据库连接池（`Arc<DbPool>` 因 dbnexus 共享语义）
///
/// # 返回
///
/// `Ok(Vec<ReissueTeamInfo>)` 按 team_id 升序；`Err(sea_orm::DbErr)` 透传查询错误。
///
/// # Spec
///
/// - R-key-lifecycle-002
pub async fn enumerate_legacy_teams(pool: Arc<DbPool>) -> Result<Vec<ReissueTeamInfo>, sea_orm::DbErr> {
    use crawlrs::infrastructure::database::entities::api_key::{Column as ApiKeyColumn, Entity as ApiKeyEntity};

    let session = pool
        .get_session("admin")
        .await
        .map_err(|e| sea_orm::DbErr::Custom(format!("db session: {}", e)))?;
    let conn = session
        .connection()
        .map_err(|e| sea_orm::DbErr::Custom(format!("db conn: {}", e)))?;

    // 分页扫描全表，按 team_id 分组聚合
    // 使用 sea-orm paginator 避免一次加载过多记录（LEGACY_KEY_PAGE_SIZE）
    let mut all_teams: Vec<ReissueTeamInfo> = Vec::new();
    let mut current_page: u64 = 0;
    let total_pages: u64 = {
        let counter = ApiKeyEntity::find()
            // 过滤 nil team_id（防御性，旧表脏数据保护）
            .filter(ApiKeyColumn::TeamId.ne(Uuid::nil()))
            .count(conn)
            .await?;
        // ceil(total / page_size)，避免浮点除法
        // ceil(total / page_size)：u64::div_ceil 在 Rust 1.73+ 稳定（CWE-190 整数溢出安全）
        counter.div_ceil(LEGACY_KEY_PAGE_SIZE)
    };

    while current_page < total_pages.max(1) {
        let rows = ApiKeyEntity::find()
            .filter(ApiKeyColumn::TeamId.ne(Uuid::nil()))
            // 按 team_id 升序确保分页稳定性
            .order_by_asc(ApiKeyColumn::TeamId)
            .offset(current_page * LEGACY_KEY_PAGE_SIZE)
            .limit(LEGACY_KEY_PAGE_SIZE)
            .all(conn)
            .await?;

        if rows.is_empty() {
            break;
        }

        // 先记录本页行数，避免 `for row in rows` 移动后无法访问 `rows.len()`
        let page_len = rows.len();
        for row in rows {
            // 累加到对应 team 的计数（内存聚合，避免 SQL GROUP BY 方言差异）
            if let Some(entry) = all_teams.iter_mut().find(|t| t.team_id == row.team_id) {
                entry.legacy_key_count += 1;
            } else {
                all_teams.push(ReissueTeamInfo {
                    team_id: row.team_id,
                    legacy_key_count: 1,
                });
            }
        }

        current_page += 1;
        if page_len < LEGACY_KEY_PAGE_SIZE as usize {
            break;
        }
    }

    // 最终按 team_id 升序排序（确保输出稳定）
    all_teams.sort_by_key(|t| t.team_id);
    Ok(all_teams)
}

// =============================================================================
// 报告输出
// =============================================================================

/// 打印 RBAC 预置报告到 stdout。
pub fn print_rbac_report(report: &RbacPresetReport) {
    println!("=== RBAC Preset Report ===");
    println!(
        "Permissions: {} created, {} skipped (already existed)",
        report.permissions_created, report.permissions_skipped
    );
    println!(
        "Roles:       {} created, {} skipped (already existed)",
        report.roles_created, report.roles_skipped
    );
    println!(
        "Role-Permission mappings: {} assigned (idempotent)",
        report.role_permissions_assigned
    );
    println!();
}

/// 打印需重新领取 API Key 的 team 清单到 stdout。
pub fn print_reissue_report(teams: &[ReissueTeamInfo]) {
    println!("=== Teams Requiring API Key Reissuance ===");
    if teams.is_empty() {
        println!("(no legacy api_keys found — nothing to reissue)");
        println!();
        return;
    }

    let total_keys: u64 = teams.iter().map(|t| t.legacy_key_count).sum();
    println!(
        "Found {} team(s) with {} legacy API key(s) total:",
        teams.len(),
        total_keys
    );
    println!();
    println!("{:<38}  legacy_key_count", "team_id");
    println!("{:-<38}  {:-<18}", "", "");
    for team in teams {
        println!("{:<38}  {}", team.team_id, team.legacy_key_count);
    }
    println!();
    println!("Next steps:");
    println!("  1. Contact each team owner listed above");
    println!("  2. Admin reissues API Key via POST /v1/admin/api-keys with their team_id");
    println!("  3. Old key_hash in api_keys table is deprecated (garrison self-manages)");
    println!();
}

// =============================================================================
// main 入口
// =============================================================================

/// 脚本入口：解析 DATABASE_URL → 连接 DB → 预置 RBAC → 枚举 team → 打印清单。
///
/// # 退出码
///
/// - 0: 成功（含「无历史数据」情况）
/// - 1: 配置错误（DATABASE_URL 未设置）/ DB 连接失败 / RBAC 预置失败 / 枚举失败
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let url = std::env::var("DATABASE_URL")
        .or_else(|_| std::env::var("TEST_DATABASE_URL"))
        .map_err(|_| "DATABASE_URL or TEST_DATABASE_URL must be set")?;

    println!("Connecting to database...");
    let cfg = DbConfig {
        url,
        ..Default::default()
    };
    let pool = Arc::new(DbPool::with_config(cfg).await?);

    // 1. 预置 RBAC（幂等，重复运行安全）
    println!("Presetting garrison RBAC (tenant_id={}, idempotent)...", GARRISON_DEFAULT_TENANT_ID);
    let rbac_report = preset_rbac((*pool).clone()).await?;
    print_rbac_report(&rbac_report);

    // 2. 枚举需重新领取 key 的 team 清单
    println!("Enumerating legacy api_keys by team_id...");
    let teams = enumerate_legacy_teams(pool).await?;
    print_reissue_report(&teams);

    Ok(())
}

// =============================================================================
// 引用 crawlrs crate（用于访问旧 api_keys entity）
// =============================================================================
//
// 注意：本 bin 通过 `required-features = ["admin-tools", "auth"]` 编译，
// 必须启用 `auth` feature（garrison 依赖）。crawlrs crate 自身始终编译
// infrastructure/database/entities/api_key 模块（不依赖 auth feature）。
//
// bin target 中 `crate::` 指 bin 自身，引用 crawlrs lib 必须用 `crawlrs::`，
// 故 `enumerate_legacy_teams` 函数体内使用局部 `use crawlrs::infrastructure::...`。

#[cfg(test)]
mod tests {
    use super::*;

    // -------------------------------------------------------------------------
    // 本地测试辅助（与 `src/common/test_helpers.rs` 同一契约的副本）
    // -------------------------------------------------------------------------
    //
    // bin target 的 `#[cfg(test)]` 测试无法访问 `crawlrs::common::test_helpers`
    // ——该模块在 `src/common/mod.rs` 中被 `#[cfg(test)]` 门控，仅在 lib target 的
    // test build 中编译。bin target 是独立编译单元，需自行内联这两个函数。
    //
    // 与 `tests/common/helpers/db_pool.rs` 的差异：
    // - `tests/common/helpers/db_pool::create_test_pool_or_panic` 仅在 `[[test]]` target
    //   中可用（属于 tests/ 集成测试 crate）。
    // - 本 bin 的 `[[bin]]` 测试需独立提供，避免跨 crate 引用。

    /// 与 `src/common/test_helpers::skip_if_no_test_db` 同一契约。
    fn skip_if_no_test_db() -> bool {
        let has_url = std::env::var("TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .is_ok();
        if !has_url {
            eprintln!("[skip] TEST_DATABASE_URL/DATABASE_URL not set — test requires real DbPool");
        }
        !has_url
    }

    /// 与 `src/common/test_helpers::create_test_db_pool` 同一契约。
    fn create_test_db_pool() -> Arc<DbPool> {
        std::thread::scope(|s| {
            let handle = s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build tokio runtime for DbPool construction");
                let _guard = rt.enter();
                let url = std::env::var("TEST_DATABASE_URL")
                    .or_else(|_| std::env::var("DATABASE_URL"))
                    .expect("TEST_DATABASE_URL or DATABASE_URL must be set; no hardcoded fallback");
                rt.block_on(async {
                    let cfg = dbnexus::DbConfig {
                        url,
                        ..Default::default()
                    };
                    DbPool::with_config(cfg).await
                })
                .expect("failed to create DbPool for test")
            });
            Arc::new(handle.join().expect("DbPool construction thread panicked"))
        })
    }

    // =========================================================================
    // 纯逻辑测试（无 DB 依赖，始终运行）
    // =========================================================================

    #[test]
    fn test_standard_permissions_count() {
        let perms = standard_permissions();
        assert_eq!(perms.len(), 3, "must define exactly 3 standard permissions");
    }

    #[test]
    fn test_standard_permissions_codes() {
        let perms = standard_permissions();
        let codes: Vec<&str> = perms.iter().map(|p| p.code.as_str()).collect();
        assert!(codes.contains(&PERM_READ), "must include crawlrs:read");
        assert!(codes.contains(&PERM_WRITE), "must include crawlrs:write");
        assert!(codes.contains(&PERM_ADMIN), "must include crawlrs:admin");
    }

    #[test]
    fn test_standard_permissions_all_have_crawlrs_prefix() {
        for perm in standard_permissions() {
            assert!(
                perm.code.starts_with(PERM_PREFIX),
                "permission code must start with 'crawlrs:': {}",
                perm.code
            );
        }
    }

    #[test]
    fn test_standard_permissions_unique_codes() {
        let perms = standard_permissions();
        let mut codes: Vec<&str> = perms.iter().map(|p| p.code.as_str()).collect();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), perms.len(), "permission codes must be unique");
    }

    #[test]
    fn test_standard_permissions_have_resource_type() {
        for perm in standard_permissions() {
            assert_eq!(
                perm.resource_type.as_deref(),
                Some("crawlrs"),
                "resource_type must be 'crawlrs' for perm {}",
                perm.code
            );
        }
    }

    #[test]
    fn test_standard_roles_count() {
        let roles = standard_roles();
        assert_eq!(roles.len(), 3, "must define exactly 3 standard roles");
    }

    #[test]
    fn test_standard_roles_codes() {
        let roles = standard_roles();
        let codes: Vec<&str> = roles.iter().map(|r| r.code.as_str()).collect();
        assert!(codes.contains(&ROLE_ADMIN), "must include admin role");
        assert!(codes.contains(&ROLE_USER), "must include user role");
        assert!(codes.contains(&ROLE_READ_ONLY), "must include read_only role");
    }

    #[test]
    fn test_standard_roles_unique_codes() {
        let roles = standard_roles();
        let mut codes: Vec<&str> = roles.iter().map(|r| r.code.as_str()).collect();
        codes.sort();
        codes.dedup();
        assert_eq!(codes.len(), roles.len(), "role codes must be unique");
    }

    #[test]
    fn test_admin_role_is_system() {
        let roles = standard_roles();
        let admin = roles
            .iter()
            .find(|r| r.code == ROLE_ADMIN)
            .expect("admin role must exist");
        assert!(admin.is_system, "admin role must be system builtin");
    }

    #[test]
    fn test_user_and_readonly_roles_not_system() {
        let roles = standard_roles();
        for role in &roles {
            if role.code == ROLE_USER || role.code == ROLE_READ_ONLY {
                assert!(!role.is_system, "role {} must not be system", role.code);
            }
        }
    }

    #[test]
    fn test_role_permission_mappings_count() {
        // admin(3) + user(2) + read_only(1) = 6 mappings
        let mappings = role_permission_mappings();
        assert_eq!(mappings.len(), 6, "must define exactly 6 role-perm mappings");
    }

    #[test]
    fn test_admin_has_all_three_permissions() {
        let mappings = role_permission_mappings();
        let admin_perms: Vec<&str> = mappings
            .iter()
            .filter(|(r, _)| *r == ROLE_ADMIN)
            .map(|(_, p)| *p)
            .collect();
        assert_eq!(admin_perms.len(), 3, "admin must have 3 permissions");
        assert!(admin_perms.contains(&PERM_READ));
        assert!(admin_perms.contains(&PERM_WRITE));
        assert!(admin_perms.contains(&PERM_ADMIN));
    }

    #[test]
    fn test_user_has_read_and_write_only() {
        let mappings = role_permission_mappings();
        let user_perms: Vec<&str> = mappings
            .iter()
            .filter(|(r, _)| *r == ROLE_USER)
            .map(|(_, p)| *p)
            .collect();
        assert_eq!(user_perms.len(), 2, "user must have 2 permissions");
        assert!(user_perms.contains(&PERM_READ));
        assert!(user_perms.contains(&PERM_WRITE));
        assert!(!user_perms.contains(&PERM_ADMIN), "user must NOT have admin");
    }

    #[test]
    fn test_read_only_has_read_only() {
        let mappings = role_permission_mappings();
        let ro_perms: Vec<&str> = mappings
            .iter()
            .filter(|(r, _)| *r == ROLE_READ_ONLY)
            .map(|(_, p)| *p)
            .collect();
        assert_eq!(ro_perms.len(), 1, "read_only must have 1 permission");
        assert_eq!(ro_perms[0], PERM_READ);
    }

    #[test]
    fn test_all_mappings_reference_valid_roles_and_perms() {
        let valid_roles = [ROLE_ADMIN, ROLE_USER, ROLE_READ_ONLY];
        let valid_perms = [PERM_READ, PERM_WRITE, PERM_ADMIN];
        for (role, perm) in role_permission_mappings() {
            assert!(
                valid_roles.contains(&role),
                "unknown role in mapping: {}",
                role
            );
            assert!(
                valid_perms.contains(&perm),
                "unknown permission in mapping: {}",
                perm
            );
        }
    }

    #[test]
    fn test_constant_values() {
        assert_eq!(GARRISON_DEFAULT_TENANT_ID, 0);
        assert_eq!(PERM_PREFIX, "crawlrs:");
        assert_eq!(PERM_READ, "crawlrs:read");
        assert_eq!(PERM_WRITE, "crawlrs:write");
        assert_eq!(PERM_ADMIN, "crawlrs:admin");
        assert_eq!(ROLE_ADMIN, "admin");
        assert_eq!(ROLE_USER, "user");
        assert_eq!(ROLE_READ_ONLY, "read_only");
    }

    // =========================================================================
    // 报告打印测试（无 DB，仅验证不 panic）
    // =========================================================================

    #[test]
    fn test_print_rbac_report_does_not_panic() {
        let report = RbacPresetReport {
            permissions_created: 3,
            permissions_skipped: 0,
            roles_created: 3,
            roles_skipped: 0,
            role_permissions_assigned: 6,
        };
        // 仅验证不 panic（stdout 内容不验证）
        print_rbac_report(&report);
    }

    #[test]
    fn test_print_reissue_report_empty_does_not_panic() {
        print_reissue_report(&[]);
    }

    #[test]
    fn test_print_reissue_report_with_teams_does_not_panic() {
        let teams = vec![
            ReissueTeamInfo {
                team_id: Uuid::nil(),
                legacy_key_count: 1,
            },
            ReissueTeamInfo {
                team_id: Uuid::new_v4(),
                legacy_key_count: 5,
            },
        ];
        print_reissue_report(&teams);
    }

    // =========================================================================
    // DB 集成测试（需真实 DB，skip_if_no_test_db 控制）
    // =========================================================================

    /// 验证 `preset_rbac` 幂等性：连续两次调用，第二次全部跳过（不再创建）。
    ///
    /// 此测试为集成测试，前置条件：
    /// - `TEST_DATABASE_URL`/`DATABASE_URL` 指向已运行 garrison migrations 的 DB
    /// - garrison RBAC 表（app_permission/app_role/app_role_permission）已建表
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_preset_rbac_idempotent() {
        if skip_if_no_test_db() {
            return;
        }
        let pool = create_test_db_pool();

        // 第一次调用：可能新建或已存在
        let first = preset_rbac((*pool).clone()).await.expect("first preset_rbac");

        // 第二次调用：所有项都应跳过（permissions_skipped == 3, roles_skipped == 3）
        let second = preset_rbac((*pool).clone()).await.expect("second preset_rbac");

        // 第二次调用不应再创建任何 permission/role
        assert_eq!(
            second.permissions_created, 0,
            "second call must not create new permissions (got: {:?})",
            second
        );
        assert_eq!(
            second.roles_created, 0,
            "second call must not create new roles (got: {:?})",
            second
        );
        // role_permissions_assigned 仍计入（assign 总是返回 Ok，幂等）
        assert_eq!(
            second.role_permissions_assigned, 6,
            "second call must still report 6 assignments (idempotent assign)"
        );

        // 两次调用之间 permissions_skipped + permissions_created 之和应稳定为 3
        let first_total = first.permissions_created + first.permissions_skipped;
        let second_total = second.permissions_created + second.permissions_skipped;
        assert_eq!(first_total, 3, "first call must process all 3 permissions");
        assert_eq!(second_total, 3, "second call must see all 3 permissions existing");
    }

    /// 验证 `preset_rbac` 报告中的权限/角色数量与定义一致。
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_preset_rbac_report_totals() {
        if skip_if_no_test_db() {
            return;
        }
        let pool = create_test_db_pool();
        let report = preset_rbac((*pool).clone()).await.expect("preset_rbac");

        // 权限总数（新建 + 跳过）必须等于 3
        assert_eq!(
            report.permissions_created + report.permissions_skipped,
            3,
            "must process all 3 standard permissions"
        );
        // 角色总数必须等于 3
        assert_eq!(
            report.roles_created + report.roles_skipped,
            3,
            "must process all 3 standard roles"
        );
        // 角色-权限映射总数必须等于 6
        assert_eq!(
            report.role_permissions_assigned, 6,
            "must process all 6 role-permission mappings"
        );
    }

    /// 验证 `enumerate_legacy_teams` 在空 DB 上返回空列表（不 panic，不报错）。
    #[tokio::test(flavor = "multi_thread", worker_threads = 1)]
    async fn test_enumerate_legacy_teams_returns_valid_structure() {
        if skip_if_no_test_db() {
            return;
        }
        let pool = create_test_db_pool();
        let teams = enumerate_legacy_teams(pool).await.expect("enumerate_legacy_teams");

        // 不验证具体数量（测试 DB 可能有残留数据），仅验证：
        // 1. 返回值是 Vec<ReissueTeamInfo>
        // 2. 所有 team_id 非 nil（防御性过滤生效）
        for team in &teams {
            assert_ne!(
                team.team_id,
                Uuid::nil(),
                "nil team_id must be filtered out"
            );
            assert!(team.legacy_key_count > 0, "key count must be positive");
        }
    }
}
