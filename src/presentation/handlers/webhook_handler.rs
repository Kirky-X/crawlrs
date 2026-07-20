// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::application::dto::webhook_request::{
    CreateWebhookRequest, WebhookListResponse, WebhookResponse,
};
use crate::domain::models::Webhook;
use crate::domain::repositories::webhook_repository::WebhookRepository;
use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::domain::use_cases::create_webhook::CreateWebhookUseCase;
use crate::engines::validators::validate_url;
use crate::presentation::errors::CrawlRsError;
use crate::presentation::handlers::response_builder::ApiResponse;
use crate::presentation::helpers::rate_limit_helper::check_rate_limit_as_app_error;
use crate::presentation::middleware::auth_middleware::AuthState;
use axum::{http::StatusCode, Extension, Json};
use std::sync::Arc;

pub async fn create_webhook<R: WebhookRepository>(
    Extension(repo): Extension<Arc<R>>,
    Extension(rate_limiting_service): Extension<Arc<dyn RateLimitingService>>,
    Extension(auth_state): Extension<AuthState>,
    Json(payload): Json<CreateWebhookRequest>,
) -> Result<(StatusCode, Json<Webhook>), CrawlRsError> {
    let team_id = auth_state.team_id;
    let api_key = auth_state.api_key_id.to_string();

    // Validate webhook URL for SSRF protection
    match validate_url(&payload.url).await {
        Ok(validated) => {
            log::debug!(
                "Webhook URL passed SSRF validation url={} team_id={} resolved_ips={:?}",
                payload.url,
                team_id,
                validated.resolved_ips
            );
        }
        Err(e) => {
            log::warn!("SSRF attack attempt blocked via webhook URL url={} team_id={} api_key_id={} error={}", payload.url, team_id, auth_state.api_key_id, e);
            return Err(CrawlRsError::Validation(
                "Invalid webhook URL: potential security risk detected".to_string(),
            ));
        }
    }

    // 1. 检查限流
    check_rate_limit_as_app_error(rate_limiting_service.as_ref(), &api_key, "/v1/webhooks").await?;

    let use_case = CreateWebhookUseCase::new(repo);
    let webhook = use_case.execute(team_id, payload.url).await?;
    Ok((StatusCode::CREATED, Json(webhook)))
}

