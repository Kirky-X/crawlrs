// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Credits service tests
//!
//! Tests for the CreditsService including credit deduction and balance management

use std::sync::Arc;
use uuid::Uuid;

use crawlrs::domain::models::CreditsTransactionType;
use crawlrs::domain::repositories::credits_repository::CreditsRepository;
use crawlrs::domain::services::credits_service::{CreditsService, CreditsServiceConfig};

// === Mock Credits Repository ===

struct MockCreditsRepository {
    balances: Arc<std::sync::Mutex<std::collections::HashMap<Uuid, i64>>>,
    should_fail: Arc<std::sync::atomic::AtomicBool>,
}

impl MockCreditsRepository {
    fn new() -> Self {
        Self {
            balances: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            should_fail: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn with_balance(team_id: Uuid, balance: i64) -> Self {
        let mut balances = std::collections::HashMap::new();
        balances.insert(team_id, balance);
        Self {
            balances: Arc::new(std::sync::Mutex::new(balances)),
            should_fail: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    fn get_balance_value(&self, team_id: Uuid) -> i64 {
        let balances = self.balances.lock().unwrap();
        *balances.get(&team_id).unwrap_or(&0)
    }
}

#[async_trait::async_trait]
impl CreditsRepository for MockCreditsRepository {
    async fn get_balance(&self, team_id: Uuid) -> Result<i64, anyhow::Error> {
        if self.should_fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(anyhow::anyhow!("Database error"));
        }

        let balances = self.balances.lock().unwrap();
        Ok(*balances.get(&team_id).unwrap_or(&0))
    }

    async fn deduct_credits(
        &self,
        team_id: Uuid,
        amount: i64,
        _transaction_type: CreditsTransactionType,
        _description: String,
        _reference_id: Option<Uuid>,
    ) -> Result<(), anyhow::Error> {
        if self.should_fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(anyhow::anyhow!("Database error"));
        }

        let mut balances = self.balances.lock().unwrap();
        let balance = balances.entry(team_id).or_insert(0);
        *balance -= amount;
        Ok(())
    }

    async fn add_credits(
        &self,
        team_id: Uuid,
        amount: i64,
        _transaction_type: CreditsTransactionType,
        _description: String,
        _reference_id: Option<Uuid>,
    ) -> Result<i64, anyhow::Error> {
        if self.should_fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(anyhow::anyhow!("Database error"));
        }

        let mut balances = self.balances.lock().unwrap();
        let balance = balances.entry(team_id).or_insert(0);
        *balance += amount;
        Ok(*balance)
    }

    async fn get_transaction_history(
        &self,
        _team_id: Uuid,
        _limit: u64,
    ) -> Result<Vec<crate::domain::models::credits_model::CreditsTransaction>, anyhow::Error> {
        Ok(vec![])
    }

    async fn initialize_team_credits(
        &self,
        team_id: Uuid,
        initial_balance: i64,
    ) -> Result<i64, anyhow::Error> {
        let mut balances = self.balances.lock().unwrap();
        balances.insert(team_id, initial_balance);
        Ok(initial_balance)
    }
}

// === Helper Functions ===

fn create_test_service() -> CreditsService<MockCreditsRepository> {
    let repo = Arc::new(MockCreditsRepository::new());
    CreditsService::with_default_config(repo)
}

fn create_service_with_config(config: CreditsServiceConfig) -> CreditsService<MockCreditsRepository> {
    let repo = Arc::new(MockCreditsRepository::new());
    CreditsService::new(repo, config)
}

// === Unit Tests ===

#[tokio::test]
async fn test_service_creation_with_default_config() {
    let repo = Arc::new(MockCreditsRepository::new());
    let service = CreditsService::with_default_config(repo);

    // Service created successfully with default config
    assert_eq!(service.config.screenshot_cost, 2);
    assert_eq!(service.config.proxy_cost, 1);
    assert_eq!(service.config.tokens_per_credit, 1000);
}

#[tokio::test]
async fn test_service_creation_with_custom_config() {
    let repo = Arc::new(MockCreditsRepository::new());
    let config = CreditsServiceConfig {
        screenshot_cost: 5,
        proxy_cost: 3,
        tokens_per_credit: 500,
    };

    let service = CreditsService::new(repo, config);

    assert_eq!(service.config.screenshot_cost, 5);
    assert_eq!(service.config.proxy_cost, 3);
    assert_eq!(service.config.tokens_per_credit, 500);
}

#[tokio::test]
async fn test_get_balance() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository::with_balance(team_id, 100));
    let service = CreditsService::with_default_config(repo);

    let balance = service.get_balance(team_id).await;

    assert!(balance.is_ok());
    assert_eq!(balance.unwrap(), 100);
}

#[tokio::test]
async fn test_get_balance_zero() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository::new());
    let service = CreditsService::with_default_config(repo);

    let balance = service.get_balance(team_id).await;

    assert!(balance.is_ok());
    assert_eq!(balance.unwrap(), 0);
}

