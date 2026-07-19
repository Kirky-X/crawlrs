// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Credits repository implementation using Sea-ORM with Mapper

use async_trait::async_trait;
use chrono::Utc;
use dbnexus::DbPool;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder,
    QuerySelect, Set,
};
use std::sync::Arc;
use uuid::Uuid;

use crate::common::time_utils;
use crate::domain::models::{CreditsTransaction, CreditsTransactionType};
use crate::domain::repositories::credits_repository::{CreditsRepository, CreditsRepositoryError};
use crate::infrastructure::database::entities::{credits, credits_transactions};
use crate::infrastructure::persistence::mappers::CreditsTransactionMapper;

pub struct CreditsRepositoryImpl {
    pool: Arc<DbPool>,
}

impl CreditsRepositoryImpl {
    pub fn new(pool: Arc<DbPool>) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl CreditsRepository for CreditsRepositoryImpl {
    async fn get_balance(&self, team_id: Uuid) -> Result<i64, CreditsRepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session
            .connection()
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        let credits = credits::Entity::find()
            .filter(credits::Column::TeamId.eq(team_id))
            .one(conn)
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
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session
            .connection()
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        // Use the stored procedure for atomic deduction with row-level locking
        // Note: Using execute_unprepared for stored procedure call
        let sql = format!(
            "SELECT deduct_credits_safe('{}', {}, '{}', '{}', {})",
            team_id,
            amount,
            transaction_type,
            description.replace("'", "''"),
            reference_id
                .map(|id| format!("'{}'", id))
                .unwrap_or("NULL".to_string())
        );

        conn.execute_unprepared(&sql)
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
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session
            .connection()
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        // Use the stored procedure for atomic addition
        // Note: Using execute_unprepared for stored procedure call
        let sql = format!(
            "SELECT add_credits_safe('{}', {}, '{}', '{}', {})",
            team_id,
            amount,
            transaction_type,
            description.replace("'", "''"),
            reference_id
                .map(|id| format!("'{}'", id))
                .unwrap_or("NULL".to_string())
        );

        conn.execute_unprepared(&sql)
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
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session
            .connection()
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        let mut query = credits_transactions::Entity::find()
            .filter(credits_transactions::Column::TeamId.eq(team_id))
            .order_by_desc(credits_transactions::Column::CreatedAt);

        if let Some(limit) = limit {
            query = query.limit(limit as u64);
        }

        let transactions = query
            .all(conn)
            .await
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        Ok(CreditsTransactionMapper::to_domain_list(transactions))
    }

    async fn initialize_team_credits(
        &self,
        team_id: Uuid,
        initial_balance: i64,
    ) -> Result<i64, CreditsRepositoryError> {
        let session = self
            .pool
            .get_session("admin")
            .await
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        let conn = session
            .connection()
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        // Check if credits already exist
        let existing = credits::Entity::find()
            .filter(credits::Column::TeamId.eq(team_id))
            .one(conn)
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
            created_at: Set(Utc::now().with_timezone(&time_utils::UTC_OFFSET)),
            updated_at: Set(Utc::now().with_timezone(&time_utils::UTC_OFFSET)),
        };

        credits
            .insert(conn)
            .await
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        Ok(initial_balance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::test_helpers::create_test_db_pool;

    // ============================================================
    // Construction tests
    // ============================================================

    #[test]
    #[ignore = "requires TEST_DATABASE_URL"]
    fn test_new_creates_repository_instance() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        // Repository should be constructible without connecting to DB
        // (pool is lazy, no connection until get_session is called)
        let _ = repo;
    }

