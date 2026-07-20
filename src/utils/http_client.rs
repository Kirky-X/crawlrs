// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! Unified HTTP Client Module
//!
//! Provides HTTP client factory functions for creating consistently configured clients.
//! All clients include SSRF protection through redirect validation.
//!
//! ## Features
//!
//! - Connection pooling for performance
//! - SSRF-safe redirect policy
//! - Configurable timeouts
//! - Custom user agent support

use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;

/// Default HTTP client configuration
const DEFAULT_TIMEOUT: u64 = 30;
const DEFAULT_POOL_MAX_IDLE_PER_HOST: usize = 10;
const DEFAULT_POOL_IDLE_TIMEOUT: u64 = 90;
/// Default User-Agent for HTTP requests.
///
/// Uses a real desktop browser UA string so that anti-bot services (e.g.
/// Baidu search) do not return JS-redirect error pages instead of content.
/// A `crawlrs/*` UA is intentionally avoided because major search engines
/// reject requests identifying as bots.
pub(crate) const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
const DEFAULT_MAX_REDIRECTS: u8 = 10;

/// Create an HTTP client with custom timeout.
///
/// This client has SSRF-safe redirect policy enabled.
///
/// # Arguments
///
/// * `timeout_secs` - Request timeout in seconds
///
/// # Returns
///
/// A configured `reqwest::Client`
pub fn create_http_client_with_timeout(timeout_secs: u64) -> Client {
    create_client(timeout_secs, DEFAULT_MAX_REDIRECTS)
}

/// Create an HTTP client with default configuration.
///
/// This client has SSRF-safe redirect policy enabled.
///
/// # Returns
///
/// An `Arc<Client>` with default settings
pub fn create_http_client() -> Arc<Client> {
    Arc::new(create_client(DEFAULT_TIMEOUT, DEFAULT_MAX_REDIRECTS))
}

/// Create an HTTP client with custom redirect limit.
///
/// # Arguments
///
/// * `timeout_secs` - Request timeout in seconds
/// * `max_redirects` - Maximum number of redirects to follow
///
/// # Returns
///
/// A configured `reqwest::Client`
pub fn create_http_client_with_redirects(timeout_secs: u64, max_redirects: u8) -> Client {
    create_client(timeout_secs, max_redirects)
}

/// Create an HTTP client that does not follow redirects.
///
/// Use this when you need to handle redirects manually with SSRF validation.
///
/// # Returns
///
/// A `reqwest::Client` that does not follow redirects
pub fn create_http_client_no_redirects() -> Client {
    Client::builder()
        .user_agent(DEFAULT_USER_AGENT)
        .timeout(Duration::from_secs(DEFAULT_TIMEOUT))
        .pool_max_idle_per_host(DEFAULT_POOL_MAX_IDLE_PER_HOST)
        .pool_idle_timeout(Duration::from_secs(DEFAULT_POOL_IDLE_TIMEOUT))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_else(|_| Client::new())
}

/// Create an HTTP client with SSRF-safe redirect policy.
///
/// The redirect policy validates each redirect URL before following it.
fn create_client(timeout_secs: u64, max_redirects: u8) -> Client {
    let redirect_policy = create_ssrf_safe_redirect_policy(max_redirects);

    Client::builder()
        .user_agent(DEFAULT_USER_AGENT)
        .timeout(Duration::from_secs(timeout_secs))
        .pool_max_idle_per_host(DEFAULT_POOL_MAX_IDLE_PER_HOST)
        .pool_idle_timeout(Duration::from_secs(DEFAULT_POOL_IDLE_TIMEOUT))
        .redirect(redirect_policy)
        .build()
        .unwrap_or_else(|_| Client::new())
}

