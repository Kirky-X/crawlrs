// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

#[cfg(test)]
mod tests {
    use crate::engines::reqwest_engine::ReqwestEngine;
    use crate::engines::traits::{ScrapeRequest, ScraperEngine};
    use axum::{
        http::StatusCode,
        response::{IntoResponse, Response},
        routing::get,
        Router,
    };
    use std::collections::HashMap;
    use std::time::Duration;
    use tokio::net::TcpListener;

    async fn start_test_server() -> String {
        let app = Router::new()
            .route(
                "/test",
                get(|| async {
                    Response::builder()
                        .header("content-type", "text/html")
                        .body("<html><body>Test content</body></html>".to_string())
                        .unwrap()
                }),
            )
            .route(
                "/error",
                get(|| async { StatusCode::INTERNAL_SERVER_ERROR.into_response() }),
            );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        format!("http://{}", addr)
    }

    #[tokio::test]
    async fn test_reqwest_engine_basic_scraping() {
        std::env::set_var("CRAWLRS_DISABLE_SSRF_PROTECTION", "true");
        let server_url = start_test_server().await;

        let engine = ReqwestEngine;
        let request = ScrapeRequest {
            url: format!("{}/test", server_url),
            headers: HashMap::new(),
            timeout: Duration::from_secs(10),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
        };

        let result = engine.scrape(&request).await;
        if let Err(e) = &result {
            tracing::error!("Scrape failed: {:?}", e);
        }
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.status_code, 200);
        assert!(response.content.contains("Test content"));
        assert!(response.content_type.contains("text/html"));

        std::env::remove_var("CRAWLRS_DISABLE_SSRF_PROTECTION");
    }

    #[tokio::test]
    async fn test_reqwest_engine_error_handling() {
        std::env::set_var("CRAWLRS_DISABLE_SSRF_PROTECTION", "true");
        let server_url = start_test_server().await;

        let engine = ReqwestEngine;
        let request = ScrapeRequest {
            url: format!("{}/error", server_url),
            headers: HashMap::new(),
            timeout: Duration::from_secs(10),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
        };

        let result = engine.scrape(&request).await;
        assert!(result.is_ok());

        let response = result.unwrap();
        assert_eq!(response.status_code, 500);

        std::env::remove_var("CRAWLRS_DISABLE_SSRF_PROTECTION");
    }

    #[tokio::test]
    async fn test_reqwest_engine_support_score() {
        let engine = ReqwestEngine;

        let basic_request = ScrapeRequest {
            url: "https://example.com".to_string(),
            headers: HashMap::new(),
            timeout: Duration::from_secs(10),
            needs_js: false,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
        };
        assert_eq!(engine.support_score(&basic_request), 100);

        let js_request = ScrapeRequest {
            url: "https://example.com".to_string(),
            headers: HashMap::new(),
            timeout: Duration::from_secs(10),
            needs_js: true,
            needs_screenshot: false,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
        };
        assert_eq!(engine.support_score(&js_request), 10);

        let screenshot_request = ScrapeRequest {
            url: "https://example.com".to_string(),
            headers: HashMap::new(),
            timeout: Duration::from_secs(10),
            needs_js: false,
            needs_screenshot: true,
            screenshot_config: None,
            mobile: false,
            proxy: None,
            skip_tls_verification: false,
            needs_tls_fingerprint: false,
            use_fire_engine: false,
        };
        assert_eq!(engine.support_score(&screenshot_request), 10);
    }

    #[tokio::test]
    async fn test_reqwest_engine_name() {
        let engine = ReqwestEngine;
        assert_eq!(engine.name(), "reqwest");
    }
}
