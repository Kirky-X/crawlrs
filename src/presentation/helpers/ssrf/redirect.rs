// Copyright (c) 2025 Kirky.X
//
// Licensed under the Apache License, Version 2.0
// See LICENSE file in the project root for full license information.

//! HTTP Redirect Validation for SSRF Protection
//!
//! This module provides redirect validation to prevent SSRF attacks
//! through HTTP redirects. Attackers can use redirects to bypass
//! initial URL validation by:
//!
//! 1. **Open Redirect Exploitation**: Using an open redirect on a trusted domain
//!    to redirect to internal addresses
//! 2. **Redirect Chains**: Using multiple redirects to obscure the final destination
//! 3. **Meta Refresh**: Using HTML meta refresh tags (handled by browser engines)
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                  Redirect Validation Flow                   │
//! ├─────────────────────────────────────────────────────────────┤
//! │                                                             │
//! │  HTTP Request                                               │
//! │       │                                                     │
//! │       ▼                                                     │
//! │  ┌─────────────┐                                           │
//! │  │ Send Request│                                           │
//! │  └──────┬──────┘                                           │
//! │         │                                                   │
//! │         ▼                                                   │
//! │  ┌─────────────────┐                                       │
//! │  │ Check Response  │                                       │
//! │  │ for Redirect    │                                       │
//! │  └────────┬────────┘                                       │
//! │           │                                                 │
//! │     ┌─────┴─────┐                                          │
//! │     │           │                                          │
//! │     ▼           ▼                                          │
//! │  3xx         Non-3xx                                       │
//! │  Redirect    Response                                      │
//! │     │           │                                          │
//! │     ▼           │                                          │
//! │  ┌─────────────┐│                                          │
//! │  │Validate     ││                                          │
//! │  │Redirect URL ││                                          │
//! │  └──────┬──────┘│                                          │
//! │         │       │                                          │
//! │    ┌────┴───┐   │                                          │
//! │    │        │   │                                          │
//! │    ▼        ▼   │                                          │
//! │  Valid   Invalid│                                          │
//! │    │        │   │                                          │
//! │    ▼        ▼   ▼                                          │
//! │ Follow   Block Return                                      │
//! │ Redirect Error Response                                    │
//! │                                                             │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Usage
//!
//! The redirect validation is integrated into HTTP clients automatically.
//! For custom implementations, use `RedirectValidator` directly.

use super::error::SsrfError;
use super::static_validator::is_internal_url;
use std::collections::HashSet;
use url::Url;

/// Maximum number of redirects to follow
const DEFAULT_MAX_REDIRECTS: u8 = 10;

/// Redirect policy for HTTP clients.
///
/// This enum defines how redirects should be handled.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedirectPolicy {
    /// Do not follow any redirects
    None,
    /// Follow redirects with validation (default)
    FollowWithValidation {
        /// Maximum number of redirects to follow
        max_redirects: u8,
    },
    /// Follow redirects only to the same host
    SameHostOnly {
        /// Maximum number of redirects to follow
        max_redirects: u8,
    },
}

impl Default for RedirectPolicy {
    fn default() -> Self {
        Self::FollowWithValidation {
            max_redirects: DEFAULT_MAX_REDIRECTS,
        }
    }
}

impl RedirectPolicy {
    /// Create a policy that does not follow redirects.
    pub fn none() -> Self {
        Self::None
    }

    /// Create a policy that follows redirects with validation.
    pub fn follow_with_validation(max_redirects: u8) -> Self {
        Self::FollowWithValidation { max_redirects }
    }

    /// Create a policy that follows redirects to the same host only.
    pub fn same_host_only(max_redirects: u8) -> Self {
        Self::SameHostOnly { max_redirects }
    }

    /// Get the maximum number of redirects.
    pub fn max_redirects(&self) -> u8 {
        match self {
            Self::None => 0,
            Self::FollowWithValidation { max_redirects } => *max_redirects,
            Self::SameHostOnly { max_redirects } => *max_redirects,
        }
    }

    /// Check if redirects are allowed.
    pub fn allows_redirects(&self) -> bool {
        !matches!(self, Self::None)
    }
}

/// Redirect validator for SSRF protection.
///
/// This struct validates redirect URLs to prevent SSRF attacks
/// through HTTP redirects.
#[derive(Debug, Clone)]
pub struct RedirectValidator {
    /// Redirect policy
    policy: RedirectPolicy,
    /// Original URL (for same-host validation)
    original_host: Option<String>,
    /// Redirect chain (for loop detection)
    redirect_chain: Vec<String>,
    /// Visited hosts (for tracking)
    visited_hosts: HashSet<String>,
}

