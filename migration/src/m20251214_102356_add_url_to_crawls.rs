use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Crawls::Table)
                    .add_column(
                        ColumnDef::new(Crawls::Url)
                            .string()
                            .not_null()
                            .default(""),
                    )
                    .to_owned(),
            )
            .await
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(Crawls::Table)
                    .drop_column(Crawls::Url)
                    .to_owned(),
            )
            .await
    }
}

#[derive(DeriveIden)]
enum Crawls {
    Table,
    Url,
}
