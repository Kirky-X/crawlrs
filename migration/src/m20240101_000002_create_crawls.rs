use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
                            .primary_key(),
                    )
                    .col(ColumnDef::new(Crawls::TeamId).uuid().not_null())
                    .col(ColumnDef::new(Crawls::RootUrl).string().not_null())
                    .col(ColumnDef::new(Crawls::Status).string().not_null())
                    .col(ColumnDef::new(Crawls::Config).json().not_null())
                    .col(ColumnDef::new(Crawls::TotalTasks).integer().not_null().default(0))
                    .col(ColumnDef::new(Crawls::CompletedTasks).integer().not_null().default(0))
                    .col(ColumnDef::new(Crawls::FailedTasks).integer().not_null().default(0))
                    .col(
                        ColumnDef::new(Crawls::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .col(ColumnDef::new(Crawls::CompletedAt).timestamp_with_time_zone())
                    .to_owned(),
            )
            .await?;

        // Create indexes
        manager
            .create_index(
                Index::create()
                    .name("idx_team_status")
                    .table(Crawls::Table)
                    .col(Crawls::TeamId)
                    .col(Crawls::Status)
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Crawls::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum Crawls {
    Table,
    Id,
    TeamId,
    RootUrl,
    Status,
    Config,
    TotalTasks,
    CompletedTasks,
    FailedTasks,
    CreatedAt,
    CompletedAt,
}