impl RedirectValidator {
    /// Create a new redirect validator with default policy.
    pub fn new() -> Self {
        Self {
            policy: RedirectPolicy::default(),
            original_host: None,
            redirect_chain: Vec::new(),
            visited_hosts: HashSet::new(),
        }
    }

    /// Create a validator with a specific policy.
    pub fn with_policy(policy: RedirectPolicy) -> Self {
        Self {
            policy,
            original_host: None,
            redirect_chain: Vec::new(),
            visited_hosts: HashSet::new(),
        }
    }

    /// Set the original URL for same-host validation.
    pub fn with_original_url(mut self, url: &str) -> Self {
        if let Ok(parsed) = Url::parse(url) {
            if let Some(host) = parsed.host_str() {
                self.original_host = Some(host.to_lowercase());
                self.visited_hosts.insert(host.to_lowercase());
            }
        }
        self
    }

    /// Validate a redirect URL.
    ///
    /// This method checks:
    /// 1. If redirects are allowed by policy
    /// 2. If the redirect count exceeds the limit
    /// 3. If the redirect URL is internal
    /// 4. If the redirect is to the same host (if required)
    /// 5. If there's a redirect loop
    ///
    /// # Arguments
    ///
    /// * `redirect_url` - The redirect target URL
    /// * `current_count` - Current redirect count
    ///
    /// # Returns
    ///
    /// * `Ok(())` - Redirect is allowed
    /// * `Err(SsrfError)` - Redirect is blocked
    pub fn validate(&mut self, redirect_url: &str, current_count: u8) -> Result<(), SsrfError> {
        // Check if redirects are allowed
        if !self.policy.allows_redirects() {
            return Err(SsrfError::ValidationFailed(
                "Redirects are not allowed by policy".to_string(),
            ));
        }

        // Check redirect count
        let max_redirects = self.policy.max_redirects();
        if current_count >= max_redirects {
            return Err(SsrfError::MaxRedirectsExceeded {
                limit: max_redirects,
            });
        }

        // Parse redirect URL
        let parsed_url = Url::parse(redirect_url).map_err(|e| SsrfError::InvalidUrl {
            url: redirect_url.to_string(),
            reason: e.to_string(),
        })?;

        // Check for internal URL
        if is_internal_url(redirect_url) {
            return Err(SsrfError::RedirectToInternal {
                url: redirect_url.to_string(),
            });
        }

        // Check scheme
        match parsed_url.scheme() {
            "http" | "https" => {}
            scheme => {
                return Err(SsrfError::InvalidScheme {
                    scheme: scheme.to_string(),
                });
            }
        }

        // Get redirect host
        let redirect_host = parsed_url
            .host_str()
            .ok_or_else(|| SsrfError::MissingHost {
                url: redirect_url.to_string(),
            })?
            .to_lowercase();

        // Same-host validation
        if let RedirectPolicy::SameHostOnly { .. } = self.policy {
            if let Some(ref original) = self.original_host {
                if &redirect_host != original {
                    return Err(SsrfError::ValidationFailed(format!(
                        "Cross-host redirect not allowed: {} -> {}",
                        original, redirect_host
                    )));
                }
            }
        }

        // Redirect loop detection
        if self.redirect_chain.contains(&redirect_url.to_string()) {
            return Err(SsrfError::ValidationFailed(format!(
                "Redirect loop detected: {}",
                redirect_url
            )));
        }

        // Track visited hosts for potential analysis
        self.visited_hosts.insert(redirect_host.clone());
        self.redirect_chain.push(redirect_url.to_string());

        Ok(())
    }

    /// Check if a URL would be a valid redirect target.
    ///
    /// This is a non-mutating check that doesn't update internal state.
    pub fn would_validate(&self, redirect_url: &str, current_count: u8) -> Result<(), SsrfError> {
        let mut clone = self.clone();
        clone.validate(redirect_url, current_count)
    }

    /// Get the redirect chain.
    pub fn redirect_chain(&self) -> &[String] {
        &self.redirect_chain
    }

    /// Get visited hosts.
    pub fn visited_hosts(&self) -> &HashSet<String> {
        &self.visited_hosts
    }

    /// Reset the validator state.
    pub fn reset(&mut self) {
        self.redirect_chain.clear();
        self.visited_hosts.clear();
        self.original_host = None;
    }
}

impl Default for RedirectValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Validate a redirect URL without creating a validator instance.
///
/// This is a convenience function for one-off validation.
#[allow(dead_code)]
pub fn validate_redirect(redirect_url: &str) -> Result<(), SsrfError> {
    // Quick static check
    if is_internal_url(redirect_url) {
        return Err(SsrfError::RedirectToInternal {
            url: redirect_url.to_string(),
        });
    }

    // Validate scheme
    if let Ok(url) = Url::parse(redirect_url) {
        match url.scheme() {
            "http" | "https" => Ok(()),
            scheme => Err(SsrfError::InvalidScheme {
                scheme: scheme.to_string(),
            }),
        }
    } else {
        Err(SsrfError::InvalidUrl {
            url: redirect_url.to_string(),
            reason: "Failed to parse URL".to_string(),
        })
    }
}