#[tokio::test]
async fn test_get_balance_repository_error() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository {
        balances: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        should_fail: Arc::new(std::sync::atomic::AtomicBool::new(true)),
    });
    let service = CreditsService::with_default_config(repo);

    let balance = service.get_balance(team_id).await;

    assert!(balance.is_err());
}

#[tokio::test]
async fn test_deduct_feature_credits_screenshot_only() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository::with_balance(team_id, 100));
    let service = CreditsService::with_default_config(repo);

    let task_id = Uuid::new_v4();
    let result = service
        .deduct_feature_credits(team_id, task_id, true, false)
        .await;

    assert!(result.is_ok());
    let balance = service.get_balance(team_id).await.unwrap();
    assert_eq!(balance, 98); // 100 - 2 (screenshot cost)
}

#[tokio::test]
async fn test_deduct_feature_credits_proxy_only() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository::with_balance(team_id, 100));
    let service = CreditsService::with_default_config(repo);

    let task_id = Uuid::new_v4();
    let result = service
        .deduct_feature_credits(team_id, task_id, false, true)
        .await;

    assert!(result.is_ok());
    let balance = service.get_balance(team_id).await.unwrap();
    assert_eq!(balance, 99); // 100 - 1 (proxy cost)
}

#[tokio::test]
async fn test_deduct_feature_credits_both() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository::with_balance(team_id, 100));
    let service = CreditsService::with_default_config(repo);

    let task_id = Uuid::new_v4();
    let result = service
        .deduct_feature_credits(team_id, task_id, true, true)
        .await;

    assert!(result.is_ok());
    let balance = service.get_balance(team_id).await.unwrap();
    assert_eq!(balance, 97); // 100 - 2 - 1
}

#[tokio::test]
async fn test_deduct_feature_credits_none() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository::with_balance(team_id, 100));
    let service = CreditsService::with_default_config(repo);

    let task_id = Uuid::new_v4();
    let result = service
        .deduct_feature_credits(team_id, task_id, false, false)
        .await;

    assert!(result.is_ok());
    let balance = service.get_balance(team_id).await.unwrap();
    assert_eq!(balance, 100); // No deduction
}

#[tokio::test]
async fn test_deduct_feature_credits_custom_costs() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository::with_balance(team_id, 100));
    let config = CreditsServiceConfig {
        screenshot_cost: 5,
        proxy_cost: 3,
        tokens_per_credit: 500,
    };
    let service = CreditsService::new(repo, config);

    let task_id = Uuid::new_v4();
    let result = service
        .deduct_feature_credits(team_id, task_id, true, true)
        .await;

    assert!(result.is_ok());
    let balance = service.get_balance(team_id).await.unwrap();
    assert_eq!(balance, 92); // 100 - 5 - 3
}

#[tokio::test]
async fn test_deduct_token_credits() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository::with_balance(team_id, 1000));
    let service = CreditsService::with_default_config(repo);

    let task_id = Uuid::new_v4();
    let total_tokens = 2500;

    let result = service
        .deduct_token_credits(team_id, task_id, total_tokens)
        .await;

    assert!(result.is_ok());
    let balance = service.get_balance(team_id).await.unwrap();
    assert_eq!(balance, 997); // 1000 - (2500 / 1000) = 1000 - 2 = 998, but round up = 3
}

#[tokio::test]
async fn test_deduct_token_credits_rounding() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository::with_balance(team_id, 100));
    let service = CreditsService::with_default_config(repo);

    let task_id = Uuid::new_v4();
    let total_tokens = 1500; // 1.5 credits, should round up to 2

    let result = service
        .deduct_token_credits(team_id, task_id, total_tokens)
        .await;

    assert!(result.is_ok());
    let balance = service.get_balance(team_id).await.unwrap();
    assert_eq!(balance, 98); // 100 - 2 (rounded up from 1.5)
}

#[tokio::test]
async fn test_deduct_token_credits_zero_tokens() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository::with_balance(team_id, 100));
    let service = CreditsService::with_default_config(repo);

    let task_id = Uuid::new_v4();
    let total_tokens = 0;

    let result = service
        .deduct_token_credits(team_id, task_id, total_tokens)
        .await;

    assert!(result.is_ok());
    let balance = service.get_balance(team_id).await.unwrap();
    assert_eq!(balance, 100); // No deduction
}

#[tokio::test]
async fn test_add_credits() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository::new());
    let service = CreditsService::with_default_config(repo);

    let result = service
        .add_credits(
            team_id,
            50,
            CreditsTransactionType::Deposit,
            "Test deposit".to_string(),
        )
        .await;

    assert!(result.is_ok());
    let balance = service.get_balance(team_id).await.unwrap();
    assert_eq!(balance, 50);
}

