// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::DbBackend;

/// 统一数据库迁移 - 包含所有表结构和索引
///
/// 此迁移文件合并了之前的所有独立迁移，提供完整的数据库模式。
/// 包含：teams, api_keys, tasks, task_results, teams_geo_restrictions,
///      team_ip_whitelist, webhooks, webhook_events, tasks_backlog, audit_log
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let backend = manager.get_database_backend();

        // ========================================
        // 1. 创建 teams 表
        // ========================================
        manager
            .create_table(
                Table::create()
                    .table(Teams::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Teams::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Teams::Name).string().not_null())
                    .col(ColumnDef::new(Teams::AllowedCountries).json().null())
                    .col(ColumnDef::new(Teams::IpWhitelist).json().null())
                    .col(ColumnDef::new(Teams::GeoEnabled).boolean().default(false))
                    .col(
                        ColumnDef::new(Teams::IpWhitelistEnabled)
                            .boolean()
                            .default(false),
                    )
                    .col(
                        ColumnDef::new(Teams::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Teams::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // ========================================
        // 2. 创建 api_keys 表
        // ========================================
        manager
            .create_table(
                Table::create()
                    .table(ApiKeys::Table)
                    .if_not_exists()
                    .col({
                        let mut col = ColumnDef::new(ApiKeys::Id);
                        col.uuid().not_null();
                        if backend == DbBackend::Postgres {
                            col.default(Expr::cust("gen_random_uuid()"));
                        }
                        col
                    })
                    .col(
                        ColumnDef::new(ApiKeys::Key)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ApiKeys::KeyHash).string().null())
                    .col(ColumnDef::new(ApiKeys::TeamId).uuid().not_null())
                    .col(
                        ColumnDef::new(ApiKeys::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(ApiKeys::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // api_keys 表索引
        manager
            .create_index(
                Index::create()
                    .name("idx_api_key_team")
                    .table(ApiKeys::Table)
                    .col(ApiKeys::TeamId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_api_keys_key_hash")
                    .table(ApiKeys::Table)
                    .col(ApiKeys::KeyHash)
                    .to_owned(),
            )
            .await?;

        // ========================================
        // 3. 创建 tasks 表
        // ========================================
        manager
            .create_table(
                Table::create()
                    .table(Tasks::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Tasks::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Tasks::TaskType).string().not_null())
                    .col(ColumnDef::new(Tasks::Url).string().not_null())
                    .col(ColumnDef::new(Tasks::Status).string().not_null())
                    .col(ColumnDef::new(Tasks::ResultId).uuid().null())
                    .col(ColumnDef::new(Tasks::TeamId).uuid().not_null())
                    .col(ColumnDef::new(Tasks::ApiKeyId).uuid().not_null())
                    .col(ColumnDef::new(Tasks::Priority).integer().default(0))
                    .col(ColumnDef::new(Tasks::Metadata).json().null())
                    .col(ColumnDef::new(Tasks::ErrorMessage).string().null())
                    .col(
                        ColumnDef::new(Tasks::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Tasks::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_tasks_team")
                            .from(Tasks::Table, Tasks::TeamId)
                            .to(Teams::Table, Teams::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_tasks_api_key")
                            .from(Tasks::Table, Tasks::ApiKeyId)
                            .to(ApiKeys::Table, ApiKeys::Key)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // tasks 表索引
        manager
            .create_index(
                Index::create()
                    .name("idx_tasks_team")
                    .table(Tasks::Table)
                    .col(Tasks::TeamId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_tasks_status")
                    .table(Tasks::Table)
                    .col(Tasks::Status)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_tasks_created_at")
                    .table(Tasks::Table)
                    .col(Tasks::CreatedAt)
                    .to_owned(),
            )
            .await?;

        // ========================================
        // 4. 创建 task_results 表
        // ========================================
        manager
            .create_table(
                Table::create()
                    .table(TaskResults::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TaskResults::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(TaskResults::TaskId).uuid().not_null())
                    .col(ColumnDef::new(TaskResults::Engine).string().not_null())
                    .col(ColumnDef::new(TaskResults::Status).string().not_null())
                    .col(ColumnDef::new(TaskResults::Data).json().null())
                    .col(ColumnDef::new(TaskResults::Error).string().null())
                    .col(
                        ColumnDef::new(TaskResults::ExecutionTimeMs)
                            .integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(TaskResults::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_task_results_task")
                            .from(TaskResults::Table, TaskResults::TaskId)
                            .to(Tasks::Table, Tasks::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // ========================================
        // 5. 创建 audit_log 表
        // ========================================
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

        // audit_log 表索引
        manager
            .create_index(
                Index::create()
                    .name("idx_audit_log_api_key_id")
                    .table(AuditLog::Table)
                    .col(AuditLog::ApiKeyId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_audit_log_team_id")
                    .table(AuditLog::Table)
                    .col(AuditLog::TeamId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_audit_log_created_at")
                    .table(AuditLog::Table)
                    .col(AuditLog::CreatedAt)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_audit_log_decision")
                    .table(AuditLog::Table)
                    .col(AuditLog::Decision)
                    .to_owned(),
            )
            .await?;

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

        // ========================================
        // 6. 创建 webhooks 表
        // ========================================
        manager
            .create_table(
                Table::create()
                    .table(Webhooks::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Webhooks::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Webhooks::TeamId).uuid().not_null())
                    .col(ColumnDef::new(Webhooks::Url).string().not_null())
                    .col(ColumnDef::new(Webhooks::Secret).string().not_null())
                    .col(ColumnDef::new(Webhooks::Events).json().not_null())
                    .col(ColumnDef::new(Webhooks::Enabled).boolean().default(true))
                    .col(
                        ColumnDef::new(Webhooks::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Webhooks::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_webhooks_team")
                            .from(Webhooks::Table, Webhooks::TeamId)
                            .to(Teams::Table, Teams::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // ========================================
        // 7. 创建 webhook_events 表
        // ========================================
        manager
            .create_table(
                Table::create()
                    .table(WebhookEvents::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(WebhookEvents::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(WebhookEvents::TeamId).uuid().not_null())
                    .col(ColumnDef::new(WebhookEvents::WebhookId).uuid().not_null())
                    .col(ColumnDef::new(WebhookEvents::EventType).string().not_null())
                    .col(ColumnDef::new(WebhookEvents::Status).string().not_null())
                    .col(ColumnDef::new(WebhookEvents::Payload).json().not_null())
                    .col(
                        ColumnDef::new(WebhookEvents::WebhookUrl)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(WebhookEvents::ResponseStatus)
                            .integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(WebhookEvents::AttemptCount)
                            .integer()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(WebhookEvents::MaxRetries)
                            .integer()
                            .default(3),
                    )
                    .col(
                        ColumnDef::new(WebhookEvents::NextRetryAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(WebhookEvents::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(WebhookEvents::DeliveredAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_webhook_events_team")
                            .from(WebhookEvents::Table, WebhookEvents::TeamId)
                            .to(Teams::Table, Teams::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // webhook_events 表索引
        manager
            .create_index(
                Index::create()
                    .name("idx_webhook_events_team_status")
                    .table(WebhookEvents::Table)
                    .col(WebhookEvents::TeamId)
                    .col(WebhookEvents::Status)
                    .to_owned(),
            )
            .await?;

        // ========================================
        // 8. 创建 tasks_backlog 表
        // ========================================
        manager
            .create_table(
                Table::create()
                    .table(TasksBacklog::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TasksBacklog::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(TasksBacklog::TaskId).uuid().not_null())
                    .col(ColumnDef::new(TasksBacklog::TeamId).uuid().not_null())
                    .col(ColumnDef::new(TasksBacklog::TaskType).string().not_null())
                    .col(ColumnDef::new(TasksBacklog::Priority).integer().default(0))
                    .col(ColumnDef::new(TasksBacklog::Payload).json().not_null())
                    .col(
                        ColumnDef::new(TasksBacklog::MaxRetries)
                            .integer()
                            .default(3),
                    )
                    .col(
                        ColumnDef::new(TasksBacklog::RetryCount)
                            .integer()
                            .default(0),
                    )
                    .col(ColumnDef::new(TasksBacklog::Status).string().not_null())
                    .col(
                        ColumnDef::new(TasksBacklog::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(TasksBacklog::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(TasksBacklog::ScheduledAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(TasksBacklog::ExpiresAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(TasksBacklog::ProcessedAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_tasks_backlog_team")
                            .from(TasksBacklog::Table, TasksBacklog::TeamId)
                            .to(Teams::Table, Teams::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // tasks_backlog 表索引
        manager
            .create_index(
                Index::create()
                    .name("idx_tasks_backlog_team_status")
                    .table(TasksBacklog::Table)
                    .col(TasksBacklog::TeamId)
                    .col(TasksBacklog::Status)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_tasks_backlog_priority_created")
                    .table(TasksBacklog::Table)
                    .col(TasksBacklog::Priority)
                    .col(TasksBacklog::CreatedAt)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // 按依赖关系反向删除
        manager
            .drop_table(Table::drop().table(TasksBacklog::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(WebhookEvents::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Webhooks::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(AuditLog::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(TaskResults::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Tasks::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(ApiKeys::Table).to_owned())
            .await?;
        manager
            .drop_table(Table::drop().table(Teams::Table).to_owned())
            .await?;

        Ok(())
    }
}

// ========================================
// Table Enums
// ========================================

#[derive(DeriveIden)]
enum Teams {
    Table,
    Id,
    Name,
    AllowedCountries,
    IpWhitelist,
    GeoEnabled,
    IpWhitelistEnabled,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum ApiKeys {
    Table,
    Id,
    Key,
    KeyHash,
    TeamId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Tasks {
    Table,
    Id,
    TaskType,
    Url,
    Status,
    ResultId,
    TeamId,
    ApiKeyId,
    Priority,
    Metadata,
    ErrorMessage,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum TaskResults {
    Table,
    Id,
    TaskId,
    Engine,
    Status,
    Data,
    Error,
    ExecutionTimeMs,
    CreatedAt,
}

#[derive(DeriveIden)]
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

#[derive(DeriveIden)]
enum Webhooks {
    Table,
    Id,
    TeamId,
    Url,
    Secret,
    Events,
    Enabled,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum WebhookEvents {
    Table,
    Id,
    TeamId,
    WebhookId,
    EventType,
    Status,
    Payload,
    WebhookUrl,
    ResponseStatus,
    AttemptCount,
    MaxRetries,
    NextRetryAt,
    CreatedAt,
    DeliveredAt,
}

#[derive(DeriveIden)]
enum TasksBacklog {
    Table,
    Id,
    TaskId,
    TeamId,
    TaskType,
    Priority,
    Payload,
    MaxRetries,
    RetryCount,
    Status,
    CreatedAt,
    UpdatedAt,
    ScheduledAt,
    ExpiresAt,
    ProcessedAt,
}