/// Create a reqwest redirect policy with SSRF protection.
///
/// This function creates a custom redirect policy that validates
/// each redirect URL before following it.
#[cfg(feature = "engine-reqwest")]
#[allow(dead_code)]
pub fn create_ssrf_safe_redirect_policy(max_redirects: u8) -> reqwest::redirect::Policy {
    reqwest::redirect::Policy::custom(move |attempt| {
        // Check redirect count
        if attempt.previous().len() >= max_redirects as usize {
            return attempt.stop();
        }

        // Get redirect URL
        let redirect_url = attempt.url().to_string();

        // Validate redirect URL
        if is_internal_url(&redirect_url) {
            log::warn!(
                "SSRF protection: Blocking redirect to internal URL: {}",
                redirect_url
            );
            return attempt.stop();
        }

        // Follow the redirect
        attempt.follow()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_redirect_policy_default() {
        let policy = RedirectPolicy::default();
        assert!(policy.allows_redirects());
        assert_eq!(policy.max_redirects(), DEFAULT_MAX_REDIRECTS);
    }

    #[test]
    fn test_redirect_policy_none() {
        let policy = RedirectPolicy::none();
        assert!(!policy.allows_redirects());
        assert_eq!(policy.max_redirects(), 0);
    }

    #[test]
    fn test_redirect_validator_new() {
        let validator = RedirectValidator::new();
        assert!(validator.redirect_chain.is_empty());
        assert!(validator.visited_hosts.is_empty());
        assert!(validator.original_host.is_none());
    }

    #[test]
    fn test_redirect_validator_validate_internal() {
        let mut validator = RedirectValidator::new();

        // Internal URLs should be blocked
        let result = validator.validate("http://localhost", 0);
        assert!(result.is_err());

        let result = validator.validate("http://192.168.1.1", 0);
        assert!(result.is_err());

        let result = validator.validate("http://10.0.0.1", 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_redirect_validator_validate_external() {
        let mut validator = RedirectValidator::new();

        // External URLs should be allowed
        let result = validator.validate("http://example.com", 0);
        assert!(result.is_ok());

        let result = validator.validate("https://google.com", 1);
        assert!(result.is_ok());
    }

    #[test]
    fn test_redirect_validator_max_redirects() {
        let mut validator =
            RedirectValidator::with_policy(RedirectPolicy::follow_with_validation(2));

        // First redirect should succeed
        let result = validator.validate("http://example.com", 0);
        assert!(result.is_ok());

        // Second redirect should succeed
        let result = validator.validate("http://example.org", 1);
        assert!(result.is_ok());

        // Third redirect should fail (exceeds limit)
        let result = validator.validate("http://example.net", 2);
        assert!(result.is_err());
        assert!(matches!(
            result,
            Err(SsrfError::MaxRedirectsExceeded { .. })
        ));
    }

    #[test]
    fn test_redirect_validator_same_host() {
        let mut validator = RedirectValidator::with_policy(RedirectPolicy::same_host_only(10))
            .with_original_url("http://example.com/page");

        // Same host redirect should succeed
        let result = validator.validate("http://example.com/other", 0);
        assert!(result.is_ok());

        // Cross-host redirect should fail
        let result = validator.validate("http://other.com/page", 0);
        assert!(result.is_err());
    }

    #[test]
    fn test_redirect_validator_loop_detection() {
        let mut validator = RedirectValidator::new();

        // First redirect to example.com
        let result = validator.validate("http://example.com", 0);
        assert!(result.is_ok());

        // Redirect to example.org
        let result = validator.validate("http://example.org", 1);
        assert!(result.is_ok());

        // Redirect back to example.com (loop)
        let result = validator.validate("http://example.com", 2);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("loop"));
    }

    #[test]
    fn test_validate_redirect_function() {
        // External URLs should pass
        assert!(validate_redirect("http://example.com").is_ok());
        assert!(validate_redirect("https://google.com").is_ok());

        // Internal URLs should fail
        assert!(validate_redirect("http://localhost").is_err());
        assert!(validate_redirect("http://192.168.1.1").is_err());

        // Invalid schemes should fail
        assert!(validate_redirect("ftp://example.com").is_err());
        assert!(validate_redirect("file:///etc/passwd").is_err());
    }

    #[test]
    fn test_redirect_validator_reset() {
        let mut validator = RedirectValidator::new();

        // Add some state
        validator.validate("http://example.com", 0).unwrap();
        validator.validate("http://example.org", 1).unwrap();

        assert!(!validator.redirect_chain.is_empty());
        assert!(!validator.visited_hosts.is_empty());

        // Reset
        validator.reset();

        assert!(validator.redirect_chain.is_empty());
        assert!(validator.visited_hosts.is_empty());
        assert!(validator.original_host.is_none());
    }

    #[test]
    fn test_redirect_policy_constructors() {
        let follow = RedirectPolicy::follow_with_validation(5);
        assert!(follow.allows_redirects());
        assert_eq!(follow.max_redirects(), 5);

        let same_host = RedirectPolicy::same_host_only(3);
        assert!(same_host.allows_redirects());
        assert_eq!(same_host.max_redirects(), 3);
    }

    #[test]
    fn test_redirect_validator_with_policy() {
        let validator = RedirectValidator::with_policy(RedirectPolicy::follow_with_validation(3));
        assert!(validator.original_host.is_none());
        assert!(validator.redirect_chain.is_empty());
    }

    #[test]
    fn test_with_original_url_invalid() {
        // Invalid URL should not set original_host (the if let Ok branch)
        let validator = RedirectValidator::with_policy(RedirectPolicy::same_host_only(10))
            .with_original_url("not-a-url");
        assert!(validator.original_host.is_none());
        assert!(validator.visited_hosts.is_empty());
    }

    #[test]
    fn test_validate_invalid_url_returns_invalid_url_error() {
        let mut validator = RedirectValidator::new();
        // Unparseable URL should fail with InvalidUrl
        let result = validator.validate("http://[invalid", 0);
        assert!(result.is_err());
        assert!(matches!(result, Err(SsrfError::InvalidUrl { .. })));
    }

    #[test]
    fn test_validate_redirects_disabled_by_policy() {
        let mut validator = RedirectValidator::with_policy(RedirectPolicy::none());
        let result = validator.validate("http://example.com", 0);
        assert!(result.is_err());
        assert!(matches!(result, Err(SsrfError::ValidationFailed(_))));
    }

    #[test]
    fn test_would_validate_does_not_mutate() {
        let mut validator = RedirectValidator::new();
        // would_validate should not change internal state
        let _ = validator.would_validate("http://example.com", 0);
        assert!(validator.redirect_chain.is_empty());

        // Now actually validate - should succeed and update state
        let result = validator.validate("http://example.com", 0);
        assert!(result.is_ok());
        assert_eq!(validator.redirect_chain.len(), 1);
    }

    #[test]
    fn test_would_validate_rejects_internal() {
        let validator = RedirectValidator::new();
        assert!(validator.would_validate("http://localhost", 0).is_err());
        assert!(validator.would_validate("http://10.0.0.1", 0).is_err());
    }

    #[test]
    fn test_redirect_chain_and_visited_hosts_getters() {
        let mut validator = RedirectValidator::with_policy(RedirectPolicy::same_host_only(10))
            .with_original_url("http://example.com");

        validator.validate("http://example.com/page1", 0).unwrap();
        validator.validate("http://example.com/page2", 1).unwrap();

        // redirect_chain should track URLs
        assert_eq!(validator.redirect_chain().len(), 2);
        assert!(validator
            .redirect_chain()
            .iter()
            .any(|u| u.contains("page1")));

        // visited_hosts should contain the host (set by with_original_url + validate)
        assert!(validator.visited_hosts().contains("example.com"));
    }

    #[test]
    fn test_validate_redirect_function_invalid_url() {
        // Unparseable URL should fail with InvalidUrl (the else branch)
        let result = validate_redirect("http://[invalid");
        assert!(result.is_err());
        assert!(matches!(result, Err(SsrfError::InvalidUrl { .. })));
    }

    // =========================================================================
    // create_ssrf_safe_redirect_policy 测试
    // =========================================================================

    #[test]
    fn tc_create_ssrf_safe_redirect_policy_zero() {
        let policy = create_ssrf_safe_redirect_policy(0);
        let client_result = reqwest::Client::builder()
            .redirect(policy)
            .build();
        assert!(
            client_result.is_ok(),
            "ClientBuilder with redirect policy(0) should succeed"
        );
    }

    #[test]
    fn tc_create_ssrf_safe_redirect_policy_default() {
        let policy = create_ssrf_safe_redirect_policy(10);
        let client_result = reqwest::Client::builder()
            .redirect(policy)
            .build();
        assert!(
            client_result.is_ok(),
            "ClientBuilder with redirect policy(10) should succeed"
        );
    }

    #[test]
    fn tc_create_ssrf_safe_redirect_policy_max_u8() {
        let policy = create_ssrf_safe_redirect_policy(255);
        let client_result = reqwest::Client::builder()
            .redirect(policy)
            .build();
        assert!(
            client_result.is_ok(),
            "ClientBuilder with redirect policy(255) should succeed"
        );
    }
}
