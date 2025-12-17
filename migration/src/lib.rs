// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

pub use sea_orm_migration::prelude::*;

mod m20251211_initial_schema;

/// 数据库迁移器
///
/// 管理数据库模式迁移，负责数据库结构的版本控制
/// 包含所有数据库迁移的定义和执行逻辑
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    /// 获取所有迁移
    ///
    /// # 返回值
    ///
    /// 返回迁移列表
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![
            Box::new(m20251211_initial_schema::Migration),
        ]
    }
}
