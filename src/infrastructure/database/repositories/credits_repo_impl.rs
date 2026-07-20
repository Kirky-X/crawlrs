// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Credits repository implementation using Sea-ORM with Mapper

use async_trait::async_trait;
use chrono::Utc;
use dbnexus::DbPool;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseBackend, EntityTrait, QueryFilter,
    QueryOrder, QuerySelect, Set, Statement,
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

        // Use the stored procedure for atomic deduction with row-level locking.
        // 参数化查询（Statement::from_sql_and_values）避免 SQL 注入：
        // 之前的 format! 拼接仅用 description.replace("'", "''") 转义单引号，
        // 不完整且易被 Unicode/反斜杠等绕过。参数化查询是 SQL 注入的根本防御。
        let stmt = Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT deduct_credits_safe($1, $2, $3, $4, $5)",
            [
                team_id.into(),
                amount.into(),
                transaction_type.to_string().into(),
                description.into(),
                reference_id.into(),
            ],
        );

        conn.execute_raw(stmt)
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

        // Use the stored procedure for atomic addition.
        // 参数化查询（与 deduct_credits 一致）避免 SQL 注入。
        // 存储过程返回新余额（RETURNS BIGINT），用 query_one_raw + try_get_by_index 提取，
        // 而非返回 Ok(0) 占位符（违反函数契约）。
        let stmt = Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT add_credits_safe($1, $2, $3, $4, $5) AS new_balance",
            [
                team_id.into(),
                amount.into(),
                transaction_type.to_string().into(),
                description.into(),
                reference_id.into(),
            ],
        );

        // query_one_raw 接受 Statement by value（sea-orm 2.0 中 query_one 改为接受 &S 引用）
        let result = conn
            .query_one_raw(stmt)
            .await
            .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;

        match result {
            Some(row) => {
                // 存储过程 RETURNS BIGINT NOT NULL（migrations/001 中 add_credits_safe 定义）。
                // 用 try_get_by_index（非 nullable 版本）提取 i64：
                // - 列为 NULL 时返回 DbErr（违反 NOT NULL 契约 → 显性失败，规则 12）
                // - 类型不匹配时返回 DbErr（防御性）
                // 注意：try_get_by_index_nullable 返回 TryGetError 未实现 Display，
                // 故使用 try_get_by_index 返回 DbErr（实现了 Display）。
                let new_balance: i64 = row
                    .try_get_by_index(0)
                    .map_err(|e| CreditsRepositoryError::DatabaseError(e.to_string()))?;
                Ok(new_balance)
            }
            None => Err(CreditsRepositoryError::DatabaseError(
                "add_credits_safe returned no row".to_string(),
            )),
        }
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
    fn test_new_creates_repository_instance() {
        let pool = create_test_db_pool();
        let repo = CreditsRepositoryImpl::new(pool);
        // Repository wraps the pool Arc; construction itself does not
        // open a new connection — get_session on the inner DbPool does.
        let _ = repo;
    }

    // ============================================================
    // CRUD tests — verify get_balance / deduct / add / history /
    // initialize against a real PostgreSQL database.
    // ============================================================

    #[tokio::test]
    async fn test_get_balance_returns_zero_for_unknown_team() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        let team_id = Uuid::new_v4();
        // get_balance on unknown team auto-initializes with 0 and returns Ok(0).
        let result = repo.get_balance(team_id).await;
        assert!(result.is_ok(), "get_balance failed: {:?}", result.err());
        assert_eq!(result.unwrap(), 0, "auto-initialized balance should be 0");
        // Calling again should return the same 0 balance.
        let result2 = repo.get_balance(team_id).await;
        assert!(
            result2.is_ok(),
            "second get_balance failed: {:?}",
            result2.err()
        );
        assert_eq!(result2.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_deduct_credits_succeeds() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        let team_id = Uuid::new_v4();
        // Initialize with 200 credits first so deduction won't go negative.
        repo.initialize_team_credits(team_id, 200)
            .await
            .expect("initialize failed");
        let result = repo
            .deduct_credits(
                team_id,
                100,
                CreditsTransactionType::Search,
                "test deduct".to_string(),
                None,
            )
            .await;
        assert!(result.is_ok(), "deduct_credits failed: {:?}", result.err());
        // Verify DB state: balance should be 100.
        let balance = repo.get_balance(team_id).await.expect("get_balance failed");
        assert_eq!(balance, 100, "balance should be 200 - 100 = 100");
        // Verify transaction history has 1 record with negative amount.
        let history = repo
            .get_transaction_history(team_id, Some(10))
            .await
            .expect("get_transaction_history failed");
        assert_eq!(history.len(), 1, "should have 1 transaction");
        assert_eq!(history[0].amount, -100, "transaction amount should be -100");
    }

    #[tokio::test]
    async fn test_add_credits_succeeds() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        let team_id = Uuid::new_v4();
        let result = repo
            .add_credits(
                team_id,
                100,
                CreditsTransactionType::Search,
                "test add".to_string(),
                None,
            )
            .await;
        assert!(result.is_ok(), "add_credits failed: {:?}", result.err());
        // Verify DB state: balance should be 100.
        let balance = repo.get_balance(team_id).await.expect("get_balance failed");
        assert_eq!(balance, 100, "balance should be 100 after adding 100");
        // Verify transaction history has 1 record with positive amount.
        let history = repo
            .get_transaction_history(team_id, Some(10))
            .await
            .expect("get_transaction_history failed");
        assert_eq!(history.len(), 1, "should have 1 transaction");
        assert_eq!(history[0].amount, 100, "transaction amount should be 100");
    }

    #[tokio::test]
    async fn test_get_transaction_history_returns_empty_for_unknown_team() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        let result = repo.get_transaction_history(Uuid::new_v4(), Some(10)).await;
        assert!(
            result.is_ok(),
            "get_transaction_history failed: {:?}",
            result.err()
        );
        assert!(
            result.unwrap().is_empty(),
            "unknown team should return empty history"
        );
    }

    #[tokio::test]
    async fn test_initialize_team_credits_succeeds() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        let team_id = Uuid::new_v4();
        let result = repo.initialize_team_credits(team_id, 0).await;
        assert!(
            result.is_ok(),
            "initialize_team_credits failed: {:?}",
            result.err()
        );
        assert_eq!(result.unwrap(), 0, "should return the initial balance");
        // Verify DB state: get_balance should return 0.
        let balance = repo.get_balance(team_id).await.expect("get_balance failed");
        assert_eq!(balance, 0);
        // Calling initialize again on existing team should return existing balance.
        let result2 = repo.initialize_team_credits(team_id, 500).await;
        assert!(
            result2.is_ok(),
            "second initialize failed: {:?}",
            result2.err()
        );
        assert_eq!(
            result2.unwrap(),
            0,
            "should return existing balance, not new"
        );
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
    // Additional tests — cover remaining branches (limit / nil uuid /
    // reference_id / zero amount / quoted description / type variants)
    // ============================================================

    #[tokio::test]
    async fn test_get_transaction_history_with_no_limit() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        // limit=None exercises the branch where no `.limit()` is applied
        let result = repo.get_transaction_history(Uuid::new_v4(), None).await;
        assert!(
            result.is_ok(),
            "get_transaction_history failed: {:?}",
            result.err()
        );
        assert!(
            result.unwrap().is_empty(),
            "unknown team should return empty"
        );
    }

    #[tokio::test]
    async fn test_get_transaction_history_with_zero_limit() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        let result = repo.get_transaction_history(Uuid::new_v4(), Some(0)).await;
        assert!(
            result.is_ok(),
            "get_transaction_history failed: {:?}",
            result.err()
        );
        assert!(result.unwrap().is_empty(), "limit=0 should return empty");
    }

    #[tokio::test]
    async fn test_get_transaction_history_with_large_limit() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        let result = repo
            .get_transaction_history(Uuid::new_v4(), Some(u32::MAX))
            .await;
        assert!(
            result.is_ok(),
            "get_transaction_history failed: {:?}",
            result.err()
        );
        assert!(
            result.unwrap().is_empty(),
            "unknown team should return empty"
        );
    }

    #[tokio::test]
    async fn test_get_balance_with_nil_uuid() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        // Nil UUID is a valid UUID; behavior depends on DB state (auto-init or existing).
        // We only assert Ok because nil UUID is shared across test runs and prior
        // runs may have mutated the balance via deduct/add.
        let result = repo.get_balance(Uuid::nil()).await;
        assert!(
            result.is_ok(),
            "get_balance with nil uuid failed: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_deduct_credits_with_reference_id() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        let team_id = Uuid::new_v4();
        repo.initialize_team_credits(team_id, 200)
            .await
            .expect("initialize failed");
        // Some(reference_id) exercises the `format!("'{}'", id)` branch
        let reference_id = Uuid::new_v4();
        let result = repo
            .deduct_credits(
                team_id,
                50,
                CreditsTransactionType::Scrape,
                "deduct with ref".to_string(),
                Some(reference_id),
            )
            .await;
        assert!(result.is_ok(), "deduct_credits failed: {:?}", result.err());
        // Verify balance updated.
        let balance = repo.get_balance(team_id).await.expect("get_balance failed");
        assert_eq!(balance, 150, "balance should be 200 - 50 = 150");
        // Verify transaction history recorded the reference_id.
        let history = repo
            .get_transaction_history(team_id, Some(10))
            .await
            .expect("get_transaction_history failed");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].reference_id, Some(reference_id));
    }

    #[tokio::test]
    async fn test_deduct_credits_with_zero_amount() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        let team_id = Uuid::new_v4();
        repo.initialize_team_credits(team_id, 100)
            .await
            .expect("initialize failed");
        let result = repo
            .deduct_credits(
                team_id,
                0,
                CreditsTransactionType::Search,
                "zero deduct".to_string(),
                None,
            )
            .await;
        assert!(
            result.is_ok(),
            "deduct_credits with 0 failed: {:?}",
            result.err()
        );
        // Verify balance unchanged.
        let balance = repo.get_balance(team_id).await.expect("get_balance failed");
        assert_eq!(balance, 100, "balance should remain 100");
        // Verify transaction history recorded amount=0.
        let history = repo
            .get_transaction_history(team_id, Some(10))
            .await
            .expect("get_transaction_history failed");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].amount, 0);
    }

    #[tokio::test]
    async fn test_deduct_credits_with_description_containing_quotes() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        let team_id = Uuid::new_v4();
        repo.initialize_team_credits(team_id, 100)
            .await
            .expect("initialize failed");
        // Description with single quotes exercises the `.replace("'", "''")` branch
        let description = "it's a 'test' description".to_string();
        let result = repo
            .deduct_credits(
                team_id,
                10,
                CreditsTransactionType::Extract,
                description.clone(),
                None,
            )
            .await;
        assert!(result.is_ok(), "deduct_credits failed: {:?}", result.err());
        // Verify the description was persisted correctly (SQL escaping worked).
        let history = repo
            .get_transaction_history(team_id, Some(10))
            .await
            .expect("get_transaction_history failed");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].description, description);
    }

    #[tokio::test]
    async fn test_add_credits_with_reference_id() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        let team_id = Uuid::new_v4();
        let reference_id = Uuid::new_v4();
        let result = repo
            .add_credits(
                team_id,
                200,
                CreditsTransactionType::Subscription,
                "add with ref".to_string(),
                Some(reference_id),
            )
            .await;
        assert!(result.is_ok(), "add_credits failed: {:?}", result.err());
        // Verify balance.
        let balance = repo.get_balance(team_id).await.expect("get_balance failed");
        assert_eq!(balance, 200, "balance should be 200");
        // Verify transaction recorded reference_id.
        let history = repo
            .get_transaction_history(team_id, Some(10))
            .await
            .expect("get_transaction_history failed");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].reference_id, Some(reference_id));
    }

    #[tokio::test]
    async fn test_add_credits_with_zero_amount() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        let team_id = Uuid::new_v4();
        let result = repo
            .add_credits(
                team_id,
                0,
                CreditsTransactionType::Refund,
                "zero add".to_string(),
                None,
            )
            .await;
        assert!(
            result.is_ok(),
            "add_credits with 0 failed: {:?}",
            result.err()
        );
        // Verify balance is 0.
        let balance = repo.get_balance(team_id).await.expect("get_balance failed");
        assert_eq!(balance, 0, "balance should be 0 after adding 0");
        // Verify transaction recorded amount=0.
        let history = repo
            .get_transaction_history(team_id, Some(10))
            .await
            .expect("get_transaction_history failed");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].amount, 0);
    }

    #[tokio::test]
    async fn test_add_credits_with_description_containing_quotes() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        let team_id = Uuid::new_v4();
        let description = "it's a 'test'".to_string();
        let result = repo
            .add_credits(
                team_id,
                100,
                CreditsTransactionType::ManualAdjustment,
                description.clone(),
                None,
            )
            .await;
        assert!(result.is_ok(), "add_credits failed: {:?}", result.err());
        // Verify the description was persisted correctly.
        let history = repo
            .get_transaction_history(team_id, Some(10))
            .await
            .expect("get_transaction_history failed");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].description, description);
    }

    #[tokio::test]
    async fn test_initialize_team_credits_with_non_zero_balance() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        let team_id = Uuid::new_v4();
        let result = repo.initialize_team_credits(team_id, 1000).await;
        assert!(result.is_ok(), "initialize failed: {:?}", result.err());
        assert_eq!(result.unwrap(), 1000, "should return the initial balance");
        // Verify DB state.
        let balance = repo.get_balance(team_id).await.expect("get_balance failed");
        assert_eq!(balance, 1000);
    }

    #[tokio::test]
    async fn test_initialize_team_credits_with_negative_balance() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        let team_id = Uuid::new_v4();
        // i64 allows negative; the method should still create the record.
        let result = repo.initialize_team_credits(team_id, -500).await;
        assert!(
            result.is_ok(),
            "initialize with negative failed: {:?}",
            result.err()
        );
        assert_eq!(result.unwrap(), -500, "should return the negative balance");
        // Verify DB state.
        let balance = repo.get_balance(team_id).await.expect("get_balance failed");
        assert_eq!(balance, -500);
    }

    #[tokio::test]
    async fn test_initialize_team_credits_with_nil_uuid() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        // Nil UUID is a valid UUID. Behavior depends on DB state:
        // - If no credits row exists: creates one with balance 0, returns Ok(0)
        // - If a row already exists: returns Ok(existing balance)
        // We only assert Ok to avoid cross-test coupling on the shared nil UUID.
        let result = repo.initialize_team_credits(Uuid::nil(), 0).await;
        assert!(
            result.is_ok(),
            "initialize with nil uuid failed: {:?}",
            result.err()
        );
    }

    // ============================================================
    // CreditsTransactionType — every variant exercised
    // ============================================================

    #[tokio::test]
    async fn test_deduct_credits_with_crawl_type() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        let team_id = Uuid::new_v4();
        repo.initialize_team_credits(team_id, 100)
            .await
            .expect("initialize failed");
        let result = repo
            .deduct_credits(
                team_id,
                5,
                CreditsTransactionType::Crawl,
                "crawl deduct".to_string(),
                None,
            )
            .await;
        assert!(
            result.is_ok(),
            "deduct_credits with Crawl failed: {:?}",
            result.err()
        );
        // Verify balance.
        let balance = repo.get_balance(team_id).await.expect("get_balance failed");
        assert_eq!(balance, 95, "balance should be 100 - 5 = 95");
        // Verify transaction type.
        let history = repo
            .get_transaction_history(team_id, Some(10))
            .await
            .expect("get_transaction_history failed");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].transaction_type, CreditsTransactionType::Crawl);
    }

    #[tokio::test]
    async fn test_add_credits_with_extract_type() {
        let repo = CreditsRepositoryImpl::new(create_test_db_pool());
        let team_id = Uuid::new_v4();
        let result = repo
            .add_credits(
                team_id,
                50,
                CreditsTransactionType::Extract,
                "extract add".to_string(),
                None,
            )
            .await;
        assert!(
            result.is_ok(),
            "add_credits with Extract failed: {:?}",
            result.err()
        );
        // Verify balance.
        let balance = repo.get_balance(team_id).await.expect("get_balance failed");
        assert_eq!(balance, 50, "balance should be 50");
        // Verify transaction type.
        let history = repo
            .get_transaction_history(team_id, Some(10))
            .await
            .expect("get_transaction_history failed");
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].transaction_type, CreditsTransactionType::Extract);
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