/// 列出团队的 Webhooks
pub async fn list_webhooks<R: WebhookRepository>(
    Extension(repo): Extension<Arc<R>>,
    Extension(auth_state): Extension<AuthState>,
) -> Result<Json<ApiResponse<WebhookListResponse>>, CrawlRsError> {
    let team_id = auth_state.team_id;
    let webhooks = repo.find_by_team_id(team_id).await?;
    let webhook_responses: Vec<WebhookResponse> = webhooks
        .into_iter()
        .map(|w| WebhookResponse {
            id: w.id,
            team_id: w.team_id,
            url: w.url,
            created_at: w.created_at,
            is_active: true,
            secret: None,
        })
        .collect();
    let total = webhook_responses.len();
    Ok(Json(ApiResponse::success(WebhookListResponse {
        webhooks: webhook_responses,
        total,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::auth::ApiKeyScope;
    use crate::domain::repositories::task_repository::RepositoryError;
    use crate::domain::services::rate_limiting_service::{
        BacklogService, ConcurrencyConfig, ConcurrencyControlService, ConcurrencyResult,
        QuotaService, RateLimitConfig, RateLimitResult, RateLimitService, RateLimitingError,
    };
    use async_trait::async_trait;
    use chrono::Utc;
    use dbnexus::DbPool;
    use std::sync::Mutex;
    use uuid::Uuid;

    // ========== CreateWebhookRequest tests ==========

    #[test]
    fn test_create_webhook_request_valid() {
        let json = r#"{"url":"https://example.com/webhook"}"#;
        let req: CreateWebhookRequest = serde_json::from_str(json).unwrap();
        assert_eq!(req.url, "https://example.com/webhook");
    }

    #[test]
    fn test_create_webhook_request_rejects_unknown_fields() {
        let json = r#"{"url":"https://example.com","extra":"field"}"#;
        let result: Result<CreateWebhookRequest, _> = serde_json::from_str(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_create_webhook_request_serialization() {
        let req = CreateWebhookRequest {
            url: "https://example.com/hook".to_string(),
        };
        let json = serde_json::to_string(&req).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["url"], "https://example.com/hook");
    }

    #[test]
    fn test_create_webhook_request_round_trip() {
        let original = CreateWebhookRequest {
            url: "https://my.webhook.site/abc123".to_string(),
        };
        let json = serde_json::to_string(&original).unwrap();
        let deserialized: CreateWebhookRequest = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.url, original.url);
    }

    // ========== Webhook to WebhookResponse mapping ==========

    #[test]
    fn test_webhook_to_response_mapping() {
        let webhook_id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let webhook = Webhook {
            id: webhook_id,
            team_id,
            url: "https://example.com/hook".to_string(),
            created_at: Utc::now(),
        };
        let response = WebhookResponse {
            id: webhook.id,
            team_id: webhook.team_id,
            url: webhook.url.clone(),
            created_at: webhook.created_at,
            is_active: true,
            secret: None,
        };
        assert_eq!(response.id, webhook_id);
        assert_eq!(response.team_id, team_id);
        assert_eq!(response.url, "https://example.com/hook");
        assert!(response.is_active);
        assert!(response.secret.is_none());
    }

    #[test]
    fn test_webhook_response_serialization() {
        let response = WebhookResponse {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            url: "https://example.com/hook".to_string(),
            created_at: Utc::now(),
            is_active: true,
            secret: Some("secret123".to_string()),
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["url"], "https://example.com/hook");
        assert_eq!(parsed["is_active"], true);
        assert_eq!(parsed["secret"], "secret123");
    }

    #[test]
    fn test_webhook_response_secret_none_serialized() {
        let response = WebhookResponse {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            url: "https://example.com/hook".to_string(),
            created_at: Utc::now(),
            is_active: false,
            secret: None,
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["is_active"], false);
        assert!(parsed["secret"].is_null());
    }

    // ========== WebhookListResponse serialization ==========

    #[test]
    fn test_webhook_list_response_empty() {
        let response = WebhookListResponse {
            webhooks: vec![],
            total: 0,
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["total"], 0);
        assert_eq!(parsed["webhooks"], serde_json::Value::Array(vec![]));
    }

    #[test]
    fn test_webhook_list_response_with_items() {
        let webhook1 = WebhookResponse {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            url: "https://hook1.example.com".to_string(),
            created_at: Utc::now(),
            is_active: true,
            secret: None,
        };
        let webhook2 = WebhookResponse {
            id: Uuid::new_v4(),
            team_id: Uuid::new_v4(),
            url: "https://hook2.example.com".to_string(),
            created_at: Utc::now(),
            is_active: false,
            secret: None,
        };
        let response = WebhookListResponse {
            webhooks: vec![webhook1, webhook2],
            total: 2,
        };
        let json = serde_json::to_string(&response).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed["total"], 2);
        assert_eq!(parsed["webhooks"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["webhooks"][0]["url"], "https://hook1.example.com");
        assert_eq!(parsed["webhooks"][1]["url"], "https://hook2.example.com");
    }

    #[test]
    fn test_webhook_list_response_total_matches_count() {
        let webhooks: Vec<WebhookResponse> = (0..5)
            .map(|_| WebhookResponse {
                id: Uuid::new_v4(),
                team_id: Uuid::new_v4(),
                url: "https://example.com".to_string(),
                created_at: Utc::now(),
                is_active: true,
                secret: None,
            })
            .collect();
        let count = webhooks.len();
        let response = WebhookListResponse {
            webhooks,
            total: count,
        };
        assert_eq!(response.total, 5);
        assert_eq!(response.webhooks.len(), 5);
    }

    // ========== Webhook model construction ==========

    #[test]
    fn test_webhook_new_constructor() {
        let id = Uuid::new_v4();
        let team_id = Uuid::new_v4();
        let webhook = Webhook::new(id, team_id, "https://example.com/hook".to_string());
        assert_eq!(webhook.id, id);
        assert_eq!(webhook.team_id, team_id);
        assert_eq!(webhook.url, "https://example.com/hook");
        assert!(webhook.created_at <= Utc::now());
    }

    // ========== Handler test infrastructure ==========

    /// Construct a lazy `DbPool` that does not connect to any database.
    ///
    /// `DbPool::try_from` is lazy: it builds the internal struct (including the
    /// permission policy cache) without opening a connection. The connection is
    /// only established on `get_session()`, which the webhook handlers never
    /// call — they only read `team_id` / `api_key_id` from `AuthState`.
    ///
    /// The `permission` feature variant of `try_from` internally calls
    /// `Handle::current().block_on(...)` to build the oxcache policy cache.
    /// Calling `block_on` from within a `#[tokio::test]` runtime panics with
    /// "Cannot start a runtime from within a runtime", so we construct the pool
    /// on a dedicated OS thread with its own runtime.
    fn make_test_db_pool() -> Arc<DbPool> {
        std::thread::scope(|s| {
            let handle = s.spawn(|| {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("failed to build tokio runtime for DbPool construction");
                let _guard = rt.enter();
                let url = std::env::var("TEST_DATABASE_URL")
                    .expect("TEST_DATABASE_URL must be set; no hardcoded fallback");
                rt.block_on(async {
                    let cfg = dbnexus::DbConfig {
                        url,
                        ..Default::default()
                    };
                    DbPool::with_config(cfg).await
                })
                .expect("failed to create DbPool for test")
            });
            Arc::new(handle.join().expect("DbPool construction thread panicked"))
        })
    }

    /// Build an `AuthState` suitable for handler unit tests.
    ///
    /// Uses a lazy (non-connecting) `DbPool` — see `make_test_db_pool`.
    fn make_test_auth_state() -> AuthState {
        AuthState::new(
            make_test_db_pool(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            ApiKeyScope::default(),
        )
    }

    // ========== MockWebhookRepository ==========

    /// Mock `WebhookRepository` whose `create` / `find_by_team_id` behaviour is
    /// configurable per test via the builder methods.
    struct MockWebhookRepository {
        /// When `Some`, `create` returns this error; otherwise it echoes the
        /// webhook back successfully.
        create_error: Mutex<Option<RepositoryError>>,
        /// When `Some`, `find_by_team_id` returns this stored result; otherwise
        /// it returns an empty list.
        find_by_team_id_result: Mutex<Option<Result<Vec<Webhook>, RepositoryError>>>,
    }

    impl MockWebhookRepository {
        fn new() -> Self {
            Self {
                create_error: Mutex::new(None),
                find_by_team_id_result: Mutex::new(None),
            }
        }

        fn with_create_error(err: RepositoryError) -> Self {
            Self {
                create_error: Mutex::new(Some(err)),
                find_by_team_id_result: Mutex::new(None),
            }
        }

        fn with_find_result(result: Result<Vec<Webhook>, RepositoryError>) -> Self {
            Self {
                create_error: Mutex::new(None),
                find_by_team_id_result: Mutex::new(Some(result)),
            }
        }
    }

    #[async_trait]
    impl WebhookRepository for MockWebhookRepository {
        async fn create(&self, webhook: &Webhook) -> Result<Webhook, RepositoryError> {
            if let Some(err) = self.create_error.lock().unwrap().take() {
                return Err(err);
            }
            Ok(webhook.clone())
        }

        async fn find_by_id(&self, _id: Uuid) -> Result<Option<Webhook>, RepositoryError> {
            Ok(None)
        }

        async fn find_by_team_id(&self, _team_id: Uuid) -> Result<Vec<Webhook>, RepositoryError> {
            match self.find_by_team_id_result.lock().unwrap().take() {
                Some(result) => result,
                None => Ok(vec![]),
            }
        }
    }

    // ========== MockRateLimitingService ==========

    /// Mock `RateLimitingService` with configurable `check_rate_limit` result.
    /// All other trait methods return benign defaults.
    struct MockRateLimitingService {
        rate_limit_result: RateLimitResult,
    }

    impl MockRateLimitingService {
        fn new_allowed() -> Self {
            Self {
                rate_limit_result: RateLimitResult::Allowed,
            }
        }

        fn new_denied(reason: &str) -> Self {
            Self {
                rate_limit_result: RateLimitResult::Denied {
                    reason: reason.to_string(),
                },
            }
        }
    }

    #[async_trait]
    impl RateLimitService for MockRateLimitingService {
        async fn check_rate_limit(
            &self,
            _api_key: &str,
            _endpoint: &str,
        ) -> Result<RateLimitResult, RateLimitingError> {
            Ok(self.rate_limit_result.clone())
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
    impl ConcurrencyControlService for MockRateLimitingService {
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
    impl BacklogService for MockRateLimitingService {
        async fn process_backlog_tasks(&self, _team_id: Uuid) -> Result<u32, RateLimitingError> {
            Ok(0)
        }
    }

    #[async_trait]
    impl QuotaService for MockRateLimitingService {
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
            Ok(0)
        }
    }

    // 组合 trait（向后兼容，空实现即可）
    impl RateLimitingService for MockRateLimitingService {}

    // ========== create_webhook handler tests ==========

    #[tokio::test]
    async fn test_create_webhook_success() {
        let repo = Arc::new(MockWebhookRepository::new());
        let rate_limit = Arc::new(MockRateLimitingService::new_allowed());
        let auth = make_test_auth_state();
        let team_id = auth.team_id;
        let payload = CreateWebhookRequest {
            url: "https://example.com/webhook".to_string(),
        };

        let result = create_webhook(
            Extension(repo),
            Extension(rate_limit as Arc<dyn RateLimitingService>),
            Extension(auth),
            Json(payload),
        )
        .await;

        assert!(result.is_ok(), "create_webhook should succeed");
        let (status, webhook) = result.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(webhook.team_id, team_id);
        assert_eq!(webhook.url, "https://example.com/webhook");
    }

    #[tokio::test]
    async fn test_create_webhook_ssrf_blocked() {
        let repo = Arc::new(MockWebhookRepository::new());
        let rate_limit = Arc::new(MockRateLimitingService::new_allowed());
        let auth = make_test_auth_state();
        let payload = CreateWebhookRequest {
            url: "http://127.0.0.1:8080".to_string(),
        };

        let result = create_webhook(
            Extension(repo),
            Extension(rate_limit as Arc<dyn RateLimitingService>),
            Extension(auth),
            Json(payload),
        )
        .await;

        assert!(result.is_err(), "SSRF URL should be rejected");
        match result.unwrap_err() {
            CrawlRsError::Validation(msg) => {
                assert!(
                    msg.contains("Invalid webhook URL"),
                    "expected SSRF validation message, got: {}",
                    msg
                );
            }
            other => panic!("expected CrawlRsError::Validation, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_create_webhook_rate_limit_exceeded() {
        let repo = Arc::new(MockWebhookRepository::new());
        let rate_limit = Arc::new(MockRateLimitingService::new_denied("too many requests"));
        let auth = make_test_auth_state();
        let payload = CreateWebhookRequest {
            url: "https://example.com/webhook".to_string(),
        };

        let result = create_webhook(
            Extension(repo),
            Extension(rate_limit as Arc<dyn RateLimitingService>),
            Extension(auth),
            Json(payload),
        )
        .await;

        assert!(result.is_err(), "rate-limited request should fail");
        match result.unwrap_err() {
            CrawlRsError::RateLimit(msg) => {
                assert!(
                    msg.contains("Rate limit exceeded"),
                    "expected rate limit message, got: {}",
                    msg
                );
            }
            other => panic!("expected CrawlRsError::RateLimit, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_create_webhook_repo_create_failure() {
        let repo = Arc::new(MockWebhookRepository::with_create_error(
            RepositoryError::Database(anyhow::anyhow!("repo down")),
        ));
        let rate_limit = Arc::new(MockRateLimitingService::new_allowed());
        let auth = make_test_auth_state();
        let payload = CreateWebhookRequest {
            url: "https://example.com/webhook".to_string(),
        };

        let result = create_webhook(
            Extension(repo),
            Extension(rate_limit as Arc<dyn RateLimitingService>),
            Extension(auth),
            Json(payload),
        )
        .await;

        assert!(result.is_err(), "repo failure should propagate");
        match result.unwrap_err() {
            CrawlRsError::Other(msg) => {
                assert!(
                    msg.contains("repo down"),
                    "expected repo failure message, got: {}",
                    msg
                );
            }
            other => panic!("expected CrawlRsError::Other, got {:?}", other),
        }
    }

    // ========== list_webhooks handler tests ==========

    #[tokio::test]
    async fn test_list_webhooks_empty() {
        let repo = Arc::new(MockWebhookRepository::new());
        let auth = make_test_auth_state();

        let result = list_webhooks::<MockWebhookRepository>(Extension(repo), Extension(auth)).await;

        assert!(
            result.is_ok(),
            "list_webhooks should succeed for empty list"
        );
        let response = result.unwrap();
        let data = response
            .data
            .as_ref()
            .expect("response data should be present");
        assert_eq!(data.total, 0);
        assert!(data.webhooks.is_empty());
    }

    #[tokio::test]
    async fn test_list_webhooks_with_items() {
        let team_id = Uuid::new_v4();
        let webhook1 = Webhook::new(
            Uuid::new_v4(),
            team_id,
            "https://hook1.example.com".to_string(),
        );
        let webhook2 = Webhook::new(
            Uuid::new_v4(),
            team_id,
            "https://hook2.example.com".to_string(),
        );
        let repo = Arc::new(MockWebhookRepository::with_find_result(Ok(vec![
            webhook1.clone(),
            webhook2.clone(),
        ])));
        let auth = make_test_auth_state();

        let result = list_webhooks::<MockWebhookRepository>(Extension(repo), Extension(auth)).await;

        assert!(result.is_ok(), "list_webhooks should succeed with items");
        let response = result.unwrap();
        let data = response
            .data
            .as_ref()
            .expect("response data should be present");
        assert_eq!(data.total, 2);
        assert_eq!(data.webhooks.len(), 2);
        assert_eq!(data.webhooks[0].url, "https://hook1.example.com");
        assert_eq!(data.webhooks[1].url, "https://hook2.example.com");
    }

    #[tokio::test]
    async fn test_list_webhooks_repo_failure() {
        let repo = Arc::new(MockWebhookRepository::with_find_result(Err(
            RepositoryError::Database(anyhow::anyhow!("find_by_team_id failed")),
        )));
        let auth = make_test_auth_state();

        let result = list_webhooks::<MockWebhookRepository>(Extension(repo), Extension(auth)).await;

        assert!(result.is_err(), "repo failure should propagate");
        match result.unwrap_err() {
            CrawlRsError::Other(msg) => {
                assert!(
                    msg.contains("find_by_team_id failed"),
                    "expected repo failure message, got: {}",
                    msg
                );
            }
            other => panic!("expected CrawlRsError::Other, got {:?}", other),
        }
    }

    // ========== Test logger for covering log::debug! format args ==========

    use log::{LevelFilter, Log, Metadata, Record};
    use std::sync::Once;

    static LOGGER_INIT: Once = Once::new();

    struct CapturingLogger;

    impl Log for CapturingLogger {
        fn enabled(&self, metadata: &Metadata) -> bool {
            metadata.level() <= log::Level::Debug
        }
        fn log(&self, _record: &Record) {}
        fn flush(&self) {}
    }

    /// Install a global debug-level logger so `log::debug!` format arguments
    /// (handler lines 34-37) are evaluated and counted as covered.
    fn ensure_debug_logger() {
        LOGGER_INIT.call_once(|| {
            static CAPTURING_LOGGER: CapturingLogger = CapturingLogger;
            let _ = log::set_logger(&CAPTURING_LOGGER);
            log::set_max_level(LevelFilter::Debug);
        });
    }

    #[tokio::test]
    async fn test_create_webhook_debug_log_evaluated() {
        // With debug logging enabled, the log::debug! format args on lines
        // 34-37 are evaluated (even though CapturingLogger discards them).
        ensure_debug_logger();
        let repo = Arc::new(MockWebhookRepository::new());
        let rate_limit = Arc::new(MockRateLimitingService::new_allowed());
        let auth = make_test_auth_state();
        let payload = CreateWebhookRequest {
            url: "https://example.com/webhook".to_string(),
        };

        let result = create_webhook(
            Extension(repo),
            Extension(rate_limit as Arc<dyn RateLimitingService>),
            Extension(auth),
            Json(payload),
        )
        .await;

        assert!(result.is_ok());
        let (status, webhook) = result.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(webhook.url, "https://example.com/webhook");
    }
}
