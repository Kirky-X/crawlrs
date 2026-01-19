// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::sea_orm::EnumIter;
use sea_orm_migration::prelude::*;

/// 为审计日志表添加性能优化索引
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 为 api_key_id 添加索引 - 优化按 API 密钥查询
        manager
            .create_index(
                Index::create()
                    .name("idx_audit_log_api_key_id")
                    .table(AuditLog::Table)
                    .col(AuditLog::ApiKeyId)
                    .to_owned(),
            )
            .await?;

        // 为 team_id 添加索引 - 优化按团队查询
        manager
            .create_index(
                Index::create()
                    .name("idx_audit_log_team_id")
                    .table(AuditLog::Table)
                    .col(AuditLog::TeamId)
                    .to_owned(),
            )
            .await?;

        // 为 created_at 添加索引 - 优化时间排序查询
        manager
            .create_index(
                Index::create()
                    .name("idx_audit_log_created_at")
                    .table(AuditLog::Table)
                    .col(AuditLog::CreatedAt)
                    .to_owned(),
            )
            .await?;

        // 为 decision 添加索引 - 优化决策类型过滤
        manager
            .create_index(
                Index::create()
                    .name("idx_audit_log_decision")
                    .table(AuditLog::Table)
                    .col(AuditLog::Decision)
                    .to_owned(),
            )
            .await?;

        // 组合索引: api_key_id + created_at - 优化最常见的查询模式
        manager
            .create_index(
                Index::create()
                    .name("idx_audit_log_api_key_created")
                    .table(AuditLog::Table)
                    .col(AuditLog::ApiKeyId)
                    .col(AuditLog::CreatedAt)
                    .to_owned(),
            )
            .await?;

        // 组合索引: team_id + created_at - 优化团队日志查询
        manager
            .create_index(
                Index::create()
                    .name("idx_audit_log_team_created")
                    .table(AuditLog::Table)
                    .col(AuditLog::TeamId)
                    .col(AuditLog::CreatedAt)
                    .to_owned(),
            )
            .await?;

        // 组合索引: api_key_id + decision - 优化拒绝请求查询
        manager
            .create_index(
                Index::create()
                    .name("idx_audit_log_api_key_decision")
                    .table(AuditLog::Table)
                    .col(AuditLog::ApiKeyId)
                    .col(AuditLog::Decision)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(
                Index::drop()
                    .name("idx_audit_log_api_key_id")
                    .table(AuditLog::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_audit_log_team_id")
                    .table(AuditLog::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_audit_log_created_at")
                    .table(AuditLog::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_audit_log_decision")
                    .table(AuditLog::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_audit_log_api_key_created")
                    .table(AuditLog::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_audit_log_team_created")
                    .table(AuditLog::Table)
                    .to_owned(),
            )
            .await?;

        manager
            .drop_index(
                Index::drop()
                    .name("idx_audit_log_api_key_decision")
                    .table(AuditLog::Table)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveIden)]
enum AuditLog {
    Table,
    ApiKeyId,
    TeamId,
    Decision,
    CreatedAt,
}
