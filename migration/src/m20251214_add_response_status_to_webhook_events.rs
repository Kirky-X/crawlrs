use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Add 'response_status' column to 'webhook_events' table
        manager
            .alter_table(
                Table::alter()
                    .table(WebhookEvents::Table)
                    .add_column(
                        ColumnDef::new(WebhookEvents::ResponseStatus)
                            .small_integer()
                            .null(),
                    )
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop 'response_status' column from 'webhook_events' table
        manager
            .alter_table(
                Table::alter()
                    .table(WebhookEvents::Table)
                    .drop_column(WebhookEvents::ResponseStatus)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum WebhookEvents {
    Table,
    ResponseStatus,
}
