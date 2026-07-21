// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

use crate::application::dto::webhook_request::{
    CreateWebhookRequest, WebhookListResponse, WebhookResponse,
};
use crate::config::settings::Settings;
use crate::domain::models::Webhook;
use crate::domain::repositories::webhook_repository::WebhookRepository;
use crate::domain::services::rate_limiting_service::RateLimitingService;
// 架构 MEDIUM-2：domain 层提供 `verify_webhook_signature_from_parts`（timestamp 解析 +
// HMAC 验证 + 时间戳窗口检查），presentation 层仅负责 HTTP header → &str 提取。
// 之前的 `verify_webhook_signature_from_headers` 跨层混合 HTTP 解析 + 域逻辑，违反 SRP。
use crate::domain::services::webhook_service::{
    verify_webhook_signature_from_parts, WEBHOOK_AUTH_FAILED,
};
use crate::domain::use_cases::create_webhook::CreateWebhookUseCase;
// 架构 MEDIUM-2：与 crawl/scrape handler 统一使用 `presentation::helpers::ssrf::validate_url`。
// `engines::validators::validate_url` 仅是 re-export（见 engines/validators.rs line 39-42），
// 直接使用源模块避免读者跳两次 import 才找到实现。
use crate::presentation::helpers::ssrf::validate_url;
use crate::presentation::errors::CrawlRsError;
use crate::presentation::handlers::response_builder::ApiResponse;
use crate::presentation::helpers::rate_limit_helper::check_rate_limit_as_app_error;
use crate::presentation::middleware::auth_middleware::AuthState;
use axum::body::Bytes;
use axum::http::{HeaderMap, StatusCode};
use axum::{Extension, Json};
use std::sync::Arc;

/// Webhook 签名验证相关的 HTTP 头名称（HTTP 协议层常量）
const SIGNATURE_HEADER: &str = "X-Crawlrs-Signature";
const TIMESTAMP_HEADER: &str = "X-Crawlrs-Timestamp";

/// 构造统一的 webhook 认证失败错误
///
/// 架构 MEDIUM-2：错误消息常量 `WEBHOOK_AUTH_FAILED` 已迁移至
/// `domain::services::webhook_service`，本 helper 仅负责将 `&'static str`
/// 映射为 `CrawlRsError::Authentication`（presentation 层错误类型）。
fn auth_error() -> CrawlRsError {
    CrawlRsError::Authentication(WEBHOOK_AUTH_FAILED.to_string())
}

/// 从请求头中提取 signature + timestamp 字符串并委托给 domain 层验证
///
/// 架构 MEDIUM-2：本函数仅承担 **HTTP 协议层**职责（HeaderMap → &str 提取），
/// 不再混合 timestamp 解析 + HMAC 验证等域逻辑。
/// 失败时返回统一的 `WEBHOOK_AUTH_FAILED` 错误消息（来自 domain 层），避免泄露具体失败阶段。
fn verify_webhook_signature_from_headers(
    headers: &HeaderMap,
    secret: &str,
    body: &[u8],
) -> Result<(), CrawlRsError> {
    let signature = headers
        .get(SIGNATURE_HEADER)
        .and_then(|h| h.to_str().ok())
        .ok_or_else(auth_error)?;

    let timestamp_str = headers
        .get(TIMESTAMP_HEADER)
        .and_then(|h| h.to_str().ok())
        .ok_or_else(auth_error)?;

    // 委托给 domain 层：timestamp 解析 + HMAC 验证 + 时间戳窗口检查
    verify_webhook_signature_from_parts(secret, signature, timestamp_str, body)
        .map_err(|_| auth_error())
}

