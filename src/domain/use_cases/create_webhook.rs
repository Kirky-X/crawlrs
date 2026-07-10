// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::domain::models::Webhook;
use crate::domain::repositories::task_repository::RepositoryError;
use crate::domain::repositories::webhook_repository::WebhookRepository;
use std::sync::Arc;
use uuid::Uuid;

pub struct CreateWebhookUseCase<R: WebhookRepository> {
    repo: Arc<R>,
}

impl<R: WebhookRepository> CreateWebhookUseCase<R> {
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    pub async fn execute(&self, team_id: Uuid, url: String) -> Result<Webhook, RepositoryError> {
        let now = chrono::Utc::now();
        let webhook = Webhook {
            id: Uuid::new_v4(),
            team_id,
            url,
            created_at: now,
        };
        self.repo.create(&webhook).await?;
        Ok(webhook)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// Mock that always succeeds on create and tracks created webhooks
    #[derive(Default)]
    struct MockWebhookRepository {
        created_count: AtomicU32,
    }

    #[async_trait]
    impl WebhookRepository for MockWebhookRepository {
        async fn create(&self, webhook: &Webhook) -> Result<Webhook, RepositoryError> {
            self.created_count.fetch_add(1, Ordering::SeqCst);
            Ok(webhook.clone())
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Webhook>, RepositoryError> {
            Ok(None)
        }

        async fn find_by_team_id(&self, _team_id: Uuid) -> Result<Vec<Webhook>, RepositoryError> {
            Ok(vec![])
        }
    }

    /// Mock that always fails on create
    struct FailingWebhookRepository;

    #[async_trait]
    impl WebhookRepository for FailingWebhookRepository {
        async fn create(&self, _webhook: &Webhook) -> Result<Webhook, RepositoryError> {
            Err(RepositoryError::Database(anyhow::anyhow!("repo down")))
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Webhook>, RepositoryError> {
            Ok(None)
        }

        async fn find_by_team_id(&self, _team_id: Uuid) -> Result<Vec<Webhook>, RepositoryError> {
            Ok(vec![])
        }
    }

    // ---- new ----

    #[test]
    fn test_new_returns_use_case_with_repo() {
        let repo = Arc::new(MockWebhookRepository::default());
        let _use_case = CreateWebhookUseCase::new(repo.clone());
        // Constructor should not call repo
        assert_eq!(repo.created_count.load(Ordering::SeqCst), 0);
    }

    // ---- execute success ----

    #[tokio::test]
    async fn test_execute_success_returns_webhook_with_provided_fields() {
        let repo = Arc::new(MockWebhookRepository::default());
        let use_case = CreateWebhookUseCase::new(repo.clone());

        let team_id = Uuid::new_v4();
        let url = "https://example.com/webhook".to_string();
        let before = chrono::Utc::now();

        let result = use_case.execute(team_id, url.clone()).await;

        assert!(result.is_ok(), "execute should succeed");
        let webhook = result.unwrap();
        assert_eq!(webhook.team_id, team_id, "team_id should match");
        assert_eq!(webhook.url, url, "url should match");
        assert!(
            webhook.created_at >= before,
            "created_at should be set to now"
        );
        assert_eq!(
            repo.created_count.load(Ordering::SeqCst),
            1,
            "repo.create called once"
        );
    }

    #[tokio::test]
    async fn test_execute_generates_unique_ids() {
        let repo = Arc::new(MockWebhookRepository::default());
        let use_case = CreateWebhookUseCase::new(repo);

        let team_id = Uuid::new_v4();
        let w1 = use_case
            .execute(team_id, "https://a.com".to_string())
            .await
            .unwrap();
        let w2 = use_case
            .execute(team_id, "https://b.com".to_string())
            .await
            .unwrap();

        assert_ne!(w1.id, w2.id, "each webhook should get a unique id");
        assert_ne!(w1.url, w2.url);
    }

    // ---- execute failure propagation ----

    #[tokio::test]
    async fn test_execute_propagates_repo_error() {
        let repo = Arc::new(FailingWebhookRepository);
        let use_case = CreateWebhookUseCase::new(repo);

        let result = use_case
            .execute(Uuid::new_v4(), "https://example.com".to_string())
            .await;

        assert!(result.is_err(), "should propagate repo error");
        let err = result.unwrap_err();
        match err {
            RepositoryError::Database(msg) => {
                assert!(
                    msg.to_string().contains("repo down"),
                    "error should contain repo failure message, got: {}",
                    msg
                );
            }
            other => panic!("expected Database error, got {:?}", other),
        }
    }

    // ---- execute boundaries ----

    #[tokio::test]
    async fn test_execute_with_empty_url_still_creates() {
        // Source does not validate URL emptiness; verify it still creates
        let repo = Arc::new(MockWebhookRepository::default());
        let use_case = CreateWebhookUseCase::new(repo.clone());

        let team_id = Uuid::new_v4();
        let result = use_case.execute(team_id, String::new()).await;

        assert!(result.is_ok(), "empty url should still create");
        let webhook = result.unwrap();
        assert!(webhook.url.is_empty());
        assert_eq!(webhook.team_id, team_id);
    }

    #[tokio::test]
    async fn test_execute_with_nil_team_id_succeeds() {
        let repo = Arc::new(MockWebhookRepository::default());
        let use_case = CreateWebhookUseCase::new(repo.clone());

        let result = use_case
            .execute(Uuid::nil(), "https://example.com".to_string())
            .await;

        assert!(result.is_ok(), "nil team_id should still create");
        assert_eq!(result.unwrap().team_id, Uuid::nil());
        assert_eq!(repo.created_count.load(Ordering::SeqCst), 1);
    }
}
