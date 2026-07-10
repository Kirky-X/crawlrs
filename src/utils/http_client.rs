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
const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36";
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
}