pub async fn create_webhook<R: WebhookRepository>(
    Extension(repo): Extension<Arc<R>>,
    Extension(rate_limiting_service): Extension<Arc<dyn RateLimitingService>>,
    Extension(auth_state): Extension<AuthState>,
    Extension(settings): Extension<Arc<Settings>>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<Webhook>), CrawlRsError> {
    // 架构 MEDIUM-3：限流检查必须在最早阶段执行（与 search/scrape/crawl handler 一致），
    // 防止攻击者通过大量无效签名请求耗服（每个请求都做 HMAC 计算会消耗 CPU）。
    // 限流命中时直接返回 429，避免不必要的 HMAC + JSON 解析 + SSRF 验证。
    // 性能 LOW-1：直接传 `Uuid`（实现 Display），由 helper 内部按需 to_string，
    // 消除 handler 中的中间变量分配。
    check_rate_limit_as_app_error(rate_limiting_service.as_ref(), auth_state.api_key_id, "/v1/webhooks").await?;

    // 1. 验证 webhook 签名 (HMAC-SHA256 + 时间戳窗口，防止重放攻击)
    verify_webhook_signature_from_headers(&headers, settings.webhook.secret(), &body).map_err(
        |e| {
            log::warn!(
                "Webhook signature verification failed team_id={} api_key_id={}",
                auth_state.team_id,
                auth_state.api_key_id
            );
            e
        },
    )?;

    // 2. 解析 JSON payload (签名验证通过后再解析)
    let payload: CreateWebhookRequest =
        serde_json::from_slice(&body).map_err(|e| {
            CrawlRsError::Validation(format!("invalid JSON payload: {}", e))
        })?;

    let team_id = auth_state.team_id;

    // 3. Validate webhook URL for SSRF protection
    match validate_url(&payload.url).await {
        Ok(_) => {
            // 不记录 resolved_ips 到日志，避免泄露内部网络拓扑
            log::debug!(
                "Webhook URL passed SSRF validation url={} team_id={}",
                payload.url,
                team_id
            );
        }
        Err(e) => {
            log::warn!("SSRF attack attempt blocked via webhook URL url={} team_id={} api_key_id={} error={}", payload.url, team_id, auth_state.api_key_id, e);
            return Err(CrawlRsError::Validation(
                "Invalid webhook URL: potential security risk detected".to_string(),
            ));
        }
    }

    let use_case = CreateWebhookUseCase::new(repo);
    let webhook = use_case.execute(team_id, payload.url).await?;
    Ok((StatusCode::CREATED, Json(webhook)))
}

