// Copyright (c) 2025-2026 Kirky.X🌠
// SPDX-License-Identifier: Apache-2.0

//! Per-tenant outbound isolation configuration.
//!
//! Each tenant may declare a dedicated proxy pool and concurrency budgets for
//! browser contexts and DNS queries. Absent configuration means the shared
//! global pool and unlimited budgets (byte-for-byte current behavior).

use serde::{Deserialize, Serialize};

/// Overflow handling when a tenant-specific resource budget is exhausted.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OverflowPolicyConfig {
    /// Reject the incoming request immediately.
    #[default]
    RejectNewest,
    /// Wait briefly, then degrade to the shared pool.
    QueueWithTimeout,
}

/// One proxy entry in a named dedicated pool.
///
/// Accepts either a bare URL string (`"http://host:port"`) or a detailed table
/// (`{ url = "...", category = "html" }`), so existing single-pool URL lists
/// remain valid when moved into a named pool.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum ProxyEntryConfig {
    /// Bare proxy URL (defaults to HTML category).
    Url(String),
    /// Detailed entry with an optional proxy category.
    Detailed {
        /// Proxy URL (may carry userinfo; redacted in logs).
        url: String,
        /// Proxy category (`html` or `media`); defaults to `html`.
        #[serde(default)]
        category: Option<String>,
    },
}

impl ProxyEntryConfig {
    /// Return the configured proxy URL regardless of representation.
    pub fn url(&self) -> &str {
        match self {
            ProxyEntryConfig::Url(url) => url,
            ProxyEntryConfig::Detailed { url, .. } => url,
        }
    }
}

/// Outbound isolation settings for one tenant.
#[derive(Debug, Clone, Deserialize, Serialize, confers::Config)]
#[config(env_prefix = "CRAWLRS__OUTBOUND_ISOLATION__")]
pub struct TenantOutboundConfig {
    /// Identifier of a dedicated proxy pool; `None` uses the shared pool.
    #[serde(default)]
    pub proxy_pool: Option<String>,

    /// Max concurrent browser contexts for this tenant.
    #[serde(default)]
    pub max_concurrent_browser_contexts: Option<u32>,

    /// Max DNS queries per minute for this tenant.
    #[serde(default)]
    pub max_dns_queries_per_minute: Option<u32>,

    /// Overflow policy; defaults to `RejectNewest` when the key is absent.
    #[serde(default)]
    pub overflow_policy: OverflowPolicyConfig,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overflow_policy_defaults_to_reject_newest() {
        let cfg: TenantOutboundConfig = toml::from_str("").expect("empty toml");
        assert_eq!(cfg.overflow_policy, OverflowPolicyConfig::RejectNewest);
    }

    #[test]
    fn overflow_policy_deserializes_snake_case() {
        let cfg: TenantOutboundConfig =
            toml::from_str("overflow_policy = \"queue_with_timeout\"").expect("toml");
        assert_eq!(cfg.overflow_policy, OverflowPolicyConfig::QueueWithTimeout);
    }

    #[test]
    fn tenant_config_parses_browser_context_limit() {
        let cfg: TenantOutboundConfig =
            toml::from_str("max_concurrent_browser_contexts = 8").expect("toml");
        assert_eq!(cfg.max_concurrent_browser_contexts, Some(8));
        assert_eq!(cfg.overflow_policy, OverflowPolicyConfig::RejectNewest);
    }

    #[test]
    fn tenant_config_all_fields_optional() {
        let cfg: TenantOutboundConfig = toml::from_str("").expect("toml");
        assert!(cfg.proxy_pool.is_none());
        assert!(cfg.max_concurrent_browser_contexts.is_none());
        assert!(cfg.max_dns_queries_per_minute.is_none());
    }

    #[test]
    fn proxy_entry_config_accepts_bare_url_and_detailed_table() {
        #[derive(Deserialize)]
        struct Wrapper {
            entry: ProxyEntryConfig,
        }
        let bare: Wrapper = toml::from_str("entry = \"http://a:8080\"").expect("bare url");
        assert_eq!(bare.entry.url(), "http://a:8080");
        let detailed: Wrapper =
            toml::from_str("entry = { url = \"http://b:8080\", category = \"media\" }")
                .expect("detailed");
        assert_eq!(detailed.entry.url(), "http://b:8080");
    }

    #[test]
    fn proxy_pools_parse_two_pools_and_default_empty() {
        #[derive(Deserialize)]
        struct Wrapper {
            #[serde(default)]
            pools: std::collections::HashMap<String, Vec<ProxyEntryConfig>>,
        }
        let parsed: Wrapper = toml::from_str(
            "[pools]\nacme = [\"http://a1\", \"http://a2\"]\nbeta = [\"http://b1\"]",
        )
        .expect("pools toml");
        assert_eq!(parsed.pools.len(), 2);
        assert_eq!(parsed.pools["acme"].len(), 2);
        assert_eq!(parsed.pools["beta"][0].url(), "http://b1");

        let empty: Wrapper = toml::from_str("").expect("empty toml");
        assert!(empty.pools.is_empty());
    }
}
