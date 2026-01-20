// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, Set, Statement,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::domain::{
    models::credits::{CreditsTransaction, CreditsTransactionType},
    repositories::credits_repository::{CreditsRepository, CreditsRepositoryError},
};

use crate::infrastructure::database::entities::{credits, credits_transactions};

pub struct CreditsRepositoryImpl {
    db: Arc<DatabaseConnection>,
}

impl CreditsRepositoryImpl {
    pub fn new(db: Arc<DatabaseConnection>) -> Self {
        Self { db }
    }
}

#[async_trait]
impl CreditsRepository for CreditsRepositoryImpl {
    async fn get_balance(&self, team_id: Uuid) -> Result<i64, CreditsRepositoryError> {
        let credits = credits::Entity::find()
            .filter(credits::Column::TeamId.eq(team_id))
            .one(self.db.as_ref())
            .await
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        match credits {
            Some(credits) => Ok(credits.balance),
            None => {
                // Initialize with 0 credits if not exists
                self.initialize_team_credits(team_id, 0).await
            }
        }
    }

    async fn deduct_credits(
        &self,
        team_id: Uuid,
        amount: i64,
        transaction_type: CreditsTransactionType,
        description: String,
        reference_id: Option<Uuid>,
    ) -> Result<(), CreditsRepositoryError> {
        // Use the stored procedure for atomic deduction with row-level locking
        let sql = r#"
            SELECT deduct_credits_safe($1, $2, $3, $4, $5)
        "#;

        let stmt = Statement::from_sql_and_values(
            sea_orm::DbBackend::Postgres,
            sql,
            vec![
                team_id.into(),
                amount.into(),
                transaction_type.to_string().into(),
                description.into(),
                reference_id.into(),
            ],
        );

        self.db
            .as_ref()
            .execute(stmt)
            .await
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    async fn add_credits(
        &self,
        team_id: Uuid,
        amount: i64,
        transaction_type: CreditsTransactionType,
        description: String,
        reference_id: Option<Uuid>,
    ) -> Result<i64, CreditsRepositoryError> {
        // Use the stored procedure for atomic addition
        let sql = r#"
            SELECT add_credits_safe($1, $2, $3, $4, $5)
        "#;

        let stmt = Statement::from_sql_and_values(
            sea_orm::DbBackend::Postgres,
            sql,
            vec![
                team_id.into(),
                amount.into(),
                transaction_type.to_string().into(),
                description.into(),
                reference_id.into(),
            ],
        );

        self.db
            .execute(stmt)
            .await
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        // Extract the new balance from the result
        // The stored procedure returns the new balance
        Ok(0) // Placeholder - the actual balance is handled by the stored procedure
    }

    async fn get_transaction_history(
        &self,
        team_id: Uuid,
        limit: Option<u32>,
    ) -> Result<Vec<CreditsTransaction>, CreditsRepositoryError> {
        let mut query = credits_transactions::Entity::find()
            .filter(credits_transactions::Column::TeamId.eq(team_id))
            .order_by_desc(credits_transactions::Column::CreatedAt);

        if let Some(limit) = limit {
            query = query.limit(limit as u64);
        }

        let transactions = query
            .all(self.db.as_ref())
            .await
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        Ok(transactions
            .into_iter()
            .map(|t| CreditsTransaction {
                id: t.id,
                team_id: t.team_id,
                amount: t.amount,
                transaction_type: match t.transaction_type.as_str() {
                    "search" => CreditsTransactionType::Search,
                    "scrape" => CreditsTransactionType::Scrape,
                    "extract" => CreditsTransactionType::Extract,
                    "crawl" => CreditsTransactionType::Crawl,
                    "manual_adjustment" => CreditsTransactionType::ManualAdjustment,
                    "subscription" => CreditsTransactionType::Subscription,
                    "refund" => CreditsTransactionType::Refund,
                    _ => CreditsTransactionType::ManualAdjustment,
                },
                description: t.description,
                reference_id: t.reference_id,
                created_at: t.created_at.into(),
            })
            .collect())
    }

    async fn initialize_team_credits(
        &self,
        team_id: Uuid,
        initial_balance: i64,
    ) -> Result<i64, CreditsRepositoryError> {
        // Check if credits already exist
        let existing = credits::Entity::find()
            .filter(credits::Column::TeamId.eq(team_id))
            .one(self.db.as_ref())
            .await
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        if let Some(credits) = existing {
            return Ok(credits.balance);
        }

        // Create new credits record
        let credits = credits::ActiveModel {
            id: Set(Uuid::new_v4()),
            team_id: Set(team_id),
            balance: Set(initial_balance),
            created_at: Set(Utc::now().fixed_offset()),
            updated_at: Set(Utc::now().fixed_offset()),
        };

        credits
            .insert(self.db.as_ref())
            .await
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        Ok(initial_balance)
    }
}
