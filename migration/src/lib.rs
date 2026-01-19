// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

pub use sea_orm_migration::prelude::*;

mod m20250101_unified_schema;

/// 数据库迁移器
///
/// 管理数据库模式迁移，负责数据库结构的版本控制
pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    /// 获取所有迁移
    ///
    /// # 返回值
    ///
    /// 返回迁移列表
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m20250101_unified_schema::Migration)]
    }
}
