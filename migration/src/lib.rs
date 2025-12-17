// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

pub use sea_orm_migration::prelude::*;

mod m20251211_initial_schema;
mod m20251216_add_expires_at_to_tasks;

/// 数据库迁移器
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
            Box::new(m20251216_add_expires_at_to_tasks::Migration),
        ]
    }
}
