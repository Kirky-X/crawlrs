// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Aggregated application dependency extractor.
//!
//! `AppDeps` bundles the five most-commonly-needed request extensions
//! (task queue, settings, task repository, rate-limiting service, and
//! auth state) into a single axum extractor so that handler signatures
//! stay concise and consistent.
//!
//! # Usage
//!
//! ```rust,ignore
//! async fn my_handler(AppDeps { queue, settings, task_repo, rate_limit, auth }: AppDeps) -> impl IntoResponse {
//!     // ...
//! }
//! ```

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use std::sync::Arc;

use crate::config::settings::Settings;
use crate::domain::repositories::task_repository::TaskRepository;
use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::presentation::middleware::auth_middleware::AuthState;
use crate::queue::task_queue::TaskQueue;

/// Aggregated application dependencies extracted from request extensions.
///
/// # Fields
///
/// * `queue` - The task queue for enqueueing work items.
/// * `settings` - Application settings / configuration.
/// * `task_repo` - Repository for task persistence.
/// * `rate_limit` - Rate-limiting service for quota / concurrency checks.
/// * `auth` - Authenticated caller state (team id, api key id).
#[derive(Clone)]
pub struct AppDeps {
    pub queue: Arc<dyn TaskQueue>,
    pub settings: Arc<Settings>,
    pub task_repo: Arc<dyn TaskRepository>,
    pub rate_limit: Arc<dyn RateLimitingService>,
    pub auth: AuthState,
}

/// Rejection type returned when a required extension is missing.
fn missing_extension(name: &str) -> Response {
    let status = StatusCode::INTERNAL_SERVER_ERROR;
    let body = Json(json!({ "error": format!("Missing extension: {}", name) }));
    (status, body).into_response()
}

