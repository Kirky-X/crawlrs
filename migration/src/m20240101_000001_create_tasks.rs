use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Tasks::TaskType).string().not_null())
                    .col(ColumnDef::new(Tasks::Status).string().not_null())
                    .col(ColumnDef::new(Tasks::Priority).integer().not_null().default(0))
                    .col(ColumnDef::new(Tasks::TeamId).uuid().not_null())
                    .col(ColumnDef::new(Tasks::Url).string().not_null())
                    .col(ColumnDef::new(Tasks::Payload).json().not_null())
                    .col(ColumnDef::new(Tasks::RetryCount).integer().not_null().default(0))
                    .col(ColumnDef::new(Tasks::MaxRetries).integer().not_null().default(3))
                    .col(
                        ColumnDef::new(Tasks::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(ColumnDef::new(Tasks::StartedAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(Tasks::CompletedAt).timestamp_with_time_zone())
                    .col(ColumnDef::new(Tasks::LockToken).uuid())
                    .col(ColumnDef::new(Tasks::LockExpiresAt).timestamp_with_time_zone())
                    .to_owned(),
            )
            .await?;

        // Create indexes
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
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Tasks::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Tasks {
    Table,
    Id,
    TaskType,
    Status,
    Priority,
    TeamId,
    Url,
    Payload,
    RetryCount,
    MaxRetries,
    CreatedAt,
    StartedAt,
    CompletedAt,
    LockToken,
    LockExpiresAt,
}
