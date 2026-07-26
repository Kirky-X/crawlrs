// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

// T028：`Model` 已弃用（garrison RBAC 接管），sea-orm `DeriveEntityModel` 派生宏
// 生成的代码会访问 `Model`，引发 deprecated warning。文件级 `#[allow(deprecated)]`
// 消除派生宏 warning，仅对本文件生效。待全量重签完成后随 `scopes` 表一并移除。
#![allow(deprecated)]

use sea_orm::entity::prelude::*;
use uuid::Uuid;

/// 旧 `scopes` 表 entity（已弃用，R-key-lifecycle-003 / T028）。
///
/// # 弃用说明
///
/// garrison RBAC 接管后，`scopes` 表不再被认证/签发路径调用。
/// 权限串改为 `crawlrs:read`/`crawlrs:write`/`crawlrs:admin`，由 garrison
/// `CrawlrsGarrisonInterface` 返回，经 `auth_bridge::map_perms_to_scope` 映射。
///
/// 此 entity 保留供历史数据迁移/查询使用，不参与热路径。待全量重签完成后可移除。
#[deprecated(
    since = "0.2.0",
    note = "garrison RBAC 接管；保留仅供历史数据迁移/查询"
)]
#[derive(Clone, Debug, PartialEq, DeriveEntityModel)]
#[sea_orm(table_name = "scopes")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: Uuid,
    #[sea_orm(unique)]
    pub api_key_id: Uuid,
    pub read: bool,
    pub write: bool,
    pub admin: bool,
    pub search_limit: i32,
    pub scrape_limit: i32,
    pub created_at: ChronoDateTimeWithTimeZone,
    pub updated_at: ChronoDateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
