// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::DbBackend;

/// 完整数据库模式迁移 - 合并了初始模式、积分系统和任务积压功能
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    /// 应用完整的数据库迁移
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
        // 1. Create teams table (No dependencies)
        manager
            .create_table(
                Table::create()
                    .table(Teams::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Teams::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Teams::Name).string().not_null())
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

        // 2. Create api_keys table (Depends on Teams)
        manager
            .create_table(
                Table::create()
                    .table(ApiKeys::Table)
                    .if_not_exists()
                    .col(
                        {
                            let mut col = ColumnDef::new(ApiKeys::Id);
                            col.uuid().not_null();
                            if manager.get_database_backend() == DbBackend::Postgres {
                                col.default(Expr::cust("gen_random_uuid()"));
                            }
                            col
                        }
                    )
                    .col(
                        ColumnDef::new(ApiKeys::Key)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
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

        // Index for api_keys
        manager
            .create_index(
                Index::create()
                    .name("idx_api_key_team")
                    .table(ApiKeys::Table)
                    .col(ApiKeys::TeamId)
                    .to_owned(),
            )
            .await?;

        // 3. Create tasks table (Depends on Teams)
        manager
            .create_table(
                Table::create()
                    .table(Tasks::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Tasks::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Tasks::CrawlId).uuid().null())
                    .col(ColumnDef::new(Tasks::TaskType).string().not_null())
                    .col(ColumnDef::new(Tasks::Status).string().not_null())
                    .col(
                        ColumnDef::new(Tasks::Priority)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(ColumnDef::new(Tasks::TeamId).uuid().not_null())
                    .col(ColumnDef::new(Tasks::Url).string().not_null())
                    .col(ColumnDef::new(Tasks::Payload).json().not_null())
                    .col(
                        ColumnDef::new(Tasks::RetryCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Tasks::AttemptCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Tasks::MaxRetries)
                            .integer()
                            .not_null()
                            .default(3),
                    )
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
                    .col(
                        ColumnDef::new(Tasks::ScheduledAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .col(ColumnDef::new(Tasks::StartedAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(Tasks::CompletedAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(Tasks::LockToken).uuid())
                    .col(ColumnDef::new(Tasks::LockExpiresAt).timestamp_with_time_zone())
                    .col(
                        ColumnDef::new(Tasks::ExpiresAt)
                            .timestamp_with_time_zone()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        // Indexes for tasks
        manager
            .create_index(
                Index::create()
                    .name("idx_status_priority")
                    .table(Tasks::Table)
                    .col(Tasks::Status)
                    .col(Tasks::Priority)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_team_id")
                    .table(Tasks::Table)
                    .col(Tasks::TeamId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_tasks_status_priority_created_at")
                    .table(Tasks::Table)
                    .col(Tasks::Status)
                    .col(Tasks::Priority)
                    .col(Tasks::CreatedAt)
                    .to_owned(),
            )
            .await?;

        // 4. Create crawls table (Depends on Teams)
        manager
            .create_table(
                Table::create()
                    .table(Crawls::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Crawls::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Crawls::TeamId).uuid().not_null())
                    .col(ColumnDef::new(Crawls::Name).string().null())
                    .col(ColumnDef::new(Crawls::Url).string().not_null().default(""))
                    .col(ColumnDef::new(Crawls::RootUrl).string().not_null())
                    .col(ColumnDef::new(Crawls::Status).string().not_null())
                    .col(ColumnDef::new(Crawls::Config).json().not_null())
                    .col(
                        ColumnDef::new(Crawls::TotalTasks)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Crawls::CompletedTasks)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Crawls::FailedTasks)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(Crawls::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Crawls::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(ColumnDef::new(Crawls::CompletedAt).timestamp_with_time_zone())
                    .to_owned(),
            )
            .await?;

        // Indexes for crawls
        manager
            .create_index(
                Index::create()
                    .name("idx_team_status")
                    .table(Crawls::Table)
                    .col(Crawls::TeamId)
                    .col(Crawls::Status)
                    .to_owned(),
            )
            .await?;

        // 5. Create webhooks table (Depends on Teams)
        manager
            .create_table(
                Table::create()
                    .table(Webhooks::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Webhooks::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Webhooks::TeamId).uuid().not_null())
                    .col(ColumnDef::new(Webhooks::Url).string().not_null())
                    .col(
                        ColumnDef::new(Webhooks::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // Add index on team_id for webhooks
        manager
            .create_index(
                Index::create()
                    .name("idx_webhooks_team_id")
                    .table(Webhooks::Table)
                    .col(Webhooks::TeamId)
                    .to_owned(),
            )
            .await?;

        // 6. Create webhook_events table (Depends on Teams)
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
                    .col(ColumnDef::new(WebhookEvents::WebhookId).uuid().null())
                    .col(ColumnDef::new(WebhookEvents::TeamId).uuid().not_null())
                    .col(ColumnDef::new(WebhookEvents::EventType).string().not_null())
                    .col(ColumnDef::new(WebhookEvents::Payload).json().not_null())
                    .col(
                        ColumnDef::new(WebhookEvents::WebhookUrl)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(WebhookEvents::Status)
                            .string()
                            .not_null()
                            .default("pending"),
                    )
                    .col(
                        ColumnDef::new(WebhookEvents::ResponseStatus)
                            .small_integer()
                            .null(),
                    )
                    .col(
                        ColumnDef::new(WebhookEvents::RetryCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(WebhookEvents::AttemptCount)
                            .integer()
                            .not_null()
                            .default(0),
                    )
                    .col(
                        ColumnDef::new(WebhookEvents::MaxRetries)
                            .integer()
                            .not_null()
                            .default(5),
                    )
                    .col(ColumnDef::new(WebhookEvents::NextRetryAt).timestamp_with_time_zone())
                    .col(
                        ColumnDef::new(WebhookEvents::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(ColumnDef::new(WebhookEvents::DeliveredAt).timestamp_with_time_zone())
                    .to_owned(),
            )
            .await?;

        // Indexes for webhook_events
        manager
            .create_index(
                Index::create()
                    .name("idx_status_retry")
                    .table(WebhookEvents::Table)
                    .col(WebhookEvents::Status)
                    .col(WebhookEvents::NextRetryAt)
                    .to_owned(),
            )
            .await?;

        // 7. Create scrape_results table (Depends on Tasks)
        manager
            .create_table(
                Table::create()
                    .table(ScrapeResults::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ScrapeResults::Id)
                            .uuid()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ScrapeResults::TaskId).uuid().not_null())
                    .col(
                        ColumnDef::new(ScrapeResults::StatusCode)
                            .integer()
                            .not_null(),
                    )
                    .col(ColumnDef::new(ScrapeResults::Content).text().not_null())
                    .col(ColumnDef::new(ScrapeResults::ContentType)
                            .string()
                            .not_null(),
                    )
                    .col(ColumnDef::new(ScrapeResults::Headers).json().null())
                    .col(ColumnDef::new(ScrapeResults::MetaData).json().null())
                    .col(ColumnDef::new(ScrapeResults::Screenshot).text().null())
                    .col(
                        ColumnDef::new(ScrapeResults::ResponseTimeMs)
                            .big_integer()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(ScrapeResults::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // 8. Create credits table (Depends on Teams) - 新增积分系统
        manager
            .create_table(
                Table::create()
                    .table(Credits::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(Credits::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(Credits::TeamId).uuid().not_null())
                    .col(ColumnDef::new(Credits::Balance).big_integer().not_null().default(0))
                    .col(
                        ColumnDef::new(Credits::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(Credits::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // Index for credits
        manager
            .create_index(
                Index::create()
                    .name("idx_credits_team_id")
                    .table(Credits::Table)
                    .col(Credits::TeamId)
                    .unique()
                    .to_owned(),
            )
            .await?;

        // 9. Create credits_transactions table (Depends on Teams) - 新增积分交易
        manager
            .create_table(
                Table::create()
                    .table(CreditsTransactions::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(CreditsTransactions::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(CreditsTransactions::TeamId).uuid().not_null())
                    .col(ColumnDef::new(CreditsTransactions::Amount).big_integer().not_null())
                    .col(ColumnDef::new(CreditsTransactions::TransactionType).string().not_null())
                    .col(ColumnDef::new(CreditsTransactions::Description).string().not_null())
                    .col(ColumnDef::new(CreditsTransactions::ReferenceId).uuid().null())
                    .col(
                        ColumnDef::new(CreditsTransactions::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // Indexes for credits_transactions
        manager
            .create_index(
                Index::create()
                    .name("idx_credits_transactions_team_id")
                    .table(CreditsTransactions::Table)
                    .col(CreditsTransactions::TeamId)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_credits_transactions_created_at")
                    .table(CreditsTransactions::Table)
                    .col(CreditsTransactions::CreatedAt)
                    .to_owned(),
            )
            .await?;

        // 10. Create tasks_backlog table (Depends on Teams) - 新增任务积压
        manager
            .create_table(
                Table::create()
                    .table(TasksBacklog::Table)
                    .if_not_exists()
                    .col(ColumnDef::new(TasksBacklog::Id).uuid().not_null().primary_key())
                    .col(ColumnDef::new(TasksBacklog::TaskId).uuid().not_null())
                    .col(ColumnDef::new(TasksBacklog::TeamId).uuid().not_null())
                    .col(ColumnDef::new(TasksBacklog::TaskType).string().not_null())
                    .col(ColumnDef::new(TasksBacklog::Priority).integer().not_null().default(0))
                    .col(ColumnDef::new(TasksBacklog::Payload).json().not_null())
                    .col(ColumnDef::new(TasksBacklog::MaxRetries).integer().not_null().default(3))
                    .col(ColumnDef::new(TasksBacklog::RetryCount).integer().not_null().default(0))
                    .col(ColumnDef::new(TasksBacklog::Status).string().not_null().default("pending"))
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
                    .col(ColumnDef::new(TasksBacklog::ScheduledAt).timestamp_with_time_zone().null())
                    .col(ColumnDef::new(TasksBacklog::ExpiresAt).timestamp_with_time_zone().null())
                    .col(ColumnDef::new(TasksBacklog::ProcessedAt).timestamp_with_time_zone().null())
                    .to_owned(),
            )
            .await?;

        // Indexes for tasks_backlog
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
                    .name("idx_tasks_backlog_priority_created_at")
                    .table(TasksBacklog::Table)
                    .col(TasksBacklog::Priority)
                    .col(TasksBacklog::CreatedAt)
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_tasks_backlog_expires_at")
                    .table(TasksBacklog::Table)
                    .col(TasksBacklog::ExpiresAt)
                    .to_owned(),
            )
            .await?;

        // Add foreign key constraint for tasks_backlog
        manager
            .create_foreign_key(
                ForeignKey::create()
                    .name("fk_tasks_backlog_team")
                    .from(TasksBacklog::Table, TasksBacklog::TeamId)
                    .to(Teams::Table, Teams::Id)
                    .on_delete(ForeignKeyAction::Cascade)
                    .on_update(ForeignKeyAction::Cascade)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop tables in reverse order of creation/dependency
        manager
            .drop_table(Table::drop().table(TasksBacklog::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(CreditsTransactions::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(Credits::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(ScrapeResults::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(WebhookEvents::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(Webhooks::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(Crawls::Table).to_owned())
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

// 所有表定义枚举
#[derive(DeriveIden)]
enum Teams {
    Table,
    Id,
    Name,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum ApiKeys {
    Table,
    Id,
    Key,
    TeamId,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum Tasks {
    Table,
    Id,
    CrawlId,
    TaskType,
    Status,
    Priority,
    TeamId,
    Url,
    Payload,
    RetryCount,
    AttemptCount,
    MaxRetries,
    CreatedAt,
    UpdatedAt,
    ScheduledAt,
    StartedAt,
    CompletedAt,
    LockToken,
    LockExpiresAt,
    ExpiresAt,
}

#[derive(DeriveIden)]
enum Crawls {
    Table,
    Id,
    TeamId,
    Name,
    Url,
    RootUrl,
    Status,
    Config,
    TotalTasks,
    CompletedTasks,
    FailedTasks,
    CreatedAt,
    UpdatedAt,
    CompletedAt,
}

#[derive(DeriveIden)]
enum Webhooks {
    Table,
    Id,
    TeamId,
    Url,
    CreatedAt,
}

#[derive(DeriveIden)]
enum WebhookEvents {
    Table,
    Id,
    WebhookId,
    TeamId,
    EventType,
    Payload,
    WebhookUrl,
    Status,
    ResponseStatus,
    RetryCount,
    AttemptCount,
    MaxRetries,
    NextRetryAt,
    CreatedAt,
    DeliveredAt,
}

#[derive(DeriveIden)]
enum ScrapeResults {
    Table,
    Id,
    TaskId,
    StatusCode,
    Content,
    ContentType,
    Headers,
    MetaData,
    Screenshot,
    ResponseTimeMs,
    CreatedAt,
}

#[derive(DeriveIden)]
enum Credits {
    Table,
    Id,
    TeamId,
    Balance,
    CreatedAt,
    UpdatedAt,
}

#[derive(DeriveIden)]
enum CreditsTransactions {
    Table,
    Id,
    TeamId,
    Amount,
    TransactionType,
    Description,
    ReferenceId,
    CreatedAt,
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