impl<S> FromRequestParts<S> for AppDeps
where
    S: Send + Sync,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let queue = parts
            .extensions
            .get::<Arc<dyn TaskQueue>>()
            .cloned()
            .ok_or_else(|| missing_extension("TaskQueue"))?;

        let settings = parts
            .extensions
            .get::<Arc<Settings>>()
            .cloned()
            .ok_or_else(|| missing_extension("Settings"))?;

        let task_repo = parts
            .extensions
            .get::<Arc<dyn TaskRepository>>()
            .cloned()
            .ok_or_else(|| missing_extension("TaskRepository"))?;

        let rate_limit = parts
            .extensions
            .get::<Arc<dyn RateLimitingService>>()
            .cloned()
            .ok_or_else(|| missing_extension("RateLimitingService"))?;

        let auth = parts
            .extensions
            .get::<AuthState>()
            .cloned()
            .ok_or_else(|| missing_extension("AuthState"))?;

        Ok(AppDeps {
            queue,
            settings,
            task_repo,
            rate_limit,
            auth,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use async_trait::async_trait;
    use axum::extract::FromRequestParts;
    use axum::http::request::Parts;
    use axum::http::Request;
    use std::collections::HashSet;
    use std::sync::Arc;
    use uuid::Uuid;

    use crate::common::test_helpers::create_test_db_pool;
    use crate::domain::auth::ApiKeyScope;
    use crate::domain::models::Task;
    use crate::domain::repositories::task_repository::{RepositoryError, TaskQueryParams};
    use crate::domain::services::rate_limiting_service::{
        BacklogService, ConcurrencyConfig, ConcurrencyControlService, ConcurrencyResult,
        QuotaService, RateLimitConfig, RateLimitResult, RateLimitService, RateLimitingError,
    };
    use crate::queue::task_queue::QueueError;

    // --- Mock types ---

    struct MockTaskQueue;

    #[async_trait]
    impl TaskQueue for MockTaskQueue {
        async fn enqueue(&self, task: Task) -> Result<Task, QueueError> {
            Ok(task)
        }
        async fn dequeue(&self, _worker_id: Uuid) -> Result<Option<Task>, QueueError> {
            Ok(None)
        }
        async fn complete(&self, _task_id: Uuid) -> Result<(), QueueError> {
            Ok(())
        }
        async fn fail(&self, _task_id: Uuid) -> Result<(), QueueError> {
            Ok(())
        }
        async fn cancel(&self, _task_id: Uuid) -> Result<(), QueueError> {
            Ok(())
        }
    }

    struct MockTaskRepo;

    #[async_trait]
    impl crate::domain::repositories::task_repository::TaskRepository for MockTaskRepo {
        async fn create(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }
        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }
        async fn update(&self, task: &Task) -> Result<Task, RepositoryError> {
            Ok(task.clone())
        }
        async fn acquire_next(&self, _worker_id: Uuid) -> Result<Option<Task>, RepositoryError> {
            Ok(None)
        }
        async fn mark_completed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn mark_failed(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn mark_cancelled(&self, _id: Uuid) -> Result<(), RepositoryError> {
            Ok(())
        }
        async fn exists_by_url(&self, _url: &str) -> Result<bool, RepositoryError> {
            Ok(false)
        }
        async fn find_existing_urls(
            &self,
            _urls: &[String],
        ) -> Result<HashSet<String>, RepositoryError> {
            Ok(HashSet::new())
        }
        async fn reset_stuck_tasks(
            &self,
            _timeout: chrono::Duration,
        ) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn cancel_tasks_by_crawl_id(&self, _crawl_id: Uuid) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn expire_tasks(&self) -> Result<u64, RepositoryError> {
            Ok(0)
        }
        async fn find_by_crawl_id(&self, _crawl_id: Uuid) -> Result<Vec<Task>, RepositoryError> {
            Ok(vec![])
        }
        async fn query_tasks(
            &self,
            _params: TaskQueryParams,
        ) -> Result<(Vec<Task>, u64), RepositoryError> {
            Ok((vec![], 0))
        }
        async fn batch_cancel(
            &self,
            _task_ids: Vec<Uuid>,
            _team_id: Uuid,
            _force: bool,
        ) -> Result<(Vec<Uuid>, Vec<(Uuid, String)>), RepositoryError> {
            Ok((vec![], vec![]))
        }
    }

    struct MockRateLimitService;

    #[async_trait]
    impl RateLimitService for MockRateLimitService {
        async fn check_rate_limit(
            &self,
            _api_key: &str,
            _endpoint: &str,
        ) -> Result<RateLimitResult, RateLimitingError> {
            Ok(RateLimitResult::Allowed)
        }
        async fn get_team_rate_limit_config(
            &self,
            _team_id: Uuid,
        ) -> Result<RateLimitConfig, RateLimitingError> {
            Ok(RateLimitConfig::default())
        }
        async fn update_team_rate_limit_config(
            &self,
            _team_id: Uuid,
            _config: RateLimitConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
        async fn cleanup_expired_rate_limits(&self) -> Result<u64, RateLimitingError> {
            Ok(0)
        }
    }

    #[async_trait]
    impl ConcurrencyControlService for MockRateLimitService {
        async fn check_team_concurrency(
            &self,
            _team_id: Uuid,
            _task_id: Uuid,
        ) -> Result<ConcurrencyResult, RateLimitingError> {
            Ok(ConcurrencyResult::Allowed)
        }
        async fn release_team_concurrency_slot(
            &self,
            _team_id: Uuid,
            _task_id: Uuid,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
        async fn get_team_current_concurrency(
            &self,
            _team_id: Uuid,
        ) -> Result<u32, RateLimitingError> {
            Ok(0)
        }
        async fn get_team_concurrency_config(
            &self,
            _team_id: Uuid,
        ) -> Result<ConcurrencyConfig, RateLimitingError> {
            Ok(ConcurrencyConfig::default())
        }
        async fn update_team_concurrency_config(
            &self,
            _team_id: Uuid,
            _config: ConcurrencyConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
    }

    #[async_trait]
    impl BacklogService for MockRateLimitService {
        async fn process_backlog_tasks(&self, _team_id: Uuid) -> Result<u32, RateLimitingError> {
            Ok(0)
        }
    }

    #[async_trait]
    impl QuotaService for MockRateLimitService {
        async fn check_and_deduct_quota(
            &self,
            _team_id: Uuid,
            _amount: i64,
            _transaction_type: crate::domain::models::CreditsTransactionType,
            _description: String,
            _reference_id: Option<Uuid>,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
        async fn get_quota_balance(&self, _team_id: Uuid) -> Result<i64, RateLimitingError> {
            Ok(1000)
        }
    }

    #[async_trait]
    impl RateLimitingService for MockRateLimitService {}

    // --- Helper ---

    fn make_parts_with_extensions() -> Parts {
        let request = Request::builder().body(()).unwrap();
        let (mut parts, _) = request.into_parts();

        parts
            .extensions
            .insert(Arc::new(MockTaskQueue) as Arc<dyn TaskQueue>);
        parts
            .extensions
            .insert(Arc::new(Settings::default()) as Arc<Settings>);
        parts
            .extensions
            .insert(Arc::new(MockTaskRepo) as Arc<dyn TaskRepository>);
        parts
            .extensions
            .insert(Arc::new(MockRateLimitService) as Arc<dyn RateLimitingService>);
        parts.extensions.insert(AuthState::new(
            create_test_db_pool(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
        ));

        parts
    }

    // ========== Tests ==========

    #[tokio::test]
    async fn test_app_deps_extracts_all_fields() {
        let mut parts = make_parts_with_extensions();
        let result = AppDeps::from_request_parts(&mut parts, &()).await;
        assert!(result.is_ok(), "AppDeps should extract successfully");
    }

    #[tokio::test]
    async fn test_app_deps_missing_task_queue_returns_error() {
        let request = Request::builder().body(()).unwrap();
        let (mut parts, _) = request.into_parts();
        parts
            .extensions
            .insert(Arc::new(Settings::default()) as Arc<Settings>);
        parts
            .extensions
            .insert(Arc::new(MockTaskRepo) as Arc<dyn TaskRepository>);
        parts
            .extensions
            .insert(Arc::new(MockRateLimitService) as Arc<dyn RateLimitingService>);
        parts.extensions.insert(AuthState::new(
            create_test_db_pool(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
        ));

        let result = AppDeps::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_app_deps_missing_settings_returns_error() {
        let request = Request::builder().body(()).unwrap();
        let (mut parts, _) = request.into_parts();
        parts
            .extensions
            .insert(Arc::new(MockTaskQueue) as Arc<dyn TaskQueue>);
        parts
            .extensions
            .insert(Arc::new(MockTaskRepo) as Arc<dyn TaskRepository>);
        parts
            .extensions
            .insert(Arc::new(MockRateLimitService) as Arc<dyn RateLimitingService>);
        parts.extensions.insert(AuthState::new(
            create_test_db_pool(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
        ));

        let result = AppDeps::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_app_deps_missing_task_repo_returns_error() {
        let request = Request::builder().body(()).unwrap();
        let (mut parts, _) = request.into_parts();
        parts
            .extensions
            .insert(Arc::new(MockTaskQueue) as Arc<dyn TaskQueue>);
        parts
            .extensions
            .insert(Arc::new(Settings::default()) as Arc<Settings>);
        parts
            .extensions
            .insert(Arc::new(MockRateLimitService) as Arc<dyn RateLimitingService>);
        parts.extensions.insert(AuthState::new(
            create_test_db_pool(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
        ));

        let result = AppDeps::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_app_deps_missing_rate_limit_returns_error() {
        let request = Request::builder().body(()).unwrap();
        let (mut parts, _) = request.into_parts();
        parts
            .extensions
            .insert(Arc::new(MockTaskQueue) as Arc<dyn TaskQueue>);
        parts
            .extensions
            .insert(Arc::new(Settings::default()) as Arc<Settings>);
        parts
            .extensions
            .insert(Arc::new(MockTaskRepo) as Arc<dyn TaskRepository>);
        parts.extensions.insert(AuthState::new(
            create_test_db_pool(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
        ));

        let result = AppDeps::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_app_deps_missing_auth_state_returns_error() {
        let request = Request::builder().body(()).unwrap();
        let (mut parts, _) = request.into_parts();
        parts
            .extensions
            .insert(Arc::new(MockTaskQueue) as Arc<dyn TaskQueue>);
        parts
            .extensions
            .insert(Arc::new(Settings::default()) as Arc<Settings>);
        parts
            .extensions
            .insert(Arc::new(MockTaskRepo) as Arc<dyn TaskRepository>);
        parts
            .extensions
            .insert(Arc::new(MockRateLimitService) as Arc<dyn RateLimitingService>);

        let result = AppDeps::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_app_deps_empty_extensions_returns_error() {
        let request = Request::builder().body(()).unwrap();
        let (mut parts, _) = request.into_parts();

        let result = AppDeps::from_request_parts(&mut parts, &()).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_app_deps_clone() {
        let deps = AppDeps {
            queue: Arc::new(MockTaskQueue),
            settings: Arc::new(Settings::default()),
            task_repo: Arc::new(MockTaskRepo),
            rate_limit: Arc::new(MockRateLimitService),
            auth: AuthState::new(
                create_test_db_pool(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                ApiKeyScope::default(),
            ),
        };
        let cloned = deps.clone();
        assert_eq!(cloned.auth.team_id, deps.auth.team_id);
        assert_eq!(cloned.auth.api_key_id, deps.auth.api_key_id);
    }

    #[test]
    fn test_app_deps_struct_fields_accessible() {
        let deps = AppDeps {
            queue: Arc::new(MockTaskQueue),
            settings: Arc::new(Settings::default()),
            task_repo: Arc::new(MockTaskRepo),
            rate_limit: Arc::new(MockRateLimitService),
            auth: AuthState::new(
                create_test_db_pool(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                ApiKeyScope::default(),
            ),
        };
        // Verify all 5 fields are accessible
        let _ = &deps.queue;
        let _ = &deps.settings;
        let _ = &deps.task_repo;
        let _ = &deps.rate_limit;
        let _ = &deps.auth;
    }
}
