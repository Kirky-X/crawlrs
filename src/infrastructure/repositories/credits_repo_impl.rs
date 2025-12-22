// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
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
        let current_balance = self.get_balance(team_id).await?;

        if current_balance < amount {
            return Err(CreditsRepositoryError::InsufficientCredits {
                available: current_balance,
                required: amount,
            });
        }

        let new_balance = current_balance - amount;

        // Update credits balance
        let credits = credits::Entity::find()
            .filter(credits::Column::TeamId.eq(team_id))
            .one(self.db.as_ref())
            .await
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?
            .ok_or(CreditsRepositoryError::CreditsNotFound(team_id))?;

        let mut credits_active: credits::ActiveModel = credits.into();
        credits_active.balance = Set(new_balance);
        credits_active.updated_at = Set(Utc::now().fixed_offset());
        credits_active
            .update(self.db.as_ref())
            .await
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        // Create transaction record
        let transaction = credits_transactions::ActiveModel {
            id: Set(Uuid::new_v4()),
            team_id: Set(team_id),
            amount: Set(-amount), // Negative for deduction
            transaction_type: Set(transaction_type.to_string()),
            description: Set(description),
            reference_id: Set(reference_id),
            created_at: Set(Utc::now().fixed_offset()),
        };

        transaction
            .insert(self.db.as_ref())
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
        let current_balance = self.get_balance(team_id).await?;
        let new_balance = current_balance + amount;

        // Update credits balance
        let credits = credits::Entity::find()
            .filter(credits::Column::TeamId.eq(team_id))
            .one(self.db.as_ref())
            .await
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?
            .ok_or(CreditsRepositoryError::CreditsNotFound(team_id))?;

        let mut credits_active: credits::ActiveModel = credits.into();
        credits_active.balance = Set(new_balance);
        credits_active.updated_at = Set(Utc::now().fixed_offset());
        credits_active
            .update(self.db.as_ref())
            .await
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        // Create transaction record
        let transaction = credits_transactions::ActiveModel {
            id: Set(Uuid::new_v4()),
            team_id: Set(team_id),
            amount: Set(amount), // Positive for addition
            transaction_type: Set(transaction_type.to_string()),
            description: Set(description),
            reference_id: Set(reference_id),
            created_at: Set(Utc::now().fixed_offset()),
        };

        transaction
            .insert(self.db.as_ref())
            .await
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        Ok(new_balance)
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
