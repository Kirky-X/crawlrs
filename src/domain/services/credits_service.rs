// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Credits Service
//!
//! Provides unified credit deduction and management for scraping operations.
//! Consolidates credits logic from scrape_worker.

use crate::domain::models::credits::CreditsTransactionType;
use crate::domain::repositories::credits_repository::CreditsRepository;
use std::sync::Arc;
use tracing::{error, info};
use uuid::Uuid;

/// Configuration for credits costs
#[derive(Debug, Clone)]
pub struct CreditsServiceConfig {
    pub screenshot_cost: i64,
    pub proxy_cost: i64,
    pub tokens_per_credit: i64,
}

impl Default for CreditsServiceConfig {
    fn default() -> Self {
        Self {
            screenshot_cost: 2,
            proxy_cost: 1,
            tokens_per_credit: 1000,
        }
    }
}

/// Service for managing team credits
pub struct CreditsService<R: CreditsRepository> {
    repository: Arc<R>,
    config: CreditsServiceConfig,
}

impl<R: CreditsRepository> CreditsService<R> {
    /// Create a new CreditsService
    pub fn new(repository: Arc<R>, config: CreditsServiceConfig) -> Self {
        Self { repository, config }
    }

    /// Create a CreditsService with default config
    pub fn with_default_config(repository: Arc<R>) -> Self {
        Self::new(repository, CreditsServiceConfig::default())
    }

    /// Deduct credits for feature usage (screenshot, proxy)
    ///
    /// # Arguments
    ///
    /// * `team_id` - The team ID
    /// * `task_id` - The task ID for reference
    /// * `screenshot` - Whether screenshot feature was used
    /// * `proxy` - Whether proxy feature was used
    pub async fn deduct_feature_credits(
        &self,
        team_id: Uuid,
        task_id: Uuid,
        screenshot: bool,
        proxy: bool,
    ) -> Result<(), anyhow::Error> {
        let mut extra_credits = 0;

        // Screenshot costs
        if screenshot {
            extra_credits += self.config.screenshot_cost;
        }

        // Proxy costs
        if proxy {
            extra_credits += self.config.proxy_cost;
        }

        if extra_credits > 0 {
            self.repository
                .deduct_credits(
                    team_id,
                    extra_credits,
                    CreditsTransactionType::Scrape,
                    format!("Feature credits for task {}", task_id),
                    Some(task_id),
                )
                .await?;

            info!(
                "Deducted {} feature credits for team {} (task {})",
                extra_credits, team_id, task_id
            );
        }

        Ok(())
    }

    /// Deduct credits for token usage (LLM operations)
    pub async fn deduct_token_credits(
        &self,
        team_id: Uuid,
        task_id: Uuid,
        total_tokens: i64,
    ) -> Result<(), anyhow::Error> {
        if total_tokens <= 0 {
            return Ok(());
        }

        let credits_used =
            (total_tokens as f64 / self.config.tokens_per_credit as f64).ceil() as i64;

        self.repository
            .deduct_credits(
                team_id,
                credits_used,
                CreditsTransactionType::AiProcessing,
                format!(
                    "Token credits for task {} ({} tokens)",
                    task_id, total_tokens
                ),
                Some(task_id),
            )
            .await?;

        info!(
            "Deducted {} credits ({} tokens) for team {}",
            credits_used, total_tokens, team_id
        );

        Ok(())
    }

    /// Get current credits balance for a team
    pub async fn get_balance(&self, team_id: Uuid) -> Result<i64, anyhow::Error> {
        self.repository.get_balance(team_id).await
    }

    /// Add credits to a team account
    pub async fn add_credits(
        &self,
        team_id: Uuid,
        amount: i64,
        transaction_type: CreditsTransactionType,
        description: String,
    ) -> Result<(), anyhow::Error> {
        self.repository
            .add_credits(team_id, amount, transaction_type, description)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;

    struct MockCreditsRepository {
        deducted: Arc<std::sync::Mutex<Vec<(Uuid, i64)>>>,
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
        ) -> Result<(), anyhow::Error> {
            self.deducted.lock().unwrap().push((team_id, amount));
            Ok(())
        }

        async fn add_credits(
            &self,
            _team_id: Uuid,
            _amount: i64,
            _transaction_type: CreditsTransactionType,
            _description: String,
        ) -> Result<(), anyhow::Error> {
            Ok(())
        }

        async fn get_balance(&self, _team_id: Uuid) -> Result<i64, anyhow::Error> {
            Ok(100)
        }

        async fn get_transaction_history(
            &self,
            _team_id: Uuid,
            _limit: i32,
        ) -> Result<Vec<crate::domain::models::credits::CreditsTransaction>, anyhow::Error>
        {
            Ok(vec![])
        }

        async fn rollback_credits(&self, _transaction_id: Uuid) -> Result<(), anyhow::Error> {
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_deduct_feature_credits_screenshot() {
        let deducted = Arc::new(std::sync::Mutex::new(Vec::new()));
        let repo = MockCreditsRepository {
            deducted: deducted.clone(),
        };

        let service = CreditsService::with_default_config(Arc::new(repo));
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();

        service
            .deduct_feature_credits(team_id, task_id, true, false)
            .await
            .unwrap();

        let history = deducted.lock().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0], (team_id, 2)); // screenshot_cost = 2
    }

    #[tokio::test]
    async fn test_deduct_feature_credits_proxy() {
        let deducted = Arc::new(std::sync::Mutex::new(Vec::new()));
        let repo = MockCreditsRepository {
            deducted: deducted.clone(),
        };

        let service = CreditsService::with_default_config(Arc::new(repo));
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();

        service
            .deduct_feature_credits(team_id, task_id, false, true)
            .await
            .unwrap();

        let history = deducted.lock().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0], (team_id, 1)); // proxy_cost = 1
    }

    #[tokio::test]
    async fn test_deduct_feature_credits_both() {
        let deducted = Arc::new(std::sync::Mutex::new(Vec::new()));
        let repo = MockCreditsRepository {
            deducted: deducted.clone(),
        };

        let service = CreditsService::with_default_config(Arc::new(repo));
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();

        service
            .deduct_feature_credits(team_id, task_id, true, true)
            .await
            .unwrap();

        let history = deducted.lock().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0], (team_id, 3)); // 2 + 1 = 3
    }

    #[tokio::test]
    async fn test_no_credits_for_no_features() {
        let deducted = Arc::new(std::sync::Mutex::new(Vec::new()));
        let repo = MockCreditsRepository {
            deducted: deducted.clone(),
        };

        let service = CreditsService::with_default_config(Arc::new(repo));
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();

        service
            .deduct_feature_credits(team_id, task_id, false, false)
            .await
            .unwrap();

        let history = deducted.lock().unwrap();
        assert!(history.is_empty()); // No deduction when no features used
    }
}
