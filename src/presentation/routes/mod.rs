// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

#[cfg(feature = "dbnexus-postgres")]
pub mod handlers;
#[cfg(feature = "dbnexus-postgres")]
pub mod task;

#[cfg(feature = "dbnexus-postgres")]
pub use handlers::{health_check, routes, version};

#[cfg(all(test, feature = "dbnexus-postgres"))]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use axum::routing::get;
    use axum::Router;
    use tower::ServiceExt;

    #[tokio::test]
    async fn test_health_check_returns_healthy_status() {
        let response = health_check().await;
        let json_value = response.0;
        assert_eq!(json_value["status"], "healthy");
    }

    #[tokio::test]
    async fn test_health_check_via_router() {
        let app = Router::new().route("/health", get(health_check));

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "healthy");
    }

    #[tokio::test]
    async fn test_version_returns_cargo_pkg_version() {
        let version = version().await;
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn test_version_is_non_empty() {
        let version = version().await;
        assert!(!version.is_empty());
    }

    #[test]
    fn test_routes_returns_router_without_panic() {
        // The routes() function should build the router without requiring
        // external services (DB, cache) since it only registers handlers.
        let _router = routes();
    }

    #[tokio::test]
    async fn test_health_endpoint_exists_in_full_router() {
        let app = routes();

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_version_endpoint_exists_in_full_router() {
        let app = routes();

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/version")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), 1024)
            .await
            .unwrap();
        let body_str = String::from_utf8(body.to_vec()).unwrap();
        assert_eq!(body_str, env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn test_metrics_endpoint_exists_in_full_router() {
        let app = routes();

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Metrics endpoint should exist (may return 200 or other status)
        // We just verify the route is registered (not 404)
        assert_ne!(response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_scrape_endpoint_is_post_only() {
        let app = routes();

        // GET should return 405 Method Not Allowed
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/scrape")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn test_search_endpoint_is_post_only() {
        let app = routes();

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/search")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    }

    #[tokio::test]
    async fn test_unknown_route_returns_404() {
        let app = routes();

        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri("/v1/nonexistent")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }
}
