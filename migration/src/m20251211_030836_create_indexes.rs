use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        // Index for tasks: status and priority for scheduler performance
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
            
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(Index::drop().name("idx_tasks_status_priority_created_at").to_owned())
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum Tasks {
    Table,
    Status,
    Priority,
    CreatedAt,
}

