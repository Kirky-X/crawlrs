// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! `ai.txt` compliance checker.
//!
//! Mirrors `RobotsChecker`'s structure: fetch `https://<host>/ai.txt`, cache by
//! host, and evaluate the target path with the robotstxt matcher (ai.txt uses
//! the same `User-agent` / `Disallow` grammar subset). Fetch failures and 404
//! yield `NotFetched` (fail-open: ai.txt is advisory, not an admission block on
//! its own).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use robotstxt::DefaultMatcher;
use serde::Serialize;
use thiserror::Error;
use tokio::sync::Mutex;
use url::Url;

/// Error type for ai.txt checks (aligned with robots checker errors).
#[derive(Debug, Error)]
pub enum AiTxtCheckerError {
    /// URL parsing failed.
    #[error("URL parse failed: {0}")]
    UrlParseError(String),
    /// The HTTP fetch failed.
    #[error("ai.txt fetch failed: {0}")]
    FetchError(String),
}

/// Verdict for an ai.txt check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AiTxtVerdict {
    /// ai.txt allows this path (or has no matching rule).
    Allowed,
    /// ai.txt disallows this path for our user agent.
    Disallowed,
    /// ai.txt could not be fetched (404 / network error) or not configured.
    NotFetched,
}

/// Cached ai.txt body with its insertion time.
#[derive(Clone)]
struct CachedAiTxt {
    body: String,
    fetched_at: Instant,
}

/// Checker for per-host `ai.txt` files.
pub struct AiTxtChecker {
    user_agent: String,
    cache_ttl: Duration,
    client: reqwest::Client,
    cache: Arc<Mutex<HashMap<String, CachedAiTxt>>>,
}

impl AiTxtChecker {
    /// Create a checker with a user agent and cache TTL.
    pub fn new(user_agent: &str, cache_ttl: Duration) -> Self {
        Self {
            user_agent: user_agent.to_string(),
            cache_ttl,
            client: reqwest::Client::new(),
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Seed the cache with ai.txt content for a host (test / pre-warm path).
    pub async fn seed_cache_for_test(&self, host: &str, body: &str) {
        self.cache.lock().await.insert(
            host.to_string(),
            CachedAiTxt {
                body: body.to_string(),
                fetched_at: Instant::now(),
            },
        );
    }

    /// Check whether ai.txt disallows the target URL for our user agent.
    pub async fn check(&self, url: &Url) -> Result<AiTxtVerdict, AiTxtCheckerError> {
        let host = url
            .host_str()
            .ok_or_else(|| AiTxtCheckerError::UrlParseError("no host".to_string()))?;

        let body = match self.cached_body(host).await {
            Some(body) => body,
            None => match self.fetch(host).await {
                Some(body) => body,
                None => return Ok(AiTxtVerdict::NotFetched),
            },
        };

        let path_and_query = match url.query() {
            Some(q) => format!("{}?{}", url.path(), q),
            None => url.path().to_string(),
        };
        let mut matcher = DefaultMatcher::default();
        if matcher.one_agent_allowed_by_robots(&body, &self.user_agent, &path_and_query) {
            Ok(AiTxtVerdict::Allowed)
        } else {
            Ok(AiTxtVerdict::Disallowed)
        }
    }

    async fn cached_body(&self, host: &str) -> Option<String> {
        let cache = self.cache.lock().await;
        let entry = cache.get(host)?;
        if entry.fetched_at.elapsed() >= self.cache_ttl {
            return None;
        }
        Some(entry.body.clone())
    }

    async fn fetch(&self, host: &str) -> Option<String> {
        let url = format!("https://{host}/ai.txt");
        let response = self.client.get(&url).send().await.ok()?;
        if !response.status().is_success() {
            return None;
        }
        let body = response.text().await.ok()?;
        self.cache.lock().await.insert(
            host.to_string(),
            CachedAiTxt {
                body: body.clone(),
                fetched_at: Instant::now(),
            },
        );
        Some(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checker() -> AiTxtChecker {
        AiTxtChecker::new("crawlrs-bot", Duration::from_secs(300))
    }

    #[tokio::test]
    async fn disallow_rule_yields_disallowed() {
        let c = checker();
        c.seed_cache_for_test(
            "example.com",
            "User-agent: crawlrs-bot\nDisallow: /private\n",
        )
        .await;
        let url = Url::parse("https://example.com/private/page").unwrap();
        assert_eq!(c.check(&url).await.unwrap(), AiTxtVerdict::Disallowed);
    }

    #[tokio::test]
    async fn empty_ai_txt_yields_allowed() {
        let c = checker();
        c.seed_cache_for_test("example.com", "").await;
        let url = Url::parse("https://example.com/public").unwrap();
        assert_eq!(c.check(&url).await.unwrap(), AiTxtVerdict::Allowed);
    }

    #[tokio::test]
    async fn other_agent_rule_does_not_apply() {
        let c = checker();
        c.seed_cache_for_test("example.com", "User-agent: other-bot\nDisallow: /\n")
            .await;
        let url = Url::parse("https://example.com/public").unwrap();
        assert_eq!(c.check(&url).await.unwrap(), AiTxtVerdict::Allowed);
    }
}
