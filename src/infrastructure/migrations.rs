// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Database migration module
//!
//! This module provides database migration functionality using SeaORM.
//! The migrations are defined inline for simplicity.

pub use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Create teams table
        manager
            .create_table(
                Table::create()
                    .table(Teams::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Teams::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(ColumnDef::new(Teams::Name).string().not_null())
                    .col(ColumnDef::new(Teams::AllowedCountries).json())
                    .col(ColumnDef::new(Teams::BlockedCountries).json())
                    .col(ColumnDef::new(Teams::IpWhitelist).json())
                    .col(ColumnDef::new(Teams::DomainBlacklist).json())
                    .col(
                        ColumnDef::new(Teams::EnableGeoRestrictions)
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

        // Create api_keys table
        manager
            .create_table(
                Table::create()
                    .table(ApiKeys::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ApiKeys::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(
                        ColumnDef::new(ApiKeys::Key)
                            .string()
                            .not_null()
                            .primary_key(),
                    )
                    .col(ColumnDef::new(ApiKeys::KeyHash).string())
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
                    .col(
                        ColumnDef::new(ApiKeys::FeatureFlags)
                            .json()
                            .default(Expr::cust("'{}'::jsonb")),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_api_keys_team_id")
                            .from(ApiKeys::Table, ApiKeys::TeamId)
                            .to(Teams::Table, Teams::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Create index for api_keys.team_id
        manager
            .create_index(
                IndexCreateStatement::new()
                    .name("idx_api_key_team")
                    .table(ApiKeys::Table)
                    .col(ApiKeys::TeamId)
                    .to_owned(),
            )
            .await?;

        // Create index for api_keys.key_hash
        manager
            .create_index(
                IndexCreateStatement::new()
                    .name("idx_api_keys_key_hash")
                    .table(ApiKeys::Table)
                    .col(ApiKeys::KeyHash)
                    .to_owned(),
            )
            .await?;

        // Create tasks table
        manager
            .create_table(
                Table::create()
                    .table(Tasks::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Tasks::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(ColumnDef::new(Tasks::TaskType).string().not_null())
                    .col(ColumnDef::new(Tasks::Url).text().not_null())
                    .col(ColumnDef::new(Tasks::Status).string().not_null())
                    .col(ColumnDef::new(Tasks::ResultId).uuid())
                    .col(ColumnDef::new(Tasks::TeamId).uuid().not_null())
                    .col(ColumnDef::new(Tasks::ApiKeyId).uuid().not_null())
                    .col(ColumnDef::new(Tasks::Priority).integer().default(0))
                    .col(ColumnDef::new(Tasks::MaxRetries).integer().default(3))
                    .col(ColumnDef::new(Tasks::CurrentRetry).integer().default(0))
                    .col(ColumnDef::new(Tasks::Error).text())
                    .col(ColumnDef::new(Tasks::Metadata).json())
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
                    .col(ColumnDef::new(Tasks::StartedAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(Tasks::CompletedAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(Tasks::CreditsCost).big_integer().default(10))
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_tasks_team_id")
                            .from(Tasks::Table, Tasks::TeamId)
                            .to(Teams::Table, Teams::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_tasks_api_key_id")
                            .from(Tasks::Table, Tasks::ApiKeyId)
                            .to(ApiKeys::Table, ApiKeys::Key)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Create index for tasks.team_id
        manager
            .create_index(
                IndexCreateStatement::new()
                    .name("idx_tasks_team_id")
                    .table(Tasks::Table)
                    .col(Tasks::TeamId)
                    .to_owned(),
            )
            .await?;

        // Create index for tasks.status
        manager
            .create_index(
                IndexCreateStatement::new()
                    .name("idx_tasks_status")
                    .table(Tasks::Table)
                    .col(Tasks::Status)
                    .to_owned(),
            )
            .await?;

        // Create tasks_backlog table
        manager
            .create_table(
                Table::create()
                    .table(TasksBacklog::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(TasksBacklog::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(ColumnDef::new(TasksBacklog::TaskId).uuid().not_null())
                    .col(ColumnDef::new(TasksBacklog::Priority).integer().default(0))
                    .col(
                        ColumnDef::new(TasksBacklog::ScheduledAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(TasksBacklog::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // Create index for tasks_backlog.task_id
        manager
            .create_index(
                IndexCreateStatement::new()
                    .name("idx_tasks_backlog_task_id")
                    .table(TasksBacklog::Table)
                    .col(TasksBacklog::TaskId)
                    .to_owned(),
            )
            .await?;

        // Create scrape_results table
        manager
            .create_table(
                Table::create()
                    .table(ScrapeResults::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(ScrapeResults::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(ColumnDef::new(ScrapeResults::TaskId).uuid())
                    .col(ColumnDef::new(ScrapeResults::Url).text().not_null())
                    .col(ColumnDef::new(ScrapeResults::FinalUrl).text())
                    .col(ColumnDef::new(ScrapeResults::Title).text())
                    .col(ColumnDef::new(ScrapeResults::Content).text())
                    .col(ColumnDef::new(ScrapeResults::RawContent).binary())
                    .col(ColumnDef::new(ScrapeResults::Screenshot).text())
                    .col(ColumnDef::new(ScrapeResults::Metadata).json())
                    .col(ColumnDef::new(ScrapeResults::StatusCode).integer())
                    .col(ColumnDef::new(ScrapeResults::ResponseTimeMs).big_integer())
                    .col(ColumnDef::new(ScrapeResults::ContentType).string())
                    .col(ColumnDef::new(ScrapeResults::Headers).json())
                    .col(ColumnDef::new(ScrapeResults::Links).json())
                    .col(ColumnDef::new(ScrapeResults::Error).text())
                    .col(
                        ColumnDef::new(ScrapeResults::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await?;

        // Create credits table
        manager
            .create_table(
                Table::create()
                    .table(Credits::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Credits::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(ColumnDef::new(Credits::TeamId).uuid().not_null())
                    .col(
                        ColumnDef::new(Credits::Balance)
                            .big_integer()
                            .not_null()
                            .default(0),
                    )
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
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_credits_team_id")
                            .from(Credits::Table, Credits::TeamId)
                            .to(Teams::Table, Teams::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Create index for credits.team_id
        manager
            .create_index(
                IndexCreateStatement::new()
                    .name("idx_credits_team_id")
                    .table(Credits::Table)
                    .col(Credits::TeamId)
                    .to_owned(),
            )
            .await?;

        // Create crawls table
        manager
            .create_table(
                Table::create()
                    .table(Crawls::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Crawls::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(ColumnDef::new(Crawls::TeamId).uuid().not_null())
                    .col(ColumnDef::new(Crawls::ApiKeyId).uuid().not_null())
                    .col(ColumnDef::new(Crawls::Name).string())
                    .col(ColumnDef::new(Crawls::SeedUrls).json().not_null())
                    .col(ColumnDef::new(Crawls::Options).json())
                    .col(
                        ColumnDef::new(Crawls::Status)
                            .string()
                            .not_null()
                            .default("pending"),
                    )
                    .col(ColumnDef::new(Crawls::TotalTasks).integer().default(0))
                    .col(ColumnDef::new(Crawls::CompletedTasks).integer().default(0))
                    .col(ColumnDef::new(Crawls::FailedTasks).integer().default(0))
                    .col(ColumnDef::new(Crawls::PendingTasks).integer().default(0))
                    .col(ColumnDef::new(Crawls::Stats).json())
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
                    .col(ColumnDef::new(Crawls::StartedAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(Crawls::CompletedAt).timestamp_with_time_zone())
                    .col(
                        ColumnDef::new(Crawls::CreditsCost)
                            .big_integer()
                            .default(10),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_crawls_team_id")
                            .from(Crawls::Table, Crawls::TeamId)
                            .to(Teams::Table, Teams::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_crawls_api_key_id")
                            .from(Crawls::Table, Crawls::ApiKeyId)
                            .to(ApiKeys::Table, ApiKeys::Key)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Create index for crawls.team_id
        manager
            .create_index(
                IndexCreateStatement::new()
                    .name("idx_crawls_team_id")
                    .table(Crawls::Table)
                    .col(Crawls::TeamId)
                    .to_owned(),
            )
            .await?;

        // Create webhooks table
        manager
            .create_table(
                Table::create()
                    .table(Webhooks::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(Webhooks::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(ColumnDef::new(Webhooks::TeamId).uuid().not_null())
                    .col(ColumnDef::new(Webhooks::Url).string().not_null())
                    .col(ColumnDef::new(Webhooks::Secret).string())
                    .col(ColumnDef::new(Webhooks::Events).json().not_null())
                    .col(ColumnDef::new(Webhooks::IsActive).boolean().default(true))
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
                            .name("fk_webhooks_team_id")
                            .from(Webhooks::Table, Webhooks::TeamId)
                            .to(Teams::Table, Teams::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Create webhook_events table
        manager
            .create_table(
                Table::create()
                    .table(WebhookEvents::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(WebhookEvents::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(ColumnDef::new(WebhookEvents::WebhookId).uuid().not_null())
                    .col(ColumnDef::new(WebhookEvents::EventType).string().not_null())
                    .col(ColumnDef::new(WebhookEvents::Payload).json().not_null())
                    .col(
                        ColumnDef::new(WebhookEvents::Status)
                            .string()
                            .not_null()
                            .default("pending"),
                    )
                    .col(ColumnDef::new(WebhookEvents::Attempts).integer().default(0))
                    .col(ColumnDef::new(WebhookEvents::LastAttemptAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(WebhookEvents::NextRetryAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(WebhookEvents::Error).text())
                    .col(
                        ColumnDef::new(WebhookEvents::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_webhook_events_webhook_id")
                            .from(WebhookEvents::Table, WebhookEvents::WebhookId)
                            .to(Webhooks::Table, Webhooks::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Create index for webhook_events.webhook_id
        manager
            .create_index(
                IndexCreateStatement::new()
                    .name("idx_webhook_events_webhook_id")
                    .table(WebhookEvents::Table)
                    .col(WebhookEvents::WebhookId)
                    .to_owned(),
            )
            .await?;

        // Create geo_restrictions table
        manager
            .create_table(
                Table::create()
                    .table(GeoRestrictions::Table)
                    .if_not_exists()
                    .col(
                        ColumnDef::new(GeoRestrictions::Id)
                            .uuid()
                            .not_null()
                            .primary_key()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .col(ColumnDef::new(GeoRestrictions::TeamId).uuid().not_null())
                    .col(
                        ColumnDef::new(GeoRestrictions::CountryCode)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(GeoRestrictions::RestrictionType)
                            .string()
                            .not_null(),
                    )
                    .col(
                        ColumnDef::new(GeoRestrictions::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(
                        ColumnDef::new(GeoRestrictions::UpdatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .foreign_key(
                        ForeignKey::create()
                            .name("fk_geo_restrictions_team_id")
                            .from(GeoRestrictions::Table, GeoRestrictions::TeamId)
                            .to(Teams::Table, Teams::Id)
                            .on_delete(ForeignKeyAction::Cascade),
                    )
                    .to_owned(),
            )
            .await?;

        // Create index for geo_restrictions.team_id
        manager
            .create_index(
                IndexCreateStatement::new()
                    .name("idx_geo_restrictions_team_id")
                    .table(GeoRestrictions::Table)
                    .col(GeoRestrictions::TeamId)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, _manager: &SchemaManager) -> Result<(), DbErr> {
        // This is a simple migration - in production you would carefully handle down migrations
        Ok(())
    }
}

#[derive(Iden)]
pub enum Teams {
    Table,
    Id,
    Name,
    AllowedCountries,
    BlockedCountries,
    IpWhitelist,
    DomainBlacklist,
    EnableGeoRestrictions,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
pub enum ApiKeys {
    Table,
    Id,
    Key,
    KeyHash,
    TeamId,
    CreatedAt,
    UpdatedAt,
    FeatureFlags,
}

#[derive(Iden)]
pub enum Tasks {
    Table,
    Id,
    TaskType,
    Url,
    Status,
    ResultId,
    TeamId,
    ApiKeyId,
    Priority,
    MaxRetries,
    CurrentRetry,
    Error,
    Metadata,
    CreatedAt,
    UpdatedAt,
    StartedAt,
    CompletedAt,
    CreditsCost,
}

#[derive(Iden)]
pub enum TasksBacklog {
    Table,
    Id,
    TaskId,
    Priority,
    ScheduledAt,
    CreatedAt,
}

#[derive(Iden)]
pub enum ScrapeResults {
    Table,
    Id,
    TaskId,
    Url,
    FinalUrl,
    Title,
    Content,
    RawContent,
    Screenshot,
    Metadata,
    StatusCode,
    ResponseTimeMs,
    ContentType,
    Headers,
    Links,
    Error,
    CreatedAt,
}

#[derive(Iden)]
pub enum Credits {
    Table,
    Id,
    TeamId,
    Balance,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
pub enum Crawls {
    Table,
    Id,
    TeamId,
    ApiKeyId,
    Name,
    SeedUrls,
    Options,
    Status,
    TotalTasks,
    CompletedTasks,
    FailedTasks,
    PendingTasks,
    Stats,
    CreatedAt,
    UpdatedAt,
    StartedAt,
    CompletedAt,
    CreditsCost,
}

#[derive(Iden)]
pub enum Webhooks {
    Table,
    Id,
    TeamId,
    Url,
    Secret,
    Events,
    IsActive,
    CreatedAt,
    UpdatedAt,
}

#[derive(Iden)]
pub enum WebhookEvents {
    Table,
    Id,
    WebhookId,
    EventType,
    Payload,
    Status,
    Attempts,
    LastAttemptAt,
    NextRetryAt,
    Error,
    CreatedAt,
}

#[derive(Iden)]
pub enum GeoRestrictions {
    Table,
    Id,
    TeamId,
    CountryCode,
    RestrictionType,
    CreatedAt,
    UpdatedAt,
}
