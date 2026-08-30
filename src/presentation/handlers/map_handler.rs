// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! `POST /v1/map` handler（bdd-acceptance-hardening R-map-004）。
//!
//! 错误映射：DTO 校验失败 → 422、目标站不可达/5xx → 502 `MAP_TARGET_UNREACHABLE`、
//! 内部错 → 500；sitemap 404 是合法状态返回空 links（用例层已处理）。

use axum::{
    extract::{Extension, Json},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use std::sync::Arc;
use validator::Validate;

use crate::application::dto::map_request::MapRequestDto;
use crate::application::use_cases::map_use_case::{MapError, MapResult, MapUseCase};
use crate::presentation::handlers::check_ssrf_url;
use crate::presentation::handlers::response_builder::{error_response_with_code, success_response};
use crate::presentation::middleware::auth_middleware::AuthState;

/// `/v1/map` 响应数据
#[derive(Debug, serde::Serialize)]
pub struct MapDataDto {
    pub links: Vec<String>,
}

/// 用例结果 → HTTP 响应映射（抽为纯函数便于单元测试；SSRF 通过路径由验收套件集成覆盖）
pub(crate) fn render_map_result(result: Result<MapResult, MapError>) -> Response {
    match result {
        Ok(MapResult { links }) => success_response(StatusCode::OK, MapDataDto { links }),
        Err(MapError::TargetUnreachable(msg)) => {
            error_response_with_code(StatusCode::BAD_GATEWAY, "MAP_TARGET_UNREACHABLE", msg)
        }
        Err(MapError::Internal(msg)) => {
            log::error!("Map use case internal error: {}", msg);
            error_response_with_code(StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", msg)
        }
    }
}

/// 处理 sitemap URL 发现请求
pub async fn map(
    Extension(map_use_case): Extension<Arc<MapUseCase>>,
    Extension(auth_state): Extension<AuthState>,
    Json(payload): Json<MapRequestDto>,
) -> impl IntoResponse {
    // 1. DTO 校验（422）
    if let Err(e) = payload.validate() {
        return error_response_with_code(
            StatusCode::UNPROCESSABLE_ENTITY,
            "VALIDATION_ERROR",
            format!("{e}"),
        );
    }

    // 2. SSRF 防护（CWE-918，对齐 scrape/search 惯例）
    if let Some(response) =
        check_ssrf_url(&payload.url, auth_state.team_id, auth_state.api_key_id).await
    {
        return response;
    }

    // 3. 执行用例并渲染
    render_map_result(map_use_case.execute(&payload).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::use_cases::map_use_case::SitemapFetch;

    /// 解码渲染结果为 (status, json)。
    async fn decode(response: Response) -> (StatusCode, serde_json::Value) {
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 1_048_576)
            .await
            .unwrap();
        let json: serde_json::Value =
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null);
        (status, json)
    }

    /// R-map-004：成功结果 → 200 success 且 data.links 全量返回。
    #[tokio::test]
    async fn render_success_returns_links() {
        let (status, body) = decode(render_map_result(Ok(MapResult {
            links: vec!["https://a.com/1".into(), "https://a.com/2".into()],
        })))
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["success"], true);
        assert_eq!(body["data"]["links"].as_array().map(|a| a.len()), Some(2));
        assert_eq!(body["data"]["links"][0], "https://a.com/1");
    }

    /// R-map-004：目标站不可达 → 502 MAP_TARGET_UNREACHABLE。
    #[tokio::test]
    async fn render_target_unreachable_maps_to_502() {
        let (status, body) = decode(render_map_result(Err(MapError::TargetUnreachable(
            "sitemap returned status 500".into(),
        ))))
        .await;
        assert_eq!(status, StatusCode::BAD_GATEWAY);
        assert_eq!(body["success"], false);
        assert_eq!(body["error"]["code"], "MAP_TARGET_UNREACHABLE");
    }

    /// R-map-004：内部错 → 500。
    #[tokio::test]
    async fn render_internal_maps_to_500() {
        let (status, body) = decode(render_map_result(Err(MapError::Internal(
            "cannot parse origin".into(),
        ))))
        .await;
        assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(body["error"]["code"], "INTERNAL_ERROR");
    }

    /// R-map-004：422 校验路径（validate 在 SSRF 之前，可整体经 handler 断言）。
    #[tokio::test]
    async fn map_rejects_invalid_url_with_422() {
        use axum::{routing::post, Router};
        use tower::ServiceExt;

        let use_case = Arc::new(MapUseCase::new(Arc::new(RejectingFetcher)));
        let app = Router::new()
            .route("/v1/map", post(map))
            .layer(Extension(use_case))
            .layer(Extension(AuthState::new(
                crate::common::test_helpers::create_test_db_pool(),
                crate::common::constants::default_identity::DEFAULT_TEAM_ID,
                crate::common::constants::default_identity::DEFAULT_API_KEY_ID,
                crate::domain::auth::ApiKeyScope {
                    read: true,
                    write: true,
                    admin: false,
                    search_limit: 100,
                    scrape_limit: 100,
                },
            )));

        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .method("POST")
                    .uri("/v1/map")
                    .header("content-type", "application/json")
                    .body(axum::body::Body::from(
                        serde_json::json!({"url": "not-a-url"}).to_string(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
    }

    /// 恒拒绝 fetcher（422 用例不会触达用例层，保证不被调用）。
    struct RejectingFetcher;

    #[async_trait::async_trait]
    impl crate::application::use_cases::map_use_case::SitemapFetcher for RejectingFetcher {
        async fn fetch(&self, _url: &str) -> Result<SitemapFetch, MapError> {
            Err(MapError::Internal("must not be called".into()))
        }
    }
}