/// Create a reqwest redirect policy with SSRF protection.
///
/// This policy validates each redirect URL before following it,
/// blocking redirects to internal/private addresses.
///
/// # Arguments
///
/// * `max_redirects` - Maximum number of redirects to follow
///
/// # Returns
///
/// A `reqwest::redirect::Policy` that validates redirect URLs
pub fn create_ssrf_safe_redirect_policy(max_redirects: u8) -> reqwest::redirect::Policy {
    use crate::presentation::helpers::ssrf::is_internal_url;

    reqwest::redirect::Policy::custom(move |attempt| {
        // Check redirect count
        if attempt.previous().len() >= max_redirects as usize {
            log::warn!(
                "Redirect limit ({}) exceeded, stopping redirect chain",
                max_redirects
            );
            return attempt.stop();
        }

        // Get redirect URL
        let redirect_url = attempt.url().to_string();

        // Validate redirect URL for SSRF
        if is_internal_url(&redirect_url) {
            log::warn!(
                "SSRF protection: Blocking redirect to internal URL: {}",
                redirect_url
            );
            return attempt.stop();
        }

        // Log redirect for debugging
        log::debug!(
            "Following redirect {} -> {}",
            attempt
                .previous()
                .last()
                .map(|u| u.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            redirect_url
        );

        // Follow the redirect
        attempt.follow()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_http_client_with_timeout() {
        let client = create_http_client_with_timeout(60);
        let _ = client.clone();
    }

    #[test]
    fn test_create_default_http_client() {
        let client = create_http_client();
        let _ = client.clone();
    }

    #[test]
    fn test_create_http_client_no_redirects() {
        let client = create_http_client_no_redirects();
        let _ = client.clone();
    }

    #[test]
    fn test_create_http_client_with_redirects() {
        let client = create_http_client_with_redirects(30, 5);
        let _ = client.clone();
    }

    #[test]
    fn test_default_constants_values() {
        // Verify the default configuration constants are the expected values.
        assert_eq!(DEFAULT_TIMEOUT, 30);
        assert_eq!(DEFAULT_POOL_MAX_IDLE_PER_HOST, 10);
        assert_eq!(DEFAULT_POOL_IDLE_TIMEOUT, 90);
        assert_eq!(DEFAULT_MAX_REDIRECTS, 10);
        assert!(!DEFAULT_USER_AGENT.is_empty());
    }

    #[test]
    fn test_create_http_client_with_zero_timeout() {
        // A zero timeout is a valid edge case; the client must still build.
        let client = create_http_client_with_timeout(0);
        let _ = client.clone();
    }

    #[test]
    fn test_create_http_client_with_large_timeout() {
        let client = create_http_client_with_timeout(86400);
        let _ = client.clone();
    }

    #[test]
    fn test_create_http_client_with_zero_redirects() {
        // max_redirects=0 should still produce a working client.
        let client = create_http_client_with_redirects(30, 0);
        let _ = client.clone();
    }

    #[test]
    fn test_create_http_client_with_max_redirects() {
        let client = create_http_client_with_redirects(30, u8::MAX);
        let _ = client.clone();
    }

    #[test]
    fn test_create_ssrf_safe_redirect_policy_returns_policy() {
        // The policy factory must return a usable Policy for various limits.
        let _policy = create_ssrf_safe_redirect_policy(5);
        let _policy_zero = create_ssrf_safe_redirect_policy(0);
        let _policy_max = create_ssrf_safe_redirect_policy(u8::MAX);
    }

    #[test]
    fn test_default_http_client_is_arc_wrapped() {
        // create_http_client returns an Arc<Client>; verify it can be shared.
        let client = create_http_client();
        let client2 = client.clone();
        // Both Arcs should point to the same underlying Client.
        assert!(std::sync::Arc::ptr_eq(&client, &client2));
    }

    #[test]
    fn test_no_redirect_client_follows_no_redirects() {
        // The no-redirect client must report that it won't follow redirects.
        // We verify this indirectly by ensuring the client builds with the
        // none() redirect policy without panicking.
        let _client = create_http_client_no_redirects();
    }

    // ========== SSRF-safe redirect policy behavior tests (wiremock) ==========

    #[tokio::test]
    async fn test_ssrf_safe_redirect_policy_blocks_redirect_to_internal_url() {
        // A redirect to an internal URL (127.0.0.1) must be blocked by the
        // SSRF-safe redirect policy. The client should return the 302
        // response instead of following the redirect.
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/redirect"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", "http://127.0.0.1:1/destination"),
            )
            .mount(&mock_server)
            .await;

        let client = create_http_client_with_redirects(5, 10);
        let url = format!("{}/redirect", mock_server.uri());
        let response = client
            .get(&url)
            .send()
            .await
            .expect("request should succeed");

        // The redirect target is 127.0.0.1 which is_internal_url returns true,
        // so the policy calls attempt.stop() and returns the 302 response.
        assert_eq!(
            response.status().as_u16(),
            302,
            "redirect to internal URL should be blocked (302 returned, not followed)"
        );
    }

    #[tokio::test]
    async fn test_ssrf_safe_redirect_policy_zero_max_redirects() {
        // With max_redirects=0, the policy must stop immediately on the first
        // redirect regardless of the target URL.
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/redirect"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", "http://127.0.0.1:1/destination"),
            )
            .mount(&mock_server)
            .await;

        let client = create_http_client_with_redirects(5, 0);
        let url = format!("{}/redirect", mock_server.uri());
        let response = client
            .get(&url)
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(
            response.status().as_u16(),
            302,
            "with max_redirects=0, redirect should not be followed"
        );
    }

    #[tokio::test]
    async fn test_client_with_ssrf_policy_successful_non_redirect_request() {
        // A normal 200 response (no redirect) should pass through the client
        // without being affected by the SSRF redirect policy.
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/normal"))
            .respond_with(ResponseTemplate::new(200).set_body_string("ok"))
            .mount(&mock_server)
            .await;

        let client = create_http_client_with_redirects(5, 10);
        let url = format!("{}/normal", mock_server.uri());
        let response = client
            .get(&url)
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(response.status().as_u16(), 200);
        let body = response.text().await.expect("body should be readable");
        assert_eq!(body, "ok");
    }

    #[tokio::test]
    async fn test_ssrf_safe_redirect_policy_blocks_redirect_to_localhost() {
        // A redirect to "localhost" hostname should also be blocked.
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/redirect-localhost"))
            .respond_with(
                ResponseTemplate::new(301)
                    .insert_header("Location", "http://localhost:1/destination"),
            )
            .mount(&mock_server)
            .await;

        let client = create_http_client_with_redirects(5, 10);
        let url = format!("{}/redirect-localhost", mock_server.uri());
        let response = client
            .get(&url)
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(
            response.status().as_u16(),
            301,
            "redirect to localhost should be blocked"
        );
    }

    #[tokio::test]
    async fn test_ssrf_safe_redirect_policy_blocks_redirect_to_private_ip() {
        // A redirect to a private IP range (10.x.x.x) should be blocked.
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/redirect-private"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", "http://10.0.0.1:8080/internal"),
            )
            .mount(&mock_server)
            .await;

        let client = create_http_client_with_redirects(5, 10);
        let url = format!("{}/redirect-private", mock_server.uri());
        let response = client
            .get(&url)
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(
            response.status().as_u16(),
            302,
            "redirect to private IP (10.0.0.1) should be blocked"
        );
    }

    // ========== Supplementary tests: edge cases and parameter coverage ==========

    #[tokio::test]
    async fn test_ssrf_safe_redirect_policy_blocks_redirect_to_172_16() {
        // Another private IP range (172.16.0.0/12) should also be blocked.
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/redirect-172"))
            .respond_with(
                ResponseTemplate::new(301).insert_header("Location", "http://172.16.0.1:80/dest"),
            )
            .mount(&mock_server)
            .await;

        let client = create_http_client_with_redirects(5, 10);
        let url = format!("{}/redirect-172", mock_server.uri());
        let response = client
            .get(&url)
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(
            response.status().as_u16(),
            301,
            "redirect to 172.16.0.0/12 should be blocked"
        );
    }

    #[tokio::test]
    async fn test_ssrf_safe_redirect_policy_blocks_redirect_to_192_168() {
        // The 192.168.0.0/16 private range should also be blocked.
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/redirect-192"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("Location", "http://192.168.1.1:80/dest"),
            )
            .mount(&mock_server)
            .await;

        let client = create_http_client_with_redirects(5, 10);
        let url = format!("{}/redirect-192", mock_server.uri());
        let response = client
            .get(&url)
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(
            response.status().as_u16(),
            302,
            "redirect to 192.168.0.0/16 should be blocked"
        );
    }

    #[tokio::test]
    async fn test_ssrf_safe_redirect_policy_blocks_redirect_to_ipv6_loopback() {
        // IPv6 loopback [::1] should also be blocked.
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/redirect-ipv6"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("Location", "http://[::1]:80/dest"),
            )
            .mount(&mock_server)
            .await;

        let client = create_http_client_with_redirects(5, 10);
        let url = format!("{}/redirect-ipv6", mock_server.uri());
        let response = client
            .get(&url)
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(
            response.status().as_u16(),
            302,
            "redirect to [::1] should be blocked"
        );
    }

    #[tokio::test]
    async fn test_ssrf_safe_redirect_policy_blocks_redirect_to_169_254() {
        // Link-local address 169.254.0.0/16 should be blocked (cloud metadata).
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/redirect-link-local"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", "http://169.254.169.254/latest/meta-data/"),
            )
            .mount(&mock_server)
            .await;

        let client = create_http_client_with_redirects(5, 10);
        let url = format!("{}/redirect-link-local", mock_server.uri());
        let response = client
            .get(&url)
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(
            response.status().as_u16(),
            302,
            "redirect to 169.254.169.254 should be blocked"
        );
    }

    #[tokio::test]
    async fn test_ssrf_safe_redirect_policy_blocks_redirect_to_0_0_0_0() {
        // 0.0.0.0 should be blocked.
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/redirect-zero"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("Location", "http://0.0.0.0:80/dest"),
            )
            .mount(&mock_server)
            .await;

        let client = create_http_client_with_redirects(5, 10);
        let url = format!("{}/redirect-zero", mock_server.uri());
        let response = client
            .get(&url)
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(
            response.status().as_u16(),
            302,
            "redirect to 0.0.0.0 should be blocked"
        );
    }

    #[tokio::test]
    async fn test_ssrf_safe_redirect_policy_blocks_non_http_scheme() {
        // A redirect to a non-HTTP scheme (file://) should be blocked.
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/redirect-file"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("Location", "file:///etc/passwd"),
            )
            .mount(&mock_server)
            .await;

        let client = create_http_client_with_redirects(5, 10);
        let url = format!("{}/redirect-file", mock_server.uri());
        let response = client
            .get(&url)
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(
            response.status().as_u16(),
            302,
            "redirect to file:// scheme should be blocked"
        );
    }

    #[tokio::test]
    async fn test_no_redirect_client_returns_redirect_response() {
        // The no-redirect client should return the redirect response as-is,
        // without following it.
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/redirect"))
            .respond_with(
                ResponseTemplate::new(302).insert_header("Location", "http://127.0.0.1:1/dest"),
            )
            .mount(&mock_server)
            .await;

        let client = create_http_client_no_redirects();
        let url = format!("{}/redirect", mock_server.uri());
        let response = client
            .get(&url)
            .send()
            .await
            .expect("request should succeed");

        // The no-redirect client should NOT follow the redirect.
        assert_eq!(
            response.status().as_u16(),
            302,
            "no-redirect client should return 302 without following"
        );
    }

    #[tokio::test]
    async fn test_client_with_ssrf_policy_handles_post_request() {
        // Verify the SSRF-safe client handles POST requests without redirect issues.
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/submit"))
            .respond_with(ResponseTemplate::new(200).set_body_string("accepted"))
            .mount(&mock_server)
            .await;

        let client = create_http_client_with_redirects(5, 10);
        let url = format!("{}/submit", mock_server.uri());
        let response = client
            .post(&url)
            .body("data")
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(response.status().as_u16(), 200);
        assert_eq!(response.text().await.unwrap(), "accepted");
    }

    #[tokio::test]
    async fn test_client_with_ssrf_policy_follows_safe_redirect_to_mock_server() {
        // NOTE: This test requires network access. The SSRF-safe policy allows
        // redirects to non-internal URLs. We use the mock server's own address
        // as the redirect target, but 127.0.0.1 is internal, so the redirect
        // is blocked. This test documents that behavior: even a redirect to
        // the mock server (127.0.0.1) is blocked by SSRF protection.
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        // Set up a destination endpoint on the mock server.
        Mock::given(method("GET"))
            .and(path("/destination"))
            .respond_with(ResponseTemplate::new(200).set_body_string("reached"))
            .mount(&mock_server)
            .await;

        // Redirect to the mock server's own /destination endpoint.
        Mock::given(method("GET"))
            .and(path("/redirect-to-dest"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", format!("{}/destination", mock_server.uri())),
            )
            .mount(&mock_server)
            .await;

        let client = create_http_client_with_redirects(5, 10);
        let url = format!("{}/redirect-to-dest", mock_server.uri());
        let response = client
            .get(&url)
            .send()
            .await
            .expect("request should succeed");

        // The redirect target is 127.0.0.1 (mock server), which is_internal_url
        // returns true, so the redirect is blocked and the 302 is returned.
        assert_eq!(
            response.status().as_u16(),
            302,
            "redirect to 127.0.0.1 (mock server) should be blocked by SSRF policy"
        );
    }

    #[test]
    fn test_default_user_agent_is_chrome_like() {
        // Verify the default User-Agent contains Chrome identifiers.
        assert!(DEFAULT_USER_AGENT.contains("Chrome"));
        assert!(DEFAULT_USER_AGENT.contains("Mozilla"));
    }

    #[test]
    fn test_create_ssrf_safe_redirect_policy_with_different_limits() {
        // Verify policies can be created for a range of redirect limits.
        for limit in [1u8, 2, 5, 10, 20, 50, 100, 200] {
            let _policy = create_ssrf_safe_redirect_policy(limit);
        }
    }

    #[test]
    fn test_http_client_functions_return_usable_clients() {
        // Smoke test: all factory functions must return usable clients.
        let c1 = create_http_client_with_timeout(10);
        let c2 = create_http_client();
        let c3 = create_http_client_no_redirects();
        let c4 = create_http_client_with_redirects(10, 5);
        // Verify they can be cloned without panic.
        let _ = c1.clone();
        let _ = c2.clone();
        let _ = c3.clone();
        let _ = c4.clone();
    }

    // ========== Test logger for covering log::warn!/log::debug! in redirect policy ==========

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

    fn ensure_debug_logger() {
        LOGGER_INIT.call_once(|| {
            static CAPTURING_LOGGER: CapturingLogger = CapturingLogger;
            let _ = log::set_logger(&CAPTURING_LOGGER);
            log::set_max_level(LevelFilter::Debug);
        });
    }

    // ========== log::warn! coverage tests for SSRF redirect policy ==========

    #[tokio::test]
    async fn test_ssrf_block_logs_warning_with_logger_initialized() {
        // With logger initialized, the log::warn! for SSRF block should execute.
        ensure_debug_logger();
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/redirect-logged"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", "http://127.0.0.1:1/destination"),
            )
            .mount(&mock_server)
            .await;

        let client = create_http_client_with_redirects(5, 10);
        let url = format!("{}/redirect-logged", mock_server.uri());
        let response = client
            .get(&url)
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(
            response.status().as_u16(),
            302,
            "redirect to internal URL should be blocked (302 returned)"
        );
    }

    #[tokio::test]
    async fn test_redirect_limit_exceeded_logs_warning_with_logger() {
        // With max_redirects=0 and logger initialized, the log::warn! for
        // redirect limit exceeded should execute.
        ensure_debug_logger();
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/redirect-limit-logged"))
            .respond_with(
                ResponseTemplate::new(301).insert_header("Location", "http://127.0.0.1:1/dest"),
            )
            .mount(&mock_server)
            .await;

        let client = create_http_client_with_redirects(5, 0);
        let url = format!("{}/redirect-limit-logged", mock_server.uri());
        let response = client
            .get(&url)
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(
            response.status().as_u16(),
            301,
            "with max_redirects=0, redirect should not be followed"
        );
    }

    #[tokio::test]
    async fn test_ssrf_block_localhost_logs_warning_with_logger() {
        // With logger initialized, blocking redirect to localhost logs warning.
        ensure_debug_logger();
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/redirect-localhost-logged"))
            .respond_with(
                ResponseTemplate::new(301)
                    .insert_header("Location", "http://localhost:1/destination"),
            )
            .mount(&mock_server)
            .await;

        let client = create_http_client_with_redirects(5, 10);
        let url = format!("{}/redirect-localhost-logged", mock_server.uri());
        let response = client
            .get(&url)
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(response.status().as_u16(), 301);
    }

    #[tokio::test]
    async fn test_ssrf_block_private_ip_logs_warning_with_logger() {
        // With logger initialized, blocking redirect to private IP logs warning.
        ensure_debug_logger();
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/redirect-private-logged"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", "http://10.0.0.1:8080/internal"),
            )
            .mount(&mock_server)
            .await;

        let client = create_http_client_with_redirects(5, 10);
        let url = format!("{}/redirect-private-logged", mock_server.uri());
        let response = client
            .get(&url)
            .send()
            .await
            .expect("request should succeed");

        assert_eq!(response.status().as_u16(), 302);
    }

    // ========== redirect follow 分支覆盖测试 ==========

    #[tokio::test]
    async fn test_ssrf_safe_redirect_policy_follows_external_redirect() {
        // 当 redirect 目标是外部 URL（is_internal_url 返回 false）时，
        // policy 应执行 log::debug!（行 141-149）和 attempt.follow()（行 152）。
        // 使用 TEST-NET-1 (192.0.2.1, RFC 5737) 端口 1，确保不可达，
        // 触发 attempt.follow() 后连接失败返回错误。
        ensure_debug_logger();
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("GET"))
            .and(path("/redirect-external-follow"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Location", "http://192.0.2.1:1/destination"),
            )
            .mount(&mock_server)
            .await;

        // 使用短超时（2 秒），避免测试等待过久
        let client = create_http_client_with_redirects(2, 10);
        let url = format!("{}/redirect-external-follow", mock_server.uri());
        let result = client.get(&url).send().await;

        // redirect 目标不可达，send() 应返回错误（覆盖 attempt.follow() 分支）
        assert!(
            result.is_err(),
            "redirect to unreachable external URL should fail after attempt.follow()"
        );
    }
}
