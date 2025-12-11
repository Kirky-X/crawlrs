use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
                            .primary_key(),
                    )
                    .col(ColumnDef::new(WebhookEvents::TeamId).uuid().not_null())
                    .col(ColumnDef::new(WebhookEvents::EventType).string().not_null())
                    .col(ColumnDef::new(WebhookEvents::Payload).json().not_null())
                    .col(ColumnDef::new(WebhookEvents::WebhookUrl).string().not_null())
                    .col(
                        ColumnDef::new(WebhookEvents::Status)
                            .string()
                            .not_null()
                            .default("pending"),
                    )
                    .col(
                        ColumnDef::new(WebhookEvents::RetryCount)
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

        // Create indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_status_retry")
                    .table(WebhookEvents::Table)
                    .col(WebhookEvents::Status)
                    .col(WebhookEvents::NextRetryAt)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(WebhookEvents::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum WebhookEvents {
    Table,
    Id,
    TeamId,
    EventType,
    Payload,
    WebhookUrl,
    Status,
    RetryCount,
    MaxRetries,
    NextRetryAt,
    CreatedAt,
    DeliveredAt,
}
