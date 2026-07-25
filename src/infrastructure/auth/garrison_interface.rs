// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Garrison RBAC Interface 实现。
//!
//! [`CrawlrsGarrisonInterface`] 实现 garrison [`GarrisonInterface`] trait，
//! 提供 `get_permission_list(login_id)` 与 `get_role_list(login_id)` 两个回调方法，
//! 从 garrison RBAC 表（`app_user_role` / `app_role` / `app_role_permission` / `app_permission`）
//! 读取该 login_id 的权限/角色。
//!
//! ## 多租户上下文
//!
//! 启用 `tenant-isolation` feature 时，本实现通过
//! [`garrison::context::tenant::current_tenant_id_or_error()`] 读取当前 task-local
//! 租户上下文（fail-closed，无上下文返回 `Err`）。
//!
//! ## Spec
//!
//! - R-authz-rbac-001：`get_permission_list(login_id)` 对具 admin 角色的 id 返回含 `crawlrs:admin`；
//!   `get_role_list` 返回该 id 角色。

use async_trait::async_trait;
use dbnexus::DbPool;
use garrison::context::tenant::current_tenant_id_or_error;
use garrison::dao::repository::postgres::{
    DbnexusPostgresPermissionRepository, DbnexusPostgresRolePermissionRepository,
    DbnexusPostgresRoleRepository, DbnexusPostgresUserRoleRepository,
};
use garrison::dao::repository::{
    PermissionRepository, RolePermissionRepository, RoleRepository, UserRoleRepository,
};
use garrison::prelude::{GarrisonInterface, GarrisonResult};
use std::collections::HashSet;

/// crawlrs 业务层的 garrison RBAC Interface 实现。
///
/// # 字段
///
/// - `pool` — crawlrs 的数据库连接池（`dbnexus::DbPool`），用于查询 garrison RBAC 表。
///   `DbPool` 内部为 `Arc<DbPoolInner>`，`Clone` 廉价，故构造时直接克隆。
///
/// # Spec
///
/// - R-authz-rbac-001
///
/// # 使用
///
/// 通常由 `ServiceModule` 构建期注入到 [`garrison::manager::GarrisonManager::init`]：
///
/// ```no_run
/// # use std::sync::Arc;
/// # use crawlrs::infrastructure::auth::garrison_interface::CrawlrsGarrisonInterface;
/// # use garrison::prelude::*;
/// # fn demo(pool: dbnexus::DbPool) {
/// let interface: Arc<dyn GarrisonInterface> = Arc::new(
///     CrawlrsGarrisonInterface::new(pool)
/// );
/// # }
/// ```
pub struct CrawlrsGarrisonInterface {
    /// crawlrs 数据库连接池（共享 garrison RBAC 表所在 schema）。
    pool: DbPool,
}

