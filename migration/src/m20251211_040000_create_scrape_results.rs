use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
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
                    .col(ColumnDef::new(ScrapeResults::StatusCode).integer().not_null())
                    .col(ColumnDef::new(ScrapeResults::Content).text().not_null())
                    .col(ColumnDef::new(ScrapeResults::ContentType).string().not_null())
                    .col(ColumnDef::new(ScrapeResults::ResponseTimeMs).big_integer().not_null())
                    .col(
                        ColumnDef::new(ScrapeResults::CreatedAt)
                            .timestamp_with_time_zone()
                            .not_null()
                            .default(Expr::current_timestamp()),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(ScrapeResults::Table).to_owned())
            .await
    }
}

#[derive(DeriveIden)]
enum ScrapeResults {
    Table,
    Id,
    TaskId,
    StatusCode,
    Content,
    ContentType,
    ResponseTimeMs,
    CreatedAt,
}
