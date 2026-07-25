// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

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
//!
//! ## 设计决策（R-auth-engine-002）
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
pub mod garrison_interface;

// Re-export 业务层常用类型，避免业务代码直接 use garrison::prelude
pub use garrison_config::{build_garrison_config, GarrisonConfigError};
pub use garrison_dao::init_garrison_dao;
pub use garrison_interface::CrawlrsGarrisonInterface;
