// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use async_trait::async_trait;
use thiserror::Error;
use uuid::Uuid;

use crate::domain::models::credits::{CreditsTransaction, CreditsTransactionType};

#[derive(Error, Debug)]
pub enum CreditsRepositoryError {
    #[error("Database error: {0}")]
    DatabaseError(String),
    #[error("Insufficient credits: available {available}, required {required}")]
    InsufficientCredits { available: i64, required: i64 },
    #[error("Credits not found for team: {0}")]
    CreditsNotFound(Uuid),
}

#[async_trait]
pub trait CreditsRepository: Send + Sync {
    /// Get credits balance for a team
    async fn get_balance(&self, team_id: Uuid) -> Result<i64, CreditsRepositoryError>;

    /// Deduct credits from a team's balance
    async fn deduct_credits(
        &self,
        team_id: Uuid,
        amount: i64,
        transaction_type: CreditsTransactionType,
        description: String,
        reference_id: Option<Uuid>,
    ) -> Result<(), CreditsRepositoryError>;

    /// Add credits to a team's balance
    async fn add_credits(
        &self,
        team_id: Uuid,
        amount: i64,
        transaction_type: CreditsTransactionType,
        description: String,
        reference_id: Option<Uuid>,
    ) -> Result<i64, CreditsRepositoryError>;

    /// Get transaction history for a team
    async fn get_transaction_history(
        &self,
        team_id: Uuid,
        limit: Option<u32>,
    ) -> Result<Vec<CreditsTransaction>, CreditsRepositoryError>;

    /// Initialize credits for a new team (if not exists)
    async fn initialize_team_credits(
        &self,
        team_id: Uuid,
        initial_balance: i64,
    ) -> Result<i64, CreditsRepositoryError>;
}
