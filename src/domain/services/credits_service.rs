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
use tracing::info;
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
                CreditsTransactionType::Scrape,
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
        self.repository
            .get_balance(team_id)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))
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
            .add_credits(team_id, amount, transaction_type, description, None)
            .await
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("{}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::services::test_helpers::create_test_credits_service;

    // Mock repository is now in test_helpers module

    #[tokio::test]
    async fn test_deduct_feature_credits_screenshot() {
        let (service, deducted) = create_test_credits_service();
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
        let (service, deducted) = create_test_credits_service();
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
        let (service, deducted) = create_test_credits_service();
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
        let (service, deducted) = create_test_credits_service();
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();

        service
            .deduct_feature_credits(team_id, task_id, false, false)
            .await
            .unwrap();

        let history = deducted.lock().unwrap();
        assert!(history.is_empty()); // No deduction when no features used
    }

    // ========== Token Credit Tests ==========

    #[tokio::test]
    async fn test_deduct_token_credits_calculates_correctly() {
        let (service, deducted) = create_test_credits_service();
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();

        // 2500 tokens / 1000 tokens_per_credit = 2.5 -> ceil to 3 credits
        service
            .deduct_token_credits(team_id, task_id, 2500)
            .await
            .unwrap();

        let history = deducted.lock().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0], (team_id, 3)); // 3 credits for 2500 tokens
    }

    #[tokio::test]
    async fn test_deduct_token_credits_handles_zero_tokens() {
        let (service, deducted) = create_test_credits_service();
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();

        // Zero tokens should not call repository
        service
            .deduct_token_credits(team_id, task_id, 0)
            .await
            .unwrap();

        let history = deducted.lock().unwrap();
        assert!(history.is_empty());
    }

    #[tokio::test]
    async fn test_deduct_token_credits_handles_negative_tokens() {
        let (service, deducted) = create_test_credits_service();
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();

        // Negative tokens should be treated as zero
        service
            .deduct_token_credits(team_id, task_id, -100)
            .await
            .unwrap();

        let history = deducted.lock().unwrap();
        assert!(history.is_empty());
    }

    #[tokio::test]
    async fn test_deduct_token_credits_single_token() {
        let (service, deducted) = create_test_credits_service();
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();

        // 1 token / 1000 tokens_per_credit = 0.001 -> ceil to 1 credit
        service
            .deduct_token_credits(team_id, task_id, 1)
            .await
            .unwrap();

        let history = deducted.lock().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0], (team_id, 1));
    }

    // ========== Balance and Add Tests ==========

    #[tokio::test]
    async fn test_get_balance_returns_value() {
        let (service, _deducted) = create_test_credits_service();
        let team_id = Uuid::new_v4();

        // Mock returns 100
        let balance = service.get_balance(team_id).await.unwrap();
        assert_eq!(balance, 100);
    }

    #[tokio::test]
    async fn test_add_credits_calls_repository() {
        let (service, _deducted) = create_test_credits_service();
        let team_id = Uuid::new_v4();

        service
            .add_credits(
                team_id,
                50,
                CreditsTransactionType::ManualAdjustment,
                "Test add".to_string(),
            )
            .await
            .unwrap();

        // Add_credits doesn't use the deducted vector, so we just verify no error
    }

    #[tokio::test]
    async fn test_deduct_token_credits_large_number() {
        let (service, deducted) = create_test_credits_service();
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();

        // 100000 tokens / 1000 tokens_per_credit = 100 credits
        service
            .deduct_token_credits(team_id, task_id, 100000)
            .await
            .unwrap();

        let history = deducted.lock().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0], (team_id, 100));
    }

    #[tokio::test]
    async fn test_deduct_token_credits_exact_boundary() {
        let (service, deducted) = create_test_credits_service();
        let team_id = Uuid::new_v4();
        let task_id = Uuid::new_v4();

        // 1000 tokens exactly = 1 credit (1000/1000 = 1.0, ceil is 1)
        service
            .deduct_token_credits(team_id, task_id, 1000)
            .await
            .unwrap();

        let history = deducted.lock().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0], (team_id, 1));
    }
}
