// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::sea_orm::EnumIter;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .create_table(
                Table::create()
                    .table(AuditLog::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(AuditLog::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(AuditLog::ApiKeyId).uuid().null())
                    .col(ColumnDef::new(AuditLog::TeamId).uuid().null())
                    .col(
                        ColumnDef::new(AuditLog::RequestedAction)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(AuditLog::Decision).string().not_null())
                    .col(ColumnDef::new(AuditLog::DenialReason).string().null())
                    .col(ColumnDef::new(AuditLog::ScopeUsed).json().null())
                    .col(ColumnDef::new(AuditLog::IpAddress).string().null())
                    .col(ColumnDef::new(AuditLog::TraceId).uuid().null())
                    .col(ColumnDef::new(AuditLog::UserAgent).string().null())
                    .col(ColumnDef::new(AuditLog::RequestPath).string().null())
                    .col(ColumnDef::new(AuditLog::RequestMethod).string().null())
                    .col(ColumnDef::new(AuditLog::Metadata).json().not_null())
                    .col(
                        ColumnDef::new(AuditLog::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(AuditLog::Table).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveIden)]
enum AuditLog {
    Table,
    Id,
    ApiKeyId,
    TeamId,
    RequestedAction,
    Decision,
    DenialReason,
    ScopeUsed,
    IpAddress,
    TraceId,
    UserAgent,
    RequestPath,
    RequestMethod,
    Metadata,
    CreatedAt,
}
