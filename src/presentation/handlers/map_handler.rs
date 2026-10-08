// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! `POST /v1/map` handler（F11：sitemap 流式一等请求源；feature `sitemap` 门控）
//!
//! 链路：限流 → SSRF 验证（`payload.url` 即抓取目标 origin）→
//! [`MapUseCase::execute_streaming`] 流式拉取解析 → [`finalize_links`]
//! 聚合（去重/include/exclude/limit，语义与 `MapUseCase::execute` 一致）。

use axum::{
    extract::{Extension, Json},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use fluent_bundle::FluentValue;
use log::error;
use std::sync::Arc;

use crate::application::dto::map_request::{MapRequestDto, MapResponseDto};
use crate::application::use_cases::map_use_case::{
    finalize_links, MapError, MapUseCase, SitemapStreamOptions,
};
use crate::config::settings::Settings;
use crate::domain::services::rate_limiting_service::RateLimitingService;
use crate::i18n::{I18nBundle, Locale};
use crate::presentation::handlers::check_ssrf_url;
use crate::presentation::handlers::response_builder::{
    error_response, error_response_with_code, json_rejection_response, success_response,
};
use crate::presentation::helpers::rate_limit_helper::check_rate_limit;
use crate::presentation::middleware::auth_middleware::AuthState;

/// 处理 map 请求：抓取 `{origin}/sitemap.xml`，流式解析并返回去重后的链接
pub async fn map(
    Extension(map_use_case): Extension<Arc<MapUseCase>>,
    Extension(settings): Extension<Arc<Settings>>,
    Extension(rate_limiting_service): Extension<Arc<dyn RateLimitingService>>,
    Extension(auth_state): Extension<AuthState>,
    Extension(locale): Extension<Locale>,
    Extension(bundle): Extension<Arc<I18nBundle>>,
    payload: Result<Json<MapRequestDto>, axum::extract::rejection::JsonRejection>,
) -> impl IntoResponse {
    // 提取器级拒绝（非法 JSON / 字段缺失）映射为统一包封
    let Json(payload) = match payload {
        Ok(parsed) => parsed,
        Err(ref rej) => return json_rejection_response(rej),
    };
    let team_id = auth_state.team_id;
    let api_key_id = auth_state.api_key_id;

    // 1. 检查限流（限流必须在 SSRF 之前，避免恶意请求触发异步 DNS 解析消耗资源）
    if let Err(response) =
        check_rate_limit(rate_limiting_service.as_ref(), api_key_id, "/v1/map").await
    {
        return response;
    }

    // 2. SSRF 防护 (CWE-918)：map 的抓取目标即 payload.url 的 origin
    if let Some(response) = check_ssrf_url(&payload.url, team_id, api_key_id).await {
        return response;
    }

    map_core(&map_use_case, &settings, &payload, &locale, &bundle).await
}

/// 聚合核心：流式消费 loc 通道并按 `execute` 语义聚合为响应
///
/// 独立于限流/SSRF 外壳，便于以 mock fetcher 直接验证响应语义
/// （HTTP 层冒烟：200 有序去重 / 404 空结果 / 502 目标不可达）。
async fn map_core(
    map_use_case: &MapUseCase,
    settings: &Settings,
    payload: &MapRequestDto,
    locale: &Locale,
    bundle: &Arc<I18nBundle>,
) -> Response {
    let options = SitemapStreamOptions {
        channel_capacity: settings.sitemap.channel_capacity,
        max_urls: settings.sitemap.max_urls,
        max_concurrent_sub_sitemaps: settings.sitemap.max_concurrent_sub_sitemaps,
    };
    match map_use_case.execute_streaming(payload, options).await {
        Ok(stream) => {
            let mut rx = stream.rx;
            let mut locs: Vec<String> = Vec::new();
            while let Some(loc) = rx.recv().await {
                locs.push(loc);
            }
            success_response(
                StatusCode::OK,
                MapResponseDto {
                    links: finalize_links(locs, payload),
                },
            )
        }
        Err(MapError::TargetUnreachable(msg)) => {
            log_map_target_unreachable(&payload.url, &msg);
            error_response_with_code(StatusCode::BAD_GATEWAY, "MAP_TARGET_UNREACHABLE", msg)
        }
        Err(MapError::SitemapStatus(status)) => {
            let msg = crate::i18n::t_with_args(
                locale,
                bundle,
                "api-map-sitemap-status",
                &[("status", FluentValue::from(status as i64))],
            );
            log_map_target_unreachable(&payload.url, &msg);
            error_response_with_code(StatusCode::BAD_GATEWAY, "MAP_TARGET_UNREACHABLE", msg)
        }
        Err(MapError::OriginUnparsable(url)) => {
            let msg = crate::i18n::t_with_args(
                locale,
                bundle,
                "api-map-origin-unparsable",
                &[("url", FluentValue::from(url.as_str()))],
            );
            error_response(StatusCode::BAD_REQUEST, msg)
        }
        Err(MapError::Internal(msg)) => error_response(StatusCode::BAD_REQUEST, msg),
    }
}

/// 运维日志（启动期全局束 locale）：map 目标不可达
fn log_map_target_unreachable(url: &str, message: &str) {
    error!(
        "{}",
        crate::i18n::tr_log_args(
            "map-target-unreachable",
            &[
                ("url", FluentValue::from(url)),
                ("message", FluentValue::from(message)),
            ],
        )
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::auth::ApiKeyScope;
    use crate::domain::services::rate_limiting_service::{
        BacklogService, ConcurrencyConfig, ConcurrencyControlService, ConcurrencyResult,
        QuotaService, RateLimitConfig, RateLimitResult, RateLimitService, RateLimitingError,
        RateLimitingService,
    };
    use async_trait::async_trait;
    use axum::body::Body;
    use axum::Router;
    use std::collections::HashMap;
    use tower::ServiceExt;

    // ========== Mock fetcher / rate limiter ==========

    struct MockFetcher {
        /// url → (status, body)
        responses: HashMap<String, (u16, String)>,
    }

    #[async_trait]
    impl crate::application::use_cases::map_use_case::SitemapFetcher for MockFetcher {
        async fn fetch(
            &self,
            url: &str,
        ) -> Result<crate::application::use_cases::map_use_case::SitemapFetch, MapError> {
            match self.responses.get(url) {
                Some((status, body)) => {
                    Ok(crate::application::use_cases::map_use_case::SitemapFetch {
                        status: *status,
                        body: body.clone(),
                    })
                }
                None => Err(MapError::TargetUnreachable(format!("no mock for {url}"))),
            }
        }
    }

    fn urlset(locs: &[&str]) -> String {
        let inner: String = locs
            .iter()
            .map(|l| format!("<url><loc>{l}</loc></url>"))
            .collect();
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">{inner}</urlset>"#
        )
    }

    fn make_map_use_case(responses: HashMap<String, (u16, String)>) -> Arc<MapUseCase> {
        Arc::new(MapUseCase::new(Arc::new(MockFetcher { responses })))
    }

    fn make_settings() -> Arc<Settings> {
        Arc::new(Settings::default())
    }

    fn test_bundle() -> Arc<I18nBundle> {
        let dir = format!("{}/locales", env!("CARGO_MANIFEST_DIR"));
        Arc::new(I18nBundle::load("en-US", &["en-US", "zh-CN"], &dir).unwrap())
    }

    fn test_locale() -> Locale {
        "en-US".parse().unwrap()
    }

    /// 恒放行的限流服务 mock（对齐 search_handler 测试惯例）
    struct AllowedRateLimiter;

    #[async_trait]
    impl RateLimitService for AllowedRateLimiter {
        async fn check_rate_limit(
            &self,
            _api_key: &str,
            _endpoint: &str,
        ) -> Result<RateLimitResult, RateLimitingError> {
            Ok(RateLimitResult::Allowed)
        }
        async fn get_team_rate_limit_config(
            &self,
            _team_id: crate::domain::id::Id,
        ) -> Result<RateLimitConfig, RateLimitingError> {
            Ok(RateLimitConfig::default())
        }
        async fn update_team_rate_limit_config(
            &self,
            _team_id: crate::domain::id::Id,
            _config: RateLimitConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
        async fn cleanup_expired_rate_limits(&self) -> Result<u64, RateLimitingError> {
            Ok(0)
        }
    }
    #[async_trait]
    impl ConcurrencyControlService for AllowedRateLimiter {
        async fn check_team_concurrency(
            &self,
            _team_id: crate::domain::id::Id,
            _task_id: crate::domain::id::Id,
        ) -> Result<ConcurrencyResult, RateLimitingError> {
            Ok(ConcurrencyResult::Allowed)
        }
        async fn release_team_concurrency_slot(
            &self,
            _team_id: crate::domain::id::Id,
            _task_id: crate::domain::id::Id,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
        async fn get_team_current_concurrency(
            &self,
            _team_id: crate::domain::id::Id,
        ) -> Result<u32, RateLimitingError> {
            Ok(0)
        }
        async fn get_team_concurrency_config(
            &self,
            _team_id: crate::domain::id::Id,
        ) -> Result<ConcurrencyConfig, RateLimitingError> {
            Ok(ConcurrencyConfig::default())
        }
        async fn update_team_concurrency_config(
            &self,
            _team_id: crate::domain::id::Id,
            _config: ConcurrencyConfig,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
    }
    #[async_trait]
    impl BacklogService for AllowedRateLimiter {
        async fn process_backlog_tasks(
            &self,
            _team_id: crate::domain::id::Id,
        ) -> Result<u32, RateLimitingError> {
            Ok(0)
        }
    }
    #[async_trait]
    impl QuotaService for AllowedRateLimiter {
        async fn check_and_deduct_quota(
            &self,
            _team_id: crate::domain::id::Id,
            _amount: i64,
            _transaction_type: crate::domain::models::CreditsTransactionType,
            _description: String,
            _reference_id: Option<crate::domain::id::Id>,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
        async fn get_quota_balance(
            &self,
            _team_id: crate::domain::id::Id,
        ) -> Result<i64, RateLimitingError> {
            Ok(0)
        }
        async fn refund_quota(
            &self,
            _team_id: crate::domain::id::Id,
            _amount: i64,
            _description: String,
            _reference_id: Option<crate::domain::id::Id>,
        ) -> Result<(), RateLimitingError> {
            Ok(())
        }
    }
    impl RateLimitingService for AllowedRateLimiter {}

    fn make_auth_state() -> AuthState {
        AuthState::new(
            crate::common::test_helpers::create_test_db_pool(),
            crate::domain::id::Id::generate(),
            crate::domain::id::Id::generate(),
            ApiKeyScope::full_access(),
        )
    }

    // ========== map_core（HTTP 层冒烟语义） ==========

    /// POST /v1/map 聚合语义：200 且 links 保序去重（默认 limit=1000）
    #[tokio::test]
    async fn map_core_returns_200_with_ordered_deduped_links() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let use_case = make_map_use_case(HashMap::from([(
            "https://a.com/sitemap.xml".to_string(),
            (
                200u16,
                urlset(&[
                    "https://a.com/1",
                    "https://a.com/2",
                    "https://a.com/1", // 重复
                ]),
            ),
        )]));
        let payload = MapRequestDto {
            url: "https://a.com".to_string(),
            include_patterns: None,
            exclude_patterns: None,
            limit: None,
        };
        let response = map_core(
            &use_case,
            &make_settings(),
            &payload,
            &test_locale(),
            &test_bundle(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["success"], true);
        assert_eq!(
            json["data"]["links"],
            serde_json::json!(["https://a.com/1", "https://a.com/2"]),
            "links must be order-preserving and deduplicated"
        );
    }

    /// sitemap 404 → 200 空 links（合法状态，非错误）
    #[tokio::test]
    async fn map_core_returns_200_empty_links_on_sitemap_404() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let use_case = make_map_use_case(HashMap::from([(
            "https://a.com/sitemap.xml".to_string(),
            (404u16, String::from("not found")),
        )]));
        let payload = MapRequestDto {
            url: "https://a.com".to_string(),
            include_patterns: None,
            exclude_patterns: None,
            limit: None,
        };
        let response = map_core(
            &use_case,
            &make_settings(),
            &payload,
            &test_locale(),
            &test_bundle(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let json: serde_json::Value = serde_json::from_slice(&body).expect("json");
        assert_eq!(json["data"]["links"], serde_json::json!([]));
    }

    /// 目标站 5xx → 502 MAP_TARGET_UNREACHABLE（错误显性化）
    #[tokio::test]
    async fn map_core_maps_5xx_to_502_target_unreachable() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let use_case = make_map_use_case(HashMap::from([(
            "https://a.com/sitemap.xml".to_string(),
            (500u16, String::from("boom")),
        )]));
        let payload = MapRequestDto {
            url: "https://a.com".to_string(),
            include_patterns: None,
            exclude_patterns: None,
            limit: None,
        };
        let response = map_core(
            &use_case,
            &make_settings(),
            &payload,
            &test_locale(),
            &test_bundle(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.contains("MAP_TARGET_UNREACHABLE"),
            "error code must be MAP_TARGET_UNREACHABLE, got: {text}"
        );
    }

    /// 无法解析 origin 的 URL → 400（DTO 校验之后用例层第二道防线）
    #[tokio::test]
    async fn map_core_maps_unparsable_url_to_400() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let use_case = make_map_use_case(HashMap::new());
        let payload = MapRequestDto {
            url: "not-a-url".to_string(),
            include_patterns: None,
            exclude_patterns: None,
            limit: None,
        };
        let response = map_core(
            &use_case,
            &make_settings(),
            &payload,
            &test_locale(),
            &test_bundle(),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    // ========== 路由装配（注册与依赖注入接线） ==========

    fn test_router(use_case: Arc<MapUseCase>) -> Router {
        crate::bootstrap::route_groups::map_routes::register_map_routes()
            .layer(Extension(use_case))
            .layer(Extension(make_settings()))
            .layer(Extension(
                Arc::new(AllowedRateLimiter) as Arc<dyn RateLimitingService>
            ))
            .layer(Extension(make_auth_state()))
            // handler 提取 Extension<Locale> / Extension<Arc<I18nBundle>>：
            // 与生产装配同序（bundle 在外层先行插入，middleware 协商后注入 locale）
            .layer(axum::middleware::from_fn(crate::i18n::i18n_middleware))
            .layer(Extension(test_bundle()))
    }

    /// POST /v1/map 已注册且接线完整：内部地址被 SSRF 防护拦截（400），
    /// 证明路由可达且限流/SSRF/用例依赖注入就位
    #[tokio::test]
    async fn map_route_registered_and_ssrf_blocks_internal_target() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let app = test_router(make_map_use_case(HashMap::new()));
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/v1/map")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"url":"http://localhost/secret"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "internal target must be blocked by SSRF protection"
        );
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let text = String::from_utf8_lossy(&body);
        assert!(
            text.contains("SSRF"),
            "blocked response must mention SSRF protection, got: {text}"
        );
    }

    /// 非法 JSON → 提取器级拒绝映射为统一包封（非 500）
    #[tokio::test]
    async fn map_route_rejects_invalid_json() {
        if crate::common::test_helpers::skip_if_no_test_db() {
            return;
        }
        let app = test_router(make_map_use_case(HashMap::new()));
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method(axum::http::Method::POST)
                    .uri("/v1/map")
                    .header("content-type", "application/json")
                    .body(Body::from("{not json"))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert!(
            response.status().is_client_error(),
            "invalid JSON must be a client error, got {}",
            response.status()
        );
    }
}
