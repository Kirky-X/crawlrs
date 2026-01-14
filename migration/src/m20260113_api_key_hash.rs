// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::sea_orm::EnumIter;
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .alter_table(
                Table::alter()
                    .table(ApiKeys::Table)
                    .add_column_if_not_exists(ColumnDef::new(Column::KeyHash).string().null())
                    .to_owned(),
            )
            .await?;

        manager
            .create_index(
                Index::create()
                    .name("idx_api_keys_key_hash")
                    .table(ApiKeys::Table)
                    .col(Column::KeyHash)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_index(Index::drop().name("idx_api_keys_key_hash").to_owned())
            .await?;

        manager
            .alter_table(
                Table::alter()
                    .table(ApiKeys::Table)
                    .drop_column(Column::KeyHash)
                    .to_owned(),
            )
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum ApiKeys {
    Table,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveIden)]
enum Column {
    KeyHash,
}
