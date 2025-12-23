// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use sea_orm_migration::prelude::*;
use sea_orm::DbBackend;

/// 地理限制功能迁移 - 为团队添加地理限制和IP白名单功能
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// 应用地理限制功能的数据库迁移
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
        // 1. 为 teams 表添加地理限制相关字段 (SQLite 需要分开执行)
        // 注意：SQLite 不支持 ALTER TABLE ADD COLUMN json 类型，我们使用 TEXT 代替
        let backend = manager.get_database_backend();

        // AllowedCountries
        if backend == DbBackend::Sqlite {
             manager
                .alter_table(
                    Table::alter()
                        .table(Teams::Table)
                        .add_column_if_not_exists(
                            ColumnDef::new(Teams::AllowedCountries)
                                .text() // Use text for SQLite compatibility
                                .null()
                                .comment("允许的国家代码列表，如 [\"US\", \"CN\"]，null 表示无限制"),
                        )
                        .to_owned(),
                )
                .await?;
        } else {
             manager
                .alter_table(
                    Table::alter()
                        .table(Teams::Table)
                        .add_column_if_not_exists(
                            ColumnDef::new(Teams::AllowedCountries)
                                .json()
                                .null()
                                .comment("允许的国家代码列表，如 [\"US\", \"CN\"]，null 表示无限制"),
                        )
                        .to_owned(),
                )
                .await?;
        }

        // BlockedCountries
        if backend == DbBackend::Sqlite {
             manager
                .alter_table(
                    Table::alter()
                        .table(Teams::Table)
                        .add_column_if_not_exists(
                            ColumnDef::new(Teams::BlockedCountries)
                                .text() // Use text for SQLite compatibility
                                .null()
                                .comment("阻止的国家代码列表，如 [\"RU\", \"KP\"]，null 表示无限制"),
                        )
                        .to_owned(),
                )
                .await?;
        } else {
             manager
                .alter_table(
                    Table::alter()
                        .table(Teams::Table)
                        .add_column_if_not_exists(
                            ColumnDef::new(Teams::BlockedCountries)
                                .json()
                                .null()
                                .comment("阻止的国家代码列表，如 [\"RU\", \"KP\"]，null 表示无限制"),
                        )
                        .to_owned(),
                )
                .await?;
        }

        // IpWhitelist
        if backend == DbBackend::Sqlite {
             manager
                .alter_table(
                    Table::alter()
                        .table(Teams::Table)
                        .add_column_if_not_exists(
                            ColumnDef::new(Teams::IpWhitelist)
                                .text() // Use text for SQLite compatibility
                                .null()
                                .comment("IP白名单列表，支持CIDR格式"),
                        )
                        .to_owned(),
                )
                .await?;
        } else {
             manager
                .alter_table(
                    Table::alter()
                        .table(Teams::Table)
                        .add_column_if_not_exists(
                            ColumnDef::new(Teams::IpWhitelist)
                                .json()
                                .null()
                                .comment("IP白名单列表，支持CIDR格式"),
                        )
                        .to_owned(),
                )
                .await?;
        }

        manager
            .alter_table(
                Table::alter()
                    .table(Teams::Table)
                    .add_column_if_not_exists(
                        ColumnDef::new(Teams::EnableGeoRestrictions)
                            .boolean()
                            .not_null()
                            .default(false)
                            .comment("是否启用地理限制功能"),
                    )
                    .to_owned(),
            )
            .await?;

        // 2. 创建地理限制日志表，用于记录被拒绝的请求
        manager
            .create_table(
                Table::create()
                    .table(GeoRestrictionLogs::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(GeoRestrictionLogs::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(GeoRestrictionLogs::TeamId).uuid().not_null())
                    .col(ColumnDef::new(GeoRestrictionLogs::IpAddress).string().not_null())
                    .col(ColumnDef::new(GeoRestrictionLogs::CountryCode).string().null())
                    .col(ColumnDef::new(GeoRestrictionLogs::RestrictionType).string().not_null())
                    .col(ColumnDef::new(GeoRestrictionLogs::Url).string().null())
                    .col(ColumnDef::new(GeoRestrictionLogs::Reason).string().not_null())
                    .col(
                        ColumnDef::new(GeoRestrictionLogs::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 3. 为地理限制日志表创建索引
        manager
            .create_index(
                Index::create()
                    .name("idx_geo_logs_team_id")
                    .table(GeoRestrictionLogs::Table)
                    .col(GeoRestrictionLogs::TeamId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_geo_logs_created_at")
                    .table(GeoRestrictionLogs::Table)
                    .col(GeoRestrictionLogs::CreatedAt)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_geo_logs_ip_address")
                    .table(GeoRestrictionLogs::Table)
                    .col(GeoRestrictionLogs::IpAddress)
                    .to_owned(),
            )
            .await?;

        // 4. 添加外键约束 (Skip for SQLite to avoid panic)
        if backend != DbBackend::Sqlite {
            manager
                .create_foreign_key(
                    ForeignKey::create()
                        .name("fk_geo_logs_team")
                        .from(GeoRestrictionLogs::Table, GeoRestrictionLogs::TeamId)
                        .to(Teams::Table, Teams::Id)
                        .on_delete(ForeignKeyAction::Cascade)
                        .on_update(ForeignKeyAction::Cascade)
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }

    /// 回滚地理限制功能的数据库迁移
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
        // 删除外键约束
        manager
            .drop_foreign_key(
                ForeignKey::drop()
                    .name("fk_geo_logs_team")
                    .table(GeoRestrictionLogs::Table)
                    .to_owned(),
            )
            .await?;

        // 删除索引
        manager
            .drop_index(Index::drop().name("idx_geo_logs_ip_address").to_owned())
            .await?;
        manager
            .drop_index(Index::drop().name("idx_geo_logs_created_at").to_owned())
            .await?;
        manager
            .drop_index(Index::drop().name("idx_geo_logs_team_id").to_owned())
            .await?;

        // 删除地理限制日志表
        manager
            .drop_table(Table::drop().table(GeoRestrictionLogs::Table).to_owned())
            .await?;

        // 删除 teams 表的地理限制相关字段
        manager
            .alter_table(
                Table::alter()
                    .table(Teams::Table)
                    .drop_column(Teams::AllowedCountries)
                    .drop_column(Teams::BlockedCountries)
                    .drop_column(Teams::IpWhitelist)
                    .drop_column(Teams::EnableGeoRestrictions)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

/// Teams 表字段定义
#[derive(DeriveIden)]
enum Teams {
    Table,
    Id,
    AllowedCountries,
    BlockedCountries,
    IpWhitelist,
    EnableGeoRestrictions,
}

/// 地理限制日志表字段定义
#[derive(DeriveIden)]
enum GeoRestrictionLogs {
    Table,
    Id,
    TeamId,
    IpAddress,
    CountryCode,
    RestrictionType,
    Url,
    Reason,
    CreatedAt,
}