impl CrawlrsGarrisonInterface {
    /// 构造 [`CrawlrsGarrisonInterface`]。
    ///
    /// # 参数
    ///
    /// - `pool` — crawlrs 的数据库连接池（`DbPool` 内部为 `Arc`，克隆廉价）
    ///
    /// # Spec
    ///
    /// - R-authz-rbac-001
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl GarrisonInterface for CrawlrsGarrisonInterface {
    /// 获取指定 login_id 的权限列表。
    ///
    /// # 流程
    ///
    /// 1. 从当前 task-local 读取 `tenant_id`（fail-closed）
    /// 2. 查询 `app_user_role` 取该用户的所有 role_id
    /// 3. 对每个 role_id 查询 `app_role_permission` 取 permission_id 列表
    /// 4. 对每个 permission_id 查询 `app_permission` 取 permission code
    /// 5. 去重后返回权限串列表
    ///
    /// # 约定
    ///
    /// - 返回的权限串采用 `crawlrs:{action}` 格式（如 `crawlrs:read`、`crawlrs:write`、`crawlrs:admin`）
    /// - admin 角色自动蕴含 `read` + `write`（在 garrison role_hierarchy 表中预置）
    ///
    /// # 参数
    ///
    /// - `login_id` — 主体标识（对应 crawlrs api_key_id 的字符串形式，存于 `app_user_role.user_id`）
    ///
    /// # 返回
    ///
    /// - `Ok(Vec<String>)` — 权限串列表（去重）
    /// - `Err(GarrisonError)` — 租户上下文缺失或数据库查询失败
    ///
    /// # Spec
    ///
    /// - R-authz-rbac-001
    async fn get_permission_list(&self, login_id: &str) -> GarrisonResult<Vec<String>> {
        let tenant_id = current_tenant_id_or_error()?;

        // DbPool 内部为 Arc<DbPoolInner>，clone 廉价（Arc::clone <10ns），
        // 入口处克隆一次供多个 repository 共用，避免重复 clone（规则5 简洁优先）。
        let pool = self.pool.clone();

        // 1. 取用户角色关联
        let user_role_repo = DbnexusPostgresUserRoleRepository::new(pool.clone());
        let user_roles = user_role_repo.find_by_user_id(tenant_id, login_id).await?;

        if user_roles.is_empty() {
            return Ok(Vec::new());
        }

        // 2. 对每个 role_id 取 role_permission 关联
        let role_perm_repo = DbnexusPostgresRolePermissionRepository::new(pool.clone());
        let perm_repo = DbnexusPostgresPermissionRepository::new(pool);

        // 预估容量避免扩容 rehash（P-MEDIUM-003 修复）。
        let mut permission_ids: HashSet<String> = HashSet::with_capacity(user_roles.len());
        for ur in &user_roles {
            let role_perms = role_perm_repo.find_by_role_id(tenant_id, &ur.role_id).await?;
            for rp in role_perms {
                permission_ids.insert(rp.permission_id);
            }
        }

        if permission_ids.is_empty() {
            return Ok(Vec::new());
        }

        // 3. 对每个 permission_id 取 permission code。
        //
        // 注意：`PermissionRepository::find_by_id` 签名为 `fn find_by_id(&self, id: &str)`，
        // **不传 tenant_id**——这是 garrison 框架设计：`app_permission` 表是全局表
        // （permission code 跨租户共享，如 crawlrs:read/crawlrs:write/crawlrs:admin），
        // 租户隔离由 `app_role_permission` 关联表的 tenant_id 过滤实现（步骤 2 已过滤）。
        // garrison 0.8.1 `src/dao/repository/mod.rs:399-401` 确认此签名。
        //
        // 去重说明：permission_ids 已是 HashSet（permission_id 唯一），
        // permission_id 与 code 一对一（DB 主键约束），故 code 也唯一，无需额外 seen HashSet
        // （P-MEDIUM-002 修复：删除冗余去重）。
        let mut perms: Vec<String> = Vec::with_capacity(permission_ids.len());
        for pid in permission_ids {
            let perm = perm_repo.find_by_id(&pid).await?;
            if let Some(row) = perm {
                perms.push(row.code);
            }
        }
        Ok(perms)
    }

    /// 获取指定 login_id 的角色列表。
    ///
    /// # 流程
    ///
    /// 1. 从当前 task-local 读取 `tenant_id`（fail-closed）
    /// 2. 查询 `app_user_role` 取该用户的所有 role_id
    /// 3. 对每个 role_id 查询 `app_role` 取 role code
    /// 4. 返回角色 code 列表
    ///
    /// # 约定
    ///
    /// - 返回的角色串采用 garrison RBAC 表定义的 role `code`（如 `admin`、`user`）
    /// - crawlrs 不引入新角色，所有角色由 garrison 管理
    ///
    /// # 参数
    ///
    /// - `login_id` — 主体标识
    ///
    /// # 返回
    ///
    /// - `Ok(Vec<String>)` — 角色 code 列表
    /// - `Err(GarrisonError)` — 租户上下文缺失或数据库查询失败
    ///
    /// # Spec
    ///
    /// - R-authz-rbac-001
    async fn get_role_list(&self, login_id: &str) -> GarrisonResult<Vec<String>> {
        let tenant_id = current_tenant_id_or_error()?;

        // 入口处克隆一次供多个 repository 共用（同 get_permission_list）。
        let pool = self.pool.clone();

        // 1. 取用户角色关联
        let user_role_repo = DbnexusPostgresUserRoleRepository::new(pool.clone());
        let user_roles = user_role_repo.find_by_user_id(tenant_id, login_id).await?;

        if user_roles.is_empty() {
            return Ok(Vec::new());
        }

        // 2. 对每个 role_id 取 role code
        let role_repo = DbnexusPostgresRoleRepository::new(pool);
        let mut roles: Vec<String> = Vec::with_capacity(user_roles.len());
        for ur in &user_roles {
            let role = role_repo.find_by_id(tenant_id, &ur.role_id).await?;
            if let Some(row) = role {
                roles.push(row.code);
            }
        }
        Ok(roles)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_helpers::{create_test_db_pool, skip_if_no_test_db};
    use garrison::context::tenant::{TenantContext, TenantSource, TENANT};

    /// 在默认租户上下文（tenant_id=0）中执行 future。
    ///
    /// 替代 garrison 内部 `with_default_tenant`（仅 `cfg(test)` 或 `testing` feature 可用）。
    /// crawlrs 测试使用 garrison 公开的 `TENANT` task_local + `TenantContext` 直接 scope。
    async fn with_default_tenant<F, R>(f: F) -> R
    where
        F: std::future::Future<Output = R>,
    {
        let ctx = TenantContext {
            tenant_id: 0,
            resolved_from: TenantSource::Header,
        };
        TENANT.scope(ctx, f).await
    }

    /// R-authz-rbac-001：构造 interface 不应 panic（需要真实 DbPool，因此依赖 TEST_DATABASE_URL）。
    #[test]
    fn test_interface_construct_does_not_panic() {
        if skip_if_no_test_db() {
            return;
        }
        let pool = create_test_db_pool();
        let _interface = CrawlrsGarrisonInterface::new((*pool).clone());
    }

    /// R-authz-rbac-001：无租户上下文时 `get_role_list` 必须 fail-closed（返回 `Err`）。
    ///
    /// 此测试验证 rule 12 失败必须显性化——未进入 `TENANT.scope` 时不应静默退化为 `tenant_id=0`。
    #[tokio::test]
    async fn test_get_role_list_fails_without_tenant_context() {
        if skip_if_no_test_db() {
            return;
        }
        let pool = create_test_db_pool();
        let interface = CrawlrsGarrisonInterface::new((*pool).clone());
        // 不进入 with_default_tenant，直接调用——必须返回 Err
        let result = interface.get_role_list("any_login_id").await;
        assert!(
            result.is_err(),
            "get_role_list must fail-closed when no tenant context is set"
        );
    }

    /// R-authz-rbac-001：无租户上下文时 `get_permission_list` 必须 fail-closed（返回 `Err`）。
    #[tokio::test]
    async fn test_get_permission_list_fails_without_tenant_context() {
        if skip_if_no_test_db() {
            return;
        }
        let pool = create_test_db_pool();
        let interface = CrawlrsGarrisonInterface::new((*pool).clone());
        let result = interface.get_permission_list("any_login_id").await;
        assert!(
            result.is_err(),
            "get_permission_list must fail-closed when no tenant context is set"
        );
    }

    /// R-authz-rbac-001：admin 角色 id 返回含 crawlrs:admin 权限（需真实 DB + garrison 迁移）。
    ///
    /// 此测试为集成测试，前置条件：
    /// 1. `TEST_DATABASE_URL` 指向已运行 garrison postgres migrations 的数据库
    /// 2. 测试数据：login_id=`test-admin` 的用户被分配 `admin` 角色
    /// 3. `admin` 角色已被分配 `crawlrs:admin` 权限
    ///
    /// **标记 `#[ignore]`**：完整端到端测试在 Stage 7 (`tests/integration/auth_garrison_test.rs`) 进行。
    /// Stage 1 阶段保留此测试骨架作为占位，避免伪装成有效测试（规则17 测试要验证有意义属性）。
    #[tokio::test]
    #[ignore = "Stage 7 集成测试覆盖：需真实 DB + garrison migrations + 预置 admin 角色数据"]
    async fn test_admin_role_returns_crawlrs_admin_permission() {
        if skip_if_no_test_db() {
            return;
        }
        let pool = create_test_db_pool();
        let interface = CrawlrsGarrisonInterface::new((*pool).clone());

        // 使用 with_default_tenant 提供 tenant_id=0 上下文
        let result = with_default_tenant(async {
            interface.get_permission_list("test-admin").await
        })
        .await;

        match result {
            Ok(perms) => {
                assert!(
                    perms.iter().any(|p| p == "crawlrs:admin"),
                    "admin role should include crawlrs:admin permission, got: {:?}",
                    perms
                );
            }
            // 仅打印错误类别，不打印完整错误避免泄露 DB 连接信息（L2 安全修复）。
            Err(e) => {
                panic!(
                    "get_permission_list failed (expected garrison migrations + admin test data): error kind = {:?}",
                    std::mem::discriminant(&e)
                );
            }
        }
    }
}
