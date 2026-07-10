// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Test helpers for domain services
//!
//! Provides reusable test fixtures and mock utilities for service unit tests.

use crate::domain::models::CreditsTransaction;
use crate::domain::models::CreditsTransactionType;
use crate::domain::repositories::credits_repository::CreditsRepository;
use crate::domain::repositories::credits_repository::CreditsRepositoryError;
use crate::domain::services::credits_service::CreditsService;
use crate::domain::services::credits_service::CreditsServiceConfig;
use async_trait::async_trait;
use std::sync::Arc;
use std::sync::Mutex;
use uuid::Uuid;

type DeductedCredits = Arc<Mutex<Vec<(Uuid, i64)>>>;

/// Mock repository for testing credits service
///
/// Tracks all credit deductions for verification in tests.
#[derive(Debug)]
pub struct MockCreditsRepository {
    /// Tracks all deducted credits (team_id, amount)
    pub deducted: DeductedCredits,
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
pub type TestCreditsServiceSetup = (CreditsService<MockCreditsRepository>, DeductedCredits);

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
    let deducted: DeductedCredits = Arc::new(Mutex::new(Vec::new()));
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
) -> TestCreditsServiceSetup {
    let deducted: DeductedCredits = Arc::new(Mutex::new(Vec::new()));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_repo_deduct_credits_tracks_deduction() {
        let deducted: DeductedCredits = Arc::new(Mutex::new(Vec::new()));
        let repo = MockCreditsRepository {
            deducted: deducted.clone(),
        };
        let team_id = Uuid::new_v4();
        repo.deduct_credits(
            team_id,
            50,
            CreditsTransactionType::Scrape,
            "test deduction".to_string(),
            None,
        )
        .await
        .unwrap();

        let history = deducted.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(history.len(), 1);
        assert_eq!(history[0], (team_id, 50));
    }

    #[tokio::test]
    async fn test_mock_repo_deduct_credits_multiple() {
        let deducted: DeductedCredits = Arc::new(Mutex::new(Vec::new()));
        let repo = MockCreditsRepository {
            deducted: deducted.clone(),
        };
        let team1 = Uuid::new_v4();
        let team2 = Uuid::new_v4();

        repo.deduct_credits(
            team1,
            10,
            CreditsTransactionType::Scrape,
            "first".to_string(),
            None,
        )
        .await
        .unwrap();
        repo.deduct_credits(
            team2,
            20,
            CreditsTransactionType::Scrape,
            "second".to_string(),
            None,
        )
        .await
        .unwrap();

        let history = deducted.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(history.len(), 2);
        assert_eq!(history[0], (team1, 10));
        assert_eq!(history[1], (team2, 20));
    }

    #[tokio::test]
    async fn test_mock_repo_add_credits_returns_100() {
        let deducted: DeductedCredits = Arc::new(Mutex::new(Vec::new()));
        let repo = MockCreditsRepository {
            deducted: deducted.clone(),
        };
        let result = repo
            .add_credits(
                Uuid::new_v4(),
                50,
                CreditsTransactionType::Subscription,
                "test".to_string(),
                None,
            )
            .await
            .unwrap();
        assert_eq!(result, 100);
    }

    #[tokio::test]
    async fn test_mock_repo_get_balance_returns_100() {
        let deducted: DeductedCredits = Arc::new(Mutex::new(Vec::new()));
        let repo = MockCreditsRepository {
            deducted: deducted.clone(),
        };
        let balance = repo.get_balance(Uuid::new_v4()).await.unwrap();
        assert_eq!(balance, 100);
    }

    #[tokio::test]
    async fn test_mock_repo_get_transaction_history_returns_empty() {
        let deducted: DeductedCredits = Arc::new(Mutex::new(Vec::new()));
        let repo = MockCreditsRepository {
            deducted: deducted.clone(),
        };
        let history = repo
            .get_transaction_history(Uuid::new_v4(), Some(10))
            .await
            .unwrap();
        assert!(history.is_empty());
    }

    #[tokio::test]
    async fn test_mock_repo_get_transaction_history_no_limit() {
        let deducted: DeductedCredits = Arc::new(Mutex::new(Vec::new()));
        let repo = MockCreditsRepository {
            deducted: deducted.clone(),
        };
        let history = repo
            .get_transaction_history(Uuid::new_v4(), None)
            .await
            .unwrap();
        assert!(history.is_empty());
    }

    #[tokio::test]
    async fn test_mock_repo_initialize_team_credits_returns_100() {
        let deducted: DeductedCredits = Arc::new(Mutex::new(Vec::new()));
        let repo = MockCreditsRepository {
            deducted: deducted.clone(),
        };
        let result = repo
            .initialize_team_credits(Uuid::new_v4(), 200)
            .await
            .unwrap();
        assert_eq!(result, 100);
    }

    #[test]
    fn test_create_test_credits_service_returns_service() {
        let (service, _deducted) = create_test_credits_service();
        // The service should be usable (just verify it was created)
        let _ = &service;
    }

    #[test]
    fn test_create_test_credits_service_returns_empty_deduction_list() {
        let (_service, deducted) = create_test_credits_service();
        let history = deducted.lock().unwrap_or_else(|e| e.into_inner());
        assert!(history.is_empty());
    }

    #[tokio::test]
    async fn test_create_test_credits_service_tracks_deductions() {
        let (service, deducted) = create_test_credits_service();
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();

        service
            .deduct_feature_credits(team_id, task_id, true, false)
            .await
            .unwrap();

        let history = deducted.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].0, team_id);
    }

    #[test]
    fn test_create_test_credits_service_with_config_custom_values() {
        let (service, _deducted) = create_test_credits_service_with_config(5, 10, 1000);
        let _ = &service;
    }

    #[test]
    fn test_create_test_credits_service_with_config_empty_deductions() {
        let (_service, deducted) = create_test_credits_service_with_config(5, 10, 1000);
        let history = deducted.lock().unwrap_or_else(|e| e.into_inner());
        assert!(history.is_empty());
    }

    #[tokio::test]
    async fn test_create_test_credits_service_with_config_tracks_deductions() {
        let (service, deducted) = create_test_credits_service_with_config(5, 10, 1000);
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();

        service
            .deduct_feature_credits(team_id, task_id, true, true)
            .await
            .unwrap();

        let history = deducted.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(history.len(), 1);
        // screenshot_cost=5, proxy_cost=10, so total should be 15
        assert_eq!(history[0], (team_id, 15));
    }

    #[test]
    fn test_mock_credits_repository_debug() {
        let deducted: DeductedCredits = Arc::new(Mutex::new(Vec::new()));
        let repo = MockCreditsRepository {
            deducted: deducted.clone(),
        };
        let debug = format!("{:?}", repo);
        assert!(debug.contains("MockCreditsRepository"));
    }

    #[test]
    fn test_deducted_credits_shared_between_repo_and_return() {
        let deducted: DeductedCredits = Arc::new(Mutex::new(Vec::new()));
        let repo = MockCreditsRepository {
            deducted: deducted.clone(),
        };
        // Both should point to the same underlying data
        assert!(Arc::ptr_eq(&repo.deducted, &deducted));
    }
}
