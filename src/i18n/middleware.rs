// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! i18n 中间件
//!
//! 从请求 `Accept-Language` header 协商 locale，注入 `Extension<Locale>` 供 handler 使用。

use std::sync::Arc;

use axum::{extract::Request, middleware::Next, response::Response, Extension};

use super::bundle::I18nBundle;
use super::locale::{negotiate_locale, parse_accept_language};

/// i18n 中间件
///
/// 从 `Accept-Language` header 解析语言偏好，与 bundle 支持的 locale 协商，
/// 将最终 `Locale` 注入 `Extension<Locale>` 供下游 handler 使用。
///
/// `I18nBundle` 通过 `Extension<Arc<I18nBundle>>` 传入（在路由层 layer 注入）。
pub async fn i18n_middleware(
    Extension(bundle): Extension<Arc<I18nBundle>>,
    mut req: Request,
    next: Next,
) -> Response {
    let locale = req
        .headers()
        .get("accept-language")
        .and_then(|v| v.to_str().ok())
        .map(|header| {
            let preferred = parse_accept_language(header);
            negotiate_locale(
                &preferred,
                bundle.supported_locales(),
                bundle.default_locale(),
            )
        })
        .unwrap_or_else(|| bundle.default_locale().clone());

    req.extensions_mut().insert(locale);
    next.run(req).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Locale;
    use axum::{body::Body, http::StatusCode, routing::get, Router};
    use tower::ServiceExt;

    fn test_bundle() -> Arc<I18nBundle> {
        let dir = format!("{}/locales", env!("CARGO_MANIFEST_DIR"));
        Arc::new(I18nBundle::load("en-US", &["en-US", "zh-CN"], &dir).unwrap())
    }

    async fn handler_locale(Extension(locale): Extension<Locale>) -> String {
        locale.to_string()
    }

    fn test_app(bundle: Arc<I18nBundle>) -> Router {
        Router::new()
            .route("/locale", get(handler_locale))
            .layer(axum::middleware::from_fn(i18n_middleware))
            .layer(Extension(bundle))
    }

    #[tokio::test]
    async fn test_middleware_en_us() {
        let app = test_app(test_bundle());
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/locale")
                    .header("accept-language", "en-US")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&body), "en-US");
    }

    #[tokio::test]
    async fn test_middleware_zh_cn() {
        let app = test_app(test_bundle());
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/locale")
                    .header("accept-language", "zh-CN")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&body), "zh-CN");
    }

    #[tokio::test]
    async fn test_middleware_fallback_to_default() {
        let app = test_app(test_bundle());
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/locale")
                    .header("accept-language", "fr-FR")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        // fr-FR 不支持，回退到 default en-US
        assert_eq!(String::from_utf8_lossy(&body), "en-US");
    }

    #[tokio::test]
    async fn test_middleware_no_header_uses_default() {
        let app = test_app(test_bundle());
        let resp = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/locale")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(String::from_utf8_lossy(&body), "en-US");
    }
}
