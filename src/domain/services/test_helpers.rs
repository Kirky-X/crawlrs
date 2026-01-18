// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Test helpers for domain services
//!
//! Provides reusable test fixtures and mock utilities for service unit tests.

use crate::domain::models::credits::CreditsTransaction;
use crate::domain::models::credits::CreditsTransactionType;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::credits_repository::CreditsRepositoryError;
use crate::domain::services::credits_service::CreditsService;
use crate::domain::services::credits_service::CreditsServiceConfig;
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::Mutex;
use uuid::Uuid;

/// Mock repository for testing credits service
///
/// Tracks all credit deductions for verification in tests.
#[derive(Debug)]
pub struct MockCreditsRepository {
    /// Tracks all deducted credits (team_id, amount)
    pub deducted: Arc<Mutex<Vec<(Uuid, i64)>>>,
}

#[async_trait]
impl CreditsRepository for MockCreditsRepository {
    async fn deduct_credits(
        &self,
        team_id: Uuid,
        amount: i64,
        _transaction_type: CreditsTransactionType,
        _description: String,
        _reference_id: Option<Uuid>,
    ) -> Result<(), CreditsRepositoryError> {
        self.deducted
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((team_id, amount));
        Ok(())
    }

    async fn add_credits(
        &self,
        _team_id: Uuid,
        _amount: i64,
        _transaction_type: CreditsTransactionType,
        _description: String,
        _reference_id: Option<Uuid>,
    ) -> Result<i64, CreditsRepositoryError> {
        Ok(100)
    }

    async fn get_balance(&self, _team_id: Uuid) -> Result<i64, CreditsRepositoryError> {
        Ok(100)
    }

    async fn get_transaction_history(
        &self,
        _team_id: Uuid,
        _limit: Option<u32>,
    ) -> Result<Vec<CreditsTransaction>, CreditsRepositoryError> {
        Ok(vec![])
    }

    async fn initialize_team_credits(
        &self,
        _team_id: Uuid,
        _initial_balance: i64,
    ) -> Result<i64, CreditsRepositoryError> {
        Ok(100)
    }
}

/// Result tuple from creating a test credits service
///
/// Contains:
/// - The service instance
/// - A reference to the deducted credits tracking vector
pub type TestCreditsServiceSetup = (
    CreditsService<MockCreditsRepository>,
    Arc<Mutex<Vec<(Uuid, i64)>>>,
);

/// Creates a test setup for credits service tests
///
/// This fixture eliminates the repetitive setup code that was duplicated
/// across all credits service tests.
///
/// # Example
///
/// ```rust
/// use crate::domain::services::test_helpers::create_test_credits_service;
///
/// #[tokio::test]
/// async fn test_deduct_feature_credits_screenshot() {
///     let (service, deducted) = create_test_credits_service();
///     let team_id = Uuid::new_v4();
///     let task_id = Uuid::new_v4();
///
///     service
///         .deduct_feature_credits(team_id, task_id, true, false)
///         .await
///         .unwrap();
///
///     let history = deducted.lock().unwrap();
///     assert_eq!(history.len(), 1);
///     assert_eq!(history[0], (team_id, 2));
/// }
/// ```
pub fn create_test_credits_service() -> TestCreditsServiceSetup {
    let deducted: Arc<Mutex<Vec<(Uuid, i64)>>> = Arc::new(Mutex::new(Vec::new()));
    let repo = MockCreditsRepository {
        deducted: deducted.clone(),
    };

    let service = CreditsService::with_default_config(Arc::new(repo));

    (service, deducted)
}

/// Creates a test setup with custom service configuration
///
/// # Arguments
///
/// * `screenshot_cost` - Cost for screenshot feature
/// * `proxy_cost` - Cost for proxy feature
/// * `tokens_per_credit` - Number of tokens per credit
pub fn create_test_credits_service_with_config(
    screenshot_cost: i64,
    proxy_cost: i64,
    tokens_per_credit: i64,
) -> (
    CreditsService<MockCreditsRepository>,
    Arc<Mutex<Vec<(Uuid, i64)>>>,
) {
    let deducted: Arc<Mutex<Vec<(Uuid, i64)>>> = Arc::new(Mutex::new(Vec::new()));
    let repo = MockCreditsRepository {
        deducted: deducted.clone(),
    };

    let config = CreditsServiceConfig {
        screenshot_cost,
        proxy_cost,
        tokens_per_credit,
    };

    let service = CreditsService::new(Arc::new(repo), config);

    (service, deducted)
}
