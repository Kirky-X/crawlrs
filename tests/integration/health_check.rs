// Copyright (c) 2025 Kirky.X
//
// Licensed under the MIT License
// See LICENSE file in the project root for full license information.

use super::helpers::create_test_app;
use axum::http::StatusCode;

#[tokio::test]
async fn health_check_works() {
    let app = create_test_app().await;
    let response = app.server.get("/health").await;
    response.assert_status(StatusCode::OK);
}

#[tokio::test]
async fn scrape_endpoint_returns_401_without_auth() {
    let app = create_test_app().await;
    let response = app
        .server
        .post("/v1/scrape")
        .json(&serde_json::json!({"url": "https://example.com"}))
        .await;
    response.assert_status(StatusCode::UNAUTHORIZED);
}