/// 列出团队的 Webhooks
///
/// 只读 GET 操作，已通过 auth_middleware 验证身份，无需 HMAC 签名验证。
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
    use hmac::{Hmac, KeyInit, Mac};
    use sha2::Sha256;
    use std::sync::Mutex;
    use uuid::Uuid;

    type HmacSha256 = Hmac<Sha256>;

    /// 测试用 webhook 签名密钥
    ///
    /// 安全 MEDIUM-1：测试 secret 通过 `validate_security` 验证，确保满足生产安全要求
    /// （长度 >= 32，不在弱密钥列表中）。值明显标记为测试专用，避免与生产 secret 混淆。
    const TEST_WEBHOOK_SECRET: &str = "test-webhook-secret-key-32-chars-long!!";

    /// 构造带已知 webhook secret 的 Settings（其他字段使用默认值）
    ///
    /// 安全 MEDIUM-1：调用 `validate_security` 验证 secret 满足生产安全要求，
    /// 防止测试中使用弱密钥导致安全验证逻辑被绕过。
    fn make_test_settings_with_secret(secret: &str) -> Arc<Settings> {
        let mut settings = Settings::default();
        settings.webhook.secret = secret.to_string();
        // 验证 secret 满足生产安全要求（非空、非弱密钥、长度 >= 32）
        crate::config::settings::validate_security(&settings)
            .expect("test webhook secret must satisfy validate_security requirements");
        Arc::new(settings)
    }

    /// 使用与生产相同的算法生成测试签名：HMAC-SHA256("{timestamp}.{payload}")
    fn make_test_signature(secret: &str, payload: &str, timestamp: i64) -> String {
        let message = format!("{}.{}", timestamp, payload);
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC key error");
        mac.update(message.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    /// 构造包含有效签名 + 时间戳的 HeaderMap
    fn make_signed_headers(secret: &str, payload: &str) -> HeaderMap {
        let timestamp = Utc::now().timestamp();
        let signature = make_test_signature(secret, payload, timestamp);
        let mut headers = HeaderMap::new();
        headers.insert(
            SIGNATURE_HEADER,
            axum::http::HeaderValue::from_str(&signature).expect("signature is valid header"),
        );
        headers.insert(
            TIMESTAMP_HEADER,
            axum::http::HeaderValue::from_str(&timestamp.to_string())
                .expect("timestamp is valid header"),
        );
        headers
    }

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
        let payload_bytes = serde_json::to_vec(&payload).expect("serialize payload");
        let settings = make_test_settings_with_secret(TEST_WEBHOOK_SECRET);
        let headers = make_signed_headers(TEST_WEBHOOK_SECRET, std::str::from_utf8(&payload_bytes).expect("utf8"));

        let result = create_webhook(
            Extension(repo),
            Extension(rate_limit as Arc<dyn RateLimitingService>),
            Extension(auth),
            Extension(settings),
            headers,
            Bytes::from(payload_bytes),
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
        let payload_bytes = serde_json::to_vec(&payload).expect("serialize payload");
        let settings = make_test_settings_with_secret(TEST_WEBHOOK_SECRET);
        let headers = make_signed_headers(TEST_WEBHOOK_SECRET, std::str::from_utf8(&payload_bytes).expect("utf8"));

        let result = create_webhook(
            Extension(repo),
            Extension(rate_limit as Arc<dyn RateLimitingService>),
            Extension(auth),
            Extension(settings),
            headers,
            Bytes::from(payload_bytes),
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
        let payload_bytes = serde_json::to_vec(&payload).expect("serialize payload");
        let settings = make_test_settings_with_secret(TEST_WEBHOOK_SECRET);
        let headers = make_signed_headers(TEST_WEBHOOK_SECRET, std::str::from_utf8(&payload_bytes).expect("utf8"));

        let result = create_webhook(
            Extension(repo),
            Extension(rate_limit as Arc<dyn RateLimitingService>),
            Extension(auth),
            Extension(settings),
            headers,
            Bytes::from(payload_bytes),
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
        let payload_bytes = serde_json::to_vec(&payload).expect("serialize payload");
        let settings = make_test_settings_with_secret(TEST_WEBHOOK_SECRET);
        let headers = make_signed_headers(TEST_WEBHOOK_SECRET, std::str::from_utf8(&payload_bytes).expect("utf8"));

        let result = create_webhook(
            Extension(repo),
            Extension(rate_limit as Arc<dyn RateLimitingService>),
            Extension(auth),
            Extension(settings),
            headers,
            Bytes::from(payload_bytes),
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
        let payload_bytes = serde_json::to_vec(&payload).expect("serialize payload");
        let settings = make_test_settings_with_secret(TEST_WEBHOOK_SECRET);
        let headers = make_signed_headers(TEST_WEBHOOK_SECRET, std::str::from_utf8(&payload_bytes).expect("utf8"));

        let result = create_webhook(
            Extension(repo),
            Extension(rate_limit as Arc<dyn RateLimitingService>),
            Extension(auth),
            Extension(settings),
            headers,
            Bytes::from(payload_bytes),
        )
        .await;

        assert!(result.is_ok());
        let (status, webhook) = result.unwrap();
        assert_eq!(status, StatusCode::CREATED);
        assert_eq!(webhook.url, "https://example.com/webhook");
    }

    // ========== webhook signature verification failure tests ==========

    /// 辅助：构造带自定义签名的 HeaderMap
    fn make_headers_with_signature_and_timestamp(
        signature: &str,
        timestamp: i64,
    ) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            SIGNATURE_HEADER,
            axum::http::HeaderValue::from_str(signature).expect("signature is valid header"),
        );
        headers.insert(
            TIMESTAMP_HEADER,
            axum::http::HeaderValue::from_str(&timestamp.to_string())
                .expect("timestamp is valid header"),
        );
        headers
    }

    /// 缺少签名头 → 401 Authentication
    #[tokio::test]
    async fn test_create_webhook_missing_signature_header_returns_401() {
        let repo = Arc::new(MockWebhookRepository::new());
        let rate_limit = Arc::new(MockRateLimitingService::new_allowed());
        let auth = make_test_auth_state();
        let payload = CreateWebhookRequest {
            url: "https://example.com/webhook".to_string(),
        };
        let payload_bytes = serde_json::to_vec(&payload).expect("serialize payload");
        let settings = make_test_settings_with_secret(TEST_WEBHOOK_SECRET);

        // 仅包含 timestamp，缺少 signature
        let mut headers = HeaderMap::new();
        headers.insert(
            TIMESTAMP_HEADER,
            axum::http::HeaderValue::from_str(&Utc::now().timestamp().to_string())
                .expect("timestamp is valid header"),
        );

        let result = create_webhook(
            Extension(repo),
            Extension(rate_limit as Arc<dyn RateLimitingService>),
            Extension(auth),
            Extension(settings),
            headers,
            Bytes::from(payload_bytes),
        )
        .await;

        assert!(result.is_err(), "missing signature header should fail");
        match result.unwrap_err() {
            CrawlRsError::Authentication(msg) => {
                assert!(
                    msg.contains(WEBHOOK_AUTH_FAILED),
                    "expected unified auth failure message, got: {}",
                    msg
                );
            }
            other => panic!("expected CrawlRsError::Authentication, got {:?}", other),
        }
    }

    /// 缺少时间戳头 → 401 Authentication
    #[tokio::test]
    async fn test_create_webhook_missing_timestamp_header_returns_401() {
        let repo = Arc::new(MockWebhookRepository::new());
        let rate_limit = Arc::new(MockRateLimitingService::new_allowed());
        let auth = make_test_auth_state();
        let payload = CreateWebhookRequest {
            url: "https://example.com/webhook".to_string(),
        };
        let payload_bytes = serde_json::to_vec(&payload).expect("serialize payload");
        let settings = make_test_settings_with_secret(TEST_WEBHOOK_SECRET);

        // 仅包含 signature，缺少 timestamp
        let mut headers = HeaderMap::new();
        headers.insert(
            SIGNATURE_HEADER,
            axum::http::HeaderValue::from_static("deadbeef"),
        );

        let result = create_webhook(
            Extension(repo),
            Extension(rate_limit as Arc<dyn RateLimitingService>),
            Extension(auth),
            Extension(settings),
            headers,
            Bytes::from(payload_bytes),
        )
        .await;

        assert!(result.is_err(), "missing timestamp header should fail");
        match result.unwrap_err() {
            CrawlRsError::Authentication(msg) => {
                assert!(
                    msg.contains(WEBHOOK_AUTH_FAILED),
                    "expected unified auth failure message, got: {}",
                    msg
                );
            }
            other => panic!("expected CrawlRsError::Authentication, got {:?}", other),
        }
    }

    /// 时间戳格式无效 → 401 Authentication
    #[tokio::test]
    async fn test_create_webhook_invalid_timestamp_format_returns_401() {
        let repo = Arc::new(MockWebhookRepository::new());
        let rate_limit = Arc::new(MockRateLimitingService::new_allowed());
        let auth = make_test_auth_state();
        let payload = CreateWebhookRequest {
            url: "https://example.com/webhook".to_string(),
        };
        let payload_bytes = serde_json::to_vec(&payload).expect("serialize payload");
        let settings = make_test_settings_with_secret(TEST_WEBHOOK_SECRET);

        let mut headers = HeaderMap::new();
        headers.insert(
            SIGNATURE_HEADER,
            axum::http::HeaderValue::from_static("deadbeef"),
        );
        headers.insert(
            TIMESTAMP_HEADER,
            axum::http::HeaderValue::from_static("not-a-number"),
        );

        let result = create_webhook(
            Extension(repo),
            Extension(rate_limit as Arc<dyn RateLimitingService>),
            Extension(auth),
            Extension(settings),
            headers,
            Bytes::from(payload_bytes),
        )
        .await;

        assert!(result.is_err(), "invalid timestamp format should fail");
        match result.unwrap_err() {
            CrawlRsError::Authentication(msg) => {
                assert!(
                    msg.contains(WEBHOOK_AUTH_FAILED),
                    "expected unified auth failure message, got: {}",
                    msg
                );
            }
            other => panic!("expected CrawlRsError::Authentication, got {:?}", other),
        }
    }

    /// 签名错误 → 401 Authentication
    #[tokio::test]
    async fn test_create_webhook_wrong_signature_returns_401() {
        let repo = Arc::new(MockWebhookRepository::new());
        let rate_limit = Arc::new(MockRateLimitingService::new_allowed());
        let auth = make_test_auth_state();
        let payload = CreateWebhookRequest {
            url: "https://example.com/webhook".to_string(),
        };
        let payload_bytes = serde_json::to_vec(&payload).expect("serialize payload");
        let settings = make_test_settings_with_secret(TEST_WEBHOOK_SECRET);

        // 使用错误的签名（不是基于真实 secret 计算的）
        let headers = make_headers_with_signature_and_timestamp(
            "deadbeefcafebabe",
            Utc::now().timestamp(),
        );

        let result = create_webhook(
            Extension(repo),
            Extension(rate_limit as Arc<dyn RateLimitingService>),
            Extension(auth),
            Extension(settings),
            headers,
            Bytes::from(payload_bytes),
        )
        .await;

        assert!(result.is_err(), "wrong signature should fail");
        match result.unwrap_err() {
            CrawlRsError::Authentication(msg) => {
                assert!(
                    msg.contains(WEBHOOK_AUTH_FAILED),
                    "expected unified auth failure message, got: {}",
                    msg
                );
            }
            other => panic!("expected CrawlRsError::Authentication, got {:?}", other),
        }
    }

    /// 时间戳过期（超出 5 分钟窗口）→ 401 Authentication
    #[tokio::test]
    async fn test_create_webhook_expired_timestamp_returns_401() {
        let repo = Arc::new(MockWebhookRepository::new());
        let rate_limit = Arc::new(MockRateLimitingService::new_allowed());
        let auth = make_test_auth_state();
        let payload = CreateWebhookRequest {
            url: "https://example.com/webhook".to_string(),
        };
        let payload_bytes = serde_json::to_vec(&payload).expect("serialize payload");
        let settings = make_test_settings_with_secret(TEST_WEBHOOK_SECRET);

        // 时间戳为 10 分钟前（超出 MAX_TIMESTAMP_AGE = 300 秒窗口）
        let expired_timestamp = Utc::now().timestamp() - 600;
        let payload_str = std::str::from_utf8(&payload_bytes).expect("utf8");
        let signature = make_test_signature(TEST_WEBHOOK_SECRET, payload_str, expired_timestamp);
        let headers = make_headers_with_signature_and_timestamp(&signature, expired_timestamp);

        let result = create_webhook(
            Extension(repo),
            Extension(rate_limit as Arc<dyn RateLimitingService>),
            Extension(auth),
            Extension(settings),
            headers,
            Bytes::from(payload_bytes),
        )
        .await;

        assert!(result.is_err(), "expired timestamp should fail");
        match result.unwrap_err() {
            CrawlRsError::Authentication(msg) => {
                assert!(
                    msg.contains(WEBHOOK_AUTH_FAILED),
                    "expected unified auth failure message, got: {}",
                    msg
                );
            }
            other => panic!("expected CrawlRsError::Authentication, got {:?}", other),
        }
    }

    /// 使用不同 secret 计算的签名 → 401 Authentication
    #[tokio::test]
    async fn test_create_webhook_wrong_secret_returns_401() {
        let repo = Arc::new(MockWebhookRepository::new());
        let rate_limit = Arc::new(MockRateLimitingService::new_allowed());
        let auth = make_test_auth_state();
        let payload = CreateWebhookRequest {
            url: "https://example.com/webhook".to_string(),
        };
        let payload_bytes = serde_json::to_vec(&payload).expect("serialize payload");

        // 服务器使用一个 secret，客户端用另一个 secret 签名
        let server_secret = "server-side-secret-32-chars-long-xxxx";
        let client_secret = "client-side-secret-32-chars-long-xxxx";
        let settings = make_test_settings_with_secret(server_secret);
        let payload_str = std::str::from_utf8(&payload_bytes).expect("utf8");
        let timestamp = Utc::now().timestamp();
        let signature = make_test_signature(client_secret, payload_str, timestamp);
        let headers = make_headers_with_signature_and_timestamp(&signature, timestamp);

        let result = create_webhook(
            Extension(repo),
            Extension(rate_limit as Arc<dyn RateLimitingService>),
            Extension(auth),
            Extension(settings),
            headers,
            Bytes::from(payload_bytes),
        )
        .await;

        assert!(result.is_err(), "wrong secret signature should fail");
        match result.unwrap_err() {
            CrawlRsError::Authentication(msg) => {
                assert!(
                    msg.contains(WEBHOOK_AUTH_FAILED),
                    "expected unified auth failure message, got: {}",
                    msg
                );
            }
            other => panic!("expected CrawlRsError::Authentication, got {:?}", other),
        }
    }

    /// Body 不是有效 UTF-8 → 401 Authentication
    #[tokio::test]
    async fn test_create_webhook_invalid_utf8_body_returns_401() {
        let repo = Arc::new(MockWebhookRepository::new());
        let rate_limit = Arc::new(MockRateLimitingService::new_allowed());
        let auth = make_test_auth_state();
        let settings = make_test_settings_with_secret(TEST_WEBHOOK_SECRET);

        // 包含无效 UTF-8 字节的 body
        let invalid_utf8_bytes: Vec<u8> = vec![0xff, 0xfe, 0xfd, 0x00];
        let headers = make_signed_headers(
            TEST_WEBHOOK_SECRET,
            std::str::from_utf8(&invalid_utf8_bytes).unwrap_or(""),
        );

        let result = create_webhook(
            Extension(repo),
            Extension(rate_limit as Arc<dyn RateLimitingService>),
            Extension(auth),
            Extension(settings),
            headers,
            Bytes::from(invalid_utf8_bytes),
        )
        .await;

        assert!(result.is_err(), "invalid UTF-8 body should fail");
        match result.unwrap_err() {
            CrawlRsError::Authentication(msg) => {
                assert!(
                    msg.contains(WEBHOOK_AUTH_FAILED),
                    "expected unified auth failure message, got: {}",
                    msg
                );
            }
            other => panic!("expected CrawlRsError::Authentication, got {:?}", other),
        }
    }
}
