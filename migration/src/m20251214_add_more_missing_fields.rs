use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Add 'id' column to 'api_keys' table
        manager
            .alter_table(
                Table::alter()
                    .table(ApiKeys::Table)
                    .add_column(
                        ColumnDef::new(ApiKeys::Id)
                            .uuid()
                            .not_null()
                            .default(Expr::cust("gen_random_uuid()")),
                    )
                    .to_owned(),
            )
            .await?;

        // Add 'webhook_id' column to 'webhook_events' table
        manager
            .alter_table(
                Table::alter()
                    .table(WebhookEvents::Table)
                    .add_column(ColumnDef::new(WebhookEvents::WebhookId).uuid().null())
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Drop 'webhook_id' column from 'webhook_events' table
        manager
            .alter_table(
                Table::alter()
                    .table(WebhookEvents::Table)
                    .drop_column(WebhookEvents::WebhookId)
                    .to_owned(),
            )
            .await?;

        // Drop 'id' column from 'api_keys' table
        manager
            .alter_table(
                Table::alter()
                    .table(ApiKeys::Table)
                    .drop_column(ApiKeys::Id)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum ApiKeys {
    Table,
    Id,
}

#[derive(DeriveIden)]
enum WebhookEvents {
    Table,
    WebhookId,
}