#[tokio::test]
async fn test_add_credits_to_existing_balance() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository::with_balance(team_id, 50));
    let service = CreditsService::with_default_config(repo);

    let result = service
        .add_credits(
            team_id,
            30,
            CreditsTransactionType::Deposit,
            "Test deposit".to_string(),
        )
        .await;

    assert!(result.is_ok());
    let balance = service.get_balance(team_id).await.unwrap();
    assert_eq!(balance, 80);
}

#[tokio::test]
async fn test_deduct_credits_insufficient_funds() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository::with_balance(team_id, 5));
    let service = CreditsService::with_default_config(repo);

    let task_id = Uuid::new_v4();
    let total_tokens = 10000; // Would require 10 credits

    let result = service
        .deduct_token_credits(team_id, task_id, total_tokens)
        .await;

    assert!(result.is_err());
}

// === Complex Workflow Tests ===

#[tokio::test]
async fn test_complete_workflow_deduct_and_add() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository::with_balance(team_id, 100));
    let service = CreditsService::with_default_config(repo);

    // Initial balance
    let balance = service.get_balance(team_id).await.unwrap();
    assert_eq!(balance, 100);

    // Deduct for screenshot
    let task_id = Uuid::new_v4();
    service
        .deduct_feature_credits(team_id, task_id, true, false)
        .await
        .unwrap();

    let balance = service.get_balance(team_id).await.unwrap();
    assert_eq!(balance, 98);

    // Add credits
    service
        .add_credits(
            team_id,
            10,
            CreditsTransactionType::Deposit,
            "Refill".to_string(),
        )
        .await
        .unwrap();

    let balance = service.get_balance(team_id).await.unwrap();
    assert_eq!(balance, 108);
}

#[tokio::test]
async fn test_concurrent_deductions() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository::with_balance(team_id, 100));
    let service = Arc::new(CreditsService::with_default_config(repo));

    // Spawn multiple concurrent deduction tasks
    let mut handles = vec![];

    for i in 0..10 {
        let service_clone = service.clone();
        let task_id = Uuid::new_v4();
        let handle = tokio::spawn(async move {
            service_clone
                .deduct_feature_credits(team_id, task_id, false, true)
                .await
        });
        handles.push(handle);
    }

    // Wait for all tasks to complete
    for handle in handles {
        assert!(handle.await.is_ok());
    }

    // Final balance should be 100 - (10 * 1) = 90
    let balance = service.get_balance(team_id).await.unwrap();
    assert_eq!(balance, 90);
}

// === Error Handling Tests ===

#[tokio::test]
async fn test_deduct_feature_credits_repository_error() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository {
        balances: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        should_fail: Arc::new(std::sync::atomic::AtomicBool::new(true)),
    });
    let service = CreditsService::with_default_config(repo);

    let task_id = Uuid::new_v4();
    let result = service
        .deduct_feature_credits(team_id, task_id, true, false)
        .await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_add_credits_repository_error() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository {
        balances: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        should_fail: Arc::new(std::sync::atomic::AtomicBool::new(true)),
    });
    let service = CreditsService::with_default_config(repo);

    let result = service
        .add_credits(
            team_id,
            10,
            CreditsTransactionType::Deposit,
            "Test".to_string(),
        )
        .await;

    assert!(result.is_err());
}

// === Edge Cases ===

#[tokio::test]
async fn test_deduct_zero_credits() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository::with_balance(team_id, 100));
    let config = CreditsServiceConfig {
        screenshot_cost: 0,
        proxy_cost: 0,
        tokens_per_credit: 1000,
    };
    let service = CreditsService::new(repo, config);

    let task_id = Uuid::new_v4();
    let result = service
        .deduct_feature_credits(team_id, task_id, true, true)
        .await;

    assert!(result.is_ok());
    let balance = service.get_balance(team_id).await.unwrap();
    assert_eq!(balance, 100);
}

#[tokio::test]
async fn test_add_negative_credits() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository::with_balance(team_id, 100));
    let service = CreditsService::with_default_config(repo);

    let result = service
        .add_credits(
            team_id,
            -10,
            CreditsTransactionType::Deposit,
            "Penalty".to_string(),
        )
        .await;

    assert!(result.is_ok());
    let balance = service.get_balance(team_id).await.unwrap();
    assert_eq!(balance, 90);
}

#[tokio::test]
async fn test_large_token_count() {
    let team_id = Uuid::new_v4();
    let repo = Arc::new(MockCreditsRepository::with_balance(team_id, 10000));
    let service = CreditsService::with_default_config(repo);

    let task_id = Uuid::new_v4();
    let total_tokens = 5_000_000; // 5000 credits

    let result = service
        .deduct_token_credits(team_id, task_id, total_tokens)
        .await;

    assert!(result.is_ok());
    let balance = service.get_balance(team_id).await.unwrap();
    assert_eq!(balance, 5000);
}