    // ============================================================
    // Error path tests — all methods should fail gracefully when
    // the lazy pool cannot provide a real session.
    // ============================================================

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_get_balance_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        let result = repo.get_balance(Uuid::new_v4()).await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, CreditsRepositoryError::DatabaseError(_)),
            "Expected DatabaseError, got {:?}",
            err
        );
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_deduct_credits_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        let result = repo
            .deduct_credits(
                Uuid::new_v4(),
                100,
                CreditsTransactionType::Search,
                "test deduct".to_string(),
                None,
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            matches!(err, CreditsRepositoryError::DatabaseError(_)),
            "Expected DatabaseError, got {:?}",
            err
        );
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_add_credits_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        let result = repo
            .add_credits(
                Uuid::new_v4(),
                100,
                CreditsTransactionType::Search,
                "test add".to_string(),
                None,
            )
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreditsRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_get_transaction_history_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        let result = repo.get_transaction_history(Uuid::new_v4(), Some(10)).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreditsRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_initialize_team_credits_returns_db_error_with_real_db() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        let result = repo.initialize_team_credits(Uuid::new_v4(), 0).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreditsRepositoryError::DatabaseError(_)
        ));
    }

    // ============================================================
    // CreditsRepositoryError variant tests
    // ============================================================

    #[test]
    fn test_error_database_error_display() {
        let err = CreditsRepositoryError::DatabaseError("conn refused".to_string());
        assert!(err.to_string().contains("Database error"));
        assert!(err.to_string().contains("conn refused"));
    }

    #[test]
    fn test_error_insufficient_credits_display() {
        let err = CreditsRepositoryError::InsufficientCredits {
            available: 50,
            required: 100,
        };
        assert!(err.to_string().contains("Insufficient credits"));
        assert!(err.to_string().contains("50"));
        assert!(err.to_string().contains("100"));
    }

    #[test]
    fn test_error_credits_not_found_display() {
        let team_id = Uuid::new_v4();
        let err = CreditsRepositoryError::CreditsNotFound(team_id);
        assert!(err.to_string().contains("Credits not found for team"));
        assert!(err.to_string().contains(&team_id.to_string()));
    }

    // ============================================================
    // Additional error path tests — cover remaining branches
    // ============================================================

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_get_transaction_history_with_no_limit_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        // limit=None exercises the branch where no `.limit()` is applied
        let result = repo.get_transaction_history(Uuid::new_v4(), None).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreditsRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_get_transaction_history_with_zero_limit_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        let result = repo.get_transaction_history(Uuid::new_v4(), Some(0)).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreditsRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_get_transaction_history_with_large_limit_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        let result = repo
            .get_transaction_history(Uuid::new_v4(), Some(u32::MAX))
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreditsRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_get_balance_with_nil_uuid_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        let result = repo.get_balance(Uuid::nil()).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreditsRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_deduct_credits_with_reference_id_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        // Some(reference_id) exercises the `format!("'{}'", id)` branch
        let result = repo
            .deduct_credits(
                Uuid::new_v4(),
                50,
                CreditsTransactionType::Scrape,
                "deduct with ref".to_string(),
                Some(Uuid::new_v4()),
            )
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreditsRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_deduct_credits_with_zero_amount_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        let result = repo
            .deduct_credits(
                Uuid::new_v4(),
                0,
                CreditsTransactionType::Search,
                "zero deduct".to_string(),
                None,
            )
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreditsRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_deduct_credits_with_description_containing_quotes_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        // Description with single quotes exercises the `.replace("'", "''")` branch
        let result = repo
            .deduct_credits(
                Uuid::new_v4(),
                10,
                CreditsTransactionType::Extract,
                "it's a 'test' description".to_string(),
                None,
            )
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreditsRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_add_credits_with_reference_id_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        let result = repo
            .add_credits(
                Uuid::new_v4(),
                200,
                CreditsTransactionType::Subscription,
                "add with ref".to_string(),
                Some(Uuid::new_v4()),
            )
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreditsRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_add_credits_with_zero_amount_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        let result = repo
            .add_credits(
                Uuid::new_v4(),
                0,
                CreditsTransactionType::Refund,
                "zero add".to_string(),
                None,
            )
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreditsRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_add_credits_with_description_containing_quotes_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        let result = repo
            .add_credits(
                Uuid::new_v4(),
                100,
                CreditsTransactionType::ManualAdjustment,
                "it's a 'test'".to_string(),
                None,
            )
            .await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreditsRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_initialize_team_credits_with_non_zero_balance_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        let result = repo.initialize_team_credits(Uuid::new_v4(), 1000).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreditsRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_initialize_team_credits_with_negative_balance_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        // i64 allows negative; the method should still attempt the DB call
        let result = repo.initialize_team_credits(Uuid::new_v4(), -500).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreditsRepositoryError::DatabaseError(_)
        ));
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_initialize_team_credits_with_nil_uuid_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        let result = repo.initialize_team_credits(Uuid::nil(), 0).await;
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CreditsRepositoryError::DatabaseError(_)
        ));
    }

    // ============================================================
    // CreditsTransactionType — every variant exercised (error path)
    // ============================================================

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_deduct_credits_with_crawl_type_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        let result = repo
            .deduct_credits(
                Uuid::new_v4(),
                5,
                CreditsTransactionType::Crawl,
                "crawl deduct".to_string(),
                None,
            )
            .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    #[ignore = "requires TEST_DATABASE_URL"]
    async fn test_add_credits_with_extract_type_returns_db_error() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        let result = repo
            .add_credits(
                Uuid::new_v4(),
                50,
                CreditsTransactionType::Extract,
                "extract add".to_string(),
                None,
            )
            .await;
        assert!(result.is_err());
    }

    // ============================================================
    // CreditsTransactionType display exhaustive
    // ============================================================

    #[test]
    fn test_credits_transaction_type_search_display() {
        assert_eq!(format!("{}", CreditsTransactionType::Search), "search");
    }

    #[test]
    fn test_credits_transaction_type_scrape_display() {
        assert_eq!(format!("{}", CreditsTransactionType::Scrape), "scrape");
    }

    #[test]
    fn test_credits_transaction_type_extract_display() {
        assert_eq!(format!("{}", CreditsTransactionType::Extract), "extract");
    }

    #[test]
    fn test_credits_transaction_type_crawl_display() {
        assert_eq!(format!("{}", CreditsTransactionType::Crawl), "crawl");
    }

    #[test]
    fn test_credits_transaction_type_manual_adjustment_display() {
        assert_eq!(
            format!("{}", CreditsTransactionType::ManualAdjustment),
            "manual_adjustment"
        );
    }

    #[test]
    fn test_credits_transaction_type_subscription_display() {
        assert_eq!(
            format!("{}", CreditsTransactionType::Subscription),
            "subscription"
        );
    }

    #[test]
    fn test_credits_transaction_type_refund_display() {
        assert_eq!(format!("{}", CreditsTransactionType::Refund), "refund");
    }

    // ============================================================
    // Error variant edge cases
    // ============================================================

    #[test]
    fn test_error_database_error_with_empty_message() {
        let err = CreditsRepositoryError::DatabaseError("".to_string());
        assert_eq!(format!("{}", err), "Database error: ");
    }

    #[test]
    fn test_error_database_error_with_long_message() {
        let long_msg = "x".repeat(1000);
        let err = CreditsRepositoryError::DatabaseError(long_msg.clone());
        let msg = format!("{}", err);
        assert!(msg.contains(&long_msg));
    }

    #[test]
    fn test_error_insufficient_credits_with_zero_values() {
        let err = CreditsRepositoryError::InsufficientCredits {
            available: 0,
            required: 0,
        };
        let msg = format!("{}", err);
        assert!(msg.contains("available 0"));
        assert!(msg.contains("required 0"));
    }

    #[test]
    fn test_error_insufficient_credits_with_large_values() {
        let err = CreditsRepositoryError::InsufficientCredits {
            available: i64::MAX,
            required: i64::MAX,
        };
        let msg = format!("{}", err);
        assert!(msg.contains(&i64::MAX.to_string()));
    }

    #[test]
    fn test_error_credits_not_found_with_nil_uuid() {
        let err = CreditsRepositoryError::CreditsNotFound(Uuid::nil());
        let msg = format!("{}", err);
        assert!(msg.contains("00000000-0000-0000-0000-000000000000"));
    }
}
