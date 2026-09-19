// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Garrison 认证基础设施模块。
//!
//! 封装 [`garrison`] 认证鉴权框架与 crawlrs 业务层的桥接逻辑。
//! feature 门控（`#[cfg(feature = "auth")]`）见上级 [`crate::infrastructure`] 的 `mod auth` 声明。
//!
//! ## 子模块
//!
//! - [`garrison_config`] — `GarrisonConfig` 构造器（从 confers 读 jwt_secret/超时等，弱密钥拒绝）
//! - [`garrison_dao`] — DAO 工厂（复用 garrison 内建 [`GarrisonDaoOxcache`]，无需自实现）
//! - [`garrison_interface`] — RBAC 接口（实现 garrison `GarrisonInterface`，从 RBAC 表读权限/角色）
//! - [`garrison_listener`] — 审计监听器（实现 [`GarrisonListener`]，桥接 [`GarrisonEvent`] → [`AuditServiceTrait`]）
//!
//! ## 设计决策
//!
//! - **DAO 复用而非自实现**：garrison v0.8.1 内建 [`GarrisonDaoOxcache::new()`] 已实现完整 `GarrisonDao` trait
//!   （自管理 oxcache 实例），按 proposal「全量重签 + garrison 原生存储」garrison 用自己的 schema，
//!   不读 crawlrs 旧 `api_keys`/`scopes` 表，故无需共享 crawlrs 的 `pool`/`cache`。
//! - **Interface 自实现**：`GarrisonInterface` 是业务回调 trait（`get_permission_list`/`get_role_list`），
//!   需按 crawlrs 的 RBAC 角色数据返回，不能复用内建实现。
//!
//! ## 命名约定
//!
//! - `build_*`：纯构造无副作用（如 [`garrison_config::build_garrison_config`]）
//! - `init_*`：有副作用/启动资源（如 [`garrison_dao::init_garrison_dao`] 启动 oxcache 实例）

pub mod garrison_config;
pub mod garrison_dao;
// garrison_interface 依赖 dbnexus::DbPool（仅 platform 引入该依赖），其唯一构造方
// bootstrap::services 亦为 platform 门控，故随 platform 而非 auth 编译。
#[cfg(feature = "platform")]
pub mod garrison_interface;
pub mod garrison_listener;

// Re-export 业务层常用类型，避免业务代码直接 use garrison::prelude
pub use garrison_config::{build_garrison_config, GarrisonConfigError};
pub use garrison_dao::{get_garrison_dao, init_garrison_dao, set_garrison_dao};
#[cfg(feature = "platform")]
pub use garrison_interface::CrawlrsGarrisonInterface;
pub use garrison_listener::{set_audit_service, wait_audit_tasks, CrawlrsAuditListener};

/// 重置 garrison 全局态（DAO + AUDIT_SERVICE），供集成测试二进制使用。
///
/// lib 内单元测试经 `common::test_helpers::acquire_garrison_global_state()`
/// 间接重置；但集成测试（tests/integration）是独立 crate，无法访问
/// `pub(crate)` 重置函数，且 garrison 单例在同一进程内只允许注入一次。
/// `test-mocks` 门控下暴露本函数，让同一进程内顺序运行的多个集成测试
/// 各自获得干净的全局态。
///
/// 调用方须持有测试序列化锁（如 `auth_garrison_test::GARRISON_TEST_LOCK`），
/// 确保重置与后续 `init_garrison_auth` 之间无并发窗口。
#[cfg(all(feature = "auth", feature = "test-mocks"))]
pub fn reset_garrison_global_state_for_integration_test() {
    garrison_dao::reset_garrison_dao_for_test();
    garrison_listener::reset_audit_service_for_test();
}
