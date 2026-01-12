// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Mock server utilities for integration tests
//!
//! This module provides standardized mock server configuration for testing
//! external services like search engines, FlareSolverr, etc.

#[cfg(test)]
pub mod flaresolverr {
    use serde_json::json;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Create a mock FlareSolverr server that simulates successful responses
    ///
    /// Returns the mock server and its URI for configuration
    pub async fn create_mock_server() -> (MockServer, String) {
        let mock_server = MockServer::start().await;
        let mock_uri = mock_server.uri();

        // Mock successfulFlareSolverr response
        Mock::given(method("POST"))
            .and(path("/v1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "status": "ok",
                "solution": {
                    "response": r#"<html><body><h3>Rust Programming Language</h3><p>The Rust Programming Language - official website</p></body></html>"#,
                    "cookies": [],
                    "userAgent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
                    "headers": {
                        "content-type": "text/html; charset=utf-8"
                    }
                },
                "errors": [],
                "timestamp": 1704067200
            })))
            .mount(&mock_server)
            .await;

        (mock_server, mock_uri)
    }

    /// Create a mock FlareSolverr server that simulates rate limiting
    pub async fn create_rate_limited_mock_server() -> (MockServer, String) {
        let mock_server = MockServer::start().await;
        let mock_uri = mock_server.uri();

        Mock::given(method("POST"))
            .and(path("/v1"))
            .respond_with(ResponseTemplate::new(429).set_body_json(json!({
                "status": "error",
                "message": "Rate limit exceeded",
                "errors": ["Too many requests"]
            })))
            .mount(&mock_server)
            .await;

        (mock_server, mock_uri)
    }

    /// Create a mock FlareSolverr server that simulates errors
    pub async fn create_error_mock_server() -> (MockServer, String) {
        let mock_server = MockServer::start().await;
        let mock_uri = mock_server.uri();

        Mock::given(method("POST"))
            .and(path("/v1"))
            .respond_with(ResponseTemplate::new(500).set_body_json(json!({
                "status": "error",
                "message": "Internal server error",
                "errors": ["FlareSolverr error"]
            })))
            .mount(&mock_server)
            .await;

        (mock_server, mock_uri)
    }
}

#[cfg(test)]
pub mod search_engines {
    use serde_json::json;
    use wiremock::matchers::{method, path, query_param};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Create a mock Google Custom Search API server
    pub async fn create_mock_google_server() -> (MockServer, String) {
        let mock_server = MockServer::start().await;
        let mock_uri = mock_server.uri();

        Mock::given(method("GET"))
            .and(path("/customsearch/v1"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "kind": "customsearch#search",
                "url": {
                    "type": "application/json"
                },
                "queries": {
                    "request": [{
                        "title": "Google Custom Search - rust programming",
                        "searchTerms": "rust programming",
                        "count": 10,
                        "startIndex": 0
                    }]
                },
                "items": [
                    {
                        "kind": "customsearch#result",
                        "title": "The Rust Programming Language",
                        "link": "https://www.rust-lang.org/",
                        "snippet": "A language empowering everyone to build reliable and efficient software.",
                        "displayLink": "www.rust-lang.org"
                    }
                ]
            })))
            .mount(&mock_server)
            .await;

        (mock_server, mock_uri)
    }
}
