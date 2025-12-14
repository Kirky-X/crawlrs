use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ScrapeResults::Table)
                    .add_column(ColumnDef::new(ScrapeResults::Headers).json().null())
                    .add_column(ColumnDef::new(ScrapeResults::MetaData).json().null())
                    .add_column(ColumnDef::new(ScrapeResults::Screenshot).text().null())
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ScrapeResults::Table)
                    .drop_column(ScrapeResults::Headers)
                    .drop_column(ScrapeResults::MetaData)
                    .drop_column(ScrapeResults::Screenshot)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum ScrapeResults {
    Table,
    Headers,
    MetaData,
    Screenshot,
}
