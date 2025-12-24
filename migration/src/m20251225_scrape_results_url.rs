// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use sea_orm_migration::prelude::*;
use sea_orm::DbBackend;

/// 为 scrape_results 表添加 url 字段的迁移
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// 应用迁移 - 为 scrape_results 表添加 url 字段
    ///
    /// # 参数
    ///
    /// * `manager` - 数据库模式管理器
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 迁移成功
    /// * `Err(DbErr)` - 迁移失败
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        if backend == DbBackend::Sqlite {
            manager
                .alter_table(
                    Table::alter()
                        .table(ScrapeResults::Table)
                        .add_column_if_not_exists(
                            ColumnDef::new(ScrapeResults::Url)
                                .text()
                                .not_null()
                                .default(""),
                        )
                        .to_owned(),
                )
                .await?;
        } else {
            manager
                .alter_table(
                    Table::alter()
                        .table(ScrapeResults::Table)
                        .add_column_if_not_exists(
                            ColumnDef::new(ScrapeResults::Url)
                                .string()
                                .not_null()
                                .default(""),
                        )
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }

    /// 回滚迁移 - 删除 scrape_results 表的 url 字段
    ///
    /// # 参数
    ///
    /// * `manager` - 数据库模式管理器
    ///
    /// # 返回值
    ///
    /// * `Ok(())` - 回滚成功
    /// * `Err(DbErr)` - 回滚失败
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ScrapeResults::Table)
                    .drop_column(ScrapeResults::Url)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

/// ScrapeResults 表字段定义
#[derive(DeriveIden)]
enum ScrapeResults {
    Table,
    Url,
